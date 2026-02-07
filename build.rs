use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DEFAULT_REPO: &str = "KolbyML/Manatan-Server-Public";
const DEFAULT_TAG: &str = "stable";

#[derive(Debug, Clone)]
struct ReleaseAsset {
    id: u64,
    name: String,
    download_url: String,
    updated_at: Option<String>,
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

fn sync_release_asset(
    lib_path: &Path,
    meta_path: &Path,
    target: &str,
    is_windows: bool,
) -> Result<(), String> {
    let token = env::var("MANATAN_SERVER_PUBLIC_TOKEN").ok();
    let asset = release_asset_info(target, is_windows, token.as_deref())?;
    let existing_meta = fs::read_to_string(meta_path).ok();
    let expected_meta = format!(
        "id={}\nname={}\nupdated_at={}\n",
        asset.id,
        asset.name,
        asset.updated_at.as_deref().unwrap_or_default()
    );

    let needs_download =
        !lib_path.exists() || existing_meta.as_deref() != Some(expected_meta.as_str());

    if needs_download {
        if let Some(parent) = lib_path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("create dir failed: {err}"))?;
        }
        download_file(&asset.download_url, lib_path, token.as_deref())?;
        fs::write(meta_path, expected_meta).map_err(|err| format!("write meta failed: {err}"))?;
    }

    Ok(())
}

fn release_asset_info(
    target: &str,
    is_windows: bool,
    token: Option<&str>,
) -> Result<ReleaseAsset, String> {
    let repo = env::var("MANATAN_SERVER_PUBLIC_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string());
    let tag = DEFAULT_TAG;
    let asset_ext = if is_windows { "lib" } else { "a" };
    let primary_asset_name = format!("manatan-server-{}.{}", target, asset_ext);
    let legacy_asset_name = format!("manatan-server-manatan-server-{}.{}", target, asset_ext);
    let api_url = format!("https://api.github.com/repos/{repo}/releases/tags/{tag}");
    let json = github_json(&api_url, token)?;

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

    let id = asset
        .get("id")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| "asset id missing".to_string())?;
    let name = asset
        .get("name")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "asset name missing".to_string())?
        .to_string();
    let download_url = asset
        .get("browser_download_url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "asset download URL missing".to_string())?
        .to_string();
    let updated_at = asset
        .get("updated_at")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());

    Ok(ReleaseAsset {
        id,
        name,
        download_url,
        updated_at,
    })
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
