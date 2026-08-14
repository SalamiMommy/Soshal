//! Freenet WebSocket client for contract operations and node communication.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream};

/// Freenet WebSocket client for contract operations
pub struct FreenetWebSocketClient {
    url: String,
    auth_token: String,
    socket: Arc<
        Mutex<Option<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>>,
    >,
}

impl FreenetWebSocketClient {
    /// Creates a new Freenet WebSocket client
    pub fn new(url: String, auth_token: String) -> Self {
        Self {
            url,
            auth_token,
            socket: Arc::new(Mutex::new(None)),
        }
    }

    /// Connects to the Freenet node
    pub async fn connect(&self) -> Result<(), String> {
        let url_with_auth = if self.auth_token.is_empty() {
            self.url.clone()
        } else {
            format!("{}?auth={}", self.url, self.auth_token)
        };

        let (ws_stream, _) = connect_async(&url_with_auth)
            .await
            .map_err(|e| format!("WebSocket connection failed: {e}"))?;

        let mut socket_guard = self.socket.lock().await;
        *socket_guard = Some(ws_stream);
        Ok(())
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

    /// Fetches contract state from the Freenet node
    pub async fn get_contract(&self, key: &str, subscribe: bool) -> Result<ContractState, String> {
        let request = FreenetRequest::Get(GetRequest {
            key: key.to_string(),
            fetch_contract: true,
            subscribe,
            blocking_subscribe: false,
        });

        let response = self.send_request(request).await?;

        match response {
            FreenetResponse::GetResult(result) => Ok(result.state),
            FreenetResponse::Error(err) => Err(format!("Get failed: {}", err.message)),
            _ => Err("Unexpected response type".to_string()),
        }
    }

    /// Publishes contract state to the Freenet node
    pub async fn put_contract(
        &self,
        state: ContractState,
        subscribe: bool,
    ) -> Result<String, String> {
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
        summary: Option<ContractSummary>,
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
    pub summary: Option<ContractSummary>,
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
pub struct RelatedContract {
    pub key: String,
    pub summary: Option<ContractSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContractSummary {
    pub data: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StateUpdate {
    pub delta: Vec<u8>,
    pub summary: Option<ContractSummary>,
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
}
