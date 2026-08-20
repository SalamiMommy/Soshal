//! Freenet WebSocket client for contract operations and node communication.

use crate::pqc_link::PQ_LINK_CRYPTO;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{client_async, tungstenite::Message, WebSocketStream};

pub use crate::freenet_contract::StateSummary as ContractSummary;
use crate::freenet_contract::{RelatedContract, StateSummary};

/// Freenet WebSocket client for contract operations.
///
/// When a ratchet peer is configured (`with_ratchet_peer`), contract state
/// payloads are sealed with the hybrid PQC double ratchet before the put
/// and unsealed after the get. The contract key is then derived from the
/// ciphertext, so only the designated peer (holding the matching session)
/// can read the state; everyone else sees an opaque blob. Subscribers
/// without the session cannot decode it by design.
pub struct FreenetWebSocketClient {
    url: String,
    auth_token: String,
    socket: Arc<Mutex<Option<WebSocketStream<BoxedStream>>>>,
    ratchet_peer: Option<(String, String)>,
}

/// The socket stream is either a plain TCP stream or a TLS-wrapped one, so
/// it is boxed behind this trait object at connect time.
type BoxedStream = Box<dyn WebSocketStreamTrait>;

trait WebSocketStreamTrait: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> WebSocketStreamTrait for T {}

impl FreenetWebSocketClient {
    /// Creates a new Freenet WebSocket client
    pub fn new(url: String, auth_token: String) -> Self {
        Self {
            url,
            auth_token,
            socket: Arc::new(Mutex::new(None)),
            ratchet_peer: None,
        }
    }

    /// Enables hybrid PQC ratchet sealing of contract payloads to a peer.
    /// `peer` is the shared session id both ends agree on (e.g. a Nostr
    /// pubkey hex); `peer_pk` is the peer's hybrid public key, exchanged
    /// out-of-band once (the process-wide session persists for the app
    /// lifetime). Both ends must configure each other for reads to work.
    pub fn with_ratchet_peer(mut self, peer: &str, peer_pk: &str) -> Self {
        self.ratchet_peer = Some((peer.to_string(), peer_pk.to_string()));
        self
    }

    fn seal(&self, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        match &self.ratchet_peer {
            Some((peer, peer_pk)) => {
                let context = format!("freenet:{}", peer);
                PQ_LINK_CRYPTO.ensure_session(peer, &context, peer_pk)?;
                PQ_LINK_CRYPTO.encrypt(peer, &context, &payload)
            }
            None => Ok(payload),
        }
    }

    fn unseal(&self, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        match &self.ratchet_peer {
            Some((peer, _)) => {
                let context = format!("freenet:{}", peer);
                PQ_LINK_CRYPTO.decrypt(peer, &context, &payload)
            }
            None => Ok(payload),
        }
    }

    /// Connects to the Freenet node
    pub async fn connect(&self) -> Result<(), String> {
        // SSRF guard: full shared policy for ws/wss URLs — rejects private
        // IPs, loopback, link-local, hex/decimal/alternative-encoded hosts,
        // DNS-rebinding domains, and raw IP literals outright.
        if !soshal_common_core::url::is_valid_relay_url(&self.url).0 {
            return Err(format!(
                "Freenet WebSocket blocked: URL does not pass SSRF policy: {}",
                self.url
            ));
        }
        let parsed = url::Url::parse(&self.url)
            .map_err(|e| format!("Invalid Freenet WebSocket URL: {e}"))?;
        if parsed.scheme() != "ws" && parsed.scheme() != "wss" {
            return Err(format!(
                "Freenet WebSocket blocked: scheme '{}' unsupported; use ws:// or wss://",
                parsed.scheme()
            ));
        }
        let use_tls = parsed.scheme() == "wss";
        let tls_connector: Option<tokio_rustls::TlsConnector> = if use_tls {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let cfg = rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();
            Some(tokio_rustls::TlsConnector::from(std::sync::Arc::new(cfg)))
        } else {
            None
        };
        let hostname = parsed.host_str().unwrap_or("").to_string();
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| "Freenet WebSocket blocked: URL has no port".to_string())?;

        // Resolve the hostname once and verify every address before any
        // socket is opened; the connect targets only the verified set,
        // closing the DNS-rebinding window between check and connect.
        let mut pinned: Vec<std::net::SocketAddr> = Vec::new();
        match tokio::net::lookup_host((hostname.as_str(), port)).await {
            Ok(addrs) => {
                for addr in addrs {
                    if soshal_common_core::url::is_private_ip_str(&addr.ip().to_string()) {
                        return Err(format!(
                            "Freenet WebSocket blocked: URL resolves to an internal address: {}",
                            addr.ip()
                        ));
                    }
                    pinned.push(addr);
                }
            }
            Err(e) => {
                return Err(format!(
                    "Freenet WebSocket blocked: URL does not resolve: {e}"
                ))
            }
        }
        if pinned.is_empty() {
            return Err("Freenet WebSocket blocked: URL does not resolve".to_string());
        }

        // Auth token travels as a header, never the query string (query
        // params leak via logs/referrer). Server contract change: the node
        // must read `X-Freenet-Auth` instead of `?auth=`; header preferred.
        let mut last_err: Option<String> = None;
        for addr in &pinned {
            let mut builder =
                tokio_tungstenite::tungstenite::http::Request::builder().uri(self.url.as_str());
            if !self.auth_token.is_empty() {
                builder = builder.header("X-Freenet-Auth", self.auth_token.as_str());
            }
            let request = builder
                .body(())
                .map_err(|e| format!("WebSocket request build failed: {e}"))?;

            match tokio::net::TcpStream::connect(addr).await {
                Ok(stream) => {
                    let prepared: Result<Box<dyn WebSocketStreamTrait>, String> =
                        match &tls_connector {
                            Some(conn) => {
                                let name =
                                    rustls::pki_types::ServerName::try_from(hostname.clone())
                                        .map_err(|e| format!("invalid TLS hostname: {e}"))?;
                                match conn.connect(name, stream).await {
                                    Ok(s) => Ok(Box::new(s) as _),
                                    Err(e) => Err(format!("TLS handshake failed: {e}")),
                                }
                            }
                            None => Ok(Box::new(stream) as _),
                        };
                    match prepared {
                        Ok(stream) => match client_async(request, stream).await {
                            Ok((ws, _)) => {
                                let mut socket_guard = self.socket.lock().await;
                                *socket_guard = Some(ws);
                                return Ok(());
                            }
                            Err(e) => last_err = Some(format!("WebSocket handshake failed: {e}")),
                        },
                        Err(e) => last_err = Some(e),
                    }
                }
                Err(e) => last_err = Some(format!("TCP connect to {addr} failed: {e}")),
            }
        }
        Err(last_err.unwrap_or_else(|| "WebSocket connection failed".to_string()))
    }

    /// Disconnects from the Freenet node
    pub async fn disconnect(&self) -> Result<(), String> {
        let mut socket_guard = self.socket.lock().await;
        if let Some(mut ws) = socket_guard.take() {
            ws.close(None)
                .await
                .map_err(|e| format!("WebSocket close failed: {e}"))?;
        }
        Ok(())
    }

    /// Sends a request to the Freenet node
    pub async fn send_request(&self, request: FreenetRequest) -> Result<FreenetResponse, String> {
        let mut socket_guard = self.socket.lock().await;
        let ws = socket_guard.as_mut().ok_or("WebSocket not connected")?;

        let request_json = serde_json::to_string(&request)
            .map_err(|e| format!("Request serialization failed: {e}"))?;

        ws.send(Message::Text(request_json))
            .await
            .map_err(|e| format!("WebSocket send failed: {e}"))?;

        // Wait for response
        if let Some(message) = ws.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    let response: FreenetResponse = serde_json::from_str(&text)
                        .map_err(|e| format!("Response deserialization failed: {e}"))?;
                    Ok(response)
                }
                Ok(Message::Close(_)) => Err("Connection closed by server".to_string()),
                Err(e) => Err(format!("WebSocket receive error: {e}")),
                _ => Err("Unexpected message type".to_string()),
            }
        } else {
            Err("No response received".to_string())
        }
    }

    /// Fetches contract state from the Freenet node. With a ratchet peer
    /// configured, the state payload is unsealed with the hybrid PQC
    /// ratchet before returning.
    pub async fn get_contract(&self, key: &str, subscribe: bool) -> Result<ContractState, String> {
        let request = FreenetRequest::Get(GetRequest {
            key: key.to_string(),
            fetch_contract: true,
            subscribe,
            blocking_subscribe: false,
        });

        let response = self.send_request(request).await?;

        match response {
            FreenetResponse::GetResult(mut result) => {
                result.state.state = self.unseal(result.state.state)?;
                Ok(result.state)
            }
            FreenetResponse::Error(err) => Err(format!("Get failed: {}", err.message)),
            _ => Err("Unexpected response type".to_string()),
        }
    }

    /// Publishes contract state to the Freenet node. With a ratchet peer
    /// configured, the state payload is sealed with the hybrid PQC ratchet
    /// first (the contract key derives from the ciphertext).
    pub async fn put_contract(
        &self,
        state: ContractState,
        subscribe: bool,
    ) -> Result<String, String> {
        let mut state = state;
        state.state = self.seal(state.state)?;
        let request = FreenetRequest::Put(PutRequest {
            container: None,
            wrapped_state: Some(state),
            related_contracts: vec![],
            subscribe,
            blocking_subscribe: false,
        });

        let response = self.send_request(request).await?;

        match response {
            FreenetResponse::PutResult(result) => Ok(result.key),
            FreenetResponse::Error(err) => Err(format!("Put failed: {}", err.message)),
            _ => Err("Unexpected response type".to_string()),
        }
    }

    /// Subscribes to contract updates
    pub async fn subscribe_contract(
        &self,
        key: &str,
        summary: Option<StateSummary>,
    ) -> Result<(), String> {
        let request = FreenetRequest::Subscribe(SubscribeRequest {
            key: key.to_string(),
            summary,
        });

        let response = self.send_request(request).await?;

        match response {
            FreenetResponse::SubscribeResult(_) => Ok(()),
            FreenetResponse::Error(err) => Err(format!("Subscribe failed: {}", err.message)),
            _ => Err("Unexpected response type".to_string()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FreenetRequest {
    Get(GetRequest),
    Put(PutRequest),
    Subscribe(SubscribeRequest),
    Update(UpdateRequest),
    Disconnect(DisconnectRequest),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetRequest {
    pub key: String,
    pub fetch_contract: bool,
    pub subscribe: bool,
    pub blocking_subscribe: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PutRequest {
    pub container: Option<ContractContainer>,
    pub wrapped_state: Option<ContractState>,
    pub related_contracts: Vec<RelatedContract>,
    pub subscribe: bool,
    pub blocking_subscribe: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeRequest {
    pub key: String,
    pub summary: Option<StateSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateRequest {
    pub key: Option<String>,
    pub update: Option<StateUpdate>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DisconnectRequest {
    pub cause: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FreenetResponse {
    GetResult(GetResult),
    PutResult(PutResult),
    SubscribeResult(SubscribeResult),
    UpdateResult(UpdateResult),
    Error(ErrorResponse),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetResult {
    pub state: ContractState,
    pub subscribed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PutResult {
    pub key: String,
    pub subscribed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeResult {
    pub key: String,
    pub subscribed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateResult {
    pub key: String,
    pub success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub message: String,
    pub code: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContractState {
    pub key: String,
    pub state: Vec<u8>,
    pub contract_code: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContractContainer {
    pub contract_code: Vec<u8>,
    pub state: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StateUpdate {
    pub delta: Vec<u8>,
    pub summary: Option<StateSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let request = FreenetRequest::Get(GetRequest {
            key: "test_key".to_string(),
            fetch_contract: true,
            subscribe: false,
            blocking_subscribe: false,
        });

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("Get"));
        assert!(json.contains("test_key"));
    }

    #[test]
    fn test_response_deserialization() {
        let json = r#"{"type":"GetResult","state":{"key":"test","state":[1,2,3],"contract_code":null},"subscribed":false}"#;
        let response: FreenetResponse = serde_json::from_str(json).unwrap();

        match response {
            FreenetResponse::GetResult(result) => {
                assert_eq!(result.state.key, "test");
                assert_eq!(result.state.state, vec![1, 2, 3]);
            }
            _ => panic!("Unexpected response type"),
        }
    }

    #[test]
    fn test_request_roundtrip_all_variants() {
        let get = FreenetRequest::Get(GetRequest {
            key: "k".to_string(),
            fetch_contract: true,
            subscribe: true,
            blocking_subscribe: false,
        });
        let decoded: FreenetRequest =
            serde_json::from_str(&serde_json::to_string(&get).unwrap()).unwrap();
        match decoded {
            FreenetRequest::Get(g) => {
                assert_eq!(g.key, "k");
                assert!(g.fetch_contract && g.subscribe && !g.blocking_subscribe);
            }
            _ => panic!("Unexpected request variant"),
        }

        let put = FreenetRequest::Put(PutRequest {
            container: Some(ContractContainer {
                contract_code: vec![1],
                state: vec![2],
            }),
            wrapped_state: Some(ContractState {
                key: "k".to_string(),
                state: vec![3],
                contract_code: None,
            }),
            related_contracts: vec![RelatedContract {
                key: "r".to_string(),
                summary: Some(StateSummary { data: vec![4] }),
            }],
            subscribe: false,
            blocking_subscribe: true,
        });
        let decoded: FreenetRequest =
            serde_json::from_str(&serde_json::to_string(&put).unwrap()).unwrap();
        match decoded {
            FreenetRequest::Put(p) => {
                assert_eq!(p.container.unwrap().state, vec![2]);
                assert_eq!(p.wrapped_state.unwrap().state, vec![3]);
                assert_eq!(p.related_contracts[0].key, "r");
                assert_eq!(
                    p.related_contracts[0].summary.as_ref().unwrap().data,
                    vec![4]
                );
                assert!(!p.subscribe && p.blocking_subscribe);
            }
            _ => panic!("Unexpected request variant"),
        }

        let subscribe = FreenetRequest::Subscribe(SubscribeRequest {
            key: "k".to_string(),
            summary: Some(StateSummary { data: vec![5] }),
        });
        let decoded: FreenetRequest =
            serde_json::from_str(&serde_json::to_string(&subscribe).unwrap()).unwrap();
        match decoded {
            FreenetRequest::Subscribe(s) => {
                assert_eq!(s.key, "k");
                assert_eq!(s.summary.unwrap().data, vec![5]);
            }
            _ => panic!("Unexpected request variant"),
        }

        let update = FreenetRequest::Update(UpdateRequest {
            key: Some("k".to_string()),
            update: Some(StateUpdate {
                delta: vec![6],
                summary: None,
            }),
        });
        let decoded: FreenetRequest =
            serde_json::from_str(&serde_json::to_string(&update).unwrap()).unwrap();
        match decoded {
            FreenetRequest::Update(u) => {
                assert_eq!(u.key.as_deref(), Some("k"));
                assert_eq!(u.update.unwrap().delta, vec![6]);
            }
            _ => panic!("Unexpected request variant"),
        }

        let disconnect = FreenetRequest::Disconnect(DisconnectRequest {
            cause: Some("bye".to_string()),
        });
        let decoded: FreenetRequest =
            serde_json::from_str(&serde_json::to_string(&disconnect).unwrap()).unwrap();
        match decoded {
            FreenetRequest::Disconnect(d) => assert_eq!(d.cause.as_deref(), Some("bye")),
            _ => panic!("Unexpected request variant"),
        }
    }

    #[test]
    fn test_response_roundtrip_all_variants() {
        let get = FreenetResponse::GetResult(GetResult {
            state: ContractState {
                key: "k".to_string(),
                state: vec![1],
                contract_code: None,
            },
            subscribed: true,
        });
        let decoded: FreenetResponse =
            serde_json::from_str(&serde_json::to_string(&get).unwrap()).unwrap();
        match decoded {
            FreenetResponse::GetResult(r) => {
                assert_eq!(r.state.key, "k");
                assert!(r.subscribed);
            }
            _ => panic!("Unexpected response variant"),
        }

        let put = FreenetResponse::PutResult(PutResult {
            key: "p".to_string(),
            subscribed: false,
        });
        let decoded: FreenetResponse =
            serde_json::from_str(&serde_json::to_string(&put).unwrap()).unwrap();
        match decoded {
            FreenetResponse::PutResult(r) => {
                assert_eq!(r.key, "p");
                assert!(!r.subscribed);
            }
            _ => panic!("Unexpected response variant"),
        }

        let subscribe = FreenetResponse::SubscribeResult(SubscribeResult {
            key: "s".to_string(),
            subscribed: true,
        });
        let decoded: FreenetResponse =
            serde_json::from_str(&serde_json::to_string(&subscribe).unwrap()).unwrap();
        match decoded {
            FreenetResponse::SubscribeResult(r) => {
                assert_eq!(r.key, "s");
                assert!(r.subscribed);
            }
            _ => panic!("Unexpected response variant"),
        }

        let update = FreenetResponse::UpdateResult(UpdateResult {
            key: "u".to_string(),
            success: true,
        });
        let decoded: FreenetResponse =
            serde_json::from_str(&serde_json::to_string(&update).unwrap()).unwrap();
        match decoded {
            FreenetResponse::UpdateResult(r) => {
                assert_eq!(r.key, "u");
                assert!(r.success);
            }
            _ => panic!("Unexpected response variant"),
        }

        let error = FreenetResponse::Error(ErrorResponse {
            message: "boom".to_string(),
            code: Some(7),
        });
        let decoded: FreenetResponse =
            serde_json::from_str(&serde_json::to_string(&error).unwrap()).unwrap();
        match decoded {
            FreenetResponse::Error(e) => {
                assert_eq!(e.message, "boom");
                assert_eq!(e.code, Some(7));
            }
            _ => panic!("Unexpected response variant"),
        }
    }

    #[test]
    fn test_response_rejects_junk() {
        assert!(serde_json::from_str::<FreenetResponse>("not json").is_err());
        assert!(serde_json::from_str::<FreenetResponse>(r#"{"type":"Nope"}"#).is_err());
        assert!(serde_json::from_str::<FreenetResponse>(r#"{"subscribed":true}"#).is_err());
    }

    #[tokio::test]
    async fn test_connect_rejects_ssrf_and_bad_urls() {
        for bad in [
            "ws://localhost:8888",
            "ws://127.0.0.1:8888",
            "ws://127.1:8888",
            "ws://0x7f000001:8888",
            "ws://10.0.0.1:8888",
            "ws://192.168.1.1:8888",
            "ws://169.254.169.254:8888",
            "ws://1.2.3.4.nip.io:8888",
            "ws://1.2.3.4:8888",
            "wss://relay.example.com:443",
            "http://example.com:8888",
        ] {
            let client = FreenetWebSocketClient::new(bad.to_string(), "tok".to_string());
            assert!(
                client.connect().await.is_err(),
                "connect should reject {bad}"
            );
        }
    }
}
