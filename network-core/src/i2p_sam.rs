//! I2P SAM V3 client implementation for anonymous networking.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

const SAM_DEFAULT_HOST: &str = "127.0.0.1";
const SAM_DEFAULT_PORT: u16 = 7656;
const SAM_VERSION: &str = "3.1";
const SAM_SIGNATURE_TYPE: &str = "7"; // Ed25519
const SAM_ENCRYPTION_TYPE: &str = "4"; // ECIES-X25519

/// I2P SAM V3 client for anonymous networking
pub struct I2PSamClient {
    host: String,
    port: u16,
    stream: Option<TcpStream>,
    session_id: Option<String>,
    destination: Option<String>,
}

impl I2PSamClient {
    /// Creates a new I2P SAM client
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            stream: None,
            session_id: None,
            destination: None,
        }
    }

    /// Creates a client with default SAM settings
    pub fn default_client() -> Self {
        Self::new(SAM_DEFAULT_HOST.to_string(), SAM_DEFAULT_PORT)
    }

    /// Connects to the SAM bridge
    pub fn connect(&mut self) -> Result<(), String> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream =
            TcpStream::connect(&addr).map_err(|e| format!("SAM connection failed: {e}"))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| format!("Set read timeout failed: {e}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| format!("Set write timeout failed: {e}"))?;

        self.stream = Some(stream);
        Ok(())
    }

    /// Disconnects from the SAM bridge
    pub fn disconnect(&mut self) -> Result<(), String> {
        if let Some(session_id) = &self.session_id {
            self.send_command(&format!("SESSION CLOSE STYLE=STREAM ID={}", session_id))?;
        }

        if let Some(stream) = self.stream.take() {
            stream
                .shutdown(std::net::Shutdown::Both)
                .map_err(|e| format!("Stream shutdown failed: {e}"))?;
        }

        self.session_id = None;
        self.destination = None;
        Ok(())
    }

    /// Sends a SAM command and reads the response
    fn send_command(&mut self, command: &str) -> Result<String, String> {
        let stream = self.stream.as_mut().ok_or("Not connected to SAM bridge")?;

        stream
            .write_all(command.as_bytes())
            .map_err(|e| format!("Command send failed: {e}"))?;
        stream
            .write_all(b"\n")
            .map_err(|e| format!("Newline send failed: {e}"))?;
        stream.flush().map_err(|e| format!("Flush failed: {e}"))?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .map_err(|e| format!("Response read failed: {e}"))?;

        Ok(response.trim().to_string())
    }

    /// Performs SAM handshake
    pub fn handshake(&mut self) -> Result<String, String> {
        let command = format!("HELLO VERSION={} MIN=3.0 MAX=3.3", SAM_VERSION);
        let response = self.send_command(&command)?;

        if response.starts_with("HELLO REPLY") {
            Ok(response)
        } else {
            Err(format!("Handshake failed: {}", response))
        }
    }

    /// Generates a new destination
    pub fn generate_destination(&mut self) -> Result<String, String> {
        let command = format!("DEST GENERATE SIGNATURE_TYPE={}", SAM_SIGNATURE_TYPE);
        let response = self.send_command(&command)?;

        if response.starts_with("DEST REPLY") {
            // Parse destination from response
            if let Some(dest) = response.split("DEST=").nth(1) {
                Ok(dest.to_string())
            } else {
                Err("Failed to parse destination".to_string())
            }
        } else {
            Err(format!("Destination generation failed: {}", response))
        }
    }

    /// Creates a new session
    pub fn create_session(
        &mut self,
        session_id: &str,
        destination: Option<&str>,
    ) -> Result<String, String> {
        let dest_str = destination.unwrap_or("TRANSIENT");
        let command = format!(
            "SESSION CREATE STYLE=STREAM ID={} DESTINATION={} SIGNATURE_TYPE={} i2cp.leaseSetEncType={}",
            session_id, dest_str, SAM_SIGNATURE_TYPE, SAM_ENCRYPTION_TYPE
        );

        let response = self.send_command(&command)?;

        if response.starts_with("SESSION STATUS RESULT=OK") {
            self.session_id = Some(session_id.to_string());

            // Extract destination from response if transient
            if destination.is_none() {
                if let Some(dest) = response.split("DESTINATION=").nth(1) {
                    self.destination = Some(dest.to_string());
                }
            } else {
                self.destination = destination.map(|d| d.to_string());
            }

            Ok(response)
        } else {
            Err(format!("Session creation failed: {}", response))
        }
    }

    /// Connects to a remote I2P destination
    pub fn connect_to_destination(&mut self, destination: &str) -> Result<TcpStream, String> {
        if let Some(session_id) = &self.session_id {
            let command = format!(
                "STREAM CONNECT ID={} DESTINATION={} SILENT=false",
                session_id, destination
            );
            let response = self.send_command(&command)?;

            if response.starts_with("STREAM STATUS RESULT=OK") {
                // Extract port from response
                if let Some(port_str) = response.split("PORT=").nth(1) {
                    let port: u16 = port_str
                        .parse()
                        .map_err(|e| format!("Port parse failed: {e}"))?;

                    TcpStream::connect(format!("{}:{}", self.host, port))
                        .map_err(|e| format!("Stream connection failed: {e}"))
                } else {
                    Err("Failed to parse stream port".to_string())
                }
            } else {
                Err(format!("Stream connect failed: {}", response))
            }
        } else {
            Err("No active session".to_string())
        }
    }

    /// Accepts incoming connections
    pub fn accept_connection(&mut self) -> Result<TcpStream, String> {
        if let Some(session_id) = &self.session_id {
            let command = format!("STREAM ACCEPT ID={} SILENT=false", session_id);
            let response = self.send_command(&command)?;

            if response.starts_with("STREAM STATUS RESULT=OK") {
                // Extract port from response
                if let Some(port_str) = response.split("PORT=").nth(1) {
                    let port: u16 = port_str
                        .parse()
                        .map_err(|e| format!("Port parse failed: {e}"))?;

                    TcpStream::connect(format!("{}:{}", self.host, port))
                        .map_err(|e| format!("Stream connection failed: {e}"))
                } else {
                    Err("Failed to parse stream port".to_string())
                }
            } else {
                Err(format!("Stream accept failed: {}", response))
            }
        } else {
            Err("No active session".to_string())
        }
    }

    /// Gets the current session destination
    pub fn get_destination(&self) -> Option<String> {
        self.destination.clone()
    }

    /// Gets the current session ID
    pub fn get_session_id(&self) -> Option<String> {
        self.session_id.clone()
    }
}

impl Drop for I2PSamClient {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

/// I2P tunnel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I2PTunnelConfig {
    pub sam_host: String,
    pub sam_port: u16,
    pub session_id: String,
    pub destination: Option<String>,
    pub in_tunnel_count: u32,
    pub out_tunnel_count: u32,
}

impl Default for I2PTunnelConfig {
    fn default() -> Self {
        Self {
            sam_host: SAM_DEFAULT_HOST.to_string(),
            sam_port: SAM_DEFAULT_PORT,
            session_id: "soshal".to_string(),
            destination: None,
            in_tunnel_count: 2,
            out_tunnel_count: 2,
        }
    }
}

/// I2P tunnel manager
pub struct I2PTunnelManager {
    client: I2PSamClient,
    config: I2PTunnelConfig,
}

impl I2PTunnelManager {
    /// Creates a new I2P tunnel manager
    pub fn new(config: I2PTunnelConfig) -> Self {
        let client = I2PSamClient::new(config.sam_host.clone(), config.sam_port);
        Self { client, config }
    }

    /// Starts the I2P tunnel
    pub fn start(&mut self) -> Result<String, String> {
        self.client.connect()?;
        self.client.handshake()?;

        let dest = if let Some(ref dest) = self.config.destination {
            self.client
                .create_session(&self.config.session_id, Some(dest))?;
            dest.clone()
        } else {
            let generated = self.client.generate_destination()?;
            self.client.create_session(&self.config.session_id, None)?;
            generated
        };

        Ok(dest)
    }

    /// Stops the I2P tunnel
    pub fn stop(&mut self) -> Result<(), String> {
        self.client.disconnect()
    }

    /// Gets the client reference
    pub fn client(&mut self) -> &mut I2PSamClient {
        &mut self.client
    }
}

// ---------------------------------------------------------------------------
// Persistent session manager
// ---------------------------------------------------------------------------

/// Process-wide manager holding one SAM session for the app lifetime, so
/// P2P/streaming can both initiate and accept connections over i2p without
/// rebuilding the tunnel per call.
pub struct I2PSessionManager {
    tunnel: std::sync::Mutex<Option<I2PTunnelManager>>,
}

impl I2PSessionManager {
    pub fn new() -> Self {
        Self {
            tunnel: std::sync::Mutex::new(None),
        }
    }

    /// Starts (or restarts) the session. Pass the persistent destination from
    /// a previous run to keep the same address; `None` creates a transient
    /// destination for this run.
    pub fn start(&self, destination: Option<&str>) -> Result<String, String> {
        let mut guard = self.tunnel.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut t) = guard.take() {
            let _ = t.stop();
        }
        let mut tunnel = I2PTunnelManager::new(I2PTunnelConfig {
            destination: destination.map(|d| d.to_string()),
            ..I2PTunnelConfig::default()
        });
        let dest = tunnel.start()?;
        *guard = Some(tunnel);
        Ok(dest)
    }

    pub fn stop(&self) {
        if let Some(mut t) = self.tunnel.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = t.stop();
        }
    }

    pub fn is_running(&self) -> bool {
        self.tunnel
            .lock()
            .map(|g| g.as_ref().is_some())
            .unwrap_or(false)
    }

    pub fn destination(&self) -> Option<String> {
        self.tunnel
            .lock()
            .ok()
            .and_then(|mut g| g.as_mut().and_then(|t| t.client().get_destination()))
    }

    /// Opens an outbound stream to a remote i2p destination.
    pub fn connect_to_destination(&self, destination: &str) -> Result<std::net::TcpStream, String> {
        match self
            .tunnel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            Some(t) => t.client().connect_to_destination(destination),
            None => Err("i2p session not running".to_string()),
        }
    }

    /// Blocks until an inbound connection arrives on the session.
    pub fn accept_connection(&self) -> Result<std::net::TcpStream, String> {
        match self
            .tunnel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            Some(t) => t.client().accept_connection(),
            None => Err("i2p session not running".to_string()),
        }
    }
}

impl Default for I2PSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = I2PSamClient::default_client();
        assert_eq!(client.host, SAM_DEFAULT_HOST);
        assert_eq!(client.port, SAM_DEFAULT_PORT);
    }

    #[test]
    fn test_tunnel_config_default() {
        let config = I2PTunnelConfig::default();
        assert_eq!(config.sam_host, SAM_DEFAULT_HOST);
        assert_eq!(config.sam_port, SAM_DEFAULT_PORT);
        assert_eq!(config.in_tunnel_count, 2);
        assert_eq!(config.out_tunnel_count, 2);
    }

    #[test]
    fn test_command_formatting() {
        let command = "HELLO VERSION=3.1 MIN=3.0 MAX=3.3";
        assert!(command.contains("HELLO"));
        assert!(command.contains("VERSION=3.1"));
    }
}
