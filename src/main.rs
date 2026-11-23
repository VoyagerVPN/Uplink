#![windows_subsystem = "windows"]
use uplink::{changed, control, invite, settings};
mod tray;
mod update_worker;
mod window_process;

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::{io, ptr};
use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, WAIT_FAILED, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::core::w;

fn report(error: &dyn std::fmt::Display) {
    let message = uplink::i18n::launch_error(error);
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            message.as_ptr(),
            w!("Uplink"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(password) = std::env::var_os("UPLINK_SSH_ASKPASS") {
        if std::env::args()
            .nth(1)
            .is_some_and(|p| p.trim_end().ends_with("password:"))
        {
            use std::io::Write;
            writeln!(io::stdout().lock(), "{}", password.to_string_lossy())?;
        }
        return Ok(());
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let updated = if let [flag, pid] = args.as_slice()
        && flag == "--after-update"
    {
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
        let pid = pid.parse::<u32>()?;
        let handle = unsafe { OpenProcess(0x00100000, 0, pid) };
        if !handle.is_null() {
            let owner = unsafe { OwnedHandle::from_raw_handle(handle) };
            if unsafe { WaitForSingleObject(owner.as_raw_handle(), 30000) } != WAIT_OBJECT_0 {
                return Err(io::Error::other("Previous Uplink process is still running").into());
            }
        }
        true
    } else {
        false
    };
    let join = match args.as_slice() {
        [] => false,
        [_, _] if updated => false,
        [flag] if flag == "--background" => false,
        [link] if invite::accepts_invite(link) => true,
        _ => return Err(io::Error::other("Unknown invitation").into()),
    };
    let create = |name| -> io::Result<(OwnedHandle, bool)> {
        let raw = unsafe { CreateEventW(ptr::null(), 0, 0, name) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        Ok((unsafe { OwnedHandle::from_raw_handle(raw) }, existing))
    };
    let (event, existing) =
        create(control::scoped_name("Local\\Uplink.Room.Activate.v2").as_ptr())?;
    let background = args.first().is_some_and(|arg| arg == "--background");
    if !existing {
        settings::start_session(&settings::path()?)?;
    }
    if join {
        settings::queue_invite(&settings::path()?, &args[0])?;
        changed::notify()?;
    }
    if existing {
        if background {
            return Ok(());
        }
        if unsafe { SetEvent(event.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        return Ok(());
    }
    let mut window = window_process::WindowProcess::new()?;
    let tray = tray::install()?;
    let profile = settings::path()?;
    let updates = update_worker::start(profile.clone(), updated);
    std::thread::spawn(move || {
        loop {
            if settings::refresh_transport(&profile).unwrap_or(false) {
                let _ = changed::notify();
                control::refresh_tray_icon();
            }
            std::thread::sleep(std::time::Duration::from_secs(8));
        }
    });
    if !background {
        window.open()?;
    }
    let handles = [event.as_raw_handle()];
    loop {
        let ready = unsafe {
            MsgWaitForMultipleObjectsEx(
                handles.len() as u32,
                handles.as_ptr(),
                INFINITE,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        if ready == WAIT_FAILED {
            return Err(io::Error::last_os_error().into());
        }
        if ready == WAIT_OBJECT_0 {
            if let Err(error) = window.open() {
                report(&error);
            }
        }
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            if message.hwnd.is_null() {
                match message.message {
                    cmd @ control::UPDATE_CHECK..=control::UPDATE_AUTO => {
                        let _ = updates.try_send(cmd);
                        continue;
                    }
                    tray::OPEN => {
                        if let Err(error) = window.open() {
                            report(&error);
                        }
                        continue;
                    }
                    tray::ACTIVATE_ICON => {
                        if let Err(error) = window.activate_icon() {
                            report(&error);
                        }
                        continue;
                    }
                    control::HIDE => {
                        window.close();
                        continue;
                    }
                    tray::CONTEXT_MENU => {
                        if let Err(error) = window.open_tray_menu(&tray, message.wParam) {
                            report(&error);
                        }
                        continue;
                    }
                    tray::QUIT | WM_QUIT => {
                        window.close();
                        return Ok(());
                    }
                    _ => {}
                }
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

fn main() {
    if let Err(error) = run() {
        report(&error);
        std::process::exit(1);
    }
}
