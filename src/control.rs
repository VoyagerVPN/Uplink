use std::{io, ptr};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn scoped_name(base: &str) -> Vec<u16> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hash = DefaultHasher::new();
    let path = crate::settings::path().unwrap_or_default();
    path.to_string_lossy()
        .replace('/', "\\")
        .to_lowercase()
        .hash(&mut hash);
    format!("{base}.{:016x}\0", hash.finish())
        .encode_utf16()
        .collect()
}
pub fn class() -> *const u16 {
    static CLASS: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    CLASS
        .get_or_init(|| scoped_name("Uplink.Room.Tray.v2"))
        .as_ptr()
}
pub const SHOW: u32 = WM_APP + 20;
pub const HIDE: u32 = WM_APP + 21;
pub const ICON: u32 = WM_APP + 22;
pub const WINDOW: u32 = WM_APP + 23;
pub const UPDATE_CHECK: u32 = WM_APP + 24;
pub const UPDATE_DOWNLOAD: u32 = WM_APP + 25;
pub const UPDATE_INSTALL: u32 = WM_APP + 26;
pub const UPDATE_AUTO: u32 = WM_APP + 27;
pub const QUIT: u32 = WM_APP + 28;

pub fn tray_window() -> windows_sys::Win32::Foundation::HWND {
    unsafe { FindWindowW(class(), ptr::null()) }
}
pub fn send(message: u32) -> io::Result<usize> {
    let target = tray_window();
    if target.is_null() {
        return Err(io::Error::other("Uplink is not running"));
    }
    let mut result = 0;
    if unsafe { SendMessageTimeoutW(target, message, 0, 0, SMTO_ABORTIFHUNG, 3000, &mut result) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(result)
}

pub fn refresh_tray_icon() {
    let hwnd = tray_window();
    if !hwnd.is_null() {
        unsafe {
            PostMessageW(hwnd, ICON, 0, 0);
        }
    }
}
