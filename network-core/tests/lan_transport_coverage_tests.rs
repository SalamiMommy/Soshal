//! Coverage for the `start_lan_server` wrapper (the last uncovered pub fn in
//! lan_transport.rs — a 1-line delegator to `start_lan_server_with_store`).

use std::io::Cursor;
use std::net::SocketAddr;

use soshal_media_core::cas::ChunkStore;
use soshal_network_core::lan_transport::{fetch_verified_chunk, start_lan_server};

#[test]
fn start_lan_server_binds_ephemeral_port_and_stops_cleanly() {
    let mut server = start_lan_server([7u8; 32]).unwrap();
    assert!(server.port > 0);
    server.stop();
    // Double-stop must be a no-op, not a panic.
    server.stop();
}

#[test]
fn start_lan_server_serves_from_default_root() {
    // The wrapper resolves the store root via `default_root()`, so point
    // `SOSHAL_CHUNK_CACHE` at a store with a stored blob and fetch through
    // the wrapper end-to-end.
    let root = soshal_test_util::tmp_root("lan_cov");
    let store = ChunkStore::new(root.clone());
    let data: Vec<u8> = (0..256 * 1024).map(|i| (i % 249) as u8).collect();
    let m = store.store_reader(Cursor::new(&data)).unwrap();
    store.save_manifest(&m).unwrap();

    std::env::set_var("SOSHAL_CHUNK_CACHE", &root);
    let mut server = start_lan_server([7u8; 32]).unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], server.port));

    let chunk = &m.chunks[0];
    let got = fetch_verified_chunk(
        addr,
        [7u8; 32],
        &"ab".repeat(32),
        &chunk.blake3,
        0,
        chunk.len,
    )
    .unwrap();
    assert_eq!(got.len(), chunk.len);
    assert_eq!(blake3::hash(&got).to_hex().to_string(), chunk.blake3);

    server.stop();
    std::env::remove_var("SOSHAL_CHUNK_CACHE");
}
