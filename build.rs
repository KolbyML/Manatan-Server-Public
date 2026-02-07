use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DEFAULT_REPO: &str = "KolbyML/Manatan-Server-Public";
const DEFAULT_TAG: &str = "stable";

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
    if !lib_path.exists() {
        if let Err(err) = download_release_asset(&lib_path, &target, is_windows) {
            panic!(
                "Missing static library: {}. Expected {}. Download failed: {}",
                target,
                lib_path.display(),
                err
            );
        }
    }

    if !lib_path.exists() {
        panic!(
            "Missing static library: {}. Expected {}. Download did not produce the library.",
            target,
            lib_path.display()
        );
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=manatan_server");
    #[cfg(target_os = "linux")]
    {
        println!("cargo:rustc-link-lib=bz2");
        println!("cargo:rustc-link-lib=freetype");
        println!("cargo:rustc-link-lib=fontconfig");
    }
    println!("cargo:rerun-if-changed={}", lib_path.display());
    println!("cargo:rerun-if-env-changed=MANATAN_SERVER_PUBLIC_TOKEN");
    println!("cargo:rerun-if-env-changed=MANATAN_SERVER_PUBLIC_REPO");
}

fn download_release_asset(lib_path: &Path, target: &str, is_windows: bool) -> Result<(), String> {
    let repo = env::var("MANATAN_SERVER_PUBLIC_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string());
    let tag = DEFAULT_TAG;
    let asset_ext = if is_windows { "lib" } else { "a" };
    let primary_asset_name = format!("manatan-server-{}.{}", target, asset_ext);
    let legacy_asset_name = format!("manatan-server-manatan-server-{}.{}", target, asset_ext);
    let api_url = format!("https://api.github.com/repos/{repo}/releases/tags/{tag}");

    let token = env::var("MANATAN_SERVER_PUBLIC_TOKEN").ok();
    let json = github_json(&api_url, token.as_deref())?;

    let assets = json
        .get("assets")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "release assets missing".to_string())?;

    let asset = assets
        .iter()
        .find(|value| {
            let name = value.get("name").and_then(|v| v.as_str());
            name == Some(primary_asset_name.as_str()) || name == Some(legacy_asset_name.as_str())
        })
        .ok_or_else(|| {
            format!(
                "asset not found: {} (or {})",
                primary_asset_name, legacy_asset_name
            )
        })?;

    let download_url = asset
        .get("browser_download_url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "asset download URL missing".to_string())?;

    if let Some(parent) = lib_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create dir failed: {err}"))?;
    }

    download_file(download_url, lib_path, token.as_deref())
}

fn github_json(url: &str, token: Option<&str>) -> Result<serde_json::Value, String> {
    let mut request = ureq::get(url)
        .set("User-Agent", "manatan-server-public-build")
        .set("Accept", "application/vnd.github+json");
    if let Some(token) = token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }

    let response = request
        .call()
        .map_err(|err| format!("github api failed: {err}"))?;
    serde_json::from_reader(response.into_reader())
        .map_err(|err| format!("invalid github json: {err}"))
}

fn download_file(url: &str, path: &Path, token: Option<&str>) -> Result<(), String> {
    let mut request = ureq::get(url).set("User-Agent", "manatan-server-public-build");
    if let Some(token) = token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }

    let response = request
        .call()
        .map_err(|err| format!("download failed: {err}"))?;
    let mut reader = response.into_reader();
    let mut file = fs::File::create(path).map_err(|err| format!("create file failed: {err}"))?;
    io::copy(&mut reader, &mut file).map_err(|err| format!("write failed: {err}"))?;
    Ok(())
}
