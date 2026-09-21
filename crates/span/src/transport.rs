use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit},
};
use span_core::DeviceId;

use crate::crypto::{NONCE_BYTES, decode_hex, encode_hex, random_nonce, shared_key};

pub const TEXT_PORT: u16 = 46793;
const MAGIC: &str = "SPAN_TEXT_V3";
const PAIRING_MAGIC: &str = "SPAN_PAIR_ACCEPT_V1";
const MAX_TEXT_BYTES: usize = 64 * 1024;
const KEY_INFO: &[u8] = b"span-text-v3";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncryptedPacketKind {
    Text,
    PairingAccept,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncryptedTextPacket {
    pub kind: EncryptedPacketKind,
    pub from: DeviceId,
    pub nonce: [u8; NONCE_BYTES],
    pub ciphertext: Vec<u8>,
}

pub fn encrypt_text(
    from: &DeviceId,
    sender_private_key: &[u8; 32],
    recipient_public_key: &str,
    text: &str,
) -> io::Result<EncryptedTextPacket> {
    let key_bytes = shared_key(sender_private_key, recipient_public_key, KEY_INFO)?;
    let cipher = ChaCha20Poly1305::new_from_slice(&key_bytes)
        .map_err(|_| io::Error::other("cipher init failed"))?;
    let nonce = random_nonce();
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), text.as_bytes())
        .map_err(|_| io::Error::other("encrypt failed"))?;

    Ok(EncryptedTextPacket {
        kind: EncryptedPacketKind::Text,
        from: from.clone(),
        nonce,
        ciphertext,
    })
}

pub fn encrypt_pairing_accept(
    from: &DeviceId,
    sender_private_key: &[u8; 32],
    recipient_public_key: &str,
    proof: &str,
) -> io::Result<EncryptedTextPacket> {
    let mut packet = encrypt_text(from, sender_private_key, recipient_public_key, proof)?;
    packet.kind = EncryptedPacketKind::PairingAccept;
    Ok(packet)
}

pub fn decrypt_text(
    packet: &EncryptedTextPacket,
    receiver_private_key: &[u8; 32],
    sender_public_key: &str,
) -> io::Result<String> {
    let key_bytes = shared_key(receiver_private_key, sender_public_key, KEY_INFO)?;
    let cipher = ChaCha20Poly1305::new_from_slice(&key_bytes)
        .map_err(|_| io::Error::other("cipher init failed"))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&packet.nonce), packet.ciphertext.as_ref())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "decrypt failed"))?;

    String::from_utf8(plaintext)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "plaintext is not utf-8"))
}

pub fn send_text(addr: impl ToSocketAddrs, packet: &EncryptedTextPacket) -> io::Result<()> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;
    write_packet(&mut stream, packet)
}

pub fn receive_text_once(timeout: Duration) -> io::Result<Option<EncryptedTextPacket>> {
    let listener = TcpListener::bind(("0.0.0.0", TEXT_PORT))?;
    listener.set_nonblocking(true)?;
    let deadline = std::time::Instant::now() + timeout;

    while std::time::Instant::now() < deadline {
        match listener.accept() {
            Ok((stream, _addr)) => return read_packet(stream).map(Some),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error),
        }
    }

    Ok(None)
}

pub fn receive_text_forever(
    mut on_packet: impl FnMut(EncryptedTextPacket) -> io::Result<()>,
) -> io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", TEXT_PORT))?;

    for stream in listener.incoming() {
        dispatch_received_packet(stream.and_then(read_packet), &mut on_packet);
    }

    Ok(())
}

fn dispatch_received_packet(
    received: io::Result<EncryptedTextPacket>,
    on_packet: &mut impl FnMut(EncryptedTextPacket) -> io::Result<()>,
) {
    match received {
        Ok(packet) => {
            if let Err(error) = on_packet(packet) {
                // Authentication, trust-store, or clipboard failures are
                // scoped to one packet. A malformed/stale peer must not
                // permanently stop clipboard reception for every device.
                eprintln!("packet processing error: {error}");
            }
        }
        Err(error) => eprintln!("receive error: {error}"),
    }
}

fn write_packet(mut writer: impl Write, packet: &EncryptedTextPacket) -> io::Result<()> {
    if packet.ciphertext.len() > MAX_TEXT_BYTES + 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ciphertext too large",
        ));
    }

    writeln!(
        writer,
        "{}\t{}\t{}\t{}",
        match packet.kind {
            EncryptedPacketKind::Text => MAGIC,
            EncryptedPacketKind::PairingAccept => PAIRING_MAGIC,
        },
        packet.from,
        encode_hex(&packet.nonce),
        encode_hex(&packet.ciphertext)
    )?;
    Ok(())
}

fn read_packet(reader: impl Read) -> io::Result<EncryptedTextPacket> {
    let mut reader = BufReader::new(reader);
    let mut header = String::new();
    reader.read_line(&mut header)?;

    let mut fields = header.trim_end().split('\t');
    let kind = match fields.next() {
        Some(MAGIC) => EncryptedPacketKind::Text,
        Some(PAIRING_MAGIC) => EncryptedPacketKind::PairingAccept,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic")),
    };

    let from = fields
        .next()
        .and_then(|value| DeviceId::new(value.to_string()))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad device id"))?;
    let nonce = fields
        .next()
        .and_then(parse_nonce)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad nonce"))?;
    let ciphertext = fields
        .next()
        .and_then(|value| decode_hex(value))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad ciphertext"))?;

    if ciphertext.len() > MAX_TEXT_BYTES + 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ciphertext too large",
        ));
    }

    Ok(EncryptedTextPacket {
        kind,
        from,
        nonce,
        ciphertext,
    })
}

fn parse_nonce(value: &str) -> Option<[u8; NONCE_BYTES]> {
    let bytes = decode_hex(value)?;
    if bytes.len() != NONCE_BYTES {
        return None;
    }

    let mut nonce = [0_u8; NONCE_BYTES];
    nonce.copy_from_slice(&bytes);
    Some(nonce)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::crypto::generate_keypair;

    #[test]
    fn encrypted_packet_round_trip_preserves_header() {
        let packet = EncryptedTextPacket {
            kind: EncryptedPacketKind::Text,
            from: DeviceId::new("macbook").unwrap(),
            nonce: [7_u8; NONCE_BYTES],
            ciphertext: b"hello".to_vec(),
        };
        let mut bytes = Vec::new();

        write_packet(&mut bytes, &packet).unwrap();
        let decoded = read_packet(Cursor::new(bytes)).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn encrypt_then_decrypt_round_trip() {
        let (sender_private, sender_public) = generate_keypair();
        let (receiver_private, receiver_public) = generate_keypair();

        let encrypted = encrypt_text(
            &DeviceId::new("sender").unwrap(),
            &sender_private,
            &crate::crypto::encode_hex(&receiver_public),
            "hello from span",
        )
        .unwrap();

        let decrypted = decrypt_text(
            &encrypted,
            &receiver_private,
            &crate::crypto::encode_hex(&sender_public),
        )
        .unwrap();

        assert_eq!(decrypted, "hello from span");
    }

    #[test]
    fn pairing_packet_has_a_distinct_wire_type() {
        let (sender_private, _) = generate_keypair();
        let (_, receiver_public) = generate_keypair();
        let packet = encrypt_pairing_accept(
            &DeviceId::new("sender").unwrap(),
            &sender_private,
            &crate::crypto::encode_hex(&receiver_public),
            "pairing proof",
        )
        .unwrap();
        let mut bytes = Vec::new();
        write_packet(&mut bytes, &packet).unwrap();

        assert!(bytes.starts_with(PAIRING_MAGIC.as_bytes()));
        assert_eq!(
            read_packet(Cursor::new(bytes)).unwrap().kind,
            EncryptedPacketKind::PairingAccept
        );
    }

    #[test]
    fn packet_callback_error_does_not_stop_later_packets() {
        let packet = EncryptedTextPacket {
            kind: EncryptedPacketKind::Text,
            from: DeviceId::new("sender").unwrap(),
            nonce: [0; NONCE_BYTES],
            ciphertext: Vec::new(),
        };
        let mut calls = 0;
        let mut callback = |_packet| {
            calls += 1;
            if calls == 1 {
                Err(io::Error::new(io::ErrorKind::InvalidData, "bad packet"))
            } else {
                Ok(())
            }
        };

        dispatch_received_packet(Ok(packet.clone()), &mut callback);
        dispatch_received_packet(Ok(packet), &mut callback);

        assert_eq!(calls, 2);
    }
}
