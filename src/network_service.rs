#![windows_subsystem = "windows"]

mod discovery;

use std::{
    collections::BTreeMap,
    ffi::c_void,
    io::{self, Write},
    mem,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    path::Path,
    process::{Child, Command, Stdio},
    ptr,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{ERROR_CALL_NOT_IMPLEMENTED, ERROR_GEN_FAILURE},
    System::{JobObjects::*, Services::*, Threading::CREATE_NO_WINDOW},
};

static STOP: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn control(code: u32, _: u32, _: *mut c_void, _: *mut c_void) -> u32 {
    match code {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => STOP.store(true, Ordering::Release),
        SERVICE_CONTROL_INTERROGATE => (),
        _ => return ERROR_CALL_NOT_IMPLEMENTED,
    }
    0
}

fn report(handle: SERVICE_STATUS_HANDLE, state: u32, error: u32) -> io::Result<()> {
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        dwWin32ExitCode: error,
        dwWaitHint: if matches!(state, SERVICE_START_PENDING | SERVICE_STOP_PENDING) {
            5000
        } else {
            0
        },
        ..Default::default()
    };
    if unsafe { SetServiceStatus(handle, &status) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

struct Engine {
    process: Child,
    _job: OwnedHandle,
}

impl Engine {
    fn start(directory: &Path) -> io::Result<Self> {
        let job = unsafe {
            let handle = CreateJobObjectW(ptr::null(), ptr::null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            OwnedHandle::from_raw_handle(handle)
        };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            if SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                mem::size_of_val(&limits) as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let mut process = Command::new(directory.join("uplink-engine.exe"))
            .current_dir(directory)
            .env_clear()
            .env(
                "SystemRoot",
                std::env::var_os("SystemRoot").ok_or(io::ErrorKind::NotFound)?,
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) } == 0 {
            let error = io::Error::last_os_error();
            let _ = process.kill();
            let _ = process.wait();
            return Err(error);
        }
        Ok(Self { process, _job: job })
    }
}

fn serve(handle: SERVICE_STATUS_HANDLE) -> io::Result<()> {
    report(handle, SERVICE_START_PENDING, 0)?;
    let executable = std::env::current_exe()?;
    let directory = executable.parent().ok_or(io::ErrorKind::NotFound)?;
    let mut discovery = None;
    let mut next_discovery = std::time::Instant::now();
    let mut previous = BTreeMap::new();
    let mut engine: Option<Engine> = None;
    report(handle, SERVICE_RUNNING, 0)?;
    while !STOP.load(Ordering::Acquire) {
        let desired = uplink::tunnel_profile::loaded()?;
        if previous != desired {
            if engine.is_none() {
                engine = Some(Engine::start(directory)?);
            }
            let pipe = engine
                .as_mut()
                .and_then(|e| e.process.stdin.as_mut())
                .ok_or(io::ErrorKind::BrokenPipe)?;
            let configs = desired
                .iter()
                .map(|(id, p)| Ok((id, p.config()?)))
                .collect::<io::Result<BTreeMap<_, _>>>()?;
            let mut snapshot = serde_json::to_vec(&configs)?;
            snapshot.push(b'\n');
            pipe.write_all(&snapshot)?;
            next_discovery = std::time::Instant::now();
            previous = desired;
        }
        if std::time::Instant::now() >= next_discovery {
            let result = discovery::Discovery::refresh(
                &mut discovery,
                directory,
                previous.values().map(|p| p.ip).collect(),
            );
            next_discovery = std::time::Instant::now()
                + Duration::from_secs(if result.is_err() { 15 } else { 1 });
            if result.is_err() {
                discovery.take();
            }
        }
        if let Some(child) = &mut engine {
            if child.process.try_wait()?.is_some() {
                return Err(io::Error::other("Network engine stopped"));
            }
        }
        thread::sleep(Duration::from_millis(350));
    }
    report(handle, SERVICE_STOP_PENDING, 0)?;
    if let Some(Engine { mut process, _job }) = engine {
        drop(_job);
        process.wait()?;
    }
    Ok(())
}

unsafe extern "system" fn service_main(argc: u32, argv: *mut *mut u16) {
    if argc == 0 || argv.is_null() {
        return;
    }
    let handle = unsafe { RegisterServiceCtrlHandlerExW(*argv, Some(control), ptr::null()) };
    if handle.is_null() {
        return;
    }
    let error = serve(handle)
        .err()
        .map(|e| {
            e.raw_os_error()
                .map(|n| n as u32)
                .unwrap_or(ERROR_GEN_FAILURE)
        })
        .unwrap_or(0);
    let _ = report(handle, SERVICE_STOPPED, error);
}

fn main() {
    let mut name: Vec<u16> = uplink::service_control::NAME
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let services = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    if unsafe { StartServiceCtrlDispatcherW(services.as_ptr()) } == 0 {
        std::process::exit(1);
    }
}
