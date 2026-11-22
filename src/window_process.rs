use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::ptr;

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub static CHILD_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub fn executable() -> io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name("uplink-window.exe"))
}

pub struct WindowProcess {
    child: Option<Child>,
    job: OwnedHandle,
}

impl WindowProcess {
    pub fn new() -> io::Result<Self> {
        // Windows завершит дочернее окно, даже если процесс трея аварийно закроется.
        unsafe {
            let raw = CreateJobObjectW(ptr::null(), ptr::null());
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = OwnedHandle::from_raw_handle(raw);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                raw,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { child: None, job })
        }
    }

    pub fn open(&mut self) -> io::Result<()> {
        if let Some(child) = &mut self.child {
            if let Some(hwnd) = visible_window(child.id()) {
                unsafe {
                    if IsIconic(hwnd) != 0 {
                        ShowWindowAsync(hwnd, SW_RESTORE);
                    }
                    SetForegroundWindow(hwnd);
                }
                return Ok(());
            }
            // Между исчезновением HWND и завершением процесса окна успевает сохранить сеть.
            unsafe {
                WaitForSingleObject(child.as_raw_handle(), 250);
            }
            if child.try_wait()?.is_none() {
                return Ok(());
            }
        }
        let mut child = Command::new(executable()?).spawn()?;
        if unsafe { AssignProcessToJobObject(self.job.as_raw_handle(), child.as_raw_handle()) } == 0
        {
            let err = io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }
        CHILD_PID.store(child.id(), std::sync::atomic::Ordering::Relaxed);
        self.child = Some(child);
        Ok(())
    }

    pub fn close(&mut self) {
        if let Some(child) = &self.child {
            if let Some(hwnd) = visible_window(child.id()) {
                unsafe {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                    WaitForSingleObject(child.as_raw_handle(), 3000);
                }
            }
        }
    }
}

struct WindowSearch {
    pid: u32,
    result: HWND,
}

pub fn visible_window(pid: u32) -> Option<HWND> {
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> i32 {
        let search = unsafe { &mut *(data as *mut WindowSearch) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
        }
        if pid == search.pid
            && unsafe { IsWindowVisible(hwnd) != 0 && GetWindow(hwnd, GW_OWNER).is_null() }
        {
            // Winit также создаёт видимое служебное окно без заголовка.
            let mut title = [0u16; 8];
            let length = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
            if length != 6 || !title[..6].iter().copied().eq("Uplink".encode_utf16()) {
                return 1;
            }
            search.result = hwnd;
            return 0;
        }
        1
    }
    let mut search = WindowSearch {
        pid,
        result: ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(visit), (&mut search as *mut WindowSearch) as LPARAM);
    }
    (!search.result.is_null()).then_some(search.result)
}
