//! Media over QUIC (MoQ) Transport Engine for sub-second P2P live video & voice channels.
//! Multiplexes media tracks into Groups and Objects over QUIC streams & datagrams.
//!
//! Wire format for groups carried on a QUIC stream (little-endian, binary):
//!   [u64 group_seq][u32 object_count]
//!   per object:
//!     [u32 track_id][u64 group_sequence][u64 object_sequence]
//!     [u8  track_type (0=keyframe,1=delta,2=audio)]
//!     [u64 timestamp_ms][u32 payload_len][payload bytes]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

/// Safety caps for untrusted mesh input (hostile peer guard).
const MAX_GROUP_OBJECTS: usize = 4096;
const MAX_OBJECT_PAYLOAD: usize = 8 * 1024 * 1024;
const MAX_GROUP_BYTES: usize = 64 * 1024 * 1024;

/// Encodes a group into the on-stream binary framing above. The payload is
/// copied; callers wanting zero-copy can slice `bytes[offset..]` per object.
pub fn encode_group_stream(group: &MoqGroup) -> Result<Vec<u8>, String> {
    let total_payload: usize = group.objects.iter().map(|o| o.payload.len()).sum();
    let mut out = Vec::with_capacity(8 + 4 + group.objects.len() * 40 + total_payload);
    encode_group_stream_to_writer(group, &mut out)?;
    Ok(out)
}

pub fn encode_group_stream_to_writer<W: Write>(
    group: &MoqGroup,
    out: &mut W,
) -> Result<(), String> {
    if group.objects.len() > MAX_GROUP_OBJECTS {
        return Err("moq group too many objects".to_string());
    }
    let total_payload: usize = group.objects.iter().map(|o| o.payload.len()).sum();
    if total_payload > MAX_GROUP_BYTES {
        return Err("moq group oversized".to_string());
    }
    push_u64(out, group.group_sequence)?;
    push_u32(out, group.objects.len() as u32)?;
    for obj in &group.objects {
        // Pack the 33-byte per-object header in one write:
        //   [u32 track_id][u64 group_seq][u64 obj_seq][u8 track_type][u64 timestamp_ms][u32 payload_len]
        let mut hdr = [0u8; 33];
        hdr[0..4].copy_from_slice(&obj.header.track_id.to_le_bytes());
        hdr[4..12].copy_from_slice(&obj.header.group_sequence.to_le_bytes());
        hdr[12..20].copy_from_slice(&obj.header.object_sequence.to_le_bytes());
        hdr[20] = obj.header.track_type as u8;
        hdr[21..29].copy_from_slice(&obj.header.timestamp_ms.to_le_bytes());
        hdr[29..33].copy_from_slice(&(obj.payload.len() as u32).to_le_bytes());
        out.write_all(&hdr)
            .map_err(|e| format!("moq write failed: {e}"))?;
        out.write_all(&obj.payload)
            .map_err(|e| format!("moq write failed: {e}"))?;
    }
    Ok(())
}

/// Decodes a group from the on-stream framing using zero-copy slicing.
/// Bounds-checked against hostile input; fails closed on any overrun or inconsistency.
pub fn decode_group_stream_bytes(bytes: Bytes) -> Result<MoqGroup, String> {
    if bytes.len() > MAX_GROUP_BYTES {
        return Err("moq stream oversized".to_string());
    }
    let mut pos = 0usize;
    let group_seq = read_u64(&bytes, &mut pos)?;
    let count = read_u32(&bytes, &mut pos)? as usize;
    if count > MAX_GROUP_OBJECTS {
        return Err("moq object count exceeds cap".to_string());
    }
    let mut objects = Vec::with_capacity(count);
    for _ in 0..count {
        let track_id = read_u32(&bytes, &mut pos)?;
        let obj_group_seq = read_u64(&bytes, &mut pos)?;
        let object_seq = read_u64(&bytes, &mut pos)?;
        let track_type = read_u8(&bytes, &mut pos)?;
        let track_type = match track_type {
            0 => MoqTrackType::VideoKeyframe,
            1 => MoqTrackType::VideoDelta,
            2 => MoqTrackType::AudioDatagram,
            _ => return Err("moq bad track type".to_string()),
        };
        let timestamp_ms = read_u64(&bytes, &mut pos)?;
        let payload_len = read_u32(&bytes, &mut pos)? as usize;
        if payload_len > MAX_OBJECT_PAYLOAD {
            return Err("moq object oversized".to_string());
        }
        let end = pos
            .checked_add(payload_len)
            .ok_or_else(|| "moq length overflow".to_string())?;
        if end > bytes.len() {
            return Err("moq truncated payload".to_string());
        }
        objects.push(MoqObject {
            header: MoqObjectHeader {
                track_id,
                group_sequence: obj_group_seq,
                object_sequence: object_seq,
                payload_size: payload_len as u32,
                track_type,
                timestamp_ms,
            },
            payload: bytes.slice(pos..end),
        });
        pos = end;
    }
    if pos != bytes.len() {
        return Err("moq trailing bytes".to_string());
    }
    Ok(MoqGroup {
        group_sequence: group_seq,
        objects,
    })
}

/// Decodes a group slice from on-stream binary framing into a zero-copy MoqGroup.
pub fn decode_group_stream(bytes: &[u8]) -> Result<MoqGroup, String> {
    decode_group_stream_bytes(Bytes::copy_from_slice(bytes))
}

fn push_u32<W: Write>(out: &mut W, v: u32) -> Result<(), String> {
    out.write_all(&v.to_le_bytes())
        .map_err(|e| format!("moq write failed: {e}"))
}

fn push_u64<W: Write>(out: &mut W, v: u64) -> Result<(), String> {
    out.write_all(&v.to_le_bytes())
        .map_err(|e| format!("moq write failed: {e}"))
}

fn read_u8(bytes: &[u8], pos: &mut usize) -> Result<u8, String> {
    let b = *bytes
        .get(*pos)
        .ok_or_else(|| "moq truncated header".to_string())?;
    *pos += 1;
    Ok(b)
}

fn read_u32(bytes: &[u8], pos: &mut usize) -> Result<u32, String> {
    let end = pos
        .checked_add(4)
        .ok_or_else(|| "moq length overflow".to_string())?;
    let slice = bytes
        .get(*pos..end)
        .ok_or_else(|| "moq truncated header".to_string())?;
    *pos = end;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64(bytes: &[u8], pos: &mut usize) -> Result<u64, String> {
    let end = pos
        .checked_add(8)
        .ok_or_else(|| "moq length overflow".to_string())?;
    let slice = bytes
        .get(*pos..end)
        .ok_or_else(|| "moq truncated header".to_string())?;
    *pos = end;
    let mut arr = [0u8; 8];
    arr.copy_from_slice(slice);
    Ok(u64::from_le_bytes(arr))
}

/// Type of MoQ media track payload (video keyframe, video delta frame, audio datagram).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MoqTrackType {
    VideoKeyframe,
    VideoDelta,
    AudioDatagram,
}

/// Header for an individual MoQ Object (sub-unit of a Group).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoqObjectHeader {
    pub track_id: u32,
    pub group_sequence: u64,
    pub object_sequence: u64,
    pub payload_size: u32,
    pub track_type: MoqTrackType,
    pub timestamp_ms: u64,
}

use bytes::Bytes;

/// An individual MoQ Object containing binary media payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoqObject {
    pub header: MoqObjectHeader,
    pub payload: Bytes,
}

/// Group of MoQ objects starting with a Keyframe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoqGroup {
    pub group_sequence: u64,
    pub objects: Vec<MoqObject>,
}

/// Live MoQ Publisher Session for broadcasting audio/video tracks over QUIC.
#[derive(Debug, Clone)]
pub struct MoqPublisherSession {
    pub stream_id: String,
    pub publisher_pubkey: String,
    current_group_seq: u64,
    tracks: HashMap<u32, String>,
}

impl MoqPublisherSession {
    pub fn new(stream_id: String, publisher_pubkey: String) -> Self {
        Self {
            stream_id,
            publisher_pubkey,
            current_group_seq: 0,
            tracks: HashMap::new(),
        }
    }

    pub fn register_track(&mut self, track_id: u32, name: &str) {
        self.tracks.insert(track_id, name.to_string());
    }

    /// Package a raw video frame or audio chunk into an MoQ Object.
    pub fn create_object(
        &mut self,
        track_id: u32,
        track_type: MoqTrackType,
        timestamp_ms: u64,
        payload: impl Into<Bytes>,
    ) -> MoqObject {
        let payload = payload.into();
        if track_type == MoqTrackType::VideoKeyframe {
            self.current_group_seq += 1;
        }

        MoqObject {
            header: MoqObjectHeader {
                track_id,
                group_sequence: self.current_group_seq,
                object_sequence: payload.len() as u64,
                payload_size: payload.len() as u32,
                track_type,
                timestamp_ms,
            },
            payload,
        }
    }
}

/// Live MoQ Subscriber Session for receiving and decoding P2P media streams.
#[derive(Debug, Clone)]
pub struct MoqSubscriberSession {
    pub stream_id: String,
    pub subscriber_pubkey: String,
    received_objects: Arc<Mutex<Vec<MoqObject>>>,
}

impl MoqSubscriberSession {
    pub fn new(stream_id: String, subscriber_pubkey: String) -> Self {
        Self {
            stream_id,
            subscriber_pubkey,
            received_objects: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn push_incoming_object(&self, object: MoqObject) {
        if let Ok(mut lock) = self.received_objects.lock() {
            lock.push(object);
            if lock.len() > 1000 {
                lock.drain(0..500);
            }
        }
    }

    pub fn drain_pending_objects(&self) -> Vec<MoqObject> {
        if let Ok(mut lock) = self.received_objects.lock() {
            std::mem::take(&mut *lock)
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_moq_framing() {
        let mut publisher =
            MoqPublisherSession::new("stream_123".to_string(), "pubkey_abc".to_string());
        publisher.register_track(1, "video");

        let keyframe =
            publisher.create_object(1, MoqTrackType::VideoKeyframe, 1000, vec![0x00, 0x01, 0x02]);
        assert_eq!(keyframe.header.group_sequence, 1);
        assert_eq!(keyframe.header.track_type, MoqTrackType::VideoKeyframe);

        let delta = publisher.create_object(1, MoqTrackType::VideoDelta, 1033, vec![0x03, 0x04]);
        assert_eq!(delta.header.group_sequence, 1);

        let sub = MoqSubscriberSession::new("stream_123".to_string(), "sub_456".to_string());
        sub.push_incoming_object(keyframe);
        sub.push_incoming_object(delta);

        let drained = sub.drain_pending_objects();
        assert_eq!(drained.len(), 2);
    }

    #[test]
    fn test_moq_group_stream_roundtrip() {
        let mut publisher =
            MoqPublisherSession::new("stream_123".to_string(), "pubkey_abc".to_string());
        publisher.register_track(1, "video");
        let keyframe =
            publisher.create_object(1, MoqTrackType::VideoKeyframe, 1000, vec![0x00, 0x01, 0x02]);
        let delta = publisher.create_object(1, MoqTrackType::VideoDelta, 1033, vec![0x03, 0x04]);
        let audio = publisher.create_object(1, MoqTrackType::AudioDatagram, 1050, vec![0x05; 64]);

        let group = MoqGroup {
            group_sequence: keyframe.header.group_sequence,
            objects: vec![keyframe, delta, audio],
        };
        let wire = encode_group_stream(&group).unwrap();
        let decoded = decode_group_stream(&wire).unwrap();
        assert_eq!(decoded.group_sequence, group.group_sequence);
        assert_eq!(decoded.objects.len(), 3);
        for (a, b) in decoded.objects.iter().zip(group.objects.iter()) {
            assert_eq!(a.header, b.header);
            assert_eq!(a.payload, b.payload);
        }
    }

    #[test]
    fn test_moq_group_stream_rejects_hostile_input() {
        assert!(decode_group_stream(&[]).is_err());
        // Truncated in the middle of an object header
        assert!(decode_group_stream(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).is_err());
        // Object count beyond cap
        let mut bad = Vec::new();
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&(MAX_GROUP_OBJECTS as u32 + 1).to_le_bytes());
        assert!(decode_group_stream(&bad).is_err());
        // Trailing garbage after a valid frame
        let mut publisher = MoqPublisherSession::new("s".to_string(), "k".to_string());
        let obj = publisher.create_object(1, MoqTrackType::VideoKeyframe, 1, vec![0xAA]);
        let group = MoqGroup {
            group_sequence: 1,
            objects: vec![obj],
        };
        let mut wire = encode_group_stream(&group).unwrap();
        wire.push(0xFF);
        assert!(decode_group_stream(&wire).is_err());
    }

    #[test]
    fn test_moq_group_stream_rejects_oversized_payload() {
        // Header lies about payload_len; body is truncated. The length field
        // is validated before any payload copy, so no oversized allocation.
        let mut bad = Vec::new();
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.push(0u8);
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&((MAX_OBJECT_PAYLOAD as u32) + 1).to_le_bytes());
        assert_eq!(
            decode_group_stream(&bad).unwrap_err(),
            "moq object oversized"
        );
    }

    #[test]
    fn test_moq_group_stream_rejects_oversized_group() {
        // Encode path: total payload bytes past the group cap.
        let mut publisher = MoqPublisherSession::new("s".to_string(), "k".to_string());
        let obj = publisher.create_object(
            1,
            MoqTrackType::VideoKeyframe,
            1,
            vec![0u8; MAX_GROUP_BYTES + 1],
        );
        let group = MoqGroup {
            group_sequence: 1,
            objects: vec![obj],
        };
        assert_eq!(
            encode_group_stream(&group).unwrap_err(),
            "moq group oversized"
        );
    }

    #[test]
    fn test_moq_group_stream_rejects_bad_track_type() {
        let mut bad = Vec::new();
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.push(3u8);
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.push(0xAA);
        assert_eq!(decode_group_stream(&bad).unwrap_err(), "moq bad track type");
    }

    #[test]
    fn test_moq_publisher_subscriber_roundtrip() {
        let stream_id = "test_stream_123".to_string();
        let publisher_pubkey = "pubkey_abc".to_string();
        let subscriber_pubkey = "sub_xyz".to_string();

        let mut publisher = MoqPublisherSession::new(stream_id.clone(), publisher_pubkey);
        publisher.register_track(1, "video");
        publisher.register_track(2, "audio");

        let subscriber = MoqSubscriberSession::new(stream_id, subscriber_pubkey);

        // Publisher creates a keyframe
        let keyframe = publisher.create_object(
            1,
            MoqTrackType::VideoKeyframe,
            1000,
            vec![0x00, 0x01, 0x02, 0x03],
        );
        assert_eq!(keyframe.header.group_sequence, 1);
        assert_eq!(keyframe.header.track_type, MoqTrackType::VideoKeyframe);

        // Publisher creates delta frames
        let delta1 = publisher.create_object(1, MoqTrackType::VideoDelta, 1033, vec![0x04, 0x05]);
        let delta2 = publisher.create_object(1, MoqTrackType::VideoDelta, 1066, vec![0x06, 0x07]);

        // Publisher creates audio
        let audio = publisher.create_object(2, MoqTrackType::AudioDatagram, 1050, vec![0x08; 64]);

        // Subscriber receives objects
        subscriber.push_incoming_object(keyframe.clone());
        subscriber.push_incoming_object(delta1.clone());
        subscriber.push_incoming_object(delta2.clone());
        subscriber.push_incoming_object(audio.clone());

        let drained = subscriber.drain_pending_objects();
        assert_eq!(drained.len(), 4);

        // Verify objects match
        assert_eq!(drained[0].header, keyframe.header);
        assert_eq!(drained[0].payload, keyframe.payload);
        assert_eq!(drained[1].header, delta1.header);
        assert_eq!(drained[2].header, delta2.header);
        assert_eq!(drained[3].header, audio.header);
    }
}
