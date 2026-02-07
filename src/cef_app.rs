pub fn try_handle_subprocess() -> bool {
    unsafe { crate::ffi::manatan_server_try_handle_subprocess() }
}
