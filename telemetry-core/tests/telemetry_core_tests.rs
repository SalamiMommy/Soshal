use soshal_telemetry_core::ring::{
    ffi_advance_head, ffi_drain, ffi_pending, register_ring, DrainedEntry,
};
use soshal_telemetry_core::{RecordKind, Recorder, DUMP_MAGIC};

fn tmp_recorder(name: &str) -> std::path::PathBuf {
    soshal_test_util::tmp_path("telemetry-it", name)
}

#[test]
fn record_bytes_binary_payload_roundtrip() {
    let p = tmp_recorder("recbin.bin");
    let mut r = Recorder::init(&p, 64 * 1024).unwrap();
    let payload = [0u8, 1, 2, 3, 255, 254, 0x10, 0x20, b'A', b'Z'];
    r.record_bytes(RecordKind::Network, &payload).unwrap();
    let entries = r.read_all().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, RecordKind::Network);
    assert_eq!(entries[0].2, payload);
}

#[test]
fn record_bytes_all_kinds_roundtrip() {
    let p = tmp_recorder("reckinds.bin");
    let mut r = Recorder::init(&p, 64 * 1024).unwrap();
    for (i, kind) in [
        RecordKind::State,
        RecordKind::Ipc,
        RecordKind::Network,
        RecordKind::Ffi,
        RecordKind::App,
        RecordKind::Crash,
    ]
    .iter()
    .enumerate()
    {
        r.record_bytes(*kind, format!("kind-{i}").as_bytes())
            .unwrap();
    }
    let entries = r.read_all().unwrap();
    assert_eq!(entries.len(), 6);
    assert_eq!(
        entries.iter().map(|e| e.0).collect::<Vec<_>>(),
        vec![
            RecordKind::State,
            RecordKind::Ipc,
            RecordKind::Network,
            RecordKind::Ffi,
            RecordKind::App,
            RecordKind::Crash,
        ]
    );
}

#[test]
fn record_bytes_oversized_payload_rejected() {
    let p = tmp_recorder("recbig.bin");
    let mut r = Recorder::init(&p, 64 * 1024).unwrap();
    let big = vec![0xabu8; 4097];
    assert!(r.record_bytes(RecordKind::App, &big).is_err());
    assert!(r.read_all().unwrap().is_empty());
}

#[test]
fn key_path_suffix_and_persistence() {
    let p = tmp_recorder("keypath.bin");
    let mut r = Recorder::init(&p, 64 * 1024).unwrap();
    let kp = r.key_path().to_path_buf();
    assert_ne!(kp, p);
    assert!(kp.to_string_lossy().ends_with(".key"));
    assert_eq!(kp.parent(), p.parent());
    assert!(kp.exists());
    r.record(RecordKind::State, "pre-reopen").unwrap();
    let dump = r.dump_encrypted().unwrap();
    assert_eq!(&dump[..8], &DUMP_MAGIC[..]);

    let r2 = Recorder::init(&p, 64 * 1024).unwrap();
    assert_eq!(r2.key_path(), kp);
    let plain = r2.decrypt_dump(&dump).unwrap();
    assert!(plain.windows(10).any(|w| w == b"pre-reopen"));
}

#[test]
fn ring_register_capacity_and_head() {
    let p = tmp_recorder("ring.bin");
    let addr = register_ring(&p, 64 * 1024).unwrap();
    assert!(addr > 0);
    assert!(ffi_pending(addr).unwrap() == 0);
    assert!(ffi_advance_head(addr, 0).is_ok());
    let entries = ffi_drain(addr).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn ring_unknown_addr_errors() {
    assert!(ffi_pending(0xdeadbeef).is_err());
    assert!(ffi_advance_head(0xdeadbeef, 0).is_err());
    assert!(ffi_drain(0xdeadbeef).is_err());
}

#[test]
fn ring_head_rewind_and_capacity_errors() {
    let p = tmp_recorder("ring2.bin");
    let addr = register_ring(&p, 64 * 1024).unwrap();
    assert!(ffi_advance_head(addr, 100).is_ok());
    assert!(ffi_advance_head(addr, 99).is_err());
    let cap = 64 * 1024;
    assert!(ffi_advance_head(addr, 100 + cap as u64 + 1).is_err());
    assert_eq!(ffi_pending(addr).unwrap(), 100);
}

#[test]
fn ring_ffi_drain_returns_entries() {
    let p = tmp_recorder("ring3.bin");
    let addr = register_ring(&p, 64 * 1024).unwrap();
    let total = (4 + 1 + 8 + 4 + 5 + 3) & !3;
    let base = 4096;
    write_entry(&p, addr, total, 5, b"hello");
    assert_eq!(ffi_pending(addr).unwrap(), total as u64);
    let entries: Vec<DrainedEntry> = ffi_drain(addr).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, 5);
    assert_eq!(entries[0].payload, b"hello");
    assert_eq!(ffi_pending(addr).unwrap(), 0);
    assert!(ffi_drain(addr).unwrap().is_empty());
    assert!(base > 0);
}

fn write_entry(p: &std::path::Path, _addr: usize, total: usize, kind: u8, payload: &[u8]) {
    let mut ring = soshal_telemetry_core::ring::SharedRing::init(p, 64 * 1024).unwrap();
    ring.advance_head(0).unwrap();
    let idx = (ring.head() % ring.capacity() as u64) as usize;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(p)
        .unwrap();
    #[allow(unsafe_code)]
    let mut map = unsafe {
        memmap2::MmapOptions::new()
            .len(4096 + ring.capacity())
            .map_mut(&file)
            .unwrap()
    };
    let base = 4096 + idx;
    map[base..base + 4].copy_from_slice(&(total as u32).to_le_bytes());
    map[base + 4] = kind;
    map[base + 13..base + 17].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    map[base + 4 + 1 + 8 + 4..base + 4 + 1 + 8 + 4 + payload.len()].copy_from_slice(payload);
    map[0..8].copy_from_slice(&(total as u64).to_le_bytes());
    map.flush().unwrap();
}
