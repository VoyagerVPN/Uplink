use std::{io, os::windows::ffi::OsStrExt, ptr};
use windows_sys::Win32::{Foundation::ERROR_FILE_NOT_FOUND, System::Registry::*};
use windows_sys::core::{PCWSTR, w};

const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const NAME: PCWSTR = w!("Uplink");

pub fn enabled() -> io::Result<bool> {
    let mut size = 0;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN,
            NAME,
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut size,
        )
    };
    match result {
        0 => Ok(true),
        ERROR_FILE_NOT_FOUND => Ok(false),
        _ => Err(io::Error::from_raw_os_error(result as i32)),
    }
}

pub fn set(enable: bool) -> io::Result<()> {
    let result = if enable {
        let exe = std::env::current_exe()?.with_file_name("uplink.exe");
        if !exe.is_file() {
            return Err(io::Error::other("Cannot find uplink.exe for startup"));
        }
        let command: Vec<u16> = std::iter::once(34)
            .chain(exe.as_os_str().encode_wide())
            .chain("\" --background\0".encode_utf16())
            .collect();
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN,
                NAME,
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * 2) as u32,
            )
        }
    } else {
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, NAME) }
    };
    if result == 0 || (!enable && result == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}
