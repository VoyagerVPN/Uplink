use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::ptr;

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub static CHILD_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub fn executable() -> io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name("uplink-window.exe"))
}

pub struct WindowProcess {
    child: Option<Child>,
    menu: Option<Child>,
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
            Ok(Self {
                child: None,
                menu: None,
                job,
            })
        }
    }

    pub fn open(&mut self) -> io::Result<()> {
        self.open_room(false)
    }

    pub fn open_after_update(&mut self) -> io::Result<()> {
        self.open_room(true)
    }

    fn open_room(&mut self, focus_after_update: bool) -> io::Result<()> {
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
        let args = if focus_after_update { vec!["--focus-after-update".into()] } else { vec![] };
        let child = self.spawn(&args)?;
        if focus_after_update {
            unsafe { AllowSetForegroundWindow(child.id()) };
        }
        CHILD_PID.store(child.id(), std::sync::atomic::Ordering::Relaxed);
        self.child = Some(child);
        Ok(())
    }

    pub fn open_tray_menu(&mut self, tray: &crate::tray::Tray, at: WPARAM) -> io::Result<()> {
        if self.dismiss_menu()? {
            return Ok(());
        }
        let at = tray.menu_anchor(at);
        let child = self.spawn(&[
            "--tray-menu".into(),
            at.x.to_string(),
            at.y.to_string(),
            crate::tray::light_taskbar().to_string(),
        ])?;
        self.menu = Some(child);
        Ok(())
    }

    pub fn activate_icon(&mut self) -> io::Result<()> {
        if !self.dismiss_menu()? {
            self.open()?;
        }
        Ok(())
    }

    fn dismiss_menu(&mut self) -> io::Result<bool> {
        let Some(menu) = &mut self.menu else {
            return Ok(false);
        };
        if menu.try_wait()?.is_some() {
            self.menu = None;
            return Ok(false);
        }
        if let Some(hwnd) = process_window(menu.id(), |title| !title.is_empty()) {
            unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
        }
        Ok(true)
    }

    fn spawn(&self, args: &[String]) -> io::Result<Child> {
        let mut child = Command::new(executable()?).args(args).spawn()?;
        if unsafe { AssignProcessToJobObject(self.job.as_raw_handle(), child.as_raw_handle()) } == 0
        {
            let err = io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }
        Ok(child)
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
    matches_title: fn(&[u16]) -> bool,
}

pub fn visible_window(pid: u32) -> Option<HWND> {
    process_window(pid, |title| {
        title.iter().copied().eq("Uplink".encode_utf16())
    })
}

fn process_window(pid: u32, matches_title: fn(&[u16]) -> bool) -> Option<HWND> {
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
            let mut title = [0u16; 64];
            let length =
                unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) } as usize;
            if !(search.matches_title)(&title[..length]) {
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
        matches_title,
    };
    unsafe {
        EnumWindows(Some(visit), (&mut search as *mut WindowSearch) as LPARAM);
    }
    (!search.result.is_null()).then_some(search.result)
}
