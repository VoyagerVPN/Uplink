use libloading::os::windows::Library;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CString, c_void},
    io,
    net::{Ipv4Addr, UdpSocket},
    path::Path,
    ptr, slice,
    sync::{Arc, RwLock},
    thread,
};
use windows_sys::Win32::{
    NetworkManagement::{IpHelper::*, Ndis::NET_LUID_LH},
    Networking::WinSock::AF_INET,
    System::LibraryLoader::{LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32},
};

type Groups = BTreeMap<Ipv4Addr, BTreeSet<u32>>;
type Handle = isize;
type Open = unsafe extern "C" fn(*const i8, i32, i16, u64) -> Handle;
type Recv = unsafe extern "C" fn(Handle, *mut u8, u32, *mut u32, *mut Address) -> i32;
type SendPacket = unsafe extern "C" fn(Handle, *const u8, u32, *mut u32, *const Address) -> i32;
type Shutdown = unsafe extern "C" fn(Handle, i32) -> i32;
type Close = unsafe extern "C" fn(Handle) -> i32;

// WINDIVERT_ADDRESS версии 2.2, слой NETWORK. Нулевые флаги задают входящее направление.
#[repr(C)]
#[derive(Default)]
struct Address {
    timestamp: i64,
    flags: u32,
    reserved: u32,
    interface: u32,
    rest: [u32; 15],
}

fn overlay_index() -> Option<u32> {
    let mut luid = NET_LUID_LH::default();
    let mut index = 0;
    let alias: Vec<_> = "Uplink\0".encode_utf16().collect();
    unsafe {
        (ConvertInterfaceAliasToLuid(alias.as_ptr(), &mut luid) == 0
            && ConvertInterfaceLuidToIndex(&luid, &mut index) == 0)
            .then_some(index)
    }
}

fn subscriptions(overlay: u32) -> io::Result<Groups> {
    let mut groups = Groups::new();
    let mut table = ptr::null_mut();
    let code = unsafe { GetMulticastIpAddressTable(AF_INET, &mut table) };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    unsafe {
        let rows = slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        for row in rows {
            if row.InterfaceIndex == overlay || row.InterfaceIndex == 1 {
                continue;
            }
            let group = Ipv4Addr::from(row.Address.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes());
            if group.is_multicast() && group.octets()[..3] != [224, 0, 0] {
                groups.entry(group).or_default().insert(row.InterfaceIndex);
            }
        }
        FreeMibTable(table.cast::<c_void>());
    }
    Ok(groups)
}

#[derive(Default)]
struct Delivery {
    groups: Groups,
    locals: Vec<Ipv4Addr>,
}

impl Delivery {
    fn targets(&self, packet: &[u8]) -> Option<&BTreeSet<u32>> {
        if packet.len() < 28
            || packet[0] != 0x45
            || packet[9] != 17
            || u16::from_be_bytes([packet[2], packet[3]]) as usize != packet.len()
            || u16::from_be_bytes([packet[6], packet[7]]) & 0x3fff != 0
            || u16::from_be_bytes([packet[24], packet[25]]) as usize != packet.len() - 20
        {
            return None;
        }
        let source = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
        if self.locals.contains(&source)
            || !(1..=254).contains(&packet[15])
            || !self
                .locals
                .iter()
                .any(|ip| ip.octets()[..3] == packet[12..15])
        {
            return None;
        }
        self.groups.get(&Ipv4Addr::new(
            packet[16], packet[17], packet[18], packet[19],
        ))
    }
}

struct Capture {
    _module: Library,
    shutdown: Shutdown,
    close: Close,
    rx: Handle,
    tx: Handle,
    worker: Option<thread::JoinHandle<()>>,
    delivery: Arc<RwLock<Delivery>>,
}

impl Capture {
    fn start(dir: &Path, index: u32) -> io::Result<Self> {
        let module = unsafe {
            Library::load_with_flags(
                dir.join("WinDivert.dll"),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        }
        .map_err(io::Error::other)?;
        let (open, recv, send, shutdown, close): (Open, Recv, SendPacket, Shutdown, Close) = unsafe {
            (
                *module.get(b"WinDivertOpen").map_err(io::Error::other)?,
                *module.get(b"WinDivertRecv").map_err(io::Error::other)?,
                *module.get(b"WinDivertSend").map_err(io::Error::other)?,
                *module.get(b"WinDivertShutdown").map_err(io::Error::other)?,
                *module.get(b"WinDivertClose").map_err(io::Error::other)?,
            )
        };
        let filter = CString::new(format!(
            "inbound and !impostor and ifIdx == {index} and udp and ip.DstAddr >= 224.0.1.0 and ip.DstAddr <= 239.255.255.255"
        ))?;
        // SNIFF | RECV_ONLY: исходные пакеты продолжают обычную доставку Windows.
        let rx = unsafe { open(filter.as_ptr(), 0, 0, 1 | 4) };
        if rx == -1 {
            return Err(io::Error::last_os_error());
        }
        let tx = unsafe { open(c"false".as_ptr(), 0, 0, 8) };
        if tx == -1 {
            let error = io::Error::last_os_error();
            unsafe { close(rx) };
            return Err(error);
        }
        let mut capture = Self {
            _module: module,
            shutdown,
            close,
            rx,
            tx,
            worker: None,
            delivery: Arc::default(),
        };
        let delivery = capture.delivery.clone();
        capture.worker = Some(
            thread::Builder::new()
                .name("uplink-discovery".into())
                .spawn(move || {
                    let mut buf = vec![0; 65535];
                    let mut addr = Address::default();
                    let mut len = 0;
                    while unsafe {
                        recv(rx, buf.as_mut_ptr(), buf.len() as u32, &mut len, &mut addr)
                    } != 0
                    {
                        let state = delivery.read().unwrap();
                        if let Some(indices) = state.targets(&buf[..len as usize]) {
                            for &interface in indices {
                                let dest = Address {
                                    interface,
                                    ..Address::default()
                                };
                                unsafe { send(tx, buf.as_ptr(), len, ptr::null_mut(), &dest) };
                            }
                        }
                    }
                })?,
        );
        Ok(capture)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        unsafe { (self.shutdown)(self.rx, 1) };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        unsafe {
            (self.close)(self.rx);
            (self.close)(self.tx);
        }
    }
}

pub struct Discovery {
    index: u32,
    membership: UdpSocket,
    joined: BTreeSet<Ipv4Addr>,
    capture: Capture,
}

impl Discovery {
    pub fn refresh(active: &mut Option<Self>, dir: &Path, locals: Vec<Ipv4Addr>) -> io::Result<()> {
        if locals.is_empty() {
            active.take();
            return Ok(());
        }
        let Some(index) = overlay_index() else {
            active.take();
            return Ok(());
        };
        let groups = subscriptions(index)?;
        if active.as_ref().is_some_and(|a| a.index != index) || groups.is_empty() {
            active.take();
        }
        if groups.is_empty() {
            return Ok(());
        }
        if active.is_none() {
            *active = Some(Self {
                index,
                membership: UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?,
                joined: BTreeSet::new(),
                capture: Capture::start(dir, index)?,
            });
        }
        let this = active.as_mut().unwrap();
        // Winsock принимает индекс интерфейса в сетевом порядке байт из диапазона 0/8.
        let iface = Ipv4Addr::from(index);
        this.joined.retain(|group| {
            groups.contains_key(group) || this.membership.leave_multicast_v4(group, &iface).is_err()
        });
        for group in groups.keys() {
            if !this.joined.contains(group)
                && this.membership.join_multicast_v4(group, &iface).is_ok()
            {
                this.joined.insert(*group);
            }
        }
        *this.capture.delivery.write().unwrap() = Delivery { groups, locals };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_delivery_requires_a_joined_group_and_an_active_remote_source() {
        let group = Ipv4Addr::new(239, 213, 1, 9);
        let indices = BTreeSet::from([17]);
        let local = Ipv4Addr::new(10, 181, 1, 3);
        let state = Delivery {
            groups: BTreeMap::from([(group, indices.clone())]),
            locals: vec![local],
        };
        let mut packet = [0u8; 28];
        packet[0] = 0x45;
        packet[3] = 28;
        packet[9] = 17;
        packet[25] = 8;
        packet[16..20].copy_from_slice(&group.octets());
        packet[12..16].copy_from_slice(&[10, 181, 1, 2]);
        assert_eq!(state.targets(&packet), Some(&indices));
        for source in [local.octets(), [10, 181, 2, 2], [10, 181, 1, 255]] {
            let mut other = packet;
            other[12..16].copy_from_slice(&source);
            assert!(state.targets(&other).is_none());
        }
        for (index, value) in [(6, 32), (19, 8), (3, 30), (25, 10)] {
            let mut other = packet;
            other[index] = value;
            assert!(state.targets(&other).is_none());
        }
        assert_eq!(std::mem::size_of::<Address>(), 80);
    }
}
