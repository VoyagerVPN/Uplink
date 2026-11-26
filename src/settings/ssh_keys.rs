use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        DACL_SECURITY_INFORMATION, GetTokenInformation, PROTECTED_DACL_SECURITY_INFORMATION,
        SetFileSecurityW, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{CREATE_NO_WINDOW, GetCurrentProcess, OpenProcessToken},
};

fn profile() -> io::Result<PathBuf> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Profile, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath},
    };
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Profile, KNOWN_FOLDER_FLAG(0), None)
            .map_err(io::Error::other)?;
        let value = std::ffi::OsString::from_wide(path.as_wide());
        CoTaskMemFree(Some(path.0.cast()));
        Ok(PathBuf::from(value))
    }
}

fn regular(path: &Path, directory: bool) -> io::Result<bool> {
    const REPARSE_POINT: u32 = 0x400;
    match fs::symlink_metadata(path) {
        Ok(meta)
            if meta.file_attributes() & REPARSE_POINT == 0
                && (if directory {
                    meta.is_dir()
                } else {
                    meta.is_file()
                }) =>
        {
            Ok(true)
        }
        Ok(_) => Err(io::Error::other(
            "Unexpected redirected Uplink SSH directory or key.",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn create_dir(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    if regular(path, true)? {
        Ok(())
    } else {
        Err(io::ErrorKind::NotFound.into())
    }
}

fn restrict(path: &Path, directory: bool) -> io::Result<()> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut size = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut size,
        );
        if size == 0
            || io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
        {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            size,
            &mut size,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut sid = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let current_user = windows::core::PCWSTR(sid).to_string();
        LocalFree(sid.cast());
        let current_user = current_user.map_err(io::Error::other)?;
        let inherit = if directory { "OICI" } else { "" };
        let descriptor: Vec<u16> =
            format!("D:P(A;{inherit};FA;;;SY)(A;{inherit};FA;;;{current_user})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
        let mut acl = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut acl,
            ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let saved = SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            acl,
        );
        let error = if saved == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        };
        LocalFree(acl);
        error
    }
}

fn key_command() -> io::Result<Command> {
    let binary = std::env::var_os("SystemRoot").ok_or(io::ErrorKind::NotFound)?;
    let binary = PathBuf::from(binary).join("System32/OpenSSH/ssh-keygen.exe");
    if !binary.is_file() {
        return Err(io::Error::other(
            "Install Windows OpenSSH Client to create Uplink SSH keys.",
        ));
    }
    let mut command = Command::new(binary);
    command
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    Ok(command)
}

fn public_key(path: &Path) -> io::Result<String> {
    let output = key_command()?.args(["-y", "-f"]).arg(path).output()?;
    if !output.status.success() {
        return Err(io::Error::other("Could not read the Uplink SSH key."));
    }
    let line = std::str::from_utf8(&output.stdout).map_err(io::Error::other)?;
    let mut parts = line.split_ascii_whitespace();
    let kind = parts
        .next()
        .filter(|kind| *kind == "ssh-ed25519")
        .ok_or(io::ErrorKind::InvalidData)?;
    let encoded = parts.next().ok_or(io::ErrorKind::InvalidData)?;
    let blob = STANDARD.decode(encoded).map_err(io::Error::other)?;
    if blob.len() != 51 || !blob.starts_with(b"\0\0\0\x0bssh-ed25519\0\0\0\x20") {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(format!("{kind} {encoded}"))
}

fn location(home: &Path) -> io::Result<PathBuf> {
    if !home.is_absolute() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(home
        .join(".ssh")
        .join(crate::service_control::product()?.to_ascii_lowercase()))
}

fn filename(network: &str, own: &str, peer: &str) -> String {
    let mut hash = Sha256::new();
    for part in [network, own, peer] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    format!("{:x}.key", hash.finalize())
}

fn ensure(root: &Path, file: &str) -> io::Result<String> {
    create_dir(root.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
    create_dir(root)?;
    restrict(root, true)?;
    let destination = root.join(file);
    if !regular(&destination, false)? {
        let staged = tempfile::Builder::new()
            .prefix(".key-")
            .rand_bytes(16)
            .tempdir_in(root)?;
        let identity = staged.path().join("identity");
        let key = key_command()?
            .args(["-q", "-t", "ed25519", "-N", "", "-C", "Uplink", "-f"])
            .arg(&identity)
            .output()?;
        if !key.status.success() {
            return Err(io::Error::other("Could not create an Uplink SSH key."));
        }
        restrict(&identity, false)?;
        match fs::hard_link(&identity, &destination) {
            Ok(()) => (),
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists
                    && regular(&destination, false)? =>
            {
                ()
            }
            Err(error) => return Err(error),
        }
    }
    public_key(&destination)
}

pub(super) fn for_request(network: &str, own: &str, peer: &str) -> io::Result<String> {
    ensure(&location(&profile()?)?, &filename(network, own, peer))
}

pub(super) fn forget(network: &str, own: &str, peer: &str, expected: &str) -> io::Result<()> {
    let root = location(&profile()?)?;
    forget_at(&root, &filename(network, own, peer), expected)
}

fn forget_at(root: &Path, file: &str, expected: &str) -> io::Result<()> {
    if !regular(root.parent().ok_or(io::ErrorKind::InvalidInput)?, true)? || !regular(root, true)? {
        return Ok(());
    }
    let path = root.join(file);
    if regular(&path, false)? && public_key(&path)? == expected {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
#[test]
fn separate_peer_keys_survive_retry_and_rotate_after_the_matching_revocation() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join(".ssh/uplink");
    let slot = filename("network-a", "self", "friend");
    let other_slot = filename("network-b", "self", "friend");
    let first = ensure(&store, &slot).unwrap();
    let other = ensure(&store, &other_slot).unwrap();
    assert!(first.starts_with("ssh-ed25519 "));
    assert_ne!(first, other);
    assert_eq!(ensure(&store, &slot).unwrap(), first);
    forget_at(&store, &slot, &other).unwrap();
    assert_eq!(ensure(&store, &slot).unwrap(), first);
    forget_at(&store, &slot, &first).unwrap();
    assert!(!store.join(&slot).exists());
    let fresh = ensure(&store, &slot).unwrap();
    assert_ne!(fresh, first);
    forget_at(&store, &slot, &first).unwrap();
    assert_eq!(ensure(&store, &slot).unwrap(), fresh);
    assert_eq!(ensure(&store, &other_slot).unwrap(), other);
}
