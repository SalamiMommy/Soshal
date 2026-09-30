//! `SettingsRepo::set_many` — the multi-row upsert the sync engine's watermark
//! flush depends on.
//!
//! The load-bearing properties are that it agrees with `set` on every key it
//! touches, that the runtime-built `VALUES` list actually lines its bind
//! vector up with the `?n` numbering, and that the change bus is notified once
//! rather than once per key.

use soshal_db_core::repos::settings::SettingsRepo;
use soshal_db_core::Database;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn temp_db(tag: &str) -> (Database, String) {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir()
        .join(format!(
            "soshal_setmany_{tag}_{}_{n}.db",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned();
    let db = Database::open(&path).unwrap();
    db.migrate().unwrap();
    (db, path)
}

fn cleanup(path: &str) {
    for p in [
        path.to_string(),
        format!("{path}-wal"),
        format!("{path}-shm"),
    ] {
        let _ = std::fs::remove_file(&p);
    }
}

#[test]
fn empty_slice_is_a_noop() {
    let (db, path) = temp_db("empty");
    SettingsRepo::new(&db).set_many(&[]).unwrap();
    assert!(SettingsRepo::new(&db).get_all().unwrap().is_empty());
    drop(db);
    cleanup(&path);
}

#[test]
fn agrees_with_set_key_for_key() {
    let (db, path) = temp_db("agree");
    let repo = SettingsRepo::new(&db);

    repo.set("alpha", "1").unwrap();
    repo.set("beta", "2").unwrap();
    let before: std::collections::HashMap<String, String> =
        repo.get_all().unwrap().into_iter().collect();

    // Overwrite both through the batch path.
    repo.set_many(&[("alpha", "11"), ("beta", "22")]).unwrap();
    // And add one that set did not know about.
    repo.set_many(&[("gamma", "33")]).unwrap();
    let after: std::collections::HashMap<String, String> =
        repo.get_all().unwrap().into_iter().collect();

    assert_eq!(before.get("alpha"), Some(&"1".to_string()));
    assert_eq!(
        after.get("alpha"),
        Some(&"11".to_string()),
        "upsert must update"
    );
    assert_eq!(
        after.get("beta"),
        Some(&"22".to_string()),
        "upsert must update"
    );
    assert_eq!(
        after.get("gamma"),
        Some(&"33".to_string()),
        "upsert must insert"
    );
    assert_eq!(after.len(), 3, "no stray rows");

    drop(db);
    cleanup(&path);
}

/// The real hazard: `VALUES` is built at runtime, so a mismatch between the
/// `?n` numbering and the bind vector would silently write the *wrong value
/// under the wrong key* — a watermark stored against a different watermark's
/// key. Every value here is distinct so any rotation shows up.
#[test]
fn values_line_up_with_binds_across_many_rows() {
    let (db, path) = temp_db("align");
    let repo = SettingsRepo::new(&db);
    let pairs: Vec<(String, String)> = (0..17)
        .map(|i| (format!("k{i}"), format!("v{i}")))
        .collect();
    let slice: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    repo.set_many(&slice).unwrap();

    let read: std::collections::HashMap<String, String> =
        repo.get_all().unwrap().into_iter().collect();
    for (k, v) in &pairs {
        assert_eq!(
            read.get(k).map(String::as_str),
            Some(v.as_str()),
            "key {k} got the wrong value — bind vector is rotated"
        );
    }

    drop(db);
    cleanup(&path);
}

#[test]
fn notifies_the_change_bus_once_not_once_per_key() {
    let (db, path) = temp_db("notify");
    let mut rx = db.subscribe_changes();

    SettingsRepo::new(&db)
        .set_many(&[("a", "1"), ("b", "2"), ("c", "3")])
        .unwrap();

    // The first notification must arrive...
    let first = soshal_db_core::block_on(async {
        tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await
    });
    assert!(first.is_ok(), "batch write must notify at least once");

    // ...and there must not be a second one waiting, which is what three
    // separate `set` calls would have produced.
    let extra = soshal_db_core::block_on(async {
        tokio::time::timeout(std::time::Duration::from_millis(150), rx.recv()).await
    });
    assert!(
        extra.is_err(),
        "a 3-key batch must notify once, not three times (got a second event)"
    );

    drop(db);
    cleanup(&path);
}

#[test]
fn values_with_quotes_and_unicode_survive() {
    let (db, path) = temp_db("quotes");
    let repo = SettingsRepo::new(&db);
    let pairs: [(&str, &str); 3] = [
        ("has'quote", "it's fine"),
        ("ünïcödé", "välue ✓ 🎉"),
        ("empty", ""),
    ];
    repo.set_many(&pairs).unwrap();

    assert_eq!(repo.get("has'quote").unwrap().as_deref(), Some("it's fine"));
    assert_eq!(repo.get("ünïcödé").unwrap().as_deref(), Some("välue ✓ 🎉"));
    assert_eq!(repo.get("empty").unwrap().as_deref(), Some(""));

    drop(db);
    cleanup(&path);
}

#[test]
fn a_partial_overlap_only_touches_the_named_keys() {
    let (db, path) = temp_db("partial");
    let repo = SettingsRepo::new(&db);
    repo.set("keep", "original").unwrap();
    repo.set("change", "before").unwrap();

    repo.set_many(&[("change", "after")]).unwrap();

    assert_eq!(
        repo.get("keep").unwrap().as_deref(),
        Some("original"),
        "a key not in the batch must be untouched"
    );
    assert_eq!(repo.get("change").unwrap().as_deref(), Some("after"));

    drop(db);
    cleanup(&path);
}
