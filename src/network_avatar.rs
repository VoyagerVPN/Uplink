use crate::transport::Membership;
use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use image::{ImageFormat, RgbaImage};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const PIXELS: usize = 512 * 512 * 4;

fn invalid() -> io::Error {
    io::Error::other("Invalid network avatar")
}

pub fn encode(pixels: &[u8]) -> io::Result<String> {
    if pixels.len() != PIXELS {
        return Err(invalid());
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(pixels)?;
    Ok(STANDARD.encode(encoder.finish()?))
}

impl Membership {
    fn image_path(&self, prefs: &Path, revision: &str) -> Option<PathBuf> {
        if revision.len() != 64 || !revision.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let network = Sha256::digest(self.key().as_bytes());
        Some(
            prefs
                .with_file_name("avatars")
                .join(format!("{network:x}"))
                .join(format!("{revision}.png")),
        )
    }

    pub fn avatar_file(&self, prefs: &Path) -> Option<PathBuf> {
        self.image_path(prefs, self.avatar_revision.as_deref()?)
    }

    pub(crate) fn known_avatar(&self, prefs: &Path) -> &str {
        if self.avatar_file(prefs).is_some_and(|p| p.is_file()) {
            self.avatar_revision.as_deref().unwrap_or_default()
        } else {
            ""
        }
    }

    pub(crate) fn apply_avatar(&mut self, prefs: &Path, data: &Value) -> io::Result<()> {
        let previous = self.avatar_file(prefs);
        let revision = data
            .get("avatar_revision")
            .map(|r| r.as_str().ok_or_else(invalid))
            .transpose()?;
        if let Some(rev) = revision.filter(|r| !r.is_empty()) {
            let path = self.image_path(prefs, rev).ok_or_else(invalid)?;
            if !path.is_file() {
                let value = data["avatar"]
                    .as_str()
                    .filter(|s| s.len() <= 1_410_000)
                    .ok_or_else(invalid)?;
                let bytes = STANDARD.decode(value).map_err(|_| invalid())?;
                if format!("{:x}", Sha256::digest(&bytes)) != rev {
                    return Err(invalid());
                }
                let mut pixels = Vec::new();
                ZlibDecoder::new(bytes.as_slice())
                    .take(PIXELS as u64 + 1)
                    .read_to_end(&mut pixels)?;
                if pixels.len() != PIXELS {
                    return Err(invalid());
                }
                let image = RgbaImage::from_raw(512, 512, pixels).ok_or_else(invalid)?;
                let dir = path.parent().ok_or_else(invalid)?;
                fs::create_dir_all(dir)?;
                let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
                image
                    .write_to(tmp.as_file_mut(), ImageFormat::Png)
                    .map_err(io::Error::other)?;
                tmp.persist(&path)?;
            }
        }
        self.avatar_revision = revision.map(str::to_owned);
        if let Some(old) = previous.filter(|p| Some(p) != self.avatar_file(prefs).as_ref()) {
            let _ = fs::remove_file(old);
        }
        Ok(())
    }
}
