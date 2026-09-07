//! Reticulum binary packet parsing and generation.

use super::address::ReticulumAddress;
use serde::{Deserialize, Serialize};

pub const MAX_HOPS: u8 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ReticulumPacketType {
    Data = 0x00,
    Announce = 0x01,
    LinkRequest = 0x02,
    Proof = 0x03,
}

impl ReticulumPacketType {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x00 => Some(Self::Data),
            0x01 => Some(Self::Announce),
            0x02 => Some(Self::LinkRequest),
            0x03 => Some(Self::Proof),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReticulumPacket {
    pub flags: u8,
    pub hops: u8,
    pub destination: ReticulumAddress,
    pub packet_type: ReticulumPacketType,
    pub payload: Vec<u8>,
}

impl ReticulumPacket {
    pub fn new(
        destination: ReticulumAddress,
        packet_type: ReticulumPacketType,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            flags: 0x00,
            hops: 0,
            destination,
            packet_type,
            payload,
        }
    }

    /// Serializes the packet into a byte vector for wire transmission.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + 1 + 16 + 1 + self.payload.len());
        out.push(self.flags);
        out.push(self.hops);
        out.extend_from_slice(&self.destination.0);
        out.push(self.packet_type as u8);
        out.extend_from_slice(&self.payload);
        out
    }

    /// Parses raw wire bytes into a ReticulumPacket.
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() < 19 {
            return Err("Packet too short (minimum 19 bytes)".to_string());
        }
        let flags = data[0];
        let hops = data[1];
        if hops > MAX_HOPS {
            return Err(format!("Exceeded MAX_HOPS: {}", hops));
        }

        let mut dest_bytes = [0u8; 16];
        dest_bytes.copy_from_slice(&data[2..18]);
        let destination = ReticulumAddress::from_bytes(dest_bytes);

        let packet_type_u8 = data[18];
        let packet_type = ReticulumPacketType::from_u8(packet_type_u8)
            .ok_or_else(|| format!("Unknown packet type 0x{:02x}", packet_type_u8))?;

        let payload = data[19..].to_vec();

        Ok(Self {
            flags,
            hops,
            destination,
            packet_type,
            payload,
        })
    }

    /// Returns a new packet with hop count incremented by 1.
    pub fn increment_hops(&self) -> Option<Self> {
        if self.hops >= MAX_HOPS {
            None
        } else {
            let mut next = self.clone();
            next.hops += 1;
            Some(next)
        }
    }

    /// Increments hop count in-place by 1. Returns false if MAX_HOPS is reached.
    pub fn increment_hops_in_place(&mut self) -> bool {
        if self.hops >= MAX_HOPS {
            false
        } else {
            self.hops += 1;
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_serialization_roundtrip() {
        let addr = ReticulumAddress::from_pubkey("test_pubkey");
        let pkt = ReticulumPacket::new(addr, ReticulumPacketType::Announce, vec![1, 2, 3, 4, 5]);
        let bytes = pkt.to_bytes();
        let parsed = ReticulumPacket::from_bytes(&bytes).unwrap();
        assert_eq!(pkt, parsed);
    }

    #[test]
    fn test_hop_count_increment() {
        let addr = ReticulumAddress::from_pubkey("test");
        let mut pkt = ReticulumPacket::new(addr, ReticulumPacketType::Data, vec![]);
        pkt.hops = 127;
        let inc = pkt.increment_hops().unwrap();
        assert_eq!(inc.hops, 128);
        assert!(inc.increment_hops().is_none());
    }
}
