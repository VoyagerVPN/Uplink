use serde_json::{Value, json};
use std::{
    fmt::Write as _,
    fs,
    io::{self, Write},
    os::windows::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
};

fn challenge_digest(node: &str, challenge: &str, session: &str) -> io::Result<String> {
    let body = format!("uplink/membership/v1\n{node}\n{challenge}\n{session}");
    let mut digest = [0u8; 32];
    let mut len = digest.len() as u32;
    let success = unsafe {
        windows_sys::Win32::Security::Cryptography::CryptHashCertificate2(
            windows_sys::Win32::Security::Cryptography::BCRYPT_SHA256_ALGORITHM,
            0,
            std::ptr::null(),
            body.as_ptr(),
            body.len() as u32,
            digest.as_mut_ptr(),
            &mut len,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    Ok(encoded)
}

pub fn sign(home: &Path, node: &str, challenge: &str, session: &str) -> io::Result<Value> {
    use crate::settings::join_log;
    let keys = home.join("uplink-proof");
    let missing = |err: io::Error| {
        if let Some(code) = err.raw_os_error() {
            join_log::number("win32", code.into());
        }
        io::Error::other("Run the Uplink transport installer to verify this device")
    };
    join_log::operation("read_proof_public_key");
    let delegate = fs::read_to_string(keys.join("delegate.public")).map_err(missing)?;
    join_log::operation("read_proof_certificate");
    let certificate = fs::read_to_string(keys.join("certificate")).map_err(missing)?;
    join_log::operation("prepare_proof_challenge");
    let mut msg = tempfile::NamedTempFile::new()?;
    msg.write_all(challenge_digest(node, challenge, session)?.as_bytes())?;
    msg.flush()?;
    join_log::operation("sign_proof_challenge");
    let output = Command::new(home.join("zerotier-one_x64.exe"))
        .args(["-i", "sign"])
        .arg(keys.join("delegate.secret"))
        .arg(msg.path())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .output()?;
    if !output.status.success() {
        if let Some(code) = output.status.code() {
            join_log::number("exit_code", code.into());
        }
        return Err(io::Error::other(
            "Run the Uplink transport installer to verify this device",
        ));
    }
    let signature = std::str::from_utf8(&output.stdout)
        .map_err(io::Error::other)?
        .trim();
    Ok(json!({"delegate":delegate.trim(),"certificate":certificate.trim(),"signature":signature}))
}

#[cfg(test)]
mod tests {
    #[test]
    fn delegate_signs_an_enrollment_with_the_installed_zerotier() -> std::io::Result<()> {
        use std::os::windows::process::CommandExt;

        let installed =
            std::path::PathBuf::from(std::env::var_os("ProgramData").unwrap_or_default())
                .join("ZeroTier/One/zerotier-one_x64.exe");
        if !installed.is_file() {
            return Ok(());
        }
        let scratch = tempfile::tempdir()?;
        let home = scratch.path();
        let keys = home.join("uplink-proof");
        std::fs::create_dir(&keys)?;
        let binary = home.join("zerotier-one_x64.exe");
        std::fs::copy(installed, &binary)?;
        let run = |args: &[&str]| -> std::io::Result<String> {
            let output = std::process::Command::new(&binary)
                .arg("-i")
                .args(args)
                .current_dir(&keys)
                .creation_flags(0x08000000)
                .output()?;
            assert!(output.status.success(), "ZeroTier test operation failed");
            Ok(String::from_utf8(output.stdout).unwrap().trim().to_owned())
        };
        run(&["generate", "node.secret", "node.public"])?;
        run(&["generate", "delegate.secret", "delegate.public"])?;
        let delegate = std::fs::read_to_string(keys.join("delegate.public"))?;
        std::fs::write(
            keys.join("binding"),
            format!("uplink/delegate/v1\n{}", delegate.trim()),
        )?;
        let certificate = run(&["sign", "node.secret", "binding"])?;
        std::fs::write(keys.join("certificate"), &certificate)?;
        let proof = super::sign(home, "0123456789", &"a".repeat(64), &"b".repeat(64))?;
        assert_eq!(proof["certificate"].as_str(), Some(certificate.as_str()));
        assert_eq!(proof["delegate"].as_str(), Some(delegate.trim()));
        std::fs::write(
            keys.join("expected"),
            "9e81ba2c97dd0ddf6a95f4fdcb91fde07058fa112c3a7d7cab00193c74a0e352",
        )?;
        run(&[
            "verify",
            "delegate.public",
            "expected",
            proof["signature"].as_str().unwrap(),
        ])?;
        Ok(())
    }

    #[test]
    fn digest_matches_the_server_challenge_encoding() {
        let digest =
            super::challenge_digest("0123456789", &"a".repeat(64), &"b".repeat(64)).unwrap();
        assert_eq!(
            digest,
            "9e81ba2c97dd0ddf6a95f4fdcb91fde07058fa112c3a7d7cab00193c74a0e352"
        );
    }
}
