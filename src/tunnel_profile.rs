use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io,
    net::{Ipv4Addr, SocketAddrV4},
    ptr,
};
use windows_sys::Win32::{Foundation::*, System::Registry::*};

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub ip: Ipv4Addr,
    pub endpoint: SocketAddrV4,
    pub credential: String,
}

impl Profile {
    pub fn config(&self) -> io::Result<String> {
        let ip = self.ip;
        let peer = self.endpoint.ip();
        if self.id.len() != 32
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !ip.is_private()
            || !(2..=254).contains(&ip.octets()[3])
            || peer.is_unspecified()
            || peer.is_multicast()
            || peer.is_broadcast()
            || self.endpoint.port() == 0
            || !STANDARD
                .decode(&self.credential)
                .is_ok_and(|bytes| bytes.len() == 32 && STANDARD.encode(&bytes) == self.credential)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid Uplink network profile",
            ));
        }
        let id = &self.id;
        let uuid = uuid::Uuid::parse_str(id).map_err(io::Error::other)?;
        let endpoint = self.endpoint;
        let credential = &self.credential;
        Ok(format!(
            r#"instance_name = "{id}"
instance_id = "{uuid}"
ipv4 = "{ip}/24"
dhcp = false
listeners = []
stun_servers = []
routes = []
[[peer]]
uri = "udp://{endpoint}"
[network_identity]
network_name = "{id}"
network_secret = ""
[secure_mode]
enabled = true
local_private_key = "{credential}"
[flags]
dev_name = "Uplink"
private_mode = true
enable_ipv6 = false
disable_upnp = true
bind_device = false
"#
        ))
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn check(code: u32) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}
fn subkey() -> io::Result<String> {
    Ok(format!(
        "Software\\{}\\Networks",
        crate::service_control::product()?
    ))
}

pub fn save(profile: &Profile) -> io::Result<()> {
    profile.config()?;
    let data = serde_json::to_vec(profile)?;
    check(unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            wide(&subkey()?).as_ptr(),
            wide(&profile.id).as_ptr(),
            REG_BINARY,
            data.as_ptr().cast(),
            data.len() as u32,
        )
    })
}

pub fn remove(id: &str) -> io::Result<()> {
    let code = unsafe {
        RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            wide(&subkey()?).as_ptr(),
            wide(id).as_ptr(),
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        check(code)
    }
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

// Служба читает только значения фиксированного размера и формирует конфигурацию сама.
pub fn loaded() -> io::Result<BTreeMap<String, Profile>> {
    let mut profiles = BTreeMap::new();
    let subkey = subkey()?;
    for i in 0..256 {
        let mut sid = [0u16; 256];
        let mut n = sid.len() as u32;
        let code = unsafe {
            RegEnumKeyExW(
                HKEY_USERS,
                i,
                sid.as_mut_ptr(),
                &mut n,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if code == ERROR_NO_MORE_ITEMS {
            break;
        }
        check(code)?;
        let sid = String::from_utf16_lossy(&sid[..n as usize]);
        if !sid.starts_with("S-1-") || !sid[2..].bytes().all(|b| b.is_ascii_digit() || b == b'-') {
            continue;
        }
        let mut raw = ptr::null_mut();
        let code = unsafe {
            RegOpenKeyExW(
                HKEY_USERS,
                wide(&format!("{sid}\\{subkey}")).as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut raw,
            )
        };
        if matches!(
            code,
            ERROR_FILE_NOT_FOUND | ERROR_ACCESS_DENIED | ERROR_KEY_DELETED
        ) {
            continue;
        }
        check(code)?;
        let key = Key(raw);
        for index in 0..128 {
            let mut name = [0u16; 64];
            let mut data = [0u8; 1024];
            let (mut n, mut size, mut ty) = (name.len() as u32, data.len() as u32, 0);
            let code = unsafe {
                RegEnumValueW(
                    key.0,
                    index,
                    name.as_mut_ptr(),
                    &mut n,
                    ptr::null(),
                    &mut ty,
                    data.as_mut_ptr(),
                    &mut size,
                )
            };
            if code == ERROR_NO_MORE_ITEMS || code == ERROR_KEY_DELETED {
                break;
            }
            if code != 0 || ty != REG_BINARY {
                continue;
            }
            let Ok(profile) = serde_json::from_slice::<Profile>(&data[..size as usize]) else {
                continue;
            };
            if profile.id != String::from_utf16_lossy(&name[..n as usize]) {
                continue;
            }
            if profile.config().is_ok() {
                profiles.entry(profile.id.clone()).or_insert(profile);
            }
        }
    }
    Ok(profiles)
}
