#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub java_runtime_url: String,
    pub webview_enabled: bool,
    pub aidoku_index_url: String,
    pub aidoku_enabled: bool,
    pub aidoku_cache_path: String,
    pub db_path: String,
    pub migrate_path: Option<String>,
    pub tracker_remote_search: bool,
    pub tracker_search_ttl_seconds: i64,
    pub downloads_path: String,
    pub local_manga_path: String,
    pub local_anime_path: String,
}

impl Config {
    pub fn from_env() -> Self {
        let host = std::env::var("MANATAN_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("MANATAN_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(4568);
        let java_runtime_url = std::env::var("MANATAN_JAVA_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:4566".to_string());
        let webview_enabled = env_bool("MANATAN_WEBVIEW_ENABLED", false);
        let db_path =
            std::env::var("MANATAN_DB_PATH").unwrap_or_else(|_| "manatan.sqlite".to_string());
        let db_parent = std::path::PathBuf::from(&db_path)
            .parent()
            .map(|path| {
                if path.as_os_str().is_empty() {
                    std::path::PathBuf::from(".")
                } else {
                    path.to_path_buf()
                }
            })
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let aidoku_index_url = std::env::var("MANATAN_AIDOKU_INDEX").unwrap_or_default();
        let aidoku_enabled = env_bool("MANATAN_AIDOKU_ENABLED", true);
        let migrate_path = std::env::var("MANATAN_MIGRATE_PATH").ok();
        let tracker_remote_search = env_bool("MANATAN_TRACKER_REMOTE_SEARCH", true);
        let tracker_search_ttl_seconds = std::env::var("MANATAN_TRACKER_SEARCH_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(3600);
        let downloads_path = std::env::var("MANATAN_DOWNLOADS_PATH")
            .unwrap_or_else(|_| db_parent.join("downloads").to_string_lossy().to_string());
        let local_manga_path = std::env::var("MANATAN_LOCAL_MANGA_PATH")
            .unwrap_or_else(|_| db_parent.join("local-manga").to_string_lossy().to_string());
        let local_anime_path = std::env::var("MANATAN_LOCAL_ANIME_PATH")
            .unwrap_or_else(|_| db_parent.join("local-anime").to_string_lossy().to_string());
        let aidoku_cache_path = std::env::var("MANATAN_AIDOKU_CACHE")
            .unwrap_or_else(|_| db_parent.join("aidoku").to_string_lossy().to_string());

        Self {
            host,
            port,
            java_runtime_url,
            webview_enabled,
            aidoku_index_url,
            aidoku_enabled,
            aidoku_cache_path,
            db_path,
            migrate_path,
            tracker_remote_search,
            tracker_search_ttl_seconds,
            downloads_path,
            local_manga_path,
            local_anime_path,
        }
    }

    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|value| match value.to_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}
