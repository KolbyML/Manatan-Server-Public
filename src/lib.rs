mod ffi;

pub mod app;
pub mod cef_app;
pub mod config;

use std::ffi::CString;

pub use app::{build_router, build_router_without_cors, AppState};
pub use config::Config;

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}

pub async fn build_state(config: Config) -> Result<AppState, Error> {
    let backend_host = std::env::var("MANATAN_BACKEND_HOST")
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    let backend_port = std::env::var("MANATAN_BACKEND_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_else(|| config.port.saturating_add(1));

    let backend_url = format!("http://{}:{}", backend_host, backend_port);

    let host = to_cstring(&backend_host, "backend_host")?;
    let java_runtime_url = to_cstring(&config.java_runtime_url, "java_runtime_url")?;
    let aidoku_index_url = to_cstring(&config.aidoku_index_url, "aidoku_index_url")?;
    let aidoku_cache_path = to_cstring(&config.aidoku_cache_path, "aidoku_cache_path")?;
    let db_path = to_cstring(&config.db_path, "db_path")?;
    let downloads_path = to_cstring(&config.downloads_path, "downloads_path")?;
    let local_manga_path = to_cstring(&config.local_manga_path, "local_manga_path")?;
    let local_anime_path = to_cstring(&config.local_anime_path, "local_anime_path")?;

    let migrate_path = match config.migrate_path.as_deref() {
        Some(value) if !value.is_empty() => Some(to_cstring(value, "migrate_path")?),
        _ => None,
    };

    let ffi_config = ffi::ManatanServerConfig {
        host: host.as_ptr(),
        port: backend_port,
        java_runtime_url: java_runtime_url.as_ptr(),
        webview_enabled: if config.webview_enabled { 1 } else { 0 },
        aidoku_index_url: aidoku_index_url.as_ptr(),
        aidoku_enabled: if config.aidoku_enabled { 1 } else { 0 },
        aidoku_cache_path: aidoku_cache_path.as_ptr(),
        db_path: db_path.as_ptr(),
        migrate_path: migrate_path
            .as_ref()
            .map(|value| value.as_ptr())
            .unwrap_or(std::ptr::null()),
        tracker_remote_search: if config.tracker_remote_search { 1 } else { 0 },
        tracker_search_ttl_seconds: config.tracker_search_ttl_seconds,
        downloads_path: downloads_path.as_ptr(),
        local_manga_path: local_manga_path.as_ptr(),
        local_anime_path: local_anime_path.as_ptr(),
    };

    let handle = unsafe { ffi::manatan_server_start(&ffi_config) };
    if handle.is_null() {
        return Err(Error("manatan_server_start failed".to_string()));
    }

    Ok(app::new_state(config, backend_url, handle))
}

fn to_cstring(value: &str, label: &str) -> Result<CString, Error> {
    CString::new(value).map_err(|_| Error(format!("{label} contains NUL bytes")))
}
