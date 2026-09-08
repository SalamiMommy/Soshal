//! Freenet WebSocket client for contract operations and node communication.

use crate::pqc_link::PQ_LINK_CRYPTO;
use freenet_stdlib::client_api::{
    ClientError as FnetClientError, ClientRequest as FnetClientRequest,
    ContractRequest as FnetContractRequest, HostResponse as FnetHostResponse,
};
use freenet_stdlib::prelude::{
    ContractCode as FnetContractCode, ContractContainer as FnetContractContainer,
    ContractInstanceId, ContractWasmAPIVersion as FnetContractWasmApi,
    Parameters as FnetParameters, RelatedContracts as FnetRelatedContracts, State as FnetState,
    StateSummary as FnetStateSummary, WrappedContract as FnetWrappedContract,
    WrappedState as FnetWrappedState,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{client_async, tungstenite::Message, WebSocketStream};

/// Per-phase bound for Freenet node connections (TCP connect, TLS handshake,
/// WS upgrade). Prevents hangs on unreachable nodes.
const WS_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The freenet-core node serves the client WebSocket API only on this path
/// (both v1 and v2 exist; v2 is current). A bare `ws://host:port` root URL
/// hits the HTML dashboard handler, not the WS upgrade.
const WS_API_PATH: &str = "/v2/contract/command";

/// Requests the node's native (bincode) message encoding instead of the
/// default flatbuffers byte format.
const ENCODING_PROTOCOL_HEADER: &str = "encoding-protocol";
const ENCODING_PROTOCOL_NATIVE: &str = "native";

/// Maximum frames drained for one request/response exchange. Message types not
/// matched to the pending request (async subscribe notifications, heartbeats)
/// are skipped, but the loop is bounded so a misbehaving node cannot pin the
/// caller.
const MAX_RESPONSE_DRAIN: usize = 8;

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
        let parsed = url::Url::parse(&self.url)
            .map_err(|e| format!("Invalid Freenet WebSocket URL: {e}"))?;
        if parsed.scheme() != "ws" && parsed.scheme() != "wss" {
            return Err(format!(
                "Freenet WebSocket blocked: scheme '{}' unsupported; use ws:// or wss://",
                parsed.scheme()
            ));
        }
        let hostname = parsed.host_str().unwrap_or("").to_string();
        // The Freenet gateway is a user-configured LOCAL endpoint (typed into
        // settings, never relay-derived content), so a loopback host bypasses
        // the relay SSRF policy. Remote hosts keep the full shared policy:
        // private IPs, loopback, link-local, hex/decimal/alternative-encoded
        // hosts, DNS-rebinding domains, and raw IP literals all rejected.
        let loopback = soshal_common_core::url::is_loopback_host(&hostname);
        if !loopback && !soshal_common_core::url::is_valid_relay_url(&self.url).0 {
            return Err(format!(
                "Freenet WebSocket blocked: URL does not pass SSRF policy: {}",
                self.url
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
                    if !loopback
                        && soshal_common_core::url::is_private_ip_str(&addr.ip().to_string())
                    {
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

        // The node serves its WebSocket API only under `WS_API_PATH`. A bare
        // root URL (the historical 8888 gateway convention) points at the
        // dashboard HTML handler, so normalize the path when it is missing.
        let mut ws_url = parsed.clone();
        if ws_url.path() == "/" || ws_url.path().is_empty() {
            ws_url.set_path(WS_API_PATH);
        }
        let ws_url = ws_url.to_string();

        // Auth + encoding travel as headers, never the query string (query
        // params leak via logs/referrer). The freenet-core node authenticates
        // with `Authorization: Bearer <token>` (or `?auth_token=`); a token is
        // OPTIONAL for loopback clients (anonymous connections are accepted,
        // close code 4401 = AUTH_TOKEN_INVALID only fires for a stale token).
        // The bespoke `X-Freenet-User-Token` header is reserved for hosted-mode
        // user contexts and must not be reused here.
        // Every phase (TCP connect, TLS handshake, WS upgrade) is bounded —
        // an unreachable node must fail fast instead of hanging the caller.
        let mut last_err: Option<String> = None;
        for addr in &pinned {
            let mut builder = tokio_tungstenite::tungstenite::http::Request::builder()
                .uri(ws_url.as_str())
                .header(ENCODING_PROTOCOL_HEADER, ENCODING_PROTOCOL_NATIVE);
            if !self.auth_token.is_empty() {
                builder = builder.header(
                    "Authorization",
                    format!("Bearer {}", self.auth_token).as_str(),
                );
            }
            let request = builder
                .body(())
                .map_err(|e| format!("WebSocket request build failed: {e}"))?;

            match tokio::time::timeout(WS_CONNECT_TIMEOUT, tokio::net::TcpStream::connect(addr))
                .await
            {
                Ok(Ok(stream)) => {
                    let prepared: Result<Box<dyn WebSocketStreamTrait>, String> =
                        match &tls_connector {
                            Some(conn) => {
                                let name =
                                    rustls::pki_types::ServerName::try_from(hostname.clone())
                                        .map_err(|e| format!("invalid TLS hostname: {e}"))?;
                                match tokio::time::timeout(
                                    WS_CONNECT_TIMEOUT,
                                    conn.connect(name, stream),
                                )
                                .await
                                {
                                    Ok(Ok(s)) => Ok(Box::new(s) as _),
                                    Ok(Err(e)) => Err(format!("TLS handshake failed: {e}")),
                                    Err(_) => Err("TLS handshake timed out".to_string()),
                                }
                            }
                            None => Ok(Box::new(stream) as _),
                        };
                    match prepared {
                        Ok(stream) => {
                            match tokio::time::timeout(
                                WS_CONNECT_TIMEOUT,
                                client_async(request, stream),
                            )
                            .await
                            {
                                Ok(Ok((ws, _))) => {
                                    let mut socket_guard = self.socket.lock().await;
                                    *socket_guard = Some(ws);
                                    return Ok(());
                                }
                                Ok(Err(e)) => {
                                    last_err = Some(format!("WebSocket handshake failed: {e}"))
                                }
                                Err(_) => {
                                    last_err = Some("WebSocket handshake timed out".to_string())
                                }
                            }
                        }
                        Err(e) => last_err = Some(e),
                    }
                }
                Ok(Err(e)) => last_err = Some(format!("TCP connect to {addr} failed: {e}")),
                Err(_) => last_err = Some(format!("TCP connect to {addr} timed out")),
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

    /// Sends a request to the Freenet node over the native (bincode) client
    /// API and decodes the matching response.
    pub async fn send_request(&self, request: FreenetRequest) -> Result<FreenetResponse, String> {
        let native_req = to_native_request(&request)?;
        let request_bytes = bincode::serialize(&native_req)
            .map_err(|e| format!("Freenet request serialization failed: {e}"))?;

        let mut socket_guard = self.socket.lock().await;
        let ws = socket_guard.as_mut().ok_or("WebSocket not connected")?;

        ws.send(Message::Binary(request_bytes))
            .await
            .map_err(|e| format!("Freenet WebSocket send failed: {e}"))?;

        // Drain until the response matching the pending request arrives.
        // Subscribe notifications and unrelated messages are skipped inside a
        // bounded window; a close frame surfaces the node's rejection reason.
        for _ in 0..MAX_RESPONSE_DRAIN {
            match ws.next().await {
                Some(Ok(Message::Binary(bytes))) => {
                    let native_resp: Result<FnetHostResponse, FnetClientError> =
                        bincode::deserialize(&bytes)
                            .map_err(|e| format!("Freenet response deserialization failed: {e}"))?;
                    if let Some(response) = from_native_response(&request, &native_resp) {
                        return Ok(response);
                    }
                }
                Some(Ok(Message::Ping(payload))) => {
                    let _ = ws.send(Message::Pong(payload)).await;
                }
                Some(Ok(Message::Pong(_))) | Some(Ok(Message::Frame(_))) => {}
                Some(Ok(Message::Close(frame))) => {
                    let reason = frame
                        .map(|f| {
                            let text = f.reason.to_string();
                            format!("{text} (code {})", f.code)
                        })
                        .unwrap_or_else(|| "no reason".to_string());
                    return Err(format!("Freenet connection closed by server: {reason}"));
                }
                Some(Ok(Message::Text(_))) => {}
                Some(Err(e)) => return Err(format!("Freenet WebSocket receive error: {e}")),
                None => return Err("No response received".to_string()),
            }
        }
        Err("Freenet: no matching response within the drain window".to_string())
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

/// Parses a freenet contract id (base58) from the JSON-facing key string.
fn parse_instance_id(key: &str) -> Result<ContractInstanceId, String> {
    key.parse::<ContractInstanceId>()
        .map_err(|e| format!("invalid Freenet contract id '{key}': {e}"))
}

/// Maps the JSON-facing request to the node's native `ClientRequest`.
///
/// Get/Subscribe/Disconnect map 1:1. Put requires a contract container: a
/// freenet Put creates a NEW contract from (code, params); "upsert state under
/// this key" has no native equivalent (that is `Update`, which needs the full
/// key incl. code hash and a pre-seeded contract). Container-less puts (the
/// historical relay default) therefore return an advisory error so the caller
/// can seed a carrier contract first.
fn to_native_request(request: &FreenetRequest) -> Result<FnetClientRequest<'static>, String> {
    match request {
        FreenetRequest::Get(g) => Ok(FnetClientRequest::ContractOp(FnetContractRequest::Get {
            key: parse_instance_id(&g.key)?,
            return_contract_code: g.fetch_contract,
            subscribe: g.subscribe,
            blocking_subscribe: g.blocking_subscribe,
        })),
        FreenetRequest::Put(p) => {
            let container = p.container.as_ref().ok_or_else(|| {
                "local Freenet node: Put without contract code is unsupported; \
                 seed a carrier contract (Put with a container) first"
                    .to_string()
            })?;
            let code = FnetContractCode::from(container.contract_code.clone());
            let params = FnetParameters::from(code.hash().as_ref().to_vec());
            let contract = FnetContractContainer::Wasm(FnetContractWasmApi::V1(
                FnetWrappedContract::new(Arc::new(code), params),
            ));
            let state = p
                .wrapped_state
                .as_ref()
                .map(|s| s.state.clone())
                .unwrap_or_else(|| container.state.clone());
            Ok(FnetClientRequest::ContractOp(FnetContractRequest::Put {
                contract,
                state: FnetWrappedState::from(state),
                related_contracts: to_native_related(&p.related_contracts)?,
                subscribe: p.subscribe,
                blocking_subscribe: p.blocking_subscribe,
            }))
        }
        FreenetRequest::Subscribe(s) => Ok(FnetClientRequest::ContractOp(
            FnetContractRequest::Subscribe {
                key: parse_instance_id(&s.key)?,
                summary: s
                    .summary
                    .as_ref()
                    .map(|sm| FnetStateSummary::from(sm.data.clone())),
            },
        )),
        FreenetRequest::Update(_) => Err(
            "local Freenet node: Update unsupported over the native bridge \
             (requires the full contract key incl. code hash and a seeded contract)"
                .to_string(),
        ),
        FreenetRequest::Disconnect(d) => Ok(FnetClientRequest::Disconnect {
            cause: d.cause.clone().map(Into::into),
        }),
    }
}

fn to_native_related(related: &[RelatedContract]) -> Result<FnetRelatedContracts<'static>, String> {
    let mut map: HashMap<ContractInstanceId, Option<FnetState<'static>>> = HashMap::new();
    for rc in related {
        let id = parse_instance_id(&rc.key)?;
        let state = rc.summary.as_ref().map(|s| FnetState::from(s.data.clone()));
        map.insert(id, state);
    }
    Ok(FnetRelatedContracts::from(map))
}

/// Maps a native response (or error) to the JSON-facing response expected by
/// the pending request. Returns `None` for unrelated frames (e.g. async
/// subscribe notifications) so the reader keeps draining.
fn from_native_response(
    request: &FreenetRequest,
    native: &Result<FnetHostResponse, FnetClientError>,
) -> Option<FreenetResponse> {
    let want_subscribe = match request {
        FreenetRequest::Get(g) => g.subscribe,
        FreenetRequest::Put(p) => p.subscribe,
        _ => false,
    };
    match native {
        Ok(FnetHostResponse::ContractResponse(
            freenet_stdlib::client_api::ContractResponse::GetResponse { key, state, .. },
        )) => Some(FreenetResponse::GetResult(GetResult {
            state: ContractState {
                key: key.encoded_contract_id(),
                state: state.as_ref().to_vec(),
                contract_code: None,
            },
            subscribed: want_subscribe,
        })),
        Ok(FnetHostResponse::ContractResponse(
            freenet_stdlib::client_api::ContractResponse::PutResponse { key },
        )) => Some(FreenetResponse::PutResult(PutResult {
            key: key.encoded_contract_id(),
            subscribed: want_subscribe,
        })),
        Ok(FnetHostResponse::ContractResponse(
            freenet_stdlib::client_api::ContractResponse::SubscribeResponse { key, subscribed },
        )) => Some(FreenetResponse::SubscribeResult(SubscribeResult {
            key: key.encoded_contract_id(),
            subscribed: *subscribed,
        })),
        Ok(FnetHostResponse::ContractResponse(
            freenet_stdlib::client_api::ContractResponse::UpdateResponse { key, .. },
        )) => Some(FreenetResponse::UpdateResult(UpdateResult {
            key: key.encoded_contract_id(),
            success: true,
        })),
        Ok(FnetHostResponse::ContractResponse(
            freenet_stdlib::client_api::ContractResponse::NotFound { instance_id },
        )) => Some(FreenetResponse::Error(ErrorResponse {
            message: format!(
                "Freenet contract not found: {} — seed it with a Put (container) first",
                instance_id.encode()
            ),
            code: Some(404),
        })),
        Err(e) => Some(FreenetResponse::Error(ErrorResponse {
            message: format!("Freenet node error: {e:?}"),
            code: None,
        })),
        _ => None,
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
    fn test_native_parse_instance_id() {
        let id = ContractInstanceId::from_params_and_code(
            FnetParameters::from(vec![1u8; 32]),
            FnetContractCode::from(vec![0u8; 8]),
        );
        let encoded = id.encode();
        assert_eq!(parse_instance_id(&encoded).unwrap(), id);
        assert!(parse_instance_id("soshal-mesh-v1").is_err());
        assert!(parse_instance_id("&&&").is_err());
    }

    #[test]
    fn test_native_get_maps_to_contract_get() {
        let req = FreenetRequest::Get(GetRequest {
            key: "soshal-mesh-v1".to_string(),
            fetch_contract: true,
            subscribe: true,
            blocking_subscribe: false,
        });
        assert!(to_native_request(&req).is_err(), "non-base58 key must fail");

        let id = ContractInstanceId::from_params_and_code(
            FnetParameters::from(vec![1u8; 32]),
            FnetContractCode::from(vec![0u8; 8]),
        );
        let req = FreenetRequest::Get(GetRequest {
            key: id.encode(),
            fetch_contract: true,
            subscribe: true,
            blocking_subscribe: true,
        });
        let native = to_native_request(&req).unwrap();
        match native {
            FnetClientRequest::ContractOp(FnetContractRequest::Get {
                return_contract_code,
                subscribe,
                blocking_subscribe,
                ..
            }) => {
                assert!(return_contract_code);
                assert!(subscribe);
                assert!(blocking_subscribe);
            }
            other => panic!("expected ContractOp Get, got {other:?}"),
        }
    }

    #[test]
    fn test_native_put_requires_container() {
        let req = FreenetRequest::Put(PutRequest {
            container: None,
            wrapped_state: Some(ContractState {
                key: "soshal-mesh-v1".to_string(),
                state: vec![1, 2, 3],
                contract_code: None,
            }),
            related_contracts: vec![],
            subscribe: false,
            blocking_subscribe: false,
        });
        let err = to_native_request(&req).unwrap_err();
        assert!(err.contains("carrier contract"), "got: {err}");

        let req = FreenetRequest::Put(PutRequest {
            container: Some(ContractContainer {
                contract_code: vec![0u8; 8],
                state: vec![9, 8, 7],
            }),
            wrapped_state: None,
            related_contracts: vec![],
            subscribe: true,
            blocking_subscribe: true,
        });
        let native = to_native_request(&req).unwrap();
        match native {
            FnetClientRequest::ContractOp(FnetContractRequest::Put {
                subscribe,
                blocking_subscribe,
                ..
            }) => {
                assert!(subscribe);
                assert!(blocking_subscribe);
            }
            other => panic!("expected ContractOp Put, got {other:?}"),
        }
    }

    #[test]
    fn test_native_subscribe_maps() {
        let id = ContractInstanceId::from_params_and_code(
            FnetParameters::from(vec![1u8; 32]),
            FnetContractCode::from(vec![0u8; 8]),
        );
        let req = FreenetRequest::Subscribe(SubscribeRequest {
            key: id.encode(),
            summary: Some(StateSummary { data: vec![1] }),
        });
        let native = to_native_request(&req).unwrap();
        match native {
            FnetClientRequest::ContractOp(FnetContractRequest::Subscribe { key, summary }) => {
                assert_eq!(key, id);
                assert!(summary.is_some());
            }
            other => panic!("expected ContractOp Subscribe, got {other:?}"),
        }
    }

    #[test]
    fn test_native_update_unsupported() {
        let req = FreenetRequest::Update(UpdateRequest {
            key: Some("x".to_string()),
            update: None,
        });
        assert!(to_native_request(&req).is_err());
    }

    #[test]
    fn test_native_response_mapping() {
        // NotFound seeds an explicit error response.
        let id = ContractInstanceId::from_params_and_code(
            FnetParameters::from(vec![1u8; 32]),
            FnetContractCode::from(vec![0u8; 8]),
        );
        let not_found =
            Ok::<FnetHostResponse, FnetClientError>(FnetHostResponse::ContractResponse(
                freenet_stdlib::client_api::ContractResponse::NotFound { instance_id: id },
            ));
        let req = FreenetRequest::Get(GetRequest {
            key: id.encode(),
            fetch_contract: true,
            subscribe: false,
            blocking_subscribe: false,
        });
        let mapped = from_native_response(&req, &not_found);
        match mapped {
            Some(FreenetResponse::Error(e)) if e.code == Some(404) => {}
            _ => panic!("expected NotFound mapped to Error(404), got {mapped:?}"),
        }
    }

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
            "ws://localhost:7509",
            "ws://127.0.0.1:7509",
            "ws://127.1:7509",
            "ws://0x7f000001:7509",
            "ws://10.0.0.1:7509",
            "ws://192.168.1.1:7509",
            "ws://169.254.169.254:7509",
            "ws://1.2.3.4.nip.io:7509",
            "ws://1.2.3.4:7509",
            "wss://relay.example.com:443",
            "http://example.com:7509",
        ] {
            let client = FreenetWebSocketClient::new(bad.to_string(), "tok".to_string());
            assert!(
                client.connect().await.is_err(),
                "connect should reject {bad}"
            );
        }
    }
}
