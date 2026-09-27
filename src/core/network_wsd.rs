//! One-shot WS-Discovery (WSD) probe, used as an additional discovery source
//! alongside `network.rs`'s legacy NetBIOS browsing, active subnet sweep, and
//! mDNS query (`network_mdns.rs`). WS-Discovery is how Windows' own "Network"
//! view (and printers/NAS boxes that support it) announce themselves, so it
//! can surface hosts that don't respond to NetBIOS browsing at all.
//!
//! Unlike mDNS, WS-Discovery replies (`ProbeMatch`) are sent as plain unicast
//! UDP back to the sender's own address:port - not multicast - so this is a
//! simple send-then-listen-on-the-same-socket, no multicast group join
//! needed even to receive.
//!
//! This only returns candidate IPv4 addresses, not `NetworkComputer`s - a
//! `ProbeMatch` gives a UUID-based endpoint reference plus a URL (`XAddrs`),
//! not a friendly computer name or any guarantee the device serves SMB at
//! all (a printer answering WS-Discovery, say), so the caller (`network.rs`)
//! runs each candidate through the same SMB-port-445 verification and
//! NetBIOS name resolution (`probe_smb_host`) the active subnet sweep
//! already uses, rather than duplicating that lookup here.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const WSD_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const WSD_PORT: u16 = 3702;
const WSD_LISTEN_DURATION: Duration = Duration::from_millis(2000);
const WSD_RECV_TIMEOUT: Duration = Duration::from_millis(300);

pub fn discover() -> Vec<Ipv4Addr> {
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return Vec::new();
    };
    if socket.set_read_timeout(Some(WSD_RECV_TIMEOUT)).is_err() {
        return Vec::new();
    }

    let probe = build_probe_message();
    if socket
        .send_to(probe.as_bytes(), (WSD_MULTICAST_ADDR, WSD_PORT))
        .is_err()
    {
        return Vec::new();
    }

    let mut hosts: Vec<Ipv4Addr> = Vec::new();
    let deadline = Instant::now() + WSD_LISTEN_DURATION;
    let mut buf = [0u8; 8192];

    while Instant::now() < deadline {
        match socket.recv_from(&mut buf) {
            Ok((len, from)) => {
                let ip = extract_xaddrs_ip(&buf[..len]).or(match from {
                    SocketAddr::V4(v4) => Some(*v4.ip()),
                    SocketAddr::V6(_) => None,
                });
                if let Some(ip) = ip {
                    if !hosts.contains(&ip) {
                        hosts.push(ip);
                    }
                }
            }
            Err(_) => continue,
        }
    }

    hosts
}

fn build_probe_message() -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope" xmlns:wsa="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:wsd="http://schemas.xmlsoap.org/ws/2005/04/discovery">
  <soap:Header>
    <wsa:Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</wsa:Action>
    <wsa:MessageID>urn:uuid:{}</wsa:MessageID>
    <wsa:To>urn:schemas-xmlsoap-org:ws:2005:04:discovery</wsa:To>
  </soap:Header>
  <soap:Body>
    <wsd:Probe/>
  </soap:Body>
</soap:Envelope>"#,
        random_uuid()
    )
}

/// A pseudo-random UUID string, good enough to make each `MessageID` unique -
/// this is a one-shot query, not a spec-compliant WS-Discovery client, so
/// strict RFC 4122 versioning doesn't matter here.
fn random_uuid() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::rng().random();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

/// Pulls the first host out of a `ProbeMatch` response's `wsd:XAddrs` field
/// (a space-separated list of URLs, e.g. `http://192.168.1.20:5357/wsdapi`).
/// Falls back to `None` (letting the caller use the packet's source address
/// instead) if the XML can't be parsed or contains no usable IPv4 host.
fn extract_xaddrs_ip(data: &[u8]) -> Option<Ipv4Addr> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let text = std::str::from_utf8(data).ok()?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);

    let mut in_xaddrs = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => in_xaddrs = e.local_name().as_ref() == b"XAddrs",
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"XAddrs" {
                    in_xaddrs = false;
                }
            }
            Ok(Event::Text(t)) if in_xaddrs => {
                let text = t.decode().ok()?.to_string();
                if let Some(ip) = text.split_whitespace().find_map(parse_ip_from_url) {
                    return Some(ip);
                }
            }
            Ok(Event::Eof) => return None,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

/// Extracts the host portion of a `scheme://host:port/path` URL and parses
/// it as an IPv4 address (WS-Discovery XAddrs entries are typically raw IP
/// literals, not hostnames).
fn parse_ip_from_url(url: &str) -> Option<Ipv4Addr> {
    let after_scheme = url.split("://").nth(1)?;
    let host = after_scheme
        .split(['/', ':'])
        .next()
        .unwrap_or(after_scheme);
    host.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_ip_from_probe_match_xaddrs() {
        let xml = br#"<?xml version="1.0"?>
<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope" xmlns:wsd="http://schemas.xmlsoap.org/ws/2005/04/discovery">
  <soap:Body>
    <wsd:ProbeMatches>
      <wsd:ProbeMatch>
        <wsd:XAddrs>http://192.168.1.20:5357/wsdapi</wsd:XAddrs>
      </wsd:ProbeMatch>
    </wsd:ProbeMatches>
  </soap:Body>
</soap:Envelope>"#;

        assert_eq!(
            extract_xaddrs_ip(xml),
            Some(Ipv4Addr::new(192, 168, 1, 20))
        );
    }

    #[test]
    fn malformed_xml_returns_none_without_panicking() {
        assert_eq!(extract_xaddrs_ip(b"not xml at all"), None);
    }

    #[test]
    fn parses_ip_from_plain_url() {
        assert_eq!(
            parse_ip_from_url("http://10.0.0.5:5357/wsdapi"),
            Some(Ipv4Addr::new(10, 0, 0, 5))
        );
        assert_eq!(parse_ip_from_url("http://not-an-ip/path"), None);
    }
}
