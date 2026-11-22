use super::SetupWindow;
use slint::{CloseRequestResponse, ComponentHandle};
use std::path::Path;
use std::{
    cell::Cell, io, os::windows::process::CommandExt, path::PathBuf, process::Command, rc::Rc,
};

fn close_client() -> io::Result<()> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::WAIT_OBJECT_0, System::Threading::*, UI::WindowsAndMessaging::*,
    };
    let window = uplink::control::tray_window();
    if window.is_null() {
        return Ok(());
    }
    unsafe {
        let mut pid = 0;
        let thread = GetWindowThreadProcessId(window, &mut pid);
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | 0x00100000, 0, pid);
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let _owner = OwnedHandle::from_raw_handle(handle);
        let mut path = vec![0u16; 32768];
        let mut size = path.len() as u32;
        if QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut size) == 0
            || !Path::new(&String::from_utf16_lossy(&path[..size as usize]))
                .file_name()
                .is_some_and(|s| s.eq_ignore_ascii_case("uplink.exe"))
        {
            return Err(io::Error::other("Unknown tray process"));
        }
        if GetWindowThreadProcessId(window, std::ptr::null_mut()) != thread
            || PostThreadMessageW(thread, WM_QUIT, 0, 0) == 0
            || WaitForSingleObject(handle, 8000) != WAIT_OBJECT_0
        {
            return Err(io::Error::other("Uplink is still running"));
        }
    }
    Ok(())
}

fn zerotier_options(product: &str) -> (bool, bool) {
    if product != "Uplink" {
        return (false, false);
    }
    let Some(data) = std::env::var_os("ProgramData") else {
        return (false, false);
    };
    let dir = PathBuf::from(data).join("ZeroTier/One");
    let receipt = std::fs::read_to_string(dir.join(".uplink-owner")).ok();
    let owned = receipt.is_some_and(|sid| {
        let Some(root) = std::env::var_os("SystemRoot") else {
            return false;
        };
        Command::new(PathBuf::from(root).join("System32/whoami.exe"))
            .args(["/user", "/fo", "csv", "/nh"])
            .creation_flags(0x08000000)
            .output()
            .is_ok_and(|out| {
                out.status.success()
                    && String::from_utf8_lossy(&out.stdout)
                        .trim()
                        .trim_end_matches('"')
                        .rsplit('"')
                        .next()
                        == Some(sid.as_str())
            })
    });
    (
        dir.join("zerotier-one_x64.exe").is_file()
            || dir.join(".uplink-removing").is_file()
            || owned,
        owned,
    )
}

fn choose_folder(start: &Path) -> windows::core::Result<Option<PathBuf>> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::*;
    use windows::core::HSTRING;
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let result = (|| {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(
                dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR,
            )?;
            if let Some(dir) = start.ancestors().find(|p| p.is_dir()) {
                if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(
                    &HSTRING::from(dir.as_os_str()),
                    None,
                ) {
                    dialog.SetFolder(&item)?;
                }
            }
            let owner = windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow();
            if let Err(e) = dialog.Show(Some(HWND(owner))) {
                if e.code() == ERROR_CANCELLED.to_hresult() {
                    return Ok(None);
                }
                return Err(e);
            }
            let path = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let selected = std::ffi::OsString::from_wide(path.as_wide());
            CoTaskMemFree(Some(path.0.cast()));
            Ok(Some(PathBuf::from(selected)))
        })();
        CoUninitialize();
        result
    }
}

pub fn run(args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
    let [mode, engine, directory, version, product] = args else {
        return Err(io::Error::other("Invalid setup arguments").into());
    };
    let removing = mode == "remove";
    if !matches!(
        mode.as_str(),
        "install" | "reinstall" | "update" | "downgrade" | "remove"
    ) {
        return Err(io::Error::other("Invalid setup mode").into());
    }
    let engine = PathBuf::from(engine);
    let directory = PathBuf::from(directory);
    let ui = SetupWindow::new()?;
    let language = uplink::i18n::current();
    slint::select_bundled_translation(&language).ok();
    ui.set_version(version.as_str().into());
    let root = directory
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(&directory);
    ui.set_destination(root.display().to_string().into());
    if removing {
        let (present, owned) = zerotier_options(product);
        ui.set_has_zerotier(present);
        ui.set_remove_zerotier(owned);
    }
    ui.set_removing(removing);
    ui.set_mode(mode.as_str().into());
    let rc = Rc::new(Cell::new(1));
    let result = rc.clone();
    let dir = directory.clone();
    let chooser = ui.as_weak();
    ui.on_choose_folder(move || {
        if let Some(ui) = chooser.upgrade() {
            let current = ui.get_destination();
            match choose_folder(Path::new(current.as_str())) {
                Ok(Some(folder)) => {
                    ui.set_destination(folder.display().to_string().into());
                    ui.set_failure("".into());
                }
                Ok(None) => (),
                Err(_) => ui.set_failure(
                    uplink::i18n::message(
                        &uplink::i18n::current(),
                        "Could not open the folder picker.",
                    )
                    .into(),
                ),
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_proceed(move || {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_busy() || ui.get_done() {
            return;
        }
        let consent = ui.get_confirm_close();
        if !consent && !uplink::control::tray_window().is_null() {
            ui.set_failure("".into());
            ui.set_confirm_close(true);
            return;
        }
        ui.set_confirm_close(false);
        ui.set_busy(true);
        ui.set_failure("".into());
        let weak = ui.as_weak();
        let (engine, dir) = (engine.clone(), dir.clone());
        let lang = language.clone();
        let purge = ui.get_purge();
        let remove_zerotier = ui.get_remove_zerotier();
        let target = ui.get_destination().to_string();
        std::thread::spawn(move || {
            let status = if consent && close_client().is_err() {
                Some(22)
            } else {
                let mut child = Command::new(engine);
                child.arg("/S").creation_flags(0x08000000);
                if removing {
                    if purge {
                        child.arg("/PURGEDATA");
                    }
                    if remove_zerotier {
                        child.arg("/REMOVEZEROTIER");
                    }
                    child.raw_arg(format!("_?={}", dir.display()));
                } else {
                    child.raw_arg(format!("/D={target}"));
                }
                child.status().ok().and_then(|s| s.code())
            };
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                if status == Some(0) {
                    ui.set_done(true);
                } else {
                    let detail = match status {
                        Some(21) => "Another application owns the invitation links.",
                        Some(22) => "Uplink files are in use.",
                        Some(23) => "Another setup is running.",
                        Some(29) => "Restart Windows to finish removing ZeroTier.",
                        Some(28) => "Could not clean up Uplink data or ZeroTier.",
                        _ => "Check access to the installation folder.",
                    };
                    ui.set_failure(uplink::i18n::message(&lang, detail).into());
                }
            });
        });
    });
    let weak = ui.as_weak();
    ui.on_dismiss(move || {
        if let Some(ui) = weak.upgrade() {
            if !ui.get_busy() {
                result.set(if ui.get_done() { 0 } else { 1 });
                let _ = ui.hide();
            }
        }
    });
    let weak = ui.as_weak();
    let result = rc.clone();
    ui.on_launch(move || {
        if let Some(ui) = weak.upgrade() {
            let package = Path::new(ui.get_destination().as_str())
                .join("packages")
                .join(directory.file_name().unwrap_or_default());
            if Command::new(package.join("uplink.exe")).spawn().is_ok() {
                result.set(0);
                let _ = ui.hide();
            }
        }
    });
    let weak = ui.as_weak();
    let result = rc.clone();
    ui.window().on_close_requested(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_busy() {
                return CloseRequestResponse::KeepWindowShown;
            }
            if ui.get_done() {
                result.set(0);
            }
        }
        CloseRequestResponse::HideWindow
    });
    ui.run()?;
    Ok(rc.get())
}
