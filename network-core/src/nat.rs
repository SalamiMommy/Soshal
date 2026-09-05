//! Embedded ICE NAT traversal (webrtc-ice 0.14).
//!
//! Runs the full ICE state machine — host + STUN-mapped (srflx) candidate
//! gathering, connectivity checks, pair nomination — against one remote peer
//! per session, over the agent's own UDP sockets. It resolves *reachability*:
//! whether a peer is directly dialable despite NAT, and at which address.
//!
//! Scope & honesty:
//! - The negotiated socket is NOT handed to quinn; the resolved remote
//!   address (host or srflx, from the selected candidate pair) is used as the
//!   dialing target by the QUIC/TCP transports that carry real data.
//! - srflx candidates require a reachable STUN server; without one only host
//!   candidates are gathered (still useful: loopback/LAN peers).
//! - TURN relays are out of scope here; when direct paths fail the app falls
//!   back to its relay layer (`privacy.rs`).
//!
//! Candidate exchange is out-of-band (mDNS/relay/manual): `gather` returns the
//! local candidate strings + local ufrag/pwd; the remote side passes them to
//! `add_remote` and vice versa.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use webrtc_ice::agent::agent_config::AgentConfig;
use webrtc_ice::agent::{Agent, OnCandidateHdlrFn, OnConnectionStateChangeHdlrFn};
use webrtc_ice::candidate::candidate_base::unmarshal_candidate;
use webrtc_ice::candidate::Candidate;
use webrtc_ice::state::ConnectionState;
use webrtc_ice::url::{SchemeType, Url};

/// Default ICE connectivity-check timeouts (also speeds up tests).
const CHECK_TIMEOUT_MS: u64 = 3000;
const GATHER_WAIT_MS: u64 = 2500;

/// Hard cap on stored remote candidates per session; relay-supplied
/// candidate floods past this are ignored.
const MAX_REMOTE_CANDIDATES: usize = 32;

/// Snapshot of one NAT session (serialized straight to Dart).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatSessionStatus {
    pub pubkey: String,
    pub state: String,
    /// Resolved remote dialing address (`ip:port`) once connected.
    pub connected_addr: Option<String>,
    pub local_candidates: Vec<String>,
    pub remote_candidates: Vec<String>,
}

struct Session {
    agent: Arc<Agent>,
    shared: Arc<SessionShared>,
    remote_candidates: Mutex<Vec<String>>,
    ufrag: String,
    pwd: String,
}

#[derive(Default)]
struct SessionShared {
    state: Mutex<String>,
    local_candidates: Mutex<Vec<String>>,
    connected_addr: Mutex<Option<String>>,
}

enum NatCommand {
    Gather {
        pubkey: String,
        stun_urls: Vec<String>,
        result: std::sync::mpsc::SyncSender<Result<NatSessionStatus, String>>,
    },
    AddRemote {
        pubkey: String,
        ufrag: String,
        pwd: String,
        candidates: Vec<String>,
        result: std::sync::mpsc::SyncSender<Result<(), String>>,
    },
    Remove {
        pubkey: String,
    },
    Creds {
        pubkey: String,
        result: std::sync::mpsc::SyncSender<Result<(String, String), String>>,
    },
    Status {
        result: std::sync::mpsc::SyncSender<Vec<NatSessionStatus>>,
    },
}

/// App-facing handle to the NAT manager thread.
#[derive(Clone)]
pub struct NatHandle {
    sender: std::sync::mpsc::SyncSender<NatCommand>,
    stop: Arc<AtomicBool>,
}

impl NatHandle {
    /// Creates a NAT session for `pubkey`: gathers host (+ srflx when a STUN
    /// server responds) candidates and returns them with the local ufrag/pwd
    /// that the remote side must present back via [`NatHandle::add_remote`].
    pub fn gather(&self, pubkey: &str, stun_urls: &[String]) -> Result<NatSessionStatus, String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(NatCommand::Gather {
                pubkey: pubkey.to_string(),
                stun_urls: stun_urls.to_vec(),
                result: tx,
            })
            .map_err(|e| format!("nat command: {e}"))?;
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|e| format!("nat gather timed out: {e}"))?
    }

    /// Feeds remote credentials + candidates into the session, starting the
    /// connectivity checks. The session state (`connected` + resolved address)
    /// is observable via [`NatHandle::status`].
    pub fn add_remote(
        &self,
        pubkey: &str,
        ufrag: &str,
        pwd: &str,
        candidates: &[String],
    ) -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(NatCommand::AddRemote {
                pubkey: pubkey.to_string(),
                ufrag: ufrag.to_string(),
                pwd: pwd.to_string(),
                candidates: candidates.to_vec(),
                result: tx,
            })
            .map_err(|e| format!("nat command: {e}"))?;
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|e| format!("nat add_remote timed out: {e}"))?
    }

    pub fn remove(&self, pubkey: &str) {
        if self
            .sender
            .send(NatCommand::Remove {
                pubkey: pubkey.to_string(),
            })
            .is_err()
        {
            log::warn!("NAT manager thread dead, session not cleaned up");
        }
    }

    /// Local ICE credentials (ufrag, pwd) of the session for `pubkey`.
    /// Hand these to the remote side; it must echo them back in
    /// [`NatHandle::add_remote`] so connectivity checks can authenticate.
    pub fn creds(&self, pubkey: &str) -> Result<(String, String), String> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(NatCommand::Creds {
                pubkey: pubkey.to_string(),
                result: tx,
            })
            .map_err(|e| format!("nat command: {e}"))?;
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("nat creds timed out: {e}"))?
    }

    pub fn status(&self) -> Vec<NatSessionStatus> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        if self.sender.send(NatCommand::Status { result: tx }).is_err() {
            return Vec::new();
        }
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap_or_default()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// Parses `stun:host:port`, `stun://host:port` or bare `host:port` into an
/// ICE URL. Returns an error for garbage input.
fn parse_ice_url(raw: &str) -> Result<Url, String> {
    let s = raw.trim();
    let (scheme, rest) = if let Some(r) = s.strip_prefix("stun://") {
        (SchemeType::Stun, r)
    } else if let Some(r) = s.strip_prefix("stun:") {
        (SchemeType::Stun, r)
    } else {
        // bare host[:port]
        (SchemeType::Stun, s)
    };
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>().map_err(|_| format!("bad port in {raw}"))?,
        ),
        None => (rest.to_string(), 3478),
    };
    if host.is_empty() {
        return Err(format!("bad stun url {raw}"));
    }
    Ok(Url {
        scheme,
        host,
        port,
        username: String::new(),
        password: String::new(),
        proto: webrtc_ice::url::ProtoType::Udp,
    })
}

/// Spawns the NAT manager thread + runtime. `my_pubkey` (hex) determines the
/// ICE role per session: the lexicographically greater pubkey takes the
/// controlling role so both devices compute the same nomination direction.
pub fn spawn_nat_manager(my_pubkey: String) -> Result<NatHandle, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::sync_channel::<NatCommand>(64);
    let thread_stop = stop.clone();
    let thread_my_pubkey = my_pubkey;
    std::thread::Builder::new()
        .name("soshal-nat".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("nat runtime: {e}");
                    return;
                }
            };
            rt.block_on(run_manager(rx, thread_stop, thread_my_pubkey));
        })
        .map_err(|e| format!("nat thread: {e}"))?;
    Ok(NatHandle { sender: tx, stop })
}

async fn run_manager(
    rx: std::sync::mpsc::Receiver<NatCommand>,
    stop: Arc<AtomicBool>,
    my_pubkey: String,
) {
    let mut sessions: HashMap<String, Session> = HashMap::new();
    loop {
        if stop.load(Ordering::Acquire) {
            eprintln!("nat: stop flag");
            break;
        }
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(cmd) => match cmd {
                    NatCommand::Gather {
                        pubkey,
                        stun_urls,
                        result,
                    } => {
                        if sessions.contains_key(&pubkey) {
                            let _ = result.send(Err("nat session already exists".to_string()));
                            continue;
                        }
                        match gather_session(&pubkey, &stun_urls, &my_pubkey).await {
                            Ok((agent, shared, ufrag, pwd)) => {
                                let local = shared
                                    .local_candidates
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .clone();
                                let state = shared
                                    .state
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .clone();
                                let connected_addr = shared
                                    .connected_addr
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .clone();
                                let status = NatSessionStatus {
                                    pubkey: pubkey.clone(),
                                    state,
                                    connected_addr,
                                    local_candidates: local,
                                    remote_candidates: Vec::new(),
                                };
                                sessions.insert(
                                    pubkey.clone(),
                                    Session {
                                        agent,
                                        shared: shared.clone(),
                                        remote_candidates: Mutex::new(Vec::new()),
                                        ufrag,
                                        pwd,
                                    },
                                );
                                let _ = result.send(Ok(status));
                            }
                            Err(e) => {
                                let _ = result.send(Err(e));
                            }
                        }
                    }
                    NatCommand::AddRemote {
                        pubkey,
                        ufrag,
                        pwd,
                        candidates,
                        result,
                    } => {
                        let mut added: Vec<String> = Vec::new();
                        match sessions.get(&pubkey) {
                            Some(session) => {
                                {
                                    let remote = session
                                        .remote_candidates
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner());
                                    for raw in &candidates {
                                        if remote.len() + added.len() >= MAX_REMOTE_CANDIDATES {
                                            break;
                                        }
                                        if remote.contains(raw) || added.contains(raw) {
                                            continue;
                                        }
                                        match unmarshal_candidate(raw) {
                                            Ok(c) => {
                                                // P2P is private-IP-only (same gate as
                                                // lan_transport/quic/mdns): drop remote
                                                // candidates outside the private scope so a
                                                // hostile peer can't steer ICE probes at
                                                // external hosts.
                                                let addr: std::net::IpAddr =
                                                    match c.address().parse() {
                                                        Ok(a) => a,
                                                        Err(_) => continue,
                                                    };
                                                if !crate::lan::is_private_ip(addr) {
                                                    continue;
                                                }
                                                let c: Arc<dyn Candidate + Send + Sync> =
                                                    Arc::new(c);
                                                if let Err(e) =
                                                    session.agent.add_remote_candidate(&c)
                                                {
                                                    let _ = result
                                                        .send(Err(format!("add candidate: {e}")));
                                                    continue;
                                                }
                                                added.push(raw.clone());
                                            }
                                            Err(e) => {
                                                let _ = result
                                                    .send(Err(format!("bad candidate {raw}: {e}")));
                                                continue;
                                            }
                                        }
                                    }
                                }
                                if session
                                    .agent
                                    .set_remote_credentials(ufrag.clone(), pwd.clone())
                                    .await
                                    .is_err()
                                {
                                    let _ = result
                                        .send(Err("set remote credentials failed".to_string()));
                                    continue;
                                }
                                {
                                    let mut remote = session
                                        .remote_candidates
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner());
                                    remote.extend(added.clone());
                                }
                                *session
                                    .shared
                                    .state
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner()) = "checking".to_string();
                            }
                            None => {
                                let _ = result.send(Err("no session for pubkey".to_string()));
                                continue;
                            }
                        }
                        let _ = result.send(Ok(()));
                    }
                    NatCommand::Remove { pubkey } => {
                        if let Some(session) = sessions.remove(&pubkey) {
                            let agent = session.agent.clone();
                            let _ = agent.close().await;
                        }
                    }
                    NatCommand::Creds { pubkey, result } => {
                        let out = match sessions.get(&pubkey) {
                            Some(s) => Ok((s.ufrag.clone(), s.pwd.clone())),
                            None => Err(format!("no session for pubkey {pubkey}")),
                        };
                        let _ = result.send(out);
                    }
                    NatCommand::Status { result } => {
                        let mut out = Vec::with_capacity(sessions.len());
                        for (pubkey, session) in &sessions {
                            let state = session
                                .shared
                                .state
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .clone();
                            let connected = if state == "connected" {
                                match session.agent.get_selected_candidate_pair() {
                                    Some(pair) => {
                                        let addr = format!(
                                            "{}:{}",
                                            pair.remote.address(),
                                            pair.remote.port()
                                        );
                                        *session
                                            .shared
                                            .connected_addr
                                            .lock()
                                            .unwrap_or_else(|e| e.into_inner()) =
                                            Some(addr.clone());
                                        Some(addr)
                                    }
                                    None => session
                                        .shared
                                        .connected_addr
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .clone(),
                                }
                            } else {
                                session
                                    .shared
                                    .connected_addr
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .clone()
                            };
                            let local_candidates = session
                                .shared
                                .local_candidates
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .clone();
                            let remote_candidates = session
                                .remote_candidates
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .clone();
                            out.push(NatSessionStatus {
                                pubkey: pubkey.clone(),
                                state,
                                connected_addr: connected,
                                local_candidates,
                                remote_candidates,
                            });
                        }
                        let _ = result.send(out);
                    }
                },
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            eprintln!("nat: command channel closed");
            break;
        }
        prune_failed_sessions(&mut sessions).await;
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    // Best-effort close of all agents on shutdown.
    for (_, session) in sessions.drain() {
        let agent = session.agent.clone();
        let _ = agent.close().await;
    }
}

/// Drops sessions whose ICE connectivity checks failed, releasing their
/// agents and UDP sockets. Called on each manager tick.
async fn prune_failed_sessions(sessions: &mut HashMap<String, Session>) {
    let dead: Vec<String> = sessions
        .iter()
        .filter(|(_, s)| *s.shared.state.lock().unwrap_or_else(|e| e.into_inner()) == "failed")
        .map(|(k, _)| k.clone())
        .collect();
    for pubkey in dead {
        if let Some(session) = sessions.remove(&pubkey) {
            eprintln!("nat: pruning failed session {pubkey}");
            let _ = session.agent.close().await;
        }
    }
}

fn conn_state_str(st: ConnectionState) -> String {
    match st {
        ConnectionState::Unspecified => "new",
        ConnectionState::New => "new",
        ConnectionState::Checking => "checking",
        ConnectionState::Connected => "connected",
        ConnectionState::Completed => "connected",
        ConnectionState::Disconnected => "disconnected",
        ConnectionState::Failed => "failed",
        ConnectionState::Closed => "closed",
    }
    .to_string()
}

async fn gather_session(
    pubkey: &str,
    stun_urls: &[String],
    my_pubkey: &str,
) -> Result<(Arc<Agent>, Arc<SessionShared>, String, String), String> {
    let mut urls = Vec::with_capacity(stun_urls.len());
    for raw in stun_urls {
        urls.push(parse_ice_url(raw)?);
    }
    if urls.is_empty() {
        // Default public STUN (best-effort; gather fails gracefully without it).
        urls.push(parse_ice_url("stun:stun.l.google.com:19302")?);
    }

    let mut config = AgentConfig {
        urls,
        failed_timeout: Some(std::time::Duration::from_millis(CHECK_TIMEOUT_MS)),
        disconnected_timeout: Some(std::time::Duration::from_millis(CHECK_TIMEOUT_MS)),
        network_types: webrtc_ice::network_type::supported_network_types(),
        // Deterministic role: greater pubkey controls (nominates); both
        // devices derive the same answer. Both-controlled never connects.
        is_controlling: my_pubkey > pubkey,
        ..Default::default()
    };
    // include loopback so tests (and LAN-device setups) can pair over 127.0.0.1
    config.include_loopback = true;

    let agent = Arc::new(
        Agent::new(config)
            .await
            .map_err(|e| format!("ice agent: {e}"))?,
    );
    let shared = Arc::new(SessionShared::default());

    {
        let shared = shared.clone();
        let s = shared.clone();
        let on_candidate: OnCandidateHdlrFn = Box::new(move |candidate| {
            let shared = s.clone();
            Box::pin(async move {
                if let Some(c) = candidate {
                    let raw = c.marshal();
                    shared
                        .local_candidates
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(raw);
                } else {
                    // gathering complete (None sentinel)
                }
            })
        });
        agent.on_candidate(on_candidate);
        let on_state: OnConnectionStateChangeHdlrFn = Box::new(move |st| {
            let shared = shared.clone();
            Box::pin(async move {
                *shared.state.lock().unwrap_or_else(|e| e.into_inner()) = conn_state_str(st);
            })
        });
        agent.on_connection_state_change(on_state);
    }
    let _ = agent.gather_candidates();

    // Wait for gathering to settle (candidates stream in from internal tasks).
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(GATHER_WAIT_MS);
    let mut last = 0usize;
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let n = shared
            .local_candidates
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len();
        if n == last {
            break;
        }
        last = n;
    }

    if shared
        .local_candidates
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
    {
        return Err(format!(
            "no candidates gathered for {pubkey} (no network interface?)"
        ));
    }
    let (ufrag, pwd) = agent.get_local_user_credentials().await;
    Ok((agent, shared, ufrag, pwd))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boots two managers (simulating two devices), runs a full ICE exchange
    /// over loopback, and asserts both reach `connected`.
    #[test]
    #[ignore] // Real ICE agent + UDP loopback pairing: host/timing-dependent (2.5s gather + 8s poll), flaky under CI load
    fn loopback_ice_pairing_reaches_connected() {
        // alice > bob lexicographically, so alice's manager controls.
        let a = spawn_nat_manager("alice".repeat(2)).unwrap();
        let b = spawn_nat_manager("bob".repeat(2)).unwrap();

        let sa = a.gather(&"bob".repeat(2), &[]).unwrap();
        let sb = b.gather(&"alice".repeat(2), &[]).unwrap();

        assert!(!sa.local_candidates.is_empty());
        assert!(!sb.local_candidates.is_empty());

        // Exchange candidates + credentials in both directions.
        let (ufrag_a, pwd_a) = a.creds("bob".repeat(2).as_str()).unwrap();
        let (ufrag_b, pwd_b) = b.creds("alice".repeat(2).as_str()).unwrap();
        a.add_remote(
            "bob".repeat(2).as_str(),
            &ufrag_b,
            &pwd_b,
            &sb.local_candidates,
        )
        .unwrap();
        b.add_remote(
            "alice".repeat(2).as_str(),
            &ufrag_a,
            &pwd_a,
            &sa.local_candidates,
        )
        .unwrap();

        // Poll until both connected (loopback checks are fast). Producer
        // (manager thread) can't signal the test, so back off the poll
        // interval 10ms -> 50ms -> 200ms to cut idle wakeups; the 8s
        // deadline and timeout semantics are unchanged.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut a_state = String::new();
        let mut b_state = String::new();
        let mut poll_ms = 10u64;
        while std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(poll_ms));
            let status = a.status();
            a_state = status
                .iter()
                .find(|s| s.pubkey == "bob".repeat(2))
                .map(|s| s.state.clone())
                .unwrap_or_default();
            let status = b.status();
            b_state = status
                .iter()
                .find(|s| s.pubkey == "alice".repeat(2))
                .map(|s| s.state.clone())
                .unwrap_or_default();
            if a_state == "connected" && b_state == "connected" {
                break;
            }
            poll_ms = match poll_ms {
                10 => 50,
                _ => 200,
            };
        }

        a.stop();
        b.stop();

        assert_eq!(a_state, "connected", "alice side never connected");
        assert_eq!(b_state, "connected", "bob side never connected");
    }

    #[test]
    fn parse_ice_url_accepts_common_forms() {
        let u = parse_ice_url("stun:stun.l.google.com:19302").unwrap();
        assert_eq!(u.port, 19302);
        assert_eq!(u.host, "stun.l.google.com");
        assert_eq!(u.scheme, SchemeType::Stun);

        let u = parse_ice_url("stun://turn.example.com:3478").unwrap();
        assert_eq!(u.host, "turn.example.com");

        let u = parse_ice_url("stun.example.com").unwrap();
        assert_eq!(u.port, 3478);

        assert!(parse_ice_url("").is_err());
        assert!(parse_ice_url("stun:").is_err());
        assert!(parse_ice_url("stun://:9999").is_err());
        assert!(parse_ice_url("stun:host:notaport").is_err());
        assert!(parse_ice_url("stun:turn.example.com:70000").is_err());
    }

    #[test]
    fn nat_handle_error_paths_without_sessions() {
        let handle = spawn_nat_manager("alice".repeat(2)).unwrap();
        assert!(handle.status().is_empty());
        assert!(handle.creds("bob".repeat(2).as_str()).is_err());
        assert!(
            handle
                .gather("bob".repeat(2).as_str(), &["stun:192.0.2.1:9".to_string()])
                .is_ok(),
            "host candidates still gather"
        );
        assert!(
            handle
                .add_remote("bob".repeat(2).as_str(), "u", "p", &[])
                .is_ok(),
            "no-op on missing session"
        );
        handle.remove("bob".repeat(2).as_str());
        handle.stop();
        assert!(handle.status().is_empty());
    }

    #[test]
    fn nat_session_lifecycle_error_paths() {
        let handle = spawn_nat_manager("carol".repeat(2)).unwrap();
        let pk = "dave".repeat(2);
        assert!(handle
            .gather(&pk, &["stun:192.0.2.1:9".to_string()])
            .is_ok());
        let err = handle
            .gather(&pk, &["stun:192.0.2.1:9".to_string()])
            .unwrap_err();
        assert!(err.contains("already exists"), "got {err}");
        let err = handle.add_remote(&pk, "u", "p", &["not-a-candidate".to_string()]);
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("bad candidate"));
        assert!(handle.creds(&pk).is_ok());
        let statuses = handle.status();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].pubkey, pk);
        handle.remove(&pk);
        assert!(handle.status().is_empty());
        assert!(handle.creds(&pk).is_err());
        assert!(
            handle
                .gather(&pk, &["stun:192.0.2.1:9".to_string()])
                .is_ok(),
            "re-gather after remove"
        );
        handle.stop();
    }

    #[test]
    fn add_remote_skips_duplicate_remote_candidate() {
        let handle = spawn_nat_manager("eve".repeat(2)).unwrap();
        let pk = "frank".repeat(2);
        handle
            .gather(&pk, &["stun:192.0.2.1:9".to_string()])
            .unwrap();
        let cand = "1 1 udp 2122260223 127.0.0.1 50050 typ host".to_string();
        handle
            .add_remote(&pk, "u", "p", std::slice::from_ref(&cand))
            .unwrap();
        handle
            .add_remote(&pk, "u", "p", std::slice::from_ref(&cand))
            .unwrap();
        handle
            .add_remote(&pk, "u", "p", &[cand.clone(), cand])
            .unwrap();
        let statuses = handle.status();
        assert_eq!(statuses.len(), 1);
        assert_eq!(
            statuses[0].remote_candidates.len(),
            1,
            "duplicate candidate skipped on second add_remote"
        );
        handle.remove(&pk);
        handle.stop();
    }

    #[test]
    fn add_remote_drops_non_private_candidates() {
        let handle = spawn_nat_manager("eve".repeat(2)).unwrap();
        let pk = "frank".repeat(2);
        handle
            .gather(&pk, &["stun:192.0.2.1:9".to_string()])
            .unwrap();
        let public = "1 1 udp 2122260223 8.8.8.8 50060 typ host".to_string();
        let hostname = "1 1 udp 2122260223 peer.local 50061 typ host".to_string();
        let private = "1 1 udp 2122260223 127.0.0.1 50062 typ host".to_string();
        handle
            .add_remote(&pk, "u", "p", &[public, hostname, private.clone()])
            .unwrap();
        let statuses = handle.status();
        assert_eq!(statuses.len(), 1);
        assert_eq!(
            statuses[0].remote_candidates,
            vec![private],
            "only private-IP candidate added"
        );
        handle.remove(&pk);
        handle.stop();
    }

    #[test]
    fn add_remote_caps_remote_candidates_at_32() {
        let handle = spawn_nat_manager("grace".repeat(2)).unwrap();
        let pk = "heidi".repeat(2);
        handle
            .gather(&pk, &["stun:192.0.2.1:9".to_string()])
            .unwrap();
        let candidates: Vec<String> = (0..=32)
            .map(|i| format!("1 1 udp 2122260223 127.0.0.1 {} typ host", 51000 + i))
            .collect();
        assert_eq!(candidates.len(), 33);
        handle.add_remote(&pk, "u", "p", &candidates).unwrap();
        let statuses = handle.status();
        assert_eq!(statuses.len(), 1);
        assert_eq!(
            statuses[0].remote_candidates.len(),
            MAX_REMOTE_CANDIDATES,
            "33rd candidate dropped at cap"
        );
        handle.remove(&pk);
        handle.stop();
    }

    #[test]
    fn add_remote_rejects_empty_remote_credentials() {
        let handle = spawn_nat_manager("ivan".repeat(2)).unwrap();
        let pk = "judy".repeat(2);
        handle
            .gather(&pk, &["stun:192.0.2.1:9".to_string()])
            .unwrap();
        let cand = "1 1 udp 2122260223 127.0.0.1 52000 typ host".to_string();
        let err = handle
            .add_remote(&pk, "", "p", std::slice::from_ref(&cand))
            .unwrap_err();
        assert!(err.contains("set remote credentials failed"), "got {err}");
        let err = handle.add_remote(&pk, "u", "", &[cand]).unwrap_err();
        assert!(err.contains("set remote credentials failed"), "got {err}");
        handle.remove(&pk);
        handle.stop();
    }

    #[test]
    fn prune_failed_sessions_removes_failed_state_only() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mk = |state: String| async move {
                let config = AgentConfig {
                    urls: Vec::new(),
                    failed_timeout: Some(std::time::Duration::from_millis(100)),
                    disconnected_timeout: Some(std::time::Duration::from_millis(100)),
                    network_types: webrtc_ice::network_type::supported_network_types(),
                    is_controlling: true,
                    include_loopback: true,
                    ..Default::default()
                };
                let shared = Arc::new(SessionShared::default());
                *shared.state.lock().unwrap() = state;
                Session {
                    agent: Arc::new(Agent::new(config).await.unwrap()),
                    shared,
                    remote_candidates: Mutex::new(Vec::new()),
                    ufrag: "u".to_string(),
                    pwd: "p".to_string(),
                }
            };
            let mut sessions: HashMap<String, Session> = HashMap::new();
            sessions.insert("failed-pk".to_string(), mk("failed".to_string()).await);
            sessions.insert("checking-pk".to_string(), mk("checking".to_string()).await);
            prune_failed_sessions(&mut sessions).await;
            assert_eq!(sessions.len(), 1, "failed session pruned");
            assert!(sessions.contains_key("checking-pk"), "healthy session kept");
        });
    }
}
