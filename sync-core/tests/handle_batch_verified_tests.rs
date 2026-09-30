//! `ingest::handle_batch_verified` — the pre-verified batch entry point that
//! the mesh drain path uses.
//!
//! The contract is that the caller has already checked each event's signature.
//! These tests pin what that must and must not mean:
//!
//! * for a genuinely-signed batch it must write exactly what `handle_batch`
//!   writes (it is the same code path, minus the redundant verification);
//! * it must still apply every non-signature gate — kind allowlist, dedup,
//!   p-tag-to-me, deleted-post handling — so "pre-verified" is not a licence
//!   to skip the rest of the pipeline;
//! * `handle_batch` must still reject a tampered event, i.e. the split into a
//!   masked body did not accidentally leave the mask empty.

use nostr::event::FinalizeEvent;
use soshal_db_core::Database;
use soshal_sync_core::ingest::{handle_batch, handle_batch_verified};
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn temp_db(tag: &str) -> (Database, String) {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir()
        .join(format!(
            "soshal_batchverified_{tag}_{}_{n}.db",
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

fn my_key() -> nostr::key::Keys {
    nostr::key::Keys::generate()
}

/// Default (current-time) timestamps. These tests are about which events land,
/// not about time ordering, and `nostr` 0.45's `custom_created_at` returns a
/// different builder type that has no `finalize`.
fn text_note(keys: &nostr::key::Keys, content: &str) -> nostr::event::Event {
    nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, content)
        .finalize(keys)
        .unwrap()
}

fn tx() -> (
    tokio::sync::mpsc::Sender<soshal_sync_core::SyncUpdate>,
    tokio::sync::mpsc::Receiver<soshal_sync_core::SyncUpdate>,
) {
    tokio::sync::mpsc::channel(64)
}

fn post_ids(db: &Database) -> Vec<String> {
    let conn = db.conn().unwrap();
    soshal_db_core::block_on(async {
        let mut r = conn
            .query("SELECT id FROM posts ORDER BY id", ())
            .await
            .unwrap();
        let mut out = Vec::new();
        while let Some(row) = r.next().await.unwrap() {
            out.push(row.get::<String>(0).unwrap());
        }
        out
    })
}

/// The load-bearing equivalence: a valid batch takes the identical path and
/// lands identical rows whether or not the caller pre-verified. If this
/// diverges, `handle_batch_verified` is not `handle_batch` minus a redundant
/// check.
#[test]
fn verified_batch_writes_exactly_what_the_verifying_batch_writes() {
    let keys = my_key();
    let other = my_key();
    let events = vec![
        text_note(&keys, "first"),
        text_note(&other, "second"),
        text_note(&keys, "third"),
    ];

    let (db_verify, path_verify) = temp_db("verify");
    let (db_pre, path_pre) = temp_db("pre");

    let (tx_v, _rx_v) = tx();
    let ok_v = handle_batch(&db_verify, &keys.public_key().to_hex(), &events, &tx_v).unwrap();
    let (tx_p, _rx_p) = tx();
    let ok_p = handle_batch_verified(&db_pre, &keys.public_key().to_hex(), &events, &tx_p).unwrap();

    assert_eq!(
        ok_v, ok_p,
        "the same events must report the same success set"
    );
    assert_eq!(ok_v.len(), 3, "all three are allowlisted text notes");
    assert_eq!(post_ids(&db_verify), post_ids(&db_pre));
    assert_eq!(post_ids(&db_verify).len(), 3);

    drop(db_verify);
    drop(db_pre);
    cleanup(&path_verify);
    cleanup(&path_pre);
}

/// `handle_batch` must still drop a tampered event. If the refactor into a
/// masked body ever produced an empty (or all-true) mask on the verifying
/// path, this is what would notice.
#[test]
fn the_verifying_batch_still_rejects_a_tampered_event() {
    let keys = my_key();
    let good = text_note(&keys, "honest");
    let good_id = good.id.to_hex();
    let mut tampered = text_note(&keys, "tampered");
    // Content changed after signing: the id no longer matches the payload.
    tampered.content = "tampered, actually".to_string();

    let (db, path) = temp_db("tamper");
    let (tx, _rx) = tx();
    let ok = handle_batch(&db, &keys.public_key().to_hex(), &[good, tampered], &tx).unwrap();

    assert_eq!(ok.len(), 1, "only the honest event may be written");
    let ids = post_ids(&db);
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], good_id);

    drop(db);
    cleanup(&path);
}

/// The converse: `handle_batch_verified` writes the tampered event, because that
/// is the contract — it trusts the caller. Written out explicitly so the
/// contract is visible in a test rather than only in a doc comment, and so
/// nobody later "fixes" the pre-verified path into silently re-verifying.
#[test]
fn the_preverified_batch_trusts_its_caller() {
    let keys = my_key();
    let mut tampered = text_note(&keys, "tampered");
    tampered.content = "tampered, actually".to_string();

    let (db, path) = temp_db("trust");
    let (tx, _rx) = tx();
    let ok =
        handle_batch_verified(&db, &keys.public_key().to_hex(), &[tampered.clone()], &tx).unwrap();

    assert_eq!(ok.len(), 1, "the pre-verified path does not re-check");
    let ids = post_ids(&db);
    assert_eq!(ids, vec![tampered.id.to_hex()]);

    drop(db);
    cleanup(&path);
}

/// Empty input is a no-op on both, and must not open a transaction.
#[test]
fn an_empty_batch_is_a_no_op() {
    let (db, path) = temp_db("empty");
    let (tx, _rx) = tx();
    let pk = my_key().public_key().to_hex();
    assert!(handle_batch(&db, &pk, &[], &tx).unwrap().is_empty());
    assert!(handle_batch_verified(&db, &pk, &[], &tx)
        .unwrap()
        .is_empty());
    assert!(post_ids(&db).is_empty());

    drop(db);
    cleanup(&path);
}

/// Non-allowlisted kinds are dropped by both paths. `handle_batch_verified`
/// must not become a way to get an unmodelled kind into `posts` just because
/// the signature check was skipped — the allowlist is a separate gate.
#[test]
fn the_kind_allowlist_still_applies_to_the_preverified_path() {
    let keys = my_key();
    // A kind the app does not model (NIP-04 legacy encrypted DM payload shape
    // aside, this is a metadata-kind variant outside POST_KIND_ALLOWLIST).
    let odd = nostr::event::EventBuilder::new(nostr::event::Kind::Custom(60000), "weird")
        .finalize(&keys)
        .unwrap();

    let (db_verify, path_verify) = temp_db("allow_verify");
    let (db_pre, path_pre) = temp_db("allow_pre");
    let (tx_v, _rx_v) = tx();
    let ok_v = handle_batch(
        &db_verify,
        &keys.public_key().to_hex(),
        &[odd.clone()],
        &tx_v,
    )
    .unwrap();
    let (tx_p, _rx_p) = tx();
    let ok_p =
        handle_batch_verified(&db_pre, &keys.public_key().to_hex(), &[odd.clone()], &tx_p).unwrap();

    assert_eq!(ok_v, ok_p);
    assert!(post_ids(&db_verify).is_empty());
    assert!(post_ids(&db_pre).is_empty());

    drop(db_verify);
    drop(db_pre);
    cleanup(&path_verify);
    cleanup(&path_pre);
}
