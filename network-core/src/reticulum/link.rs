//! Reticulum link establishment and management for encrypted peer connections.

use super::address::ReticulumAddress;
use super::crypto::ReticulumEncryption;
use super::packet::{ReticulumPacket, ReticulumPacketType};
use serde::{Deserialize, Serialize};
use soshal_common_core::format::now_secs;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const LINK_TIMEOUT_SECS: u64 = 300; // 5 minutes
const MAX_PENDING_LINKS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LinkState {
    Pending,
    Established,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkInfo {
    pub remote_destination: ReticulumAddress,
    pub state: LinkState,
    pub created_at: u64,
    pub last_activity: u64,
    pub tx_packets: u64,
    pub rx_packets: u64,
}

pub struct LinkManager {
    links: Arc<Mutex<HashMap<ReticulumAddress, LinkInfo>>>,
    encryption: Arc<Mutex<HashMap<ReticulumAddress, ReticulumEncryption>>>,
}

impl LinkManager {
    pub fn new() -> Self {
        Self {
            links: Arc::new(Mutex::new(HashMap::new())),
            encryption: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Initiates a link request to a remote destination
    pub fn request_link(&self, remote_dest: ReticulumAddress) -> Result<ReticulumPacket, String> {
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());

        if links.len() >= MAX_PENDING_LINKS {
            return Err("Maximum pending links reached".to_string());
        }

        let now = now_secs() as u64;

        let link_info = LinkInfo {
            remote_destination: remote_dest,
            state: LinkState::Pending,
            created_at: now,
            last_activity: now,
            tx_packets: 0,
            rx_packets: 0,
        };

        links.insert(remote_dest, link_info);

        // Create link request packet
        let payload = vec![0x01]; // Link request type
        Ok(ReticulumPacket::new(
            remote_dest,
            ReticulumPacketType::LinkRequest,
            payload,
        ))
    }

    /// Processes an incoming link request
    pub fn handle_link_request(
        &self,
        from_dest: ReticulumAddress,
    ) -> Result<ReticulumPacket, String> {
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());

        let now = now_secs() as u64;

        // Create encryption context for this link
        let encryption = ReticulumEncryption::new();
        let mut enc_map = self.encryption.lock().unwrap_or_else(|e| e.into_inner());
        enc_map.insert(from_dest, encryption.clone());

        let link_info = LinkInfo {
            remote_destination: from_dest,
            state: LinkState::Established,
            created_at: now,
            last_activity: now,
            tx_packets: 0,
            rx_packets: 0,
        };

        links.insert(from_dest, link_info);

        // Return proof packet with encrypted nonce
        let proof_payload = encryption.nonce.clone();
        Ok(ReticulumPacket::new(
            from_dest,
            ReticulumPacketType::Proof,
            proof_payload,
        ))
    }

    /// Processes a link proof response
    pub fn handle_link_proof(
        &self,
        from_dest: ReticulumAddress,
        proof_data: &[u8],
    ) -> Result<(), String> {
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(link) = links.get_mut(&from_dest) {
            if link.state == LinkState::Pending {
                link.state = LinkState::Established;
                link.last_activity = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                // Store encryption context from proof
                let encryption = ReticulumEncryption::from_key(proof_data.to_vec());
                let mut enc_map = self.encryption.lock().unwrap_or_else(|e| e.into_inner());
                enc_map.insert(from_dest, encryption);

                Ok(())
            } else {
                Err("Link not in pending state".to_string())
            }
        } else {
            Err("Unknown link".to_string())
        }
    }

    /// Encrypts data for an established link
    pub fn encrypt_for_link(
        &self,
        dest: &ReticulumAddress,
        data: &[u8],
    ) -> Result<Vec<u8>, String> {
        let enc_map = self.encryption.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(encryption) = enc_map.get(dest) {
            encryption.encrypt(data)
        } else {
            Err("No encryption context for destination".to_string())
        }
    }

    /// Decrypts data from an established link
    pub fn decrypt_from_link(
        &self,
        dest: &ReticulumAddress,
        data: &[u8],
    ) -> Result<Vec<u8>, String> {
        let enc_map = self.encryption.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(encryption) = enc_map.get(dest) {
            encryption.decrypt(data)
        } else {
            Err("No encryption context for destination".to_string())
        }
    }

    /// Updates activity timestamp for a link
    pub fn update_activity(&self, dest: &ReticulumAddress) {
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(link) = links.get_mut(dest) {
            link.last_activity = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }
    }

    /// Closes a link
    pub fn close_link(&self, dest: &ReticulumAddress) {
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());
        let mut enc_map = self.encryption.lock().unwrap_or_else(|e| e.into_inner());

        let _link = links.remove(dest);
        enc_map.remove(dest);
    }

    /// Prunes stale links
    pub fn prune_stale_links(&self) -> usize {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());
        let mut enc_map = self.encryption.lock().unwrap_or_else(|e| e.into_inner());

        let before = links.len();

        links.retain(|dest, link| {
            if now - link.last_activity > LINK_TIMEOUT_SECS {
                enc_map.remove(dest);
                false
            } else {
                true
            }
        });

        before - links.len()
    }

    /// Gets link information
    pub fn get_link_info(&self, dest: &ReticulumAddress) -> Option<LinkInfo> {
        let links = self.links.lock().unwrap_or_else(|e| e.into_inner());
        links.get(dest).cloned()
    }

    /// Gets all active links
    pub fn get_active_links(&self) -> Vec<LinkInfo> {
        let links = self.links.lock().unwrap_or_else(|e| e.into_inner());
        links
            .values()
            .filter(|l| l.state == LinkState::Established)
            .cloned()
            .collect()
    }

    /// Announces this node's presence via all active links.
    pub fn announce(&self, _pubkey: &str) -> Result<bool, String> {
        let links = self.links.lock().unwrap_or_else(|e| e.into_inner());

        if links.is_empty() {
            return Ok(false); // No active links to announce through
        }

        // In a full implementation, this would broadcast an announce packet
        // via the link layer. For now, return success if any links exist.
        Ok(links
            .iter()
            .any(|(_, link)| link.state == LinkState::Established))
    }
}

impl Default for LinkManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_establishment() {
        let manager = LinkManager::new();
        let dest = ReticulumAddress::from_pubkey("test_pubkey");

        let request = manager.request_link(dest).unwrap();
        assert_eq!(request.packet_type, ReticulumPacketType::LinkRequest);

        let link_info = manager.get_link_info(&dest).unwrap();
        assert!(matches!(link_info.state, LinkState::Pending));
    }

    #[test]
    fn test_link_proof_handling() {
        let manager = LinkManager::new();
        let dest = ReticulumAddress::from_pubkey("test_pubkey");

        let proof = manager.handle_link_request(dest).unwrap();
        assert_eq!(proof.packet_type, ReticulumPacketType::Proof);

        let link_info = manager.get_link_info(&dest).unwrap();
        assert!(matches!(link_info.state, LinkState::Established));
    }

    #[test]
    fn test_encryption_context() {
        let manager = LinkManager::new();
        let dest = ReticulumAddress::from_pubkey("test_pubkey");

        manager.handle_link_request(dest).unwrap();

        let data = b"test data";
        let encrypted = manager.encrypt_for_link(&dest, data).unwrap();
        assert_ne!(data.to_vec(), encrypted);

        let decrypted = manager.decrypt_from_link(&dest, &encrypted).unwrap();
        assert_eq!(data.to_vec(), decrypted);
    }
}
