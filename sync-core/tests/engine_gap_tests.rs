//! Engine lifecycle tests. The relay client is built against `relay.invalid`
//! (fails DNS instantly, passes the SSRF hostname check), so the engine loop
//! runs deterministically with no external network and no connected relay:
//! notifications stream goes idle, `stop` ends the pass.

use soshal_sync_core::engine::{build_client, engine_loop_with_client, spawn_engine, SyncConfig};
use soshal_sync_core::SyncUpdate;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn cfg() -> SyncConfig {
    SyncConfig {
        db_path: temp_db_path(),
        my_pubkey: "a".repeat(64),
        relays: vec!["wss://relay.invalid".to_string()],
        socks_proxy: None,
    }
}

fn temp_db_path() -> String {
    let dir = std::env::temp_dir().join(format!("soshal-engine-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!(
        "engine-{}.sqlite",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
    .to_string_lossy()
    .into_owned()
}

#[test]
fn build_client_rejects_unusable_configs() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut c = cfg();
        c.relays = vec![];
        let err = build_client(&c).await.unwrap_err();
        assert!(err.contains("no usable relay urls"), "{err}");

        c.relays = vec![
            "ws://127.0.0.1:7000".to_string(),
            "http://192.168.1.10:8080".to_string(),
            "not-a-url".to_string(),
        ];
        let err = build_client(&c).await.unwrap_err();
        assert!(err.contains("no usable relay urls"), "{err}");

        c.relays = vec!["wss://relay.invalid".to_string()];
        c.socks_proxy = Some("not-an-addr".to_string());
        let err = build_client(&c).await.unwrap_err();
        assert!(err.contains("invalid socks proxy"), "{err}");
    });
}

#[test]
fn build_client_ok_with_unreachable_public_relay() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let c = cfg();
        let client = build_client(&c)
            .await
            .expect("url valid, connect best-effort");
        let relays = client.relays().await;
        assert_eq!(relays.len(), 1, "relay registered despite failed connect");
    });
}

#[test]
fn engine_loop_runs_idle_and_stops_cleanly() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("engine-test")
        .build()
        .unwrap();
    rt.block_on(async {
        let c = cfg();
        let client = build_client(&c).await.unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<SyncUpdate>(16);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let spawned =
            tokio::spawn(async move { engine_loop_with_client(c, tx, stop2, client).await });
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        stop.store(true, Ordering::Relaxed);
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), spawned)
            .await
            .expect("engine must exit after stop")
            .expect("engine task must not panic");
        assert!(result.is_ok(), "engine loop error: {result:?}");
        while rx.try_recv().is_ok() {}
    });
}

#[test]
fn engine_loop_errors_on_bad_pubkey() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut c = cfg();
        c.my_pubkey = "not-hex".to_string();
        let client = build_client(&c).await.unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel::<SyncUpdate>(16);
        let stop = Arc::new(AtomicBool::new(false));
        let err = engine_loop_with_client(c, tx, stop, client)
            .await
            .unwrap_err();
        assert!(err.contains("invalid my_pubkey"), "{err}");
    });
}

#[test]
fn spawn_engine_thread_exits_with_no_usable_relays() {
    let mut c = cfg();
    c.relays = vec![];
    let (tx, _rx) = tokio::sync::mpsc::channel::<SyncUpdate>(16);
    let stop = Arc::new(AtomicBool::new(false));
    let handle = spawn_engine(c, tx, stop);
    handle.join().unwrap();
}
