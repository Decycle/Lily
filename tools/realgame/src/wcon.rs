//! WCON v1 framing used by Waccon.Server (TCP 52468 and WebSocket `/ws` on 52469).
//!
//! Source: samnyan/Waccon-IO `Waccon-Server/WconProtocol.cs`, and the shipped `Waccon/web/controller.js`.
//! Header (24 bytes, little-endian): "WCON", u16 version 0x0100, u16 type, u32 payload length, u32 sequence,
//! u64 sender timestamp (microseconds; informational).

use crate::cells::CELLS;
use thiserror::Error;

pub const HEADER_LEN: usize = 24;
pub const VERSION: u16 = 0x0100;
/// opbtn, gamebtn, 240 cells, u32 source id, u64 timestamp_us, u64 lease_ms (the server overwrites the lease).
pub const INPUT_PAYLOAD_LEN: usize = 2 + CELLS + 4 + 8 + 8;
const SOURCE_ID_OFFSET: usize = 2 + CELLS;
const TIMESTAMP_OFFSET: usize = SOURCE_ID_OFFSET + 4;
const LEASE_OFFSET: usize = TIMESTAMP_OFFSET + 8;

/// Operator buttons (`opbtn`) as segatools' mercuryio API defines them.
pub const OPBTN_TEST: u8 = 0x01;
pub const OPBTN_SERVICE: u8 = 0x02;
pub const OPBTN_COIN: u8 = 0x04;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    Hello = 1,
    Welcome = 2,
    InputSnapshot = 3,
    ClearInput = 4,
    Ping = 5,
    Pong = 6,
    LedSnapshot = 7,
    Error = 8,
}

impl MessageType {
    fn from_u16(v: u16) -> Option<Self> {
        use MessageType::*;
        [
            Hello,
            Welcome,
            InputSnapshot,
            ClearInput,
            Ping,
            Pong,
            LedSnapshot,
            Error,
        ]
        .into_iter()
        .find(|t| *t as u16 == v)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WconError {
    #[error("frame shorter than the 24-byte header")]
    Truncated,
    #[error("bad magic")]
    Magic,
    #[error("unsupported version {0:#06x}")]
    Version(u16),
    #[error("unknown message type {0}")]
    Type(u16),
    #[error("payload length {declared} does not match {actual}")]
    Length { declared: u32, actual: usize },
}

/// One decoded frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: MessageType,
    pub sequence: u32,
    pub timestamp_us: u64,
    pub payload: Vec<u8>,
}

/// Encodes a frame.
pub fn encode(kind: MessageType, sequence: u32, timestamp_us: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(b"WCON");
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(kind as u16).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&sequence.to_le_bytes());
    out.extend_from_slice(&timestamp_us.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Decodes exactly one complete frame.
pub fn decode(bytes: &[u8]) -> Result<Frame, WconError> {
    if bytes.len() < HEADER_LEN {
        return Err(WconError::Truncated);
    }
    if &bytes[..4] != b"WCON" {
        return Err(WconError::Magic);
    }
    let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    let u32_at =
        |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    let version = u16_at(4);
    if version != VERSION {
        return Err(WconError::Version(version));
    }
    let raw_type = u16_at(6);
    let kind = MessageType::from_u16(raw_type).ok_or(WconError::Type(raw_type))?;
    let declared = u32_at(8);
    let actual = bytes.len() - HEADER_LEN;
    if declared as usize != actual {
        return Err(WconError::Length { declared, actual });
    }
    let mut ts = [0; 8];
    ts.copy_from_slice(&bytes[16..24]);
    Ok(Frame {
        kind,
        sequence: u32_at(12),
        timestamp_us: u64::from_le_bytes(ts),
        payload: bytes[HEADER_LEN..].to_vec(),
    })
}

/// The 262-byte input record shared by WCON `InputSnapshot` payloads and the shared buffer.
pub fn input_payload(
    opbtn: u8,
    gamebtn: u8,
    cells: &[u8; CELLS],
    source_id: u32,
    timestamp_us: u64,
    lease_ms: u64,
) -> [u8; INPUT_PAYLOAD_LEN] {
    let mut p = [0; INPUT_PAYLOAD_LEN];
    p[0] = opbtn;
    p[1] = gamebtn;
    for (dst, &c) in p[2..2 + CELLS].iter_mut().zip(cells) {
        *dst = u8::from(c != 0);
    }
    p[SOURCE_ID_OFFSET..TIMESTAMP_OFFSET].copy_from_slice(&source_id.to_le_bytes());
    p[TIMESTAMP_OFFSET..LEASE_OFFSET].copy_from_slice(&timestamp_us.to_le_bytes());
    p[LEASE_OFFSET..].copy_from_slice(&lease_ms.to_le_bytes());
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_matches_controller_js() {
        // controller.js: set "WCON", u16 256, u16 type, u32 len, u32 seq, u64 Date.now()*1000.
        let f = encode(MessageType::Hello, 7, 0x0102_0304_0506_0708, &[]);
        assert_eq!(
            f,
            [
                b'W', b'C', b'O', b'N', 0x00, 0x01, 1, 0, 0, 0, 0, 0, 7, 0, 0, 0, 8, 7, 6, 5, 4, 3,
                2, 1
            ]
        );
    }

    #[test]
    fn input_round_trip() {
        let mut cells = [0; CELLS];
        cells[14] = 1;
        cells[239] = 5; // normalised to 1
        let p = input_payload(OPBTN_COIN, 0, &cells, 0xAABB_CCDD, 1_000_000, 500);
        assert_eq!(p.len(), 262);
        assert_eq!((p[0], p[2 + 14], p[2 + 239], p[2 + 15]), (4, 1, 1, 0));
        assert_eq!(&p[242..246], &0xAABB_CCDDu32.to_le_bytes());
        assert_eq!(&p[254..262], &500u64.to_le_bytes());
        let frame = decode(&encode(MessageType::InputSnapshot, 1, 2, &p)).unwrap();
        assert_eq!(frame.kind, MessageType::InputSnapshot);
        assert_eq!(frame.payload, p);
    }

    #[test]
    fn rejects_bad_frames() {
        let good = encode(MessageType::Ping, 0, 0, &[1, 2]);
        assert_eq!(decode(&good[..10]), Err(WconError::Truncated));
        let mut bad = good.clone();
        bad[0] = b'X';
        assert_eq!(decode(&bad), Err(WconError::Magic));
        let mut bad = good.clone();
        bad[6] = 99;
        assert_eq!(decode(&bad), Err(WconError::Type(99)));
        assert_eq!(
            decode(&good[..25]),
            Err(WconError::Length {
                declared: 2,
                actual: 1
            })
        );
    }
}
