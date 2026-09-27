use std::collections::HashSet;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, HANDLE, NO_ERROR};
use windows::Win32::NetworkManagement::NetManagement::{NERR_Success, NetApiBufferFree};
use windows::Win32::NetworkManagement::WNet::{
    NETRESOURCEW, RESOURCE_GLOBALNET, RESOURCETYPE_ANY, RESOURCEUSAGE_CONTAINER,
    RESOURCEUSAGE_NONE, WNetCloseEnum, WNetEnumResourceW, WNetOpenEnumW,
};
use windows::Win32::Storage::FileSystem::{NetShareEnum, SHARE_INFO_1, STYPE_DISKTREE, STYPE_MASK};
use windows::core::{PCWSTR, PWSTR};

use super::{network_mdns, network_wsd};

#[derive(Clone)]
pub struct ShareInfo {
    pub name: String,
    pub path: PathBuf,
}

/// Splits a `\\host\share\...` path into its non-empty segments, if it is one.
fn unc_segments(path: &Path) -> Option<Vec<String>> {
    let s = path.to_string_lossy();
    let rest = s.strip_prefix(r"\\")?;

    if rest.is_empty() {
        return None;
    }

    let segments: Vec<String> = rest
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();

    if segments.is_empty() {
        None
    } else {
        Some(segments)
    }
}

/// Returns the server name if `path` is a bare UNC host with no share, e.g. `\\SERVER`.
pub fn unc_host_only(path: &Path) -> Option<String> {
    let segments = unc_segments(path)?;
    (segments.len() == 1).then(|| segments[0].clone())
}

/// Returns the server name if `path` is exactly a UNC share root, e.g. `\\SERVER\Share`.
pub fn unc_share_root_host(path: &Path) -> Option<String> {
    let segments = unc_segments(path)?;
    (segments.len() == 2).then(|| segments[0].clone())
}

/// Why `list_shares` couldn't enumerate a server's shares - distinguished from
/// "the server genuinely has none" so the caller can show a real error instead
/// of a misleading empty folder. `AccessDenied` is by far the most common case
/// in practice: a server with Guest/anonymous SMB access disabled (or that
/// requires a matching Windows account this PC has no credentials for) refuses
/// `NetShareEnum` outright, even though the server itself is fully reachable
/// (SMB port open, `\\server\knownshare` might even open fine directly) - the
/// same class of thing `net view \\server` reports as "System error 5".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareEnumError {
    AccessDenied,
    Other,
}

/// Enumerate the file shares published by a server (e.g. the contents of `\\SERVER`).
/// Print queues, IPC$, and other non-disk shares are skipped. Administrative shares
/// (ending in `$`) are returned so the caller can mark them hidden like normal hidden folders.
pub fn list_shares(server: &str) -> Result<Vec<ShareInfo>, ShareEnumError> {
    let mut shares = Vec::new();

    let server_wide: Vec<u16> = OsString::from(format!(r"\\{server}"))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let status = unsafe {
        let mut buffer: *mut u8 = std::ptr::null_mut();
        let mut entries_read: u32 = 0;
        let mut total_entries: u32 = 0;

        let status = NetShareEnum(
            PCWSTR(server_wide.as_ptr()),
            1,
            &mut buffer,
            u32::MAX,
            &mut entries_read,
            &mut total_entries,
            None,
        );

        if status == NERR_Success && !buffer.is_null() {
            let entries =
                std::slice::from_raw_parts(buffer as *const SHARE_INFO_1, entries_read as usize);

            for entry in entries {
                if (entry.shi1_type.0 & STYPE_MASK.0) != STYPE_DISKTREE.0 {
                    continue;
                }

                let Some(name) = pwstr_to_string(entry.shi1_netname) else {
                    continue;
                };

                if name.is_empty() {
                    continue;
                }

                shares.push(ShareInfo {
                    path: PathBuf::from(format!(r"\\{server}\{name}")),
                    name,
                });
            }
        }

        if !buffer.is_null() {
            let _ = NetApiBufferFree(Some(buffer as *const _));
        }

        status
    };

    if status == NERR_Success {
        Ok(shares)
    } else if status == ERROR_ACCESS_DENIED.0 {
        Err(ShareEnumError::AccessDenied)
    } else {
        Err(ShareEnumError::Other)
    }
}

#[derive(Clone)]
pub struct NetworkComputer {
    pub name: String,
    pub path: PathBuf,
}

// The Win32 SDK's RESOURCEDISPLAYTYPE_SERVER; windows-rs doesn't bind this constant.
const RESOURCEDISPLAYTYPE_SERVER: u32 = 2;
const MAX_DISCOVERY_DEPTH: u32 = 4;
const MAX_DISCOVERED_COMPUTERS: usize = 200;
const NETWORK_CACHE_DURATION: Duration = Duration::from_secs(60);

struct NetworkCache {
    computers: Vec<NetworkComputer>,
    last_update: Instant,
}

lazy_static::lazy_static! {
    static ref NETWORK_CACHE: Arc<Mutex<Option<NetworkCache>>> = Arc::new(Mutex::new(None));
    static ref NETWORK_SCAN_IN_PROGRESS: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

/// Returns the cached list of computers found so far on the local network, kicking off
/// a background rescan if the cache is stale. Never blocks: on a cold cache this returns
/// an empty list immediately and the sidebar picks up results on a later frame once the
/// background scan (which can take a few seconds) completes.
pub fn get_network_computers_cached() -> Vec<NetworkComputer> {
    {
        let cache = NETWORK_CACHE.lock().unwrap();
        if let Some(ref cached) = *cache {
            if cached.last_update.elapsed() < NETWORK_CACHE_DURATION {
                return cached.computers.clone();
            }
        }
    }

    if !NETWORK_SCAN_IN_PROGRESS.swap(true, Ordering::AcqRel) {
        thread::spawn(|| {
            let computers = discover_network_computers();
            *NETWORK_CACHE.lock().unwrap() = Some(NetworkCache {
                computers,
                last_update: Instant::now(),
            });
            NETWORK_SCAN_IN_PROGRESS.store(false, Ordering::Release);
            // Without this, a fully idle window (no mouse/keyboard input)
            // never redraws to show newly-discovered devices - this scan
            // now also does two multicast round-trips (mDNS + WS-Discovery),
            // so it's a more noticeable wait than it used to be.
            crate::gui::windows::windowsoverrides::request_repaint();
        });
    }

    NETWORK_CACHE
        .lock()
        .unwrap()
        .as_ref()
        .map(|c| c.computers.clone())
        .unwrap_or_default()
}

/// Walks the network browsing hierarchy (providers -> workgroups/domains -> computers)
/// to discover computers advertising file shares on the local network. This relies on
/// legacy SMB/NetBIOS browsing, so it won't find every device on modern networks, but it
/// covers the common case of a home/office LAN with NetBIOS enabled.
fn discover_network_computers() -> Vec<NetworkComputer> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    // Legacy browse-list discovery: fast when it works, but most modern networks
    // don't run the Computer Browser service it depends on, so it often finds
    // nothing on its own.
    enumerate_network_level(None, 0, &mut results, &mut seen);

    // Active subnet scan: complements the above by actually probing every host on
    // the local /24 for an open SMB port, so it finds computers regardless of
    // whether legacy NetBIOS browsing is available.
    for computer in scan_local_subnet() {
        let key = computer.name.to_ascii_uppercase();
        if seen.insert(key) {
            results.push(computer);
        }
    }

    // mDNS and WS-Discovery: two more ways for a device to announce itself
    // that don't depend on NetBIOS browsing at all. Both only return
    // candidate IPs - each one still goes through the same SMB-port
    // verification and NetBIOS name resolution as the subnet sweep
    // (`probe_smb_host`), so a candidate that doesn't actually serve SMB
    // (a printer answering WS-Discovery, say) never ends up in the list.
    for ip in network_mdns::discover() {
        if let Some(computer) = probe_smb_host(ip) {
            let key = computer.name.to_ascii_uppercase();
            if seen.insert(key) {
                results.push(computer);
            }
        }
    }
    for ip in network_wsd::discover() {
        if let Some(computer) = probe_smb_host(ip) {
            let key = computer.name.to_ascii_uppercase();
            if seen.insert(key) {
                results.push(computer);
            }
        }
    }

    results
}

const SMB_PORT: u16 = 445;
const SMB_CONNECT_TIMEOUT: Duration = Duration::from_millis(200);
const NBNS_PORT: u16 = 137;
const NBNS_TIMEOUT: Duration = Duration::from_millis(300);

/// Finds this machine's primary LAN IPv4 address by asking the OS which local
/// address it would use to reach the internet. Nothing is actually sent: connecting
/// a UDP socket only resolves the local route/address, matching the common
/// "what's my LAN IP" trick that avoids needing adapter-enumeration APIs.
fn local_ipv4() -> Option<std::net::Ipv4Addr> {
    use std::net::{IpAddr, UdpSocket};

    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;

    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    }
}

/// Actively scans the local /24 subnet for hosts with SMB (port 445) open, resolving
/// each host's NetBIOS computer name where possible (falling back to its IP address
/// as the display name/browse target otherwise). Assumes a /24 subnet, which covers
/// the common case for home and small-office LANs.
fn scan_local_subnet() -> Vec<NetworkComputer> {
    let Some(local_ip) = local_ipv4() else {
        return Vec::new();
    };
    let octets = local_ip.octets();

    let handles: Vec<_> = (1u8..=254)
        .filter(|&last| last != octets[3])
        .map(|last| {
            let ip = std::net::Ipv4Addr::new(octets[0], octets[1], octets[2], last);
            std::thread::spawn(move || probe_smb_host(ip))
        })
        .collect();

    handles
        .into_iter()
        .filter_map(|handle| handle.join().ok().flatten())
        .collect()
}

fn probe_smb_host(ip: std::net::Ipv4Addr) -> Option<NetworkComputer> {
    if !has_smb_open(ip) {
        return None;
    }

    let name = query_netbios_name(ip).unwrap_or_else(|| ip.to_string());

    Some(NetworkComputer {
        path: PathBuf::from(format!(r"\\{name}")),
        name,
    })
}

/// Whether `ip` has the SMB port (445) open - the same check used to verify
/// hosts found by the active subnet sweep, reused to verify mDNS/WS-Discovery
/// candidates before they're shown in "Shared Network" (both of those
/// protocols can report a device that doesn't actually serve SMB - a printer
/// answering WS-Discovery, say - and every entry in that sidebar section is
/// meant to be something double-clicking will actually open).
fn has_smb_open(ip: std::net::Ipv4Addr) -> bool {
    use std::net::{SocketAddr, TcpStream};

    let addr = SocketAddr::from((ip, SMB_PORT));
    TcpStream::connect_timeout(&addr, SMB_CONNECT_TIMEOUT).is_ok()
}

/// Queries a host's NetBIOS computer name via a Node Status (NBSTAT) request to UDP
/// port 137 - the same mechanism behind the classic `nbtstat -A <ip>` command. Used
/// as a friendly-name lookup for hosts found by `scan_local_subnet`, and reused by
/// `network_wsd` (WS-Discovery gives back a UUID + URL, never a friendly name).
pub fn query_netbios_name(ip: std::net::Ipv4Addr) -> Option<String> {
    use std::net::{SocketAddr, UdpSocket};

    let mut packet = Vec::with_capacity(50);
    packet.extend_from_slice(&[0x00, 0x00]); // transaction id
    packet.extend_from_slice(&[0x00, 0x00]); // flags: standard query, no recursion
    packet.extend_from_slice(&[0x00, 0x01]); // questions = 1
    packet.extend_from_slice(&[0x00, 0x00]); // answer RRs
    packet.extend_from_slice(&[0x00, 0x00]); // authority RRs
    packet.extend_from_slice(&[0x00, 0x00]); // additional RRs

    // QNAME: first-level-encoded wildcard name "*", padded to 16 bytes, per RFC 1002.
    packet.push(32);
    let mut raw_name = [0x20u8; 16];
    raw_name[0] = b'*';
    for byte in raw_name {
        packet.push(0x41 + (byte >> 4));
        packet.push(0x41 + (byte & 0x0F));
    }
    packet.push(0x00); // name terminator

    packet.extend_from_slice(&[0x00, 0x21]); // QTYPE = NBSTAT
    packet.extend_from_slice(&[0x00, 0x01]); // QCLASS = IN

    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.set_read_timeout(Some(NBNS_TIMEOUT)).ok()?;
    socket
        .send_to(&packet, SocketAddr::from((ip, NBNS_PORT)))
        .ok()?;

    let mut buf = [0u8; 1024];
    let (len, _) = socket.recv_from(&mut buf).ok()?;
    parse_nbns_response(&buf[..len])
}

/// Parses a Node Status response, returning the host's own (non-group) NetBIOS name.
fn parse_nbns_response(data: &[u8]) -> Option<String> {
    if data.len() < 12 {
        return None;
    }

    // Skip the 12-byte header and the echoed question name (length-prefixed labels
    // ending in a zero byte, followed by QTYPE+QCLASS).
    let mut offset = 12;
    while offset < data.len() && data[offset] != 0 {
        offset += data[offset] as usize + 1;
    }
    offset += 1; // terminating zero byte
    offset += 4; // QTYPE + QCLASS

    // Answer RR: NAME (2-byte compression pointer) + TYPE(2) + CLASS(2) + TTL(4) + RDLENGTH(2).
    offset += 2 + 2 + 2 + 4 + 2;

    if offset >= data.len() {
        return None;
    }

    let num_names = data[offset] as usize;
    offset += 1;

    for _ in 0..num_names {
        if offset + 18 > data.len() {
            break;
        }

        let name_bytes = &data[offset..offset + 15];
        let suffix = data[offset + 15];
        let flags = u16::from_be_bytes([data[offset + 16], data[offset + 17]]);
        let is_group = (flags & 0x8000) != 0;

        // Suffix 0x00 + a unique (non-group) name is the host's own computer name.
        if suffix == 0x00 && !is_group {
            let name = String::from_utf8_lossy(name_bytes).trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }

        offset += 18;
    }

    None
}

fn enumerate_network_level(
    container: Option<&NETRESOURCEW>,
    depth: u32,
    results: &mut Vec<NetworkComputer>,
    seen: &mut HashSet<String>,
) {
    if depth > MAX_DISCOVERY_DEPTH || results.len() >= MAX_DISCOVERED_COMPUTERS {
        return;
    }

    let mut henum = HANDLE::default();

    let opened = unsafe {
        WNetOpenEnumW(
            RESOURCE_GLOBALNET,
            RESOURCETYPE_ANY,
            RESOURCEUSAGE_NONE,
            container.map(|c| c as *const NETRESOURCEW),
            &mut henum,
        )
    };

    if opened != NO_ERROR {
        return;
    }

    let mut buffer = vec![0u8; 64 * 1024];

    loop {
        let mut count: u32 = u32::MAX;
        let mut buffer_size = buffer.len() as u32;

        let status = unsafe {
            WNetEnumResourceW(
                henum,
                &mut count,
                buffer.as_mut_ptr() as *mut _,
                &mut buffer_size,
            )
        };

        if status != NO_ERROR || count == 0 || count == u32::MAX {
            break;
        }

        let entries = unsafe {
            std::slice::from_raw_parts(buffer.as_ptr() as *const NETRESOURCEW, count as usize)
        };

        for entry in entries {
            if entry.dwDisplayType == RESOURCEDISPLAYTYPE_SERVER {
                if let Some(name) = unsafe { pwstr_to_string(entry.lpRemoteName) } {
                    let trimmed = name.trim_start_matches('\\').to_string();
                    let key = trimmed.to_ascii_uppercase();

                    if !trimmed.is_empty() && seen.insert(key) {
                        results.push(NetworkComputer {
                            path: PathBuf::from(format!(r"\\{trimmed}")),
                            name: trimmed,
                        });
                    }
                }

                // Don't eagerly enumerate this computer's shares; those are
                // fetched on demand when the user opens `\\computer`.
                continue;
            }

            if (entry.dwUsage & RESOURCEUSAGE_CONTAINER.0) != 0 {
                enumerate_network_level(Some(entry), depth + 1, results, seen);

                if results.len() >= MAX_DISCOVERED_COMPUTERS {
                    break;
                }
            }
        }
    }

    unsafe {
        let _ = WNetCloseEnum(henum);
    }
}

unsafe fn pwstr_to_string(p: PWSTR) -> Option<String> {
    if p.0.is_null() {
        return None;
    }

    unsafe {
        let mut len = 0usize;
        let mut cursor = p.0;
        while *cursor != 0 {
            len += 1;
            cursor = cursor.add(1);
        }

        Some(
            OsString::from_wide(std::slice::from_raw_parts(p.0, len))
                .to_string_lossy()
                .to_string(),
        )
    }
}
