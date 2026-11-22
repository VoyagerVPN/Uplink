use slint::winit_030::winit::dpi::PhysicalPosition;
use windows_sys::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::GetCursorPos,
    },
};

fn center([left, top, right, bottom]: [i32; 4], size: [i32; 2], dpi: u32) -> [i32; 2] {
    let px = |dp| (dp * dpi as i32 + 48) / 96;
    [
        left + (right - left - px(size[0])).max(0) / 2,
        top + (bottom - top - px(size[1])).max(0) / 2,
    ]
}

pub fn configure(size: [i32; 2]) -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_window_attributes_hook(move |attrs| {
            let mut point = POINT::default();
            unsafe { GetCursorPos(&mut point) };
            let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
                return attrs;
            }
            let (mut xdpi, mut ydpi) = (96, 96);
            unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut xdpi, &mut ydpi) };
            let w = info.rcWork;
            let [x, y] = center([w.left, w.top, w.right, w.bottom], size, xdpi);
            attrs.with_position(PhysicalPosition::new(x, y))
        })
        .select()
}

#[cfg(test)]
#[test]
fn centers_in_the_monitor_work_area_at_its_dpi() {
    assert_eq!(center([-1920, 40, 0, 1080], [388, 540], 144), [-1251, 155]);
    assert_eq!(center([10, 20, 330, 220], [388, 252], 96), [10, 20]);
}
