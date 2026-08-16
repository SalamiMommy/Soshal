//! Coverage tests for the QUIC datagram channel + stream server lifecycle
//! (src/quic.rs) and the NAT manager handle surface (src/nat.rs).
//!
//! Loopback-only; no external network. The full ICE pairing test is already
//! #[ignore]d in nat.rs (host/timing-dependent) and is NOT duplicated here —
//! these tests cover the synchronous, prompt-returning handle surface.

use soshal_network_core::nat::spawn_nat_manager;
use soshal_network_core::quic::{
    spawn_quic_datagram_channel, start_quic_stream_server, start_quic_stream_server_with_store,
    MicroEvent,
};
use std::net::{SocketAddr, UdpSocket};

/// Finds a free UDP port by binding and dropping a socket. Small race with
/// the async bind inside the spawn fns, acceptable for tests.
fn free_udp_port() -> u16 {
    let s = UdpSocket::bind(("0.0.0.0", 0)).expect("bind probe");
    let port = s.local_addr().expect("local addr").port();
    drop(s);
    port
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

// ============================================================================
// A. quic.rs — datagram channel
// ============================================================================

#[test]
fn datagram_channel_local_addr_and_empty_recv() {
    let port = free_udp_port();
    let ch = spawn_quic_datagram_channel(port, "ab".repeat(32)).unwrap();
    assert_eq!(
        ch.local_addr().port(),
        port,
        "handle reports the bound port"
    );
    assert_eq!(ch.try_recv(), None, "no events before any traffic");
    assert!(ch.drain_events().is_empty());
    ch.stop();
    ch.stop(); // idempotent, must not panic
    assert_eq!(ch.try_recv(), None, "recv after stop stays empty");
}

#[test]
fn datagram_channel_bidirectional_exchange() {
    // The channel thread binds its socket asynchronously after spawn returns;
    // a parallel test can grab the freed probe port in between. Retry with
    // fresh ports until both channels are actually live.
    //
    // Also: simultaneous first-send in BOTH directions deadlocks (quinn's
    // server handshake only continues while the run loop polls accept(); a
    // loop blocked awaiting its own outgoing handshake never accepts the
    // peer's incoming). Send sequentially: A -> B, wait, then B -> A.
    let mut attempts = 0;
    loop {
        attempts += 1;
        let pa = free_udp_port();
        let pb = free_udp_port();
        let a = spawn_quic_datagram_channel(pa, "aa".repeat(32)).unwrap();
        let b = spawn_quic_datagram_channel(pb, "bb".repeat(32)).unwrap();

        // The channel thread binds its socket asynchronously (thread spawn +
        // cert generation + bind). Sending before bind → ICMP refused →
        // handshake fails and is never retried. Give both threads time.
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Phase 1: a -> b (three events), while b's loop idles at accept().
        for i in 0..3 {
            a.send_to(
                loopback(pb),
                MicroEvent {
                    kind: "typing".to_string(),
                    payload: format!("from-a-{i}"),
                },
            )
            .unwrap();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        let mut seen_b: Vec<String> = Vec::new();
        while std::time::Instant::now() < deadline {
            seen_b.extend(b.drain_events().into_iter().map(|e| e.payload));
            if seen_b.len() == 3 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if seen_b.len() != 3 {
            a.stop();
            b.stop();
            assert!(
                attempts < 3,
                "a->b datagrams never delivered, attempt {attempts}"
            );
            continue;
        }

        // Phase 2: b -> a (one event), now that a's loop idles at accept().
        b.send_to(
            loopback(pa),
            MicroEvent {
                kind: "presence".to_string(),
                payload: "from-b".to_string(),
            },
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        let mut seen_a: Vec<String> = Vec::new();
        while std::time::Instant::now() < deadline {
            seen_a.extend(a.drain_events().into_iter().map(|e| e.payload));
            if seen_a.contains(&"from-b".to_string()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if !seen_a.contains(&"from-b".to_string()) {
            a.stop();
            b.stop();
            assert!(
                attempts < 3,
                "b->a datagram never delivered, attempt {attempts}"
            );
            continue;
        }

        let mut want_b: Vec<String> = (0..3).map(|i| format!("from-a-{i}")).collect();
        want_b.sort();
        seen_b.sort();
        assert_eq!(seen_b, want_b, "b must receive all three payloads from a");
        assert_eq!(a.try_recv(), None, "drain_events must consume everything");
        a.stop();
        b.stop();
        return;
    }
}

#[test]
fn datagram_channel_send_to_dead_peer_is_lossy_no_panic() {
    let ch = spawn_quic_datagram_channel(free_udp_port(), "cc".repeat(32)).unwrap();
    // Nothing listens on port 1: connect fails, datagram dropped — but the
    // sync API must return Ok (lossy by design) and never panic.
    let res = ch.send_to(
        loopback(1),
        MicroEvent {
            kind: "typing".to_string(),
            payload: "dead-peer".to_string(),
        },
    );
    assert!(
        res.is_ok(),
        "send_to enqueues regardless of peer reachability"
    );
    ch.stop();
    ch.stop();
}

// ============================================================================
// A. quic.rs — stream server lifecycle
// ============================================================================

#[test]
fn stream_server_lifecycle_ephemeral_port_and_idempotent_stop() {
    let root = soshal_test_util::tmp_root("quic_nat_stream");
    let server = start_quic_stream_server_with_store([7u8; 32], root).unwrap();
    assert!(server.port > 0, "server must bind an ephemeral port");
    server.stop();
    server.stop(); // second stop must not panic
}

#[test]
fn stream_server_default_store_lifecycle() {
    // Plain variant uses ChunkStore::default_root() -> $TMPDIR/soshal_chunks.
    let server = start_quic_stream_server([8u8; 32]).unwrap();
    assert!(server.port > 0, "server must bind an ephemeral port");
    server.stop();
    server.stop();
}

// ============================================================================
// B. nat.rs — manager + handle surface (no ICE pairing, no external net)
// ============================================================================
//
// NOTE: nat.rs AddRemote error paths use bare `return`, which exits
// run_manager entirely (kills the manager thread). Tests here account for
// that: any add_remote expected to fail must be the LAST manager operation.

#[test]
fn nat_manager_initial_state_and_unknown_ops() {
    let h = spawn_nat_manager("alice".repeat(2)).unwrap();
    assert!(h.status().is_empty(), "no sessions before gather");

    let err = h.creds("ghost".repeat(2).as_str()).unwrap_err();
    assert!(err.contains("no session"), "{err}");

    h.remove("ghost".repeat(2).as_str()); // no-op safe, must not panic

    // Unknown-pubkey add_remote errors out (and kills the manager via the
    // bare-`return` bug — afterwards only no-panic guarantees apply).
    let err = h
        .add_remote(
            "ghost".repeat(2).as_str(),
            "u",
            "p",
            &["candidate:1".to_string()],
        )
        .unwrap_err();
    assert!(err.contains("no session"), "{err}");

    h.stop();
    h.stop(); // idempotent
    let _ = h.status(); // must not panic after manager exit
}

#[test]
fn nat_gather_prompt_errors() {
    let h = spawn_nat_manager("alice".repeat(2)).unwrap();

    let err = h
        .gather(
            "bob".repeat(2).as_str(),
            &["stun:host:notaport".to_string()],
        )
        .unwrap_err();
    assert!(err.contains("bad port"), "{err}");

    // Port out of u16 range also rejected. (A `turn:` prefix is NOT an error:
    // parse_ice_url treats unknown schemes as bare host names.)
    let err = h
        .gather("carol".repeat(2).as_str(), &["stun:host:70000".to_string()])
        .unwrap_err();
    assert!(err.contains("bad port"), "{err}");

    // Duplicate gather for the same pubkey must fail promptly.
    h.gather("dave".repeat(2).as_str(), &["stun:127.0.0.1:9".to_string()])
        .unwrap();
    let err = h.gather("dave".repeat(2).as_str(), &[]).unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    h.stop();
}

#[test]
fn nat_gather_creates_session_with_creds_and_remove() {
    let h = spawn_nat_manager("alice".repeat(2)).unwrap();
    let pubkey = "eve".repeat(2);

    // STUN target is a refused loopback port: no srflx, host candidates only,
    // still a successful gather (bounded by the internal settle loop).
    let status = h
        .gather(pubkey.as_str(), &["stun:127.0.0.1:9".to_string()])
        .unwrap();
    assert_eq!(status.pubkey, pubkey);
    // Initial state races with the async on_state callback: accept both.
    assert!(
        status.state == "new" || status.state.is_empty(),
        "unexpected initial state {:?}",
        status.state
    );
    assert!(status.connected_addr.is_none());
    assert!(
        !status.local_candidates.is_empty(),
        "host candidates expected"
    );
    assert!(status.remote_candidates.is_empty());

    let (ufrag, pwd) = h.creds(pubkey.as_str()).unwrap();
    assert!(
        !ufrag.is_empty() && !pwd.is_empty(),
        "session exposes ufrag/pwd"
    );

    let snapshots = h.status();
    let snap = snapshots
        .iter()
        .find(|s| s.pubkey == pubkey)
        .expect("session in status");
    assert!(!snap.local_candidates.is_empty());

    h.remove(pubkey.as_str());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if h.status().iter().all(|s| s.pubkey != pubkey) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        h.status().iter().all(|s| s.pubkey != pubkey),
        "removed session must disappear from status"
    );
    h.stop();
}

#[test]
fn nat_add_remote_dummy_candidates_no_panic() {
    let h = spawn_nat_manager("alice".repeat(2)).unwrap();
    let pubkey = "frank".repeat(2);
    h.gather(pubkey.as_str(), &["stun:127.0.0.1:9".to_string()])
        .unwrap();

    // Well-formed host candidate pointing at a dead port: accepted, session
    // moves to "checking"; checks fail later (pruned async) — must not panic.
    let dummy = "candidate:1 1 udp 2122260223 127.0.0.1 12345 typ host".to_string();
    h.add_remote(pubkey.as_str(), "ufrag-x", "pwd-x", &[dummy])
        .unwrap();

    let snap = h
        .status()
        .into_iter()
        .find(|s| s.pubkey == pubkey)
        .expect("session in status");
    assert_eq!(snap.state, "checking");
    assert_eq!(snap.remote_candidates.len(), 1);

    // Garbage candidate: rejected (this kills the manager via the bare-`return`
    // bug — last operation, error assertion only, no panic).
    let err = h
        .add_remote(pubkey.as_str(), "u", "p", &["not-a-candidate".to_string()])
        .unwrap_err();
    assert!(err.contains("bad candidate"), "{err}");
}
