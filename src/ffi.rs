use std::os::raw::c_char;

#[repr(C)]
pub struct ManatanServerConfig {
    pub host: *const c_char,
    pub port: u16,
    pub java_runtime_url: *const c_char,
    pub webview_enabled: u8,
    pub aidoku_index_url: *const c_char,
    pub aidoku_enabled: u8,
    pub aidoku_cache_path: *const c_char,
    pub db_path: *const c_char,
    pub migrate_path: *const c_char,
    pub tracker_remote_search: u8,
    pub tracker_search_ttl_seconds: i64,
    pub downloads_path: *const c_char,
    pub local_manga_path: *const c_char,
    pub local_anime_path: *const c_char,
}

#[repr(C)]
pub struct ManatanServerHandle {
    _private: [u8; 0],
}

extern "C" {
    pub fn manatan_server_start(config: *const ManatanServerConfig) -> *mut ManatanServerHandle;
    pub fn manatan_server_stop(handle: *mut ManatanServerHandle);
    pub fn manatan_server_port(handle: *const ManatanServerHandle) -> u16;
    pub fn manatan_server_try_handle_subprocess() -> bool;
}
