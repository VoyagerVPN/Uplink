use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::System::Threading::*;

pub fn listen() -> io::Result<OwnedHandle> {
    named("Local\\Uplink.Room.Changed.v2")
}
pub fn listen_update() -> io::Result<OwnedHandle> {
    named("Local\\Uplink.Update.Changed.v1")
}
pub fn notify_update() -> io::Result<()> {
    let event = listen_update()?;
    if unsafe { SetEvent(event.as_raw_handle()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn named(name: &str) -> io::Result<OwnedHandle> {
    let h = unsafe {
        CreateEventW(
            std::ptr::null(),
            0,
            0,
            crate::control::scoped_name(name).as_ptr(),
        )
    };
    if h.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(h) })
}

pub fn notify() -> io::Result<()> {
    let e = listen()?;
    if unsafe { SetEvent(e.as_raw_handle()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    crate::control::refresh_tray_icon();
    Ok(())
}
