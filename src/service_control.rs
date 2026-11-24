use std::{
    io, ptr, thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_NOT_ACTIVE},
    System::Services::*,
};

pub const NAME: &str = "UplinkNetwork";

pub fn product() -> io::Result<String> {
    let exe = std::env::current_exe()?;
    let package = exe.with_file_name("package.id");
    let marker = match std::fs::read_to_string(package) {
        Ok(marker) => marker,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok("Uplink".into()),
        Err(e) => return Err(e),
    };
    let (product, _) = marker.split_once('|').ok_or(io::ErrorKind::InvalidData)?;
    if product == "Uplink" {
        return Ok("Uplink".into());
    }
    if let Some(suffix) = product.strip_prefix("Uplink-QA-")
        && !suffix.is_empty()
        && suffix
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return Ok(product.to_owned());
    }
    Err(io::Error::from(io::ErrorKind::InvalidData))
}

pub fn power(enabled: bool) -> io::Result<()> {
    set_running(&format!("{}Network", product()?), enabled)
}

struct ServiceHandle(SC_HANDLE);

impl ServiceHandle {
    fn checked(handle: SC_HANDLE) -> io::Result<Self> {
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(handle))
    }
}

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}

pub fn set_running(name: &str, enabled: bool) -> io::Result<()> {
    if name.contains('\0') {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let manager = ServiceHandle::checked(unsafe {
        OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT)
    })?;
    let access = SERVICE_QUERY_STATUS | if enabled { SERVICE_START } else { SERVICE_STOP };
    let service =
        ServiceHandle::checked(unsafe { OpenServiceW(manager.0, name.as_ptr(), access) })?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let target = if enabled {
        SERVICE_RUNNING
    } else {
        SERVICE_STOPPED
    };
    let mut requested = false;
    loop {
        let mut status = SERVICE_STATUS::default();
        if unsafe { QueryServiceStatus(service.0, &mut status) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if status.dwCurrentState == target {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        if requested
            && enabled
            && status.dwCurrentState == SERVICE_STOPPED
            && status.dwWin32ExitCode != 0
        {
            return Err(io::Error::from_raw_os_error(status.dwWin32ExitCode as i32));
        }
        if !requested && matches!(status.dwCurrentState, SERVICE_RUNNING | SERVICE_STOPPED) {
            let ok = unsafe {
                if enabled {
                    StartServiceW(service.0, 0, ptr::null())
                } else {
                    ControlService(service.0, SERVICE_CONTROL_STOP, &mut status)
                }
            };
            if ok == 0 {
                let error = io::Error::last_os_error();
                let concurrent = if enabled {
                    ERROR_SERVICE_ALREADY_RUNNING
                } else {
                    ERROR_SERVICE_NOT_ACTIVE
                };
                if error.raw_os_error() != Some(concurrent as i32) {
                    return Err(error);
                }
            }
            requested = true;
        }
        thread::sleep(Duration::from_millis(100));
    }
}
