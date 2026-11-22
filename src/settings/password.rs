use base64::{Engine, engine::general_purpose::STANDARD};
use std::{io, ptr, slice};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};

fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    }
}

fn transform(network: &str, bytes: &[u8], encrypt: bool) -> io::Result<Vec<u8>> {
    let input = blob(bytes);
    let context = blob(network.as_bytes());
    let mut output = CRYPT_INTEGER_BLOB::default();
    // DPAPI привязывает секрет к пользователю Windows, дополнительный контекст — к сети.
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                ptr::null(),
                &context,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                &context,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = if output.cbData == 0 {
        Vec::new()
    } else {
        unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() }
    };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}

pub(super) fn seal(network: &str, password: &str) -> io::Result<String> {
    transform(network, password.as_bytes(), true)
        .map(|bytes| STANDARD.encode(bytes))
        .map_err(|_| io::Error::other("Could not save the password on this PC."))
}

pub(super) fn open(network: &str, sealed: &str) -> io::Result<String> {
    let failed = || io::Error::other("Could not read the password saved on this PC.");
    let bytes = STANDARD.decode(sealed).map_err(|_| failed())?;
    String::from_utf8(transform(network, &bytes, false).map_err(|_| failed())?)
        .map_err(|_| failed())
}
