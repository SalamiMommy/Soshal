//! Flight recorder FFI: encrypted circular event log.
//! One recorder per process, backed by a mmap'd file. Crash markers seal it;
//! dumps are XChaCha20-Poly1305 envelopes (see soshal-telemetry-core).

use flutter_rust_bridge::frb;
use soshal_telemetry_core::{RecordKind, Recorder};
use std::path::Path;
use std::sync::Mutex;

static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);

fn lock() -> Result<std::sync::MutexGuard<'static, Option<Recorder>>, String> {
    RECORDER
        .lock()
        .map_err(|_| "recorder mutex poisoned".to_string())
}

/// Open (or create) the flight recorder at `path` with `capacity_mb` MiB of
/// ring space. Safe to call repeatedly; reopens the same file.
#[frb(sync, serialize)]
pub fn telemetry_init(path: String, capacity_mb: u32) -> Result<(), String> {
    let capacity = (capacity_mb.clamp(1, 4096) as usize) * 1024 * 1024;
    let recorder = Recorder::init(Path::new(&path), capacity)?;
    *lock()? = Some(recorder);
    Ok(())
}

/// Append a record. kind: 1=State 2=Ipc 3=Network 4=Ffi 5=App 6=Crash.
#[frb(sync, serialize)]
pub fn telemetry_record(kind: u8, msg: String) -> Result<(), String> {
    let kind = match kind {
        1 => RecordKind::State,
        2 => RecordKind::Ipc,
        3 => RecordKind::Network,
        4 => RecordKind::Ffi,
        5 => RecordKind::App,
        6 => RecordKind::Crash,
        _ => return Err(format!("unknown record kind {kind}")),
    };
    lock()?
        .as_mut()
        .ok_or_else(|| "recorder not initialized".to_string())?
        .record(kind, &msg)
        .into()
}

/// Seal the recorder with a crash reason; blocks further writes.
#[frb(sync, serialize)]
pub fn telemetry_mark_crash(reason: String) -> Result<(), String> {
    lock()?
        .as_mut()
        .ok_or_else(|| "recorder not initialized".to_string())?
        .mark_crash(&reason)
        .into()
}

/// Export the entire ring as an encrypted dump envelope (Map<u8,...>).
#[frb(sync, serialize)]
pub fn telemetry_dump_encrypted() -> Result<Vec<u8>, String> {
    lock()?
        .as_mut()
        .ok_or_else(|| "recorder not initialized".to_string())?
        .dump_encrypted()
        .into()
}

/// JSON info: {"capacity_bytes","used_bytes","entries","sealed"}.
#[frb(sync, serialize)]
pub fn telemetry_info_json() -> Result<String, String> {
    let info = lock()?
        .as_ref()
        .ok_or_else(|| "recorder not initialized".to_string())?
        .info()?;
    serde_json::to_string(&serde_json::json!({
        "capacity_bytes": info.capacity_bytes,
        "used_bytes": info.used_bytes,
        "entries": info.entries,
        "sealed": info.sealed,
    }))
    .map_err(super::util::to_err)
    .into()
}

#[frb(sync, serialize)]
pub fn telemetry_is_sealed() -> bool {
    let guard = lock().ok();
    guard
        .as_ref()
        .and_then(|g| g.as_ref())
        .map(|r| r.is_sealed())
        .unwrap_or(false)
}

/// Drops all events (key file stays).
#[frb(sync, serialize)]
pub fn telemetry_clear() -> Result<(), String> {
    lock()?
        .as_mut()
        .ok_or_else(|| "recorder not initialized".to_string())?
        .clear()
        .into()
}

/// JSON of recorded events: [[kind, ts_ms, payload], ...] (crash viewer).
#[frb(sync, serialize)]
pub fn telemetry_read_all_json() -> Result<String, String> {
    let entries = lock()?
        .as_ref()
        .ok_or_else(|| "recorder not initialized".to_string())?
        .read_all()?;
    let arr: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|(kind, ts, payload)| {
            serde_json::json!([kind as u8, ts, String::from_utf8_lossy(&payload)])
        })
        .collect();
    serde_json::to_string(&arr)
        .map_err(super::util::to_err)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset() {
        *lock().unwrap() = None;
    }

    fn init(p: &std::path::Path) {
        telemetry_init(p.to_string_lossy().to_string(), 1).unwrap();
    }

    #[test]
    fn test_init_and_info_json() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p = soshal_test_util::tmp_path("telemetry", "rec.bin");
        reset();
        init(&p);
        let v: serde_json::Value = serde_json::from_str(&telemetry_info_json().unwrap()).unwrap();
        assert_eq!(v["capacity_bytes"], 1024 * 1024);
        assert_eq!(v["used_bytes"], 0);
        assert_eq!(v["entries"], 0);
        assert_eq!(v["sealed"], false);
        assert!(!telemetry_is_sealed());
    }

    #[test]
    fn test_record_and_read_all() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p = soshal_test_util::tmp_path("telemetry", "rec.bin");
        reset();
        init(&p);
        telemetry_record(1, "signed-in".to_string()).unwrap();
        telemetry_record(3, "relay up".to_string()).unwrap();
        telemetry_record(5, "rendered".to_string()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&telemetry_read_all_json().unwrap()).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0][0], 1);
        assert_eq!(arr[0][2], "signed-in");
        assert_eq!(arr[1][0], 3);
        assert_eq!(arr[2][0], 5);
        let info: serde_json::Value =
            serde_json::from_str(&telemetry_info_json().unwrap()).unwrap();
        assert_eq!(info["entries"], 3);
        assert!(info["used_bytes"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_record_unknown_kind_errors() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p = soshal_test_util::tmp_path("telemetry", "rec.bin");
        reset();
        init(&p);
        assert!(telemetry_record(99, "bogus".to_string()).is_err());
        assert!(telemetry_record(0, "bogus".to_string()).is_err());
    }

    #[test]
    fn test_uninitialized_errors() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert!(telemetry_record(1, "x".to_string()).is_err());
        assert!(telemetry_info_json().is_err());
        assert!(telemetry_read_all_json().is_err());
        assert!(telemetry_mark_crash("boom".to_string()).is_err());
        assert!(telemetry_clear().is_err());
        assert!(telemetry_dump_encrypted().is_err());
        assert!(!telemetry_is_sealed());
    }

    #[test]
    fn test_crash_seals_and_blocks_writes() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p = soshal_test_util::tmp_path("telemetry", "rec.bin");
        reset();
        init(&p);
        telemetry_mark_crash("fatal: oom".to_string()).unwrap();
        assert!(telemetry_is_sealed());
        assert!(telemetry_record(1, "late".to_string()).is_err());
        let v: serde_json::Value =
            serde_json::from_str(&telemetry_read_all_json().unwrap()).unwrap();
        let arr = v.as_array().unwrap();
        assert!(arr.iter().any(|e| e[0] == 6 && e[2] == "fatal: oom"));
        let info: serde_json::Value =
            serde_json::from_str(&telemetry_info_json().unwrap()).unwrap();
        assert_eq!(info["sealed"], true);
    }

    #[test]
    fn test_clear_wipes_entries() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p = soshal_test_util::tmp_path("telemetry", "rec.bin");
        reset();
        init(&p);
        telemetry_record(2, "ffi-call".to_string()).unwrap();
        telemetry_clear().unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&telemetry_read_all_json().unwrap()).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 0);
        let info: serde_json::Value =
            serde_json::from_str(&telemetry_info_json().unwrap()).unwrap();
        assert_eq!(info["entries"], 0);
        assert_eq!(info["used_bytes"], 0);
    }

    #[test]
    fn test_dump_encrypted() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p = soshal_test_util::tmp_path("telemetry", "rec.bin");
        reset();
        init(&p);
        telemetry_record(4, "ffi-boot".to_string()).unwrap();
        let dump = telemetry_dump_encrypted().unwrap();
        assert!(!dump.is_empty());
        assert!(dump.len() > 16);
    }

    #[test]
    fn test_reopen_persists_records() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p = soshal_test_util::tmp_path("telemetry", "rec.bin");
        reset();
        init(&p);
        telemetry_record(1, "persist-me".to_string()).unwrap();
        init(&p);
        let v: serde_json::Value =
            serde_json::from_str(&telemetry_read_all_json().unwrap()).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0][2], "persist-me");
    }
}
