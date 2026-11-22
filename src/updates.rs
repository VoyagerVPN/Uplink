fn version(s: &str) -> Option<[u16; 3]> {
    let mut parts = s.split('.');
    let mut out = [0; 3];
    for n in &mut out {
        let part = parts.next()?;
        *n = part.parse().ok()?;
        if n.to_string() != part {
            return None;
        }
    }
    parts.next().is_none().then_some(out)
}
pub fn newer_stable(current: &str, tag: &str) -> bool {
    version(current)
        .zip(tag.strip_prefix('v').and_then(version))
        .is_some_and(|(a, b)| b > a)
}

use base64::{Engine, engine::general_purpose::STANDARD};
use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const REPO: &str = "VoyagerVPN/Uplink";
const MAX_PACKAGE: u64 = 128 * 1024 * 1024;
const SETUP: &str = "Uplink-Setup.exe";
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid Uplink update")
}
#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    product: String,
    pub version: String,
    size: u64,
    sha256: String,
    client_sha256: String,
    pub notes: String,
}
#[derive(Deserialize)]
struct Envelope {
    payload: String,
    signature: String,
}

fn verified(bytes: &[u8], public: &[u8], current: &str, tag: &str) -> io::Result<Release> {
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let payload = STANDARD.decode(envelope.payload).map_err(|_| invalid())?;
    let signature = STANDARD.decode(envelope.signature).map_err(|_| invalid())?;
    VerifyingKey::from_sec1_bytes(public)
        .map_err(|_| invalid())?
        .verify(
            &payload,
            &Signature::from_slice(&signature).map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?;
    let release: Release = serde_json::from_slice(&payload).map_err(|_| invalid())?;
    let digest = |s: &str| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    };
    if release.product != REPO
        || !newer_stable(current, tag)
        || tag.strip_prefix('v') != Some(&release.version)
        || !(1..=MAX_PACKAGE).contains(&release.size)
        || !digest(&release.sha256)
        || !digest(&release.client_sha256)
        || release.notes.len() > 32 * 1024
    {
        return Err(invalid());
    }
    Ok(release)
}

fn fetch(url: &str, limit: u64, mut write: impl FnMut(&[u8]) -> io::Result<()>) -> io::Result<()> {
    let system = std::env::var_os("SystemRoot")
        .ok_or_else(|| io::Error::other("Windows directory unavailable"))?;
    let mut child = Command::new(PathBuf::from(system).join("System32/curl.exe"))
        .args([
            "-q",
            "--silent",
            "--fail",
            "--location",
            "--max-redirs",
            "5",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "180",
            "--user-agent",
            concat!("Uplink/", env!("CARGO_PKG_VERSION")),
            "--write-out",
            "%{stderr}%{http_code}",
            "--url",
            url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000)
        .spawn()?;
    let mut stdout = child.stdout.take().unwrap();
    let result = (|| {
        let mut buf = [0; 32768];
        let mut total = 0;
        loop {
            let n = stdout.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > limit {
                return Err(invalid());
            }
            write(&buf[..n])?;
        }
        Ok(())
    })();
    drop(stdout);
    if result.is_err() {
        let _ = child.kill();
    }
    let output = child.wait_with_output()?;
    result?;
    if output.status.success() {
        Ok(())
    } else if output.stderr == b"404" {
        Err(io::ErrorKind::NotFound.into())
    } else {
        Err(io::Error::other("Release service unavailable"))
    }
}
fn small(url: &str, limit: u64) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    fetch(url, limit, |chunk| data.write_all(chunk))?;
    Ok(data)
}
fn asset(tag: &str, name: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{tag}/{name}")
}
pub fn latest(current: &str) -> io::Result<Option<Release>> {
    #[derive(Deserialize)]
    struct GithubRelease {
        tag_name: String,
        draft: bool,
        prerelease: bool,
    }
    let data = match small(
        "https://api.github.com/repos/VoyagerVPN/Uplink/releases/latest",
        512 * 1024,
    ) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        other => other?,
    };
    let gh: GithubRelease = serde_json::from_slice(&data).map_err(|_| invalid())?;
    if gh.draft || gh.prerelease || !newer_stable(current, &gh.tag_name) {
        return Ok(None);
    }
    let key = STANDARD
        .decode(include_str!("../release/public-key.txt").trim())
        .map_err(|_| invalid())?;
    let signed = small(&asset(&gh.tag_name, "uplink-release.json"), 64 * 1024)?;
    verified(&signed, &key, current, &gh.tag_name).map(Some)
}
fn hash(mut file: &File) -> io::Result<String> {
    let mut digest = Sha256::new();
    io::copy(&mut file, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}
fn locked(path: &Path, expected: &str) -> io::Result<File> {
    let f = File::options().read(true).share_mode(1).open(path)?;
    if hash(&f)? != expected {
        return Err(invalid());
    }
    Ok(f)
}
pub struct Package {
    path: tempfile::TempPath,
}
pub fn clean_staging(dir: &Path) {
    if let Ok(files) = fs::read_dir(dir) {
        for item in files.flatten() {
            let name = item.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("uplink-update-")
                && name.ends_with(".exe")
                && item.file_type().is_ok_and(|t| t.is_file())
            {
                let _ = fs::remove_file(item.path());
            }
        }
    }
}
impl Release {
    pub fn download(
        &self,
        directory: &Path,
        mut progress: impl FnMut(u8),
        verifying: impl FnOnce(),
    ) -> io::Result<Package> {
        fs::create_dir_all(directory)?;
        let mut f = tempfile::Builder::new()
            .prefix("uplink-update-")
            .suffix(".exe")
            .tempfile_in(directory)?;
        let mut received = 0;
        let mut previous = 0;
        fetch(
            &asset(&format!("v{}", self.version), SETUP),
            self.size,
            |chunk| {
                f.write_all(chunk)?;
                received += chunk.len() as u64;
                let percent = (100 * received / self.size) as u8;
                if previous != percent {
                    progress(percent);
                    previous = percent;
                }
                Ok(())
            },
        )?;
        if received != self.size {
            return Err(invalid());
        }
        f.as_file().sync_all()?;
        let path = f.into_temp_path();
        verifying();
        locked(&path, &self.sha256)?;
        Ok(Package { path })
    }

    pub fn install(&self, package: &Package) -> io::Result<()> {
        let path: &Path = package.path.as_ref();
        let _verified = locked(path, &self.sha256)?;
        if !Command::new(path)
            .args(["/S", "/UPLINKUPDATE"])
            .creation_flags(0x08000000)
            .status()?
            .success()
        {
            return Err(io::Error::other("Uplink Setup failed"));
        }
        let exe = installed_client()?;
        let _client = locked(&exe, &self.client_sha256)?;
        Command::new(exe)
            .arg("--after-update")
            .arg(std::process::id().to_string())
            .spawn()?;
        Ok(())
    }
}
fn installed_client() -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::{Win32::System::Registry::*, core::w};
    let mut path = vec![0u16; 32768];
    let mut size = (path.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Uplink"),
            w!("InstallLocation"),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            std::ptr::null_mut(),
            path.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    path.truncate((size as usize / 2).saturating_sub(1));
    let dir = PathBuf::from(std::ffi::OsString::from_wide(&path));
    if !dir.is_absolute() {
        return Err(invalid());
    }
    Ok(dir.join("uplink.exe"))
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub phase: Phase,
    pub target: String,
    pub notes: String,
    pub percent: u8,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    Current,
    Found,
    Loading,
    Verifying,
    Ready,
    Restarting,
    Updated,
    Offline,
    Invalid,
    InstallFailed,
}
impl Status {
    pub fn load(profile: &Path) -> io::Result<Self> {
        Ok(serde_json::from_slice(&fs::read(
            profile.with_file_name("updates.json"),
        )?)?)
    }
    pub fn save(&self, profile: &Path) -> io::Result<()> {
        let dir = profile.parent().ok_or_else(invalid)?;
        fs::create_dir_all(dir)?;
        let mut f = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer(&mut f, self)?;
        f.persist(profile.with_file_name("updates.json"))?;
        crate::changed::notify_update()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::{SigningKey, signature::Signer};
    use serde_json::json;

    #[test]
    fn signed_release_binds_notes_package_and_version_to_the_publisher() {
        let key = SigningKey::from_slice(&[7; 32]).unwrap();
        let public = key.verifying_key().to_encoded_point(false);
        let create = |release: serde_json::Value| {
            let payload = serde_json::to_vec(&release).unwrap();
            let sig: Signature = key.sign(&payload);
            json!({"payload":STANDARD.encode(payload),"signature":STANDARD.encode(sig.to_bytes())})
        };
        let release = json!({"product":REPO,"version":"0.1.1","size":100,"sha256":"1".repeat(64),"client_sha256":"2".repeat(64),"notes":"## Improvements\n- Faster network discovery."});
        let signed = create(release.clone());
        let read = |envelope: &serde_json::Value, tag: &str| {
            verified(
                &serde_json::to_vec(envelope).unwrap(),
                public.as_bytes(),
                "0.1.0",
                tag,
            )
        };
        assert_eq!(read(&signed, "v0.1.1").unwrap().size, 100);
        assert!(read(&signed, "v9.0.0").is_err());
        for field in ["notes", "sha256", "version", "product"] {
            let mut changed = release.clone();
            changed[field] = json!("changed");
            let mut altered = signed.clone();
            altered["payload"] = json!(STANDARD.encode(serde_json::to_vec(&changed).unwrap()));
            assert!(read(&altered, "v0.1.1").is_err());
        }
        let mut wrong = release;
        wrong["product"] = json!("another app");
        assert!(read(&create(wrong), "v0.1.1").is_err());
    }

    #[test]
    fn modified_installer_cannot_pass_the_final_launch_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETUP);
        fs::write(&path, b"installer").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"installer"));
        let guard = locked(&path, &expected).unwrap();
        assert!(fs::write(&path, b"rewritten").is_err());
        drop(guard);
        fs::write(&path, b"rewritten").unwrap();
        assert!(locked(&path, &expected).is_err());
    }

    #[test]
    #[ignore = "requires the signed local release candidate"]
    fn publisher_manifest_matches_built_windows_package() {
        let key = STANDARD
            .decode(include_str!("../release/public-key.txt").trim())
            .unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let bytes = fs::read(root.join(".cache/packages/uplink-release.json")).unwrap();
        let r = verified(
            &bytes,
            &key,
            "0.0.0",
            concat!("v", env!("CARGO_PKG_VERSION")),
        )
        .unwrap();
        let pkg = locked(&root.join(".cache/packages").join(SETUP), &r.sha256).unwrap();
        assert_eq!(pkg.metadata().unwrap().len(), r.size);
        locked(
            &root.join(".cache/target/perf/uplink.exe"),
            &r.client_sha256,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join(format!("release/notes/v{}.md", r.version)))
                .unwrap()
                .trim(),
            r.notes
        );
    }
}
