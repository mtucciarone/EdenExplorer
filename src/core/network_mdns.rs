//! One-shot mDNS (RFC 6762/6763) discovery of SMB-sharing hosts, used as an
//! additional discovery source alongside `network.rs`'s legacy NetBIOS
//! browsing and active subnet sweep - some devices (NAS boxes, Macs, Linux
//! Samba with Avahi) advertise their SMB share via mDNS-SD and never show up
//! in NetBIOS browsing at all.
//!
//! This queries the single well-known service type `_smb._tcp.local.` with
//! the "unicast-response" (QU) bit set, so replies come back directly to our
//! own ephemeral UDP port instead of requiring us to bind port 5353 and join
//! the multicast group ourselves (which could conflict with a real mDNS
//! responder already running on this machine, e.g. Bonjour/iTunes). This is
//! a one-shot query, not a persistent responder, so no cache/TTL handling is
//! needed - each call does a fresh ~1.5s probe.
//!
//! This only returns candidate IPv4 addresses, not `NetworkComputer`s - the
//! caller (`network.rs`) still runs each candidate through the same
//! SMB-port-445 verification and NetBIOS name resolution the active subnet
//! sweep already uses (`probe_smb_host`), so a device that merely *responded
//! to the mDNS query* but doesn't actually have SMB open (unlikely for
//! `_smb._tcp` responders, but not guaranteed) is treated the same as any
//! other candidate rather than trusted blindly.

use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

const MDNS_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;
const MDNS_LISTEN_DURATION: Duration = Duration::from_millis(1500);
const MDNS_RECV_TIMEOUT: Duration = Duration::from_millis(300);

const PTR_TYPE: u16 = 12;
const SRV_TYPE: u16 = 33;
const A_TYPE: u16 = 1;

pub fn discover() -> Vec<Ipv4Addr> {
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return Vec::new();
    };
    if socket.set_read_timeout(Some(MDNS_RECV_TIMEOUT)).is_err() {
        return Vec::new();
    }

    let query = build_smb_ptr_query();
    if socket
        .send_to(&query, (MDNS_MULTICAST_ADDR, MDNS_PORT))
        .is_err()
    {
        return Vec::new();
    }

    let mut results = Vec::new();
    let deadline = Instant::now() + MDNS_LISTEN_DURATION;
    let mut buf = [0u8; 4096];

    while Instant::now() < deadline {
        match socket.recv_from(&mut buf) {
            Ok((len, _)) => results.extend(parse_smb_response(&buf[..len])),
            Err(_) => continue,
        }
    }

    results
}

/// Builds a standard mDNS query for `PTR _smb._tcp.local.` with the QU
/// (unicast-response desired) bit set in QCLASS, so a compliant responder
/// replies straight to our source port instead of via multicast.
fn build_smb_ptr_query() -> Vec<u8> {
    let mut packet = Vec::with_capacity(40);
    packet.extend_from_slice(&[0x00, 0x00]); // transaction id (unused for mDNS)
    packet.extend_from_slice(&[0x00, 0x00]); // flags: standard query
    packet.extend_from_slice(&[0x00, 0x01]); // QDCOUNT = 1
    packet.extend_from_slice(&[0x00, 0x00]); // ANCOUNT
    packet.extend_from_slice(&[0x00, 0x00]); // NSCOUNT
    packet.extend_from_slice(&[0x00, 0x00]); // ARCOUNT

    for label in ["_smb", "_tcp", "local"] {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0x00); // root label

    packet.extend_from_slice(&[0x00, PTR_TYPE as u8]); // QTYPE = PTR
    packet.extend_from_slice(&[0x80, 0x01]); // QCLASS = IN (1) | QU bit (0x8000)

    packet
}

struct DnsRecord {
    name: String,
    rtype: u16,
    rdata_offset: usize,
    rdata_len: usize,
}

/// Parses one mDNS response packet, chasing the standard mDNS-SD chain
/// (PTR service->instance, SRV instance->host:port, A host->IPv4) to produce
/// zero or more resolved hosts. A compliant responder includes the SRV and A
/// records for each PTR answer in the same packet's additional-records
/// section, so this never needs to issue follow-up queries.
fn parse_smb_response(data: &[u8]) -> Vec<Ipv4Addr> {
    if data.len() < 12 {
        return Vec::new();
    }

    let ancount = u16::from_be_bytes([data[6], data[7]]);
    let nscount = u16::from_be_bytes([data[8], data[9]]);
    let arcount = u16::from_be_bytes([data[10], data[11]]);
    let qdcount = u16::from_be_bytes([data[4], data[5]]);

    let mut offset = 12;

    // Skip the question section (we don't need it - we already know what we asked).
    for _ in 0..qdcount {
        let (_, after_name) = read_name(data, offset);
        offset = after_name + 4; // QTYPE + QCLASS
        if offset > data.len() {
            return Vec::new();
        }
    }

    let mut records = Vec::new();
    offset = parse_records(data, offset, ancount, &mut records);
    offset = parse_records(data, offset, nscount, &mut records);
    let _ = parse_records(data, offset, arcount, &mut records);

    let mut results = Vec::new();

    for ptr in records.iter().filter(|r| r.rtype == PTR_TYPE) {
        let (instance_name, _) = read_name(data, ptr.rdata_offset);

        let Some(srv) = records.iter().find(|r| {
            r.rtype == SRV_TYPE && r.name.eq_ignore_ascii_case(&instance_name)
        }) else {
            continue;
        };

        if srv.rdata_len < 6 {
            continue;
        }
        let (target_host, _) = read_name(data, srv.rdata_offset + 6);

        let Some(a_record) = records.iter().find(|r| {
            r.rtype == A_TYPE && r.name.eq_ignore_ascii_case(&target_host)
        }) else {
            continue;
        };

        if a_record.rdata_len != 4 {
            continue;
        }
        let ip = Ipv4Addr::new(
            data[a_record.rdata_offset],
            data[a_record.rdata_offset + 1],
            data[a_record.rdata_offset + 2],
            data[a_record.rdata_offset + 3],
        );

        if !results.contains(&ip) {
            results.push(ip);
        }
    }

    results
}

/// Parses `count` resource records starting at `offset`, returning the
/// offset just past the last one. Malformed input truncates the scan rather
/// than panicking or looping.
fn parse_records(data: &[u8], mut offset: usize, count: u16, out: &mut Vec<DnsRecord>) -> usize {
    for _ in 0..count {
        if offset >= data.len() {
            break;
        }

        let (name, after_name) = read_name(data, offset);
        offset = after_name;

        if offset + 10 > data.len() {
            break;
        }

        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]) as usize;
        let rdata_offset = offset + 10;

        if rdata_offset + rdlength > data.len() {
            break;
        }

        out.push(DnsRecord {
            name,
            rtype,
            rdata_offset,
            rdata_len: rdlength,
        });

        offset = rdata_offset + rdlength;
    }

    offset
}

/// Reads a (possibly compressed) DNS name starting at `offset`, following
/// `0xC0` pointer indirection. Returns the decoded dotted name and the
/// offset immediately following the name *in the original stream* (i.e.
/// right after the terminating zero byte or the 2-byte pointer that was
/// actually encountered at `offset` - not wherever a followed pointer led).
fn read_name(data: &[u8], offset: usize) -> (String, usize) {
    let mut labels: Vec<String> = Vec::new();
    let mut cursor = offset;
    let mut end_offset = None;
    let mut hops = 0;

    loop {
        if cursor >= data.len() || hops > 16 {
            break;
        }

        let len = data[cursor];

        if len == 0 {
            if end_offset.is_none() {
                end_offset = Some(cursor + 1);
            }
            break;
        } else if (len & 0xC0) == 0xC0 {
            if cursor + 1 >= data.len() {
                break;
            }
            let pointer = (((len & 0x3F) as usize) << 8) | data[cursor + 1] as usize;
            if end_offset.is_none() {
                end_offset = Some(cursor + 2);
            }
            hops += 1;
            cursor = pointer;
            continue;
        } else {
            let label_len = len as usize;
            if cursor + 1 + label_len > data.len() {
                break;
            }
            labels.push(String::from_utf8_lossy(&data[cursor + 1..cursor + 1 + label_len]).to_string());
            cursor += 1 + label_len;
        }
    }

    (labels.join("."), end_offset.unwrap_or(cursor))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-builds a minimal mDNS response packet with one PTR answer plus
    /// SRV and A records in the additional section (the shape a real
    /// Samba/Avahi responder sends), and checks the full chain resolves to
    /// the expected IP.
    #[test]
    fn parses_a_full_ptr_srv_a_chain() {
        let mut packet = Vec::new();
        packet.extend_from_slice(&[0x00, 0x00]); // id
        packet.extend_from_slice(&[0x84, 0x00]); // flags: response, authoritative
        packet.extend_from_slice(&[0x00, 0x00]); // qdcount
        packet.extend_from_slice(&[0x00, 0x01]); // ancount = 1 (PTR)
        packet.extend_from_slice(&[0x00, 0x00]); // nscount
        packet.extend_from_slice(&[0x00, 0x02]); // arcount = 2 (SRV + A)

        // PTR record: name = _smb._tcp.local, rdata = "mynas._smb._tcp.local"
        let ptr_name_offset = packet.len();
        for label in ["_smb", "_tcp", "local"] {
            packet.push(label.len() as u8);
            packet.extend_from_slice(label.as_bytes());
        }
        packet.push(0x00);
        packet.extend_from_slice(&[0x00, 12]); // type PTR
        packet.extend_from_slice(&[0x00, 0x01]); // class IN
        packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // ttl
        let rdata_len_pos = packet.len();
        packet.extend_from_slice(&[0x00, 0x00]); // rdlength placeholder
        let rdata_start = packet.len();
        packet.push(5);
        packet.extend_from_slice(b"mynas");
        // Pointer back to "_smb._tcp.local" for the rest of the instance name.
        packet.push(0xC0);
        packet.push(ptr_name_offset as u8);
        let rdata_len = packet.len() - rdata_start;
        packet[rdata_len_pos] = (rdata_len >> 8) as u8;
        packet[rdata_len_pos + 1] = rdata_len as u8;

        // SRV record: name = "mynas._smb._tcp.local", target = "mynas.local"
        packet.push(5);
        packet.extend_from_slice(b"mynas");
        packet.push(0xC0);
        packet.push(ptr_name_offset as u8);
        packet.extend_from_slice(&[0x00, 33]); // type SRV
        packet.extend_from_slice(&[0x00, 0x01]); // class IN
        packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // ttl
        let srv_rdata_len_pos = packet.len();
        packet.extend_from_slice(&[0x00, 0x00]); // rdlength placeholder
        let srv_rdata_start = packet.len();
        packet.extend_from_slice(&[0x00, 0x00]); // priority
        packet.extend_from_slice(&[0x00, 0x00]); // weight
        packet.extend_from_slice(&[0x01, 0xBD]); // port 445
        packet.push(5);
        packet.extend_from_slice(b"mynas");
        packet.push(0x00); // "mynas." + root, standalone (not "mynas.local" to
        // keep this test simple - the A record below uses the same standalone name)
        let srv_rdata_len = packet.len() - srv_rdata_start;
        packet[srv_rdata_len_pos] = (srv_rdata_len >> 8) as u8;
        packet[srv_rdata_len_pos + 1] = srv_rdata_len as u8;

        // A record: name = "mynas", rdata = 192.168.1.42
        packet.push(5);
        packet.extend_from_slice(b"mynas");
        packet.push(0x00);
        packet.extend_from_slice(&[0x00, 1]); // type A
        packet.extend_from_slice(&[0x00, 0x01]); // class IN
        packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // ttl
        packet.extend_from_slice(&[0x00, 0x04]); // rdlength = 4
        packet.extend_from_slice(&[192, 168, 1, 42]);

        let results = parse_smb_response(&packet);

        assert_eq!(results, vec![Ipv4Addr::new(192, 168, 1, 42)]);
    }

    #[test]
    fn truncated_packet_does_not_panic() {
        assert!(parse_smb_response(&[0x00, 0x00, 0x84]).is_empty());
    }
}
