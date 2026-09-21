use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

use span_core::{DeviceId, DeviceInfo, Platform, TrustState};

use crate::config::{LocalDevice, parse_platform, platform_name, sanitize};
use crate::crypto::decode_hex;

pub const DISCOVERY_PORT: u16 = 46792;
const MAGIC: &str = "SPAN_DISCOVERY_V2";
const PROBE_MAGIC: &str = "SPAN_DISCOVERY_PROBE_V1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiscoveryMessage {
    Announcement(DiscoveryPacket),
    Probe,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryPacket {
    pub id: DeviceId,
    pub name: String,
    pub platform: Platform,
    pub public_key: String,
}

impl DiscoveryPacket {
    pub fn from_local(device: &LocalDevice) -> Self {
        Self {
            id: device.id.clone(),
            name: device.name.clone(),
            platform: device.platform,
            public_key: device.public_key_hex(),
        }
    }

    pub fn into_device_info(self) -> DeviceInfo {
        DeviceInfo {
            id: self.id,
            name: self.name,
            platform: self.platform,
            trust_state: TrustState::Discovered,
            endpoint: None,
            public_key: Some(self.public_key),
        }
    }
}

pub fn broadcast_once(device: &LocalDevice) -> io::Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0))?;
    socket.set_broadcast(true)?;
    let packet = encode_packet(&DiscoveryPacket::from_local(device));
    send_to_local_networks(&socket, packet.as_bytes())
}

pub fn discover_once(
    device: &LocalDevice,
    timeout: Duration,
) -> io::Result<Vec<(DiscoveryPacket, SocketAddr)>> {
    // Use an ephemeral source port. The daemon owns UDP 46792, so a manual
    // scan must not compete with it for the discovery socket.
    let socket = UdpSocket::bind(("0.0.0.0", 0))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(150)))?;
    send_to_local_networks(&socket, PROBE_MAGIC.as_bytes())?;

    // Also advertise the scanner itself. This lets the peer daemon learn the
    // scanner even if a platform firewall drops the unicast probe response.
    let packet = encode_packet(&DiscoveryPacket::from_local(device));
    send_to_local_networks(&socket, packet.as_bytes())?;

    let deadline = Instant::now() + timeout;
    let mut buffer = [0_u8; 1024];
    let mut packets = Vec::new();

    while Instant::now() < deadline {
        match socket.recv_from(&mut buffer) {
            Ok((len, addr)) => {
                if let Ok(value) = std::str::from_utf8(&buffer[..len]) {
                    if let Some(DiscoveryMessage::Announcement(packet)) = decode_message(value) {
                        packets.push((packet, addr));
                    }
                }
            }
            Err(error) if is_transient_udp_error(&error) => {}
            Err(error) => return Err(error),
        }
    }

    Ok(packets)
}

/// Send on every IPv4 subnet instead of relying only on the limited broadcast
/// address. Windows machines commonly have WSL, VPN and virtual adapters; a
/// single `255.255.255.255` packet can otherwise leave through the wrong one.
fn send_to_local_networks(socket: &UdpSocket, payload: &[u8]) -> io::Result<()> {
    let mut targets = local_broadcast_targets();
    targets.push(Ipv4Addr::BROADCAST);
    targets.sort_unstable();
    targets.dedup();

    let mut sent = false;
    let mut last_error = None;
    for target in targets {
        match socket.send_to(payload, (target, DISCOVERY_PORT)) {
            Ok(_) => sent = true,
            Err(error) => last_error = Some(error),
        }
    }

    if sent {
        Ok(())
    } else {
        Err(last_error.unwrap_or_else(|| io::Error::other("no IPv4 broadcast target found")))
    }
}

fn local_broadcast_targets() -> Vec<Ipv4Addr> {
    get_if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|interface| match interface.addr {
            get_if_addrs::IfAddr::V4(address)
                if !address.ip.is_loopback() && !address.ip.is_unspecified() =>
            {
                Some(
                    address
                        .broadcast
                        .unwrap_or_else(|| subnet_broadcast(address.ip, address.netmask)),
                )
            }
            _ => None,
        })
        .filter(|address| !address.is_loopback() && !address.is_unspecified())
        .collect()
}

fn subnet_broadcast(ip: Ipv4Addr, netmask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) | !u32::from(netmask))
}

pub fn respond_to_probe(
    socket: &UdpSocket,
    device: &LocalDevice,
    target: SocketAddr,
) -> io::Result<()> {
    let packet = encode_packet(&DiscoveryPacket::from_local(device));
    socket.send_to(packet.as_bytes(), target)?;
    Ok(())
}

pub fn listen_forever(
    mut on_message: impl FnMut(DiscoveryMessage, SocketAddr, &UdpSocket) -> io::Result<()>,
) -> io::Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT))?;
    let mut buffer = [0_u8; 1024];

    loop {
        let (len, addr) = match socket.recv_from(&mut buffer) {
            Ok(received) => received,
            Err(error) if is_transient_udp_error(&error) => {
                // A UDP send can surface an asynchronous ICMP error on the
                // next receive (for example when a stale peer leaves Wi-Fi).
                // That peer must not permanently take discovery offline.
                eprintln!("ignoring transient discovery receive error: {error}");
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(error) => return Err(error),
        };
        if let Ok(value) = std::str::from_utf8(&buffer[..len]) {
            if let Some(message) = decode_message(value) {
                if let Err(error) = on_message(message, addr, &socket) {
                    // Bad state or an unreachable peer affects only this
                    // datagram. Keep listening for the next device.
                    eprintln!("failed to process discovery message from {addr}: {error}");
                }
            }
        }
    }
}

fn is_transient_udp_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock
            | io::ErrorKind::TimedOut
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NetworkUnreachable
    )
}

fn encode_packet(packet: &DiscoveryPacket) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}",
        MAGIC,
        packet.id,
        sanitize(&packet.name),
        platform_name(packet.platform),
        packet.public_key
    )
}

fn decode_message(value: &str) -> Option<DiscoveryMessage> {
    if value.trim() == PROBE_MAGIC {
        return Some(DiscoveryMessage::Probe);
    }

    let mut fields = value.trim().split('\t');
    if fields.next()? != MAGIC {
        return None;
    }

    Some(DiscoveryMessage::Announcement(DiscoveryPacket {
        id: DeviceId::new(fields.next()?.to_string())?,
        name: fields.next()?.to_string(),
        platform: parse_platform(fields.next()?),
        public_key: fields.next().and_then(validate_public_key)?,
    }))
}

fn validate_public_key(value: &str) -> Option<String> {
    let bytes = decode_hex(value)?;
    (bytes.len() == 32).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_round_trip_preserves_device() {
        let packet = DiscoveryPacket {
            id: DeviceId::new("abc").unwrap(),
            name: "MacBook".to_string(),
            platform: Platform::MacOs,
            public_key: "aa".repeat(32),
        };

        assert_eq!(
            decode_message(&encode_packet(&packet)),
            Some(DiscoveryMessage::Announcement(packet))
        );
        assert_eq!(decode_message(PROBE_MAGIC), Some(DiscoveryMessage::Probe));
    }

    #[test]
    fn calculates_directed_broadcast_address() {
        assert_eq!(
            subnet_broadcast(
                Ipv4Addr::new(192, 168, 31, 42),
                Ipv4Addr::new(255, 255, 255, 0)
            ),
            Ipv4Addr::new(192, 168, 31, 255)
        );
        assert_eq!(
            subnet_broadcast(Ipv4Addr::new(10, 4, 7, 8), Ipv4Addr::new(255, 255, 0, 0)),
            Ipv4Addr::new(10, 4, 255, 255)
        );
    }

    #[test]
    fn probe_response_reuses_the_listener_socket() {
        let listener = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let scanner = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        scanner
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let device = LocalDevice {
            id: DeviceId::new("listener-device").unwrap(),
            name: "Listener".to_string(),
            platform: Platform::Linux,
            private_key: [0; 32],
            public_key: [1; 32],
        };

        respond_to_probe(&listener, &device, scanner.local_addr().unwrap()).unwrap();

        let mut buffer = [0_u8; 1024];
        let (len, source) = scanner.recv_from(&mut buffer).unwrap();
        assert_eq!(source, listener.local_addr().unwrap());
        assert!(matches!(
            std::str::from_utf8(&buffer[..len])
                .ok()
                .and_then(decode_message),
            Some(DiscoveryMessage::Announcement(_))
        ));
    }

    #[test]
    fn unreachable_udp_peer_is_transient() {
        assert!(is_transient_udp_error(&io::Error::from(
            io::ErrorKind::HostUnreachable
        )));
        assert!(is_transient_udp_error(&io::Error::from(
            io::ErrorKind::ConnectionReset
        )));
        assert!(!is_transient_udp_error(&io::Error::from(
            io::ErrorKind::InvalidData
        )));
    }
}
