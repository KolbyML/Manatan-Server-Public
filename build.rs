use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_REPO: &str = "KolbyML/Manatan-Server-Public";
const DEFAULT_TAG: &str = "stable";

#[derive(Debug, Clone)]
struct ReleaseAsset {
    name: String,
    download_url: String,
}

fn main() {
    let target = env::var("TARGET").expect("TARGET not set");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let lib_dir = manifest_dir.join("lib").join(&target);

    let is_windows = target.contains("windows");
    let lib_name = if is_windows {
        "manatan_server.lib"
    } else {
        "libmanatan_server.a"
    };

    let lib_path = lib_dir.join(lib_name);
    let meta_path = lib_dir.join(format!("{}.asset-meta", lib_name));

    if let Err(err) = sync_release_asset(&lib_path, &meta_path, &target, is_windows) {
        panic!(
            "Failed to sync static library for {} at {}: {}",
            target,
            lib_path.display(),
            err
        );
    }

    if !lib_path.exists() {
        panic!(
            "Missing static library: {}. Expected {}. Download did not produce the library.",
            target,
            lib_path.display()
        );
    }

    if let Err(err) = maybe_repack_darwin_archive(&lib_path, &target) {
        panic!(
            "Failed to post-process static library for {} at {}: {}",
            target,
            lib_path.display(),
            err
        );
    }

    if let Err(err) = ensure_sqlite_alias(&lib_dir, &target, &lib_path) {
        panic!(
            "Failed to prepare sqlite3 compatibility library for {} in {}: {}",
            target,
            lib_dir.display(),
            err
        );
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static:-bundle=manatan_server");
    if target.contains("linux") && !target.contains("android") {
        println!("cargo:rustc-link-lib=dylib=bz2");
        println!("cargo:rustc-link-lib=dylib=fontconfig");
        println!("cargo:rustc-link-lib=dylib=freetype");
    }
    println!("cargo:rerun-if-changed={}", lib_path.display());
    println!("cargo:rerun-if-env-changed=MANATAN_SERVER_PUBLIC_REPO");
}

fn sync_release_asset(
    lib_path: &Path,
    meta_path: &Path,
    target: &str,
    is_windows: bool,
) -> Result<(), String> {
    let asset = release_asset_info(target, is_windows)?;
    let existing_meta = fs::read_to_string(meta_path).ok();
    let expected_meta = format!("name={}\n", asset.name);

    let needs_download =
        !lib_path.exists() || existing_meta.as_deref() != Some(expected_meta.as_str());

    if needs_download {
        if let Some(parent) = lib_path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("create dir failed: {err}"))?;
        }
        download_file(&asset.download_url, lib_path)?;
        fs::write(meta_path, expected_meta).map_err(|err| format!("write meta failed: {err}"))?;
    }

    Ok(())
}

fn release_asset_info(target: &str, is_windows: bool) -> Result<ReleaseAsset, String> {
    let repo = env::var("MANATAN_SERVER_PUBLIC_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string());
    let tag = DEFAULT_TAG;
    let asset_ext = if is_windows { "lib" } else { "a" };
    let primary_asset_name = format!("manatan-server-{}.{}", target, asset_ext);
    let legacy_asset_name = format!("manatan-server-manatan-server-{}.{}", target, asset_ext);
    let candidates = [
        (
            primary_asset_name.clone(),
            format!("https://github.com/{repo}/releases/download/{tag}/{primary_asset_name}"),
        ),
        (
            legacy_asset_name.clone(),
            format!("https://github.com/{repo}/releases/download/{tag}/{legacy_asset_name}"),
        ),
    ];

    let mut last_err = String::new();
    for (name, url) in candidates {
        if url_exists(&url) {
            return Ok(ReleaseAsset {
                name,
                download_url: url,
            });
        }
        last_err = format!("asset URL not accessible: {url}");
    }

    Err(last_err)
}

fn url_exists(url: &str) -> bool {
    ureq::head(url)
        .set("User-Agent", "manatan-server-public-build")
        .call()
        .is_ok()
}

fn download_file(url: &str, path: &Path) -> Result<(), String> {
    let request = ureq::get(url).set("User-Agent", "manatan-server-public-build");

    let response = request
        .call()
        .map_err(|err| format!("download failed: {err}"))?;
    let mut reader = response.into_reader();
    let mut file = fs::File::create(path).map_err(|err| format!("create file failed: {err}"))?;
    io::copy(&mut reader, &mut file).map_err(|err| format!("write failed: {err}"))?;
    Ok(())
}

fn ensure_sqlite_alias(lib_dir: &Path, target: &str, manatan_lib: &Path) -> Result<(), String> {
    if target.contains("apple-ios") {
        return Ok(());
    }

    let sqlite_alias = if target.contains("windows") {
        lib_dir.join("sqlite3.lib")
    } else {
        lib_dir.join("libsqlite3.a")
    };

    if sqlite_alias != manatan_lib {
        fs::copy(manatan_lib, &sqlite_alias).map_err(|err| {
            format!(
                "copy {} -> {} failed: {err}",
                manatan_lib.display(),
                sqlite_alias.display()
            )
        })?;
    }

    Ok(())
}

fn maybe_repack_darwin_archive(lib_path: &Path, target: &str) -> Result<(), String> {
    if !target.contains("apple-darwin") || !cfg!(target_os = "macos") {
        return Ok(());
    }

    if !archive_has_member_system_ar(lib_path, "//")? {
        return Ok(());
    }

    let workdir = env::temp_dir().join(format!(
        "manatan-server-public-repack-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| format!("time error: {err}"))?
            .as_millis()
    ));
    let obj_dir = workdir.join("objs");
    fs::create_dir_all(&obj_dir).map_err(|err| format!("create temp dir failed: {err}"))?;

    let ar_bin = if command_exists("llvm-ar") {
        "llvm-ar"
    } else {
        "ar"
    };
    let members_output = Command::new(ar_bin)
        .arg("t")
        .arg(lib_path)
        .output()
        .map_err(|err| format!("{ar_bin} t failed: {err}"))?;
    if !members_output.status.success() {
        return Err(format!(
            "{ar_bin} t failed: {}",
            String::from_utf8_lossy(&members_output.stderr)
        ));
    }

    let members = String::from_utf8_lossy(&members_output.stdout);
    let mut extracted = 0usize;
    for member in members.lines() {
        let member = member.trim();
        if !is_extractable_member(member) {
            continue;
        }

        let object = Command::new(ar_bin)
            .arg("p")
            .arg(lib_path)
            .arg(member)
            .output()
            .map_err(|err| format!("{ar_bin} p {member} failed: {err}"))?;
        if !object.status.success() {
            return Err(format!(
                "{ar_bin} p {member} failed: {}",
                String::from_utf8_lossy(&object.stderr)
            ));
        }

        let out = obj_dir.join(format!("m{extracted:05}.o"));
        fs::write(&out, object.stdout).map_err(|err| format!("write object failed: {err}"))?;
        extracted += 1;
    }

    if extracted == 0 {
        return Err("no object members extracted from archive".to_string());
    }

    let fixed_lib = workdir.join("libmanatan_server.fixed.a");
    let mut obj_paths = Vec::with_capacity(extracted);
    for index in 0..extracted {
        obj_paths.push(obj_dir.join(format!("m{index:05}.o")));
    }

    if command_exists("llvm-ar") {
        if !run_pack_with_llvm_ar(&fixed_lib, &obj_paths, "darwin")
            && !run_pack_with_llvm_ar(&fixed_lib, &obj_paths, "bsd")
        {
            return Err("failed to repack archive with llvm-ar".to_string());
        }
    } else {
        run_pack_with_ar(&fixed_lib, &obj_paths)?;
    }

    let _ = Command::new("ranlib").arg(&fixed_lib).output();
    fs::rename(&fixed_lib, lib_path).map_err(|err| format!("replace archive failed: {err}"))?;

    if archive_has_member_system_ar(lib_path, "//")? {
        return Err("archive repack failed; GNU-style table still present".to_string());
    }

    let _ = fs::remove_dir_all(&workdir);
    Ok(())
}

fn archive_has_member_system_ar(lib_path: &Path, member_name: &str) -> Result<bool, String> {
    let output = Command::new("ar")
        .arg("t")
        .arg(lib_path)
        .output()
        .map_err(|err| format!("ar t failed: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "ar t failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.trim() == member_name))
}

fn is_extractable_member(member: &str) -> bool {
    if member.is_empty() || member == "//" {
        return false;
    }
    !member.starts_with("__.SYMDEF")
}

fn run_pack_with_llvm_ar(fixed_lib: &Path, obj_paths: &[PathBuf], format: &str) -> bool {
    let mut command = Command::new("llvm-ar");
    command
        .arg(format!("--format={format}"))
        .arg("crs")
        .arg(fixed_lib);
    for obj in obj_paths {
        command.arg(obj);
    }

    match command.output() {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

fn run_pack_with_ar(fixed_lib: &Path, obj_paths: &[PathBuf]) -> Result<(), String> {
    let mut command = Command::new("ar");
    command.arg("crs").arg(fixed_lib);
    for obj in obj_paths {
        command.arg(obj);
    }
    let output = command
        .output()
        .map_err(|err| format!("ar crs failed: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "ar crs failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

fn command_exists(command: &str) -> bool {
    Command::new(command).arg("--version").output().is_ok()
}
