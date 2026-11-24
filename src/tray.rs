use std::{io, ptr, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{Shell::*, WindowsAndMessaging::*},
};
use windows_sys::core::w;

pub const OPEN: u32 = WM_APP + 1;
pub const QUIT: u32 = WM_APP + 2;
const CALLBACK: u32 = WM_APP + 3;
pub const CONTEXT_MENU: u32 = WM_APP + 4;
pub const ACTIVATE_ICON: u32 = WM_APP + 5;
const KEY_SELECT: u32 = NIN_SELECT | 1;
static EXPLORER_STARTED: OnceLock<u32> = OnceLock::new();

pub struct Tray {
    window: HWND,
}

impl Tray {
    pub fn menu_anchor(&self, at: WPARAM) -> POINT {
        let mut point = POINT {
            x: at as i16 as i32,
            y: (at >> 16) as i16 as i32,
        };
        unsafe {
            if point.x == -1 && point.y == -1 {
                let icon = NOTIFYICONIDENTIFIER {
                    cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
                    hWnd: self.window,
                    uID: 1,
                    ..Default::default()
                };
                let mut bounds = RECT::default();
                if Shell_NotifyIconGetRect(&icon, &mut bounds) >= 0 {
                    point.x = (bounds.left + bounds.right) / 2;
                    point.y = (bounds.top + bounds.bottom) / 2;
                } else {
                    GetCursorPos(&mut point);
                }
            }
        }
        point
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &icon_data(self.window, ptr::null_mut()));
            DestroyWindow(self.window);
        }
    }
}

fn icon_data(window: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        hIcon: icon,
        uCallbackMessage: CALLBACK,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
        ..Default::default()
    };
    for (dst, ch) in data.szTip.iter_mut().zip("Uplink".encode_utf16()) {
        *dst = ch;
    }
    data
}

fn register(window: HWND, icon: HICON) -> io::Result<()> {
    let mut data = icon_data(window, icon);
    unsafe {
        if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
            return Err(io::Error::last_os_error());
        }
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        if Shell_NotifyIconW(NIM_SETVERSION, &data) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

unsafe extern "system" fn messages(window: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        crate::control::UPDATE_CHECK..=crate::control::UPDATE_AUTO => {
            return unsafe { PostThreadMessageW(GetCurrentThreadId(), msg, 0, 0) } as isize;
        }
        crate::control::SHOW | crate::control::HIDE | crate::control::QUIT => {
            unsafe {
                PostThreadMessageW(
                    GetCurrentThreadId(),
                    match msg {
                        crate::control::SHOW => OPEN,
                        crate::control::QUIT => QUIT,
                        _ => msg,
                    },
                    0,
                    0,
                );
            }
            return 1;
        }
        crate::control::WINDOW => {
            let pid = crate::window_process::CHILD_PID.load(std::sync::atomic::Ordering::Relaxed);
            return crate::window_process::visible_window(pid).unwrap_or(ptr::null_mut()) as isize;
        }
        crate::control::ICON => return apply_icon(window).is_ok() as isize,
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            let _ = apply_icon(window);
        }
        _ => {}
    }
    if msg == CALLBACK {
        // В версии 4 клавиатура и средства доступности активируют значок через NIN_*.
        match lp as u16 as u32 {
            NIN_SELECT | KEY_SELECT | WM_LBUTTONUP | WM_LBUTTONDBLCLK => unsafe {
                PostThreadMessageW(GetCurrentThreadId(), ACTIVATE_ICON, 0, 0);
            },
            WM_CONTEXTMENU => unsafe {
                PostThreadMessageW(GetCurrentThreadId(), CONTEXT_MENU, wp, 0);
            },
            _ => {}
        }
        return 0;
    }
    if EXPLORER_STARTED.get() == Some(&msg) {
        let icon = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as HICON;
        if !icon.is_null() {
            let _ = register(window, icon);
        }
        return 0;
    }
    unsafe { DefWindowProcW(window, msg, wp, lp) }
}

pub fn install() -> io::Result<Tray> {
    // Скрытое верхнеуровневое окно принимает также TaskbarCreated при перезапуске Explorer.
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        EXPLORER_STARTED.get_or_init(|| RegisterWindowMessageW(w!("TaskbarCreated")));
        let class = WNDCLASSW {
            lpfnWndProc: Some(messages),
            hInstance: instance,
            lpszClassName: crate::control::class(),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(io::Error::last_os_error());
        }
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.lpszClassName,
            w!("Uplink Tray"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        if window.is_null() {
            return Err(io::Error::last_os_error());
        }
        let tray = Tray { window };
        let icon = choose_icon()?;
        SetWindowLongPtrW(window, GWLP_USERDATA, icon as isize);
        register(window, icon)?;
        Ok(tray)
    }
}

pub fn light_taskbar() -> bool {
    use windows_sys::Win32::System::Registry::*;
    let mut value = 0u32;
    let mut size = 4;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        ) == 0
            && value != 0
    }
}

fn ping_icon(ms: Option<u32>) -> usize {
    match uplink::peers::signal_bars(ms) {
        3 => 1,
        2 => 5,
        1 => 6,
        _ => 4,
    }
}

fn choose_icon() -> io::Result<HICON> {
    let path = crate::settings::path()?;
    let mode = crate::settings::tray_mode(&path)?;
    let options = crate::settings::options(&path)?;
    let light = light_taskbar();
    let id = match mode.as_str() {
        _ if !options.enabled => 4,
        _ if options.health == "critical" => 6,
        _ if options.health == "warning" => 5,
        "system" => {
            if light {
                2
            } else {
                3
            }
        }
        "ping" => ping_icon(
            crate::settings::rooms(&path)?
                .first()
                .and_then(|r| r.server_ms),
        ),
        _ => 1,
    };
    let id = match (light, id) {
        (true, 1) => 7,
        (true, 4) => 8,
        (true, 5) => 9,
        (true, 6) => 10,
        _ => id,
    };
    let icon = unsafe {
        LoadImageW(
            GetModuleHandleW(ptr::null()),
            id as *const u16,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_SHARED,
        )
    };
    if icon.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(icon)
    }
}

fn apply_icon(window: HWND) -> io::Result<()> {
    let icon = choose_icon()?;
    unsafe {
        if Shell_NotifyIconW(NIM_MODIFY, &icon_data(window, icon)) == 0 {
            return Err(io::Error::last_os_error());
        }
        SetWindowLongPtrW(window, GWLP_USERDATA, icon as isize);
    }
    Ok(())
}
