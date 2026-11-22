use std::{
    io, ptr,
    sync::atomic::{AtomicIsize, Ordering},
};
use uplink::control;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::VK_ESCAPE,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

static POPUP: AtomicIsize = AtomicIsize::new(0);

pub struct MenuInput {
    window: HWND,
    mouse: HHOOK,
    keys: HHOOK,
}

impl MenuInput {
    pub fn install(window: HWND) -> io::Result<Self> {
        unsafe {
            if SetWindowSubclass(window, Some(no_activate), 1, 0) == 0 {
                return Err(io::Error::last_os_error());
            }
            let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
            SetWindowLongPtrW(window, GWL_EXSTYLE, style | WS_EX_NOACTIVATE as isize);
            let module = GetModuleHandleW(ptr::null());
            let mut input = Self {
                window,
                mouse: ptr::null_mut(),
                keys: ptr::null_mut(),
            };
            input.mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_down), module, 0);
            if input.mouse.is_null() {
                return Err(io::Error::last_os_error());
            }
            input.keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(escape), module, 0);
            if input.keys.is_null() {
                return Err(io::Error::last_os_error());
            }
            POPUP.store(window as isize, Ordering::Relaxed);
            Ok(input)
        }
    }
}

impl Drop for MenuInput {
    fn drop(&mut self) {
        POPUP.store(0, Ordering::Relaxed);
        unsafe {
            if !self.mouse.is_null() {
                UnhookWindowsHookEx(self.mouse);
            }
            if !self.keys.is_null() {
                UnhookWindowsHookEx(self.keys);
            }
            RemoveWindowSubclass(self.window, Some(no_activate), 1);
        }
    }
}

unsafe extern "system" fn no_activate(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if message == WM_MOUSEACTIVATE {
        return MA_NOACTIVATE as LRESULT;
    }
    unsafe { DefSubclassProc(hwnd, message, wp, lp) }
}

// Пока меню видно, внешние щелчки и Escape нужно наблюдать без перехвата фокуса и ввода.
unsafe extern "system" fn escape(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && matches!(wp as u32, WM_KEYDOWN | WM_SYSKEYDOWN)
        && unsafe { (*(lp as *const KBDLLHOOKSTRUCT)).vkCode } == u32::from(VK_ESCAPE)
    {
        let popup = POPUP.load(Ordering::Relaxed) as HWND;
        unsafe { PostMessageW(popup, WM_CLOSE, 0, 0) };
    }
    unsafe { CallNextHookEx(ptr::null_mut(), code, wp, lp) }
}

unsafe extern "system" fn mouse_down(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && matches!(wp as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN)
    {
        let point = unsafe { (*(lp as *const MSLLHOOKSTRUCT)).pt };
        let under_mouse = unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) } as usize;
        let popup = POPUP.load(Ordering::Relaxed) as usize;
        if popup != 0 && under_mouse != popup && !explorer_tray(under_mouse as HWND) {
            let _ = slint::invoke_from_event_loop(move || {
                if control::send(control::WINDOW).ok() != Some(under_mouse) {
                    unsafe { PostMessageW(popup as HWND, WM_CLOSE, 0, 0) };
                }
            });
        }
    }
    unsafe { CallNextHookEx(ptr::null_mut(), code, wp, lp) }
}

fn explorer_tray(window: HWND) -> bool {
    let mut name = [0u16; 64];
    let length = unsafe { GetClassNameW(window, name.as_mut_ptr(), name.len() as i32) } as usize;
    [
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
        "NotifyIconOverflowWindow",
        "TopLevelWindowForOverflowXamlIsland",
    ]
    .iter()
    .any(|class| name[..length].iter().copied().eq(class.encode_utf16()))
}
