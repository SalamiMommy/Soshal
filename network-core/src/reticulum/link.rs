//! Reticulum link establishment and management for encrypted peer connections.

use super::address::ReticulumAddress;
use super::packet::{ReticulumPacket, ReticulumPacketType};
use crate::pqc_link::PqcLinkCrypto;
use serde::{Deserialize, Serialize};
use soshal_common_core::format::now_secs;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const LINK_TIMEOUT_SECS: u64 = 300; // 5 minutes
const MAX_PENDING_LINKS: usize = 50;

/// Link-request payload marker byte: a PQC hybrid public key follows.
const LINK_REQUEST_PQC: u8 = 0x01;

fn link_peer_id(dest: &ReticulumAddress) -> String {
    format!("reticulum:{}", dest)
}

fn link_context(dest: &ReticulumAddress) -> String {
    format!("reticulum-link:{}", dest)
}

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
    crypto: PqcLinkCrypto,
}

impl LinkManager {
    pub fn new() -> Self {
        Self {
            links: Arc::new(Mutex::new(HashMap::new())),
            crypto: PqcLinkCrypto::new(),
        }
    }

    /// Initiates a link request to a remote destination. The request payload
    /// carries this side's hybrid PQC public key; the peer answers with its
    /// own in the proof packet, completing the ratchet handshake.
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

        // Create link request packet carrying our hybrid PQC public key so
        // the responder can bootstrap the ratchet session.
        let own_pk = self
            .crypto
            .begin_handshake(&link_peer_id(&remote_dest), &link_context(&remote_dest))?;
        let mut payload = Vec::with_capacity(1 + own_pk.len());
        payload.push(LINK_REQUEST_PQC);
        payload.extend_from_slice(own_pk.as_bytes());

        Ok(ReticulumPacket::new(
            remote_dest,
            ReticulumPacketType::LinkRequest,
            payload,
        ))
    }

    /// Processes an incoming link request: bootstraps the responder-side
    /// ratchet session from the initiator's public key and returns a proof
    /// packet carrying our own hybrid public key.
    pub fn handle_link_request(
        &self,
        from_dest: ReticulumAddress,
        request_payload: &[u8],
    ) -> Result<ReticulumPacket, String> {
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());

        if request_payload.first() != Some(&LINK_REQUEST_PQC) {
            return Err("unsupported link request payload".to_string());
        }
        let initiator_pk = std::str::from_utf8(&request_payload[1..])
            .map_err(|_| "link request pk not ascii".to_string())?;

        // Inbound cap: attacker LinkRequest floods must not grow the map
        // (MAX_PENDING_LINKS only guarded outbound requests before).
        if links.len() >= MAX_PENDING_LINKS {
            return Err("Maximum pending links reached".to_string());
        }

        let now = now_secs() as u64;

        // Create the ratchet session keyed to the initiator's public key.
        let own_pk = self.crypto.accept_handshake_pk(
            &link_peer_id(&from_dest),
            &link_context(&from_dest),
            initiator_pk,
        )?;

        let link_info = LinkInfo {
            remote_destination: from_dest,
            state: LinkState::Established,
            created_at: now,
            last_activity: now,
            tx_packets: 0,
            rx_packets: 0,
        };

        links.insert(from_dest, link_info);

        // Return proof packet carrying our hybrid public key.
        Ok(ReticulumPacket::new(
            from_dest,
            ReticulumPacketType::Proof,
            own_pk.into_bytes(),
        ))
    }

    /// Processes a link proof response: completes the initiator-side ratchet
    /// handshake with the responder's public key.
    pub fn handle_link_proof(
        &self,
        from_dest: ReticulumAddress,
        proof_data: &[u8],
    ) -> Result<(), String> {
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(link) = links.get_mut(&from_dest) {
            if link.state == LinkState::Pending {
                link.state = LinkState::Established;
                link.last_activity = now_secs() as u64;

                // Complete the ratchet handshake with the responder's key.
                let responder_pk = std::str::from_utf8(proof_data)
                    .map_err(|_| "link proof pk not ascii".to_string())?;
                self.crypto
                    .complete_handshake(&link_peer_id(&from_dest), responder_pk)?;

                Ok(())
            } else {
                Err("Link not in pending state".to_string())
            }
        } else {
            Err("Unknown link".to_string())
        }
    }

    /// Encrypts data for an established link with the hybrid PQC double
    /// ratchet.
    pub fn encrypt_for_link(
        &self,
        dest: &ReticulumAddress,
        data: &[u8],
    ) -> Result<Vec<u8>, String> {
        self.crypto
            .encrypt(&link_peer_id(dest), &link_context(dest), data)
    }

    /// Decrypts data from an established link with the hybrid PQC double
    /// ratchet.
    pub fn decrypt_from_link(
        &self,
        dest: &ReticulumAddress,
        data: &[u8],
    ) -> Result<Vec<u8>, String> {
        self.crypto
            .decrypt(&link_peer_id(dest), &link_context(dest), data)
    }

    /// Updates activity timestamp for a link
    pub fn update_activity(&self, dest: &ReticulumAddress) {
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(link) = links.get_mut(dest) {
            link.last_activity = now_secs() as u64;
        }
    }

    /// Closes a link
    pub fn close_link(&self, dest: &ReticulumAddress) {
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());

        let _link = links.remove(dest);
        self.crypto.remove(&link_peer_id(dest));
    }

    /// Prunes stale links
    pub fn prune_stale_links(&self) -> usize {
        let now = now_secs() as u64;

        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());

        let before = links.len();

        links.retain(|dest, link| {
            if now - link.last_activity > LINK_TIMEOUT_SECS {
                self.crypto.remove(&link_peer_id(dest));
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
        let initiator = LinkManager::new();
        let responder = LinkManager::new();
        let dest = ReticulumAddress::from_pubkey("test_pubkey");

        let request = initiator.request_link(dest).unwrap();
        let proof = responder
            .handle_link_request(dest, &request.payload)
            .unwrap();
        assert_eq!(proof.packet_type, ReticulumPacketType::Proof);

        let link_info = responder.get_link_info(&dest).unwrap();
        assert!(matches!(link_info.state, LinkState::Established));
    }

    #[test]
    fn test_encryption_context() {
        let initiator = LinkManager::new();
        let responder = LinkManager::new();
        let dest = ReticulumAddress::from_pubkey("test_pubkey");

        let request = initiator.request_link(dest).unwrap();
        let proof = responder
            .handle_link_request(dest, &request.payload)
            .unwrap();
        initiator.handle_link_proof(dest, &proof.payload).unwrap();

        let data = b"test data";
        let encrypted = initiator.encrypt_for_link(&dest, data).unwrap();
        assert_ne!(data.to_vec(), encrypted);

        let decrypted = responder.decrypt_from_link(&dest, &encrypted).unwrap();
        assert_eq!(data.to_vec(), decrypted);
    }

    #[test]
    fn test_request_link_max_pending() {
        let manager = LinkManager::new();
        for i in 0..MAX_PENDING_LINKS {
            let dest = ReticulumAddress::from_pubkey(&format!("peer_{i}"));
            assert!(manager.request_link(dest).is_ok());
        }
        let extra = ReticulumAddress::from_pubkey("peer_overflow");
        let err = manager.request_link(extra).unwrap_err();
        assert!(err.contains("Maximum pending links reached"));
    }

    #[test]
    fn test_link_proof_establishes_pending_and_enables_encryption() {
        let initiator = LinkManager::new();
        let responder = LinkManager::new();
        let dest = ReticulumAddress::from_pubkey("test_pubkey");

        let request = initiator.request_link(dest).unwrap();
        assert_eq!(request.packet_type, ReticulumPacketType::LinkRequest);

        let proof = responder
            .handle_link_request(dest, &request.payload)
            .unwrap();
        initiator.handle_link_proof(dest, &proof.payload).unwrap();

        let link_info = initiator.get_link_info(&dest).unwrap();
        assert!(matches!(link_info.state, LinkState::Established));

        let data = b"proof-derived key roundtrip";
        let encrypted = initiator.encrypt_for_link(&dest, data).unwrap();
        assert_ne!(data.to_vec(), encrypted);
        let decrypted = responder.decrypt_from_link(&dest, &encrypted).unwrap();
        assert_eq!(data.to_vec(), decrypted);
    }

    #[test]
    fn test_link_proof_error_paths() {
        let manager = LinkManager::new();
        let dest = ReticulumAddress::from_pubkey("test_pubkey");

        let err = manager.handle_link_proof(dest, &[0u8; 32]).unwrap_err();
        assert!(err.contains("Unknown link"));

        let request = manager.request_link(dest).unwrap();
        // Non-ASCII proof payload fails the pk parse.
        let err = manager.handle_link_proof(dest, &[0xFFu8; 32]).unwrap_err();
        assert!(err.contains("not ascii"));
        // Reusing a request payload re-establishes the session only once.
        let err = manager
            .handle_link_request(dest, &request.payload)
            .unwrap_err();
        assert!(err.contains("session already exists"));
    }

    #[test]
    fn test_encrypt_decrypt_without_context() {
        let manager = LinkManager::new();
        let dest = ReticulumAddress::from_pubkey("test_pubkey");

        let err = manager.encrypt_for_link(&dest, b"data").unwrap_err();
        assert!(err.contains("no ratchet session"));
        let err = manager.decrypt_from_link(&dest, b"data").unwrap_err();
        assert!(err.contains("bad ratchet frame"));
    }

    #[test]
    fn test_update_activity_and_close() {
        let initiator = LinkManager::new();
        let responder = LinkManager::new();
        let dest = ReticulumAddress::from_pubkey("test_pubkey");

        let request = initiator.request_link(dest).unwrap();
        let proof = responder
            .handle_link_request(dest, &request.payload)
            .unwrap();
        initiator.handle_link_proof(dest, &proof.payload).unwrap();

        initiator.update_activity(&dest);
        assert!(initiator.get_link_info(&dest).is_some());

        initiator.close_link(&dest);
        assert!(initiator.get_link_info(&dest).is_none());
        assert!(initiator.encrypt_for_link(&dest, b"data").is_err());
        assert!(initiator.get_active_links().is_empty());
    }

    #[test]
    fn test_prune_stale_links() {
        let initiator = LinkManager::new();
        let responder = LinkManager::new();
        let fresh = ReticulumAddress::from_pubkey("fresh_peer");
        let stale = ReticulumAddress::from_pubkey("stale_peer");

        let req_fresh = initiator.request_link(fresh).unwrap();
        let proof_fresh = responder
            .handle_link_request(fresh, &req_fresh.payload)
            .unwrap();
        initiator
            .handle_link_proof(fresh, &proof_fresh.payload)
            .unwrap();

        let req_stale = initiator.request_link(stale).unwrap();
        let proof_stale = responder
            .handle_link_request(stale, &req_stale.payload)
            .unwrap();
        initiator
            .handle_link_proof(stale, &proof_stale.payload)
            .unwrap();

        {
            let mut links = initiator.links.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(link) = links.get_mut(&stale) {
                link.last_activity = 0;
            }
        }

        let pruned = initiator.prune_stale_links();
        assert_eq!(pruned, 1);
        assert!(initiator.get_link_info(&fresh).is_some());
        assert!(initiator.get_link_info(&stale).is_none());
        assert_eq!(initiator.prune_stale_links(), 0);
    }

    #[test]
    fn test_get_active_links_filters_pending() {
        let initiator = LinkManager::new();
        let responder = LinkManager::new();
        let pending = ReticulumAddress::from_pubkey("pending_peer");
        let established = ReticulumAddress::from_pubkey("established_peer");

        initiator.request_link(pending).unwrap();
        let req = initiator.request_link(established).unwrap();
        let proof = responder
            .handle_link_request(established, &req.payload)
            .unwrap();
        initiator
            .handle_link_proof(established, &proof.payload)
            .unwrap();

        let active = initiator.get_active_links();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].remote_destination, established);
    }

    #[test]
    fn test_announce() {
        let initiator = LinkManager::new();
        let responder = LinkManager::new();
        assert!(!initiator.announce("pk").unwrap());

        let pending = ReticulumAddress::from_pubkey("pending_peer");
        initiator.request_link(pending).unwrap();
        assert!(!initiator.announce("pk").unwrap());

        let established = ReticulumAddress::from_pubkey("established_peer");
        let req = initiator.request_link(established).unwrap();
        let proof = responder
            .handle_link_request(established, &req.payload)
            .unwrap();
        initiator
            .handle_link_proof(established, &proof.payload)
            .unwrap();
        assert!(initiator.announce("pk").unwrap());
    }
}
