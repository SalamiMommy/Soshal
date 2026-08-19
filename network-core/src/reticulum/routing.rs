//! Reticulum distance-vector mesh routing engine and path table.

use super::address::ReticulumAddress;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_ROUTE_TTL_SECS: u64 = 7200; // 2 hours
/// Cap on path table entries; new routes rejected past it (bounds
/// attacker-flooded announces).
pub const MAX_PATH_TABLE_ROUTES: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathEntry {
    pub destination: ReticulumAddress,
    pub next_hop: ReticulumAddress,
    pub hop_count: u8,
    pub expires_at: u64,
}

#[derive(Debug, Default)]
pub struct PathTable {
    routes: HashMap<ReticulumAddress, PathEntry>,
}

impl PathTable {
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }

    /// Registers or updates a mesh route to a destination.
    pub fn update_route(
        &mut self,
        destination: ReticulumAddress,
        next_hop: ReticulumAddress,
        hop_count: u8,
        now_secs: u64,
    ) -> bool {
        let expires_at = now_secs + DEFAULT_ROUTE_TTL_SECS;

        if let Some(existing) = self.routes.get_mut(&destination) {
            // Update if newer hop count is lower or equal, or if route is refreshed
            if hop_count <= existing.hop_count || existing.expires_at < now_secs {
                existing.next_hop = next_hop;
                existing.hop_count = hop_count;
                existing.expires_at = expires_at;
                return true;
            }
            false
        } else {
            if self.routes.len() >= MAX_PATH_TABLE_ROUTES {
                self.prune_expired(now_secs);
            }
            if self.routes.len() >= MAX_PATH_TABLE_ROUTES {
                return false;
            }
            self.routes.insert(
                destination,
                PathEntry {
                    destination,
                    next_hop,
                    hop_count,
                    expires_at,
                },
            );
            true
        }
    }

    /// Fetches an active route for a target destination.
    pub fn get_route(&self, destination: &ReticulumAddress) -> Option<PathEntry> {
        self.routes.get(destination).cloned()
    }

    /// Removes expired routes from the path table.
    pub fn prune_expired(&mut self, now_secs: u64) -> usize {
        let before = self.routes.len();
        self.routes.retain(|_, entry| entry.expires_at > now_secs);
        before - self.routes.len()
    }

    /// Returns a list of all active path entries.
    pub fn entries(&self) -> Vec<PathEntry> {
        self.routes.values().cloned().collect()
    }

    /// Returns total active routes count.
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_table_routing() {
        let mut table = PathTable::new();
        let dest = ReticulumAddress::from_pubkey("dest1");
        let hop1 = ReticulumAddress::from_pubkey("hop1");

        assert!(table.update_route(dest, hop1, 2, 1000));
        assert_eq!(table.len(), 1);

        let route = table.get_route(&dest).unwrap();
        assert_eq!(route.hop_count, 2);
        assert_eq!(route.next_hop, hop1);

        // Suboptimal route should be rejected
        let hop2 = ReticulumAddress::from_pubkey("hop2");
        assert!(!table.update_route(dest, hop2, 4, 1005));
        assert_eq!(table.get_route(&dest).unwrap().hop_count, 2);

        // Optimal route should update
        assert!(table.update_route(dest, hop2, 1, 1010));
        assert_eq!(table.get_route(&dest).unwrap().hop_count, 1);
        assert_eq!(table.get_route(&dest).unwrap().next_hop, hop2);
    }

    #[test]
    fn test_prune_expired_removes_stale_routes() {
        let mut table = PathTable::new();
        let dest = ReticulumAddress::from_pubkey("prune_dest");
        let hop = ReticulumAddress::from_pubkey("prune_hop");

        table.update_route(dest, hop, 1, 1000);
        assert_eq!(table.len(), 1);

        // Advance clock past the 7200 s TTL
        let removed = table.prune_expired(1000 + DEFAULT_ROUTE_TTL_SECS + 1);
        assert_eq!(removed, 1);
        assert_eq!(table.len(), 0);
        assert!(table.get_route(&dest).is_none());
    }

    #[test]
    fn test_update_route_accepts_worse_hop_when_expired() {
        let mut table = PathTable::new();
        let dest = ReticulumAddress::from_pubkey("expired_dest");
        let hop1 = ReticulumAddress::from_pubkey("expired_hop1");
        let hop2 = ReticulumAddress::from_pubkey("expired_hop2");

        assert!(table.update_route(dest, hop1, 2, 1000));

        // Existing route is expired (now past TTL) so a worse hop is accepted
        let now = 1000 + DEFAULT_ROUTE_TTL_SECS + 1;
        assert!(table.update_route(dest, hop2, 5, now));
        let route = table.get_route(&dest).unwrap();
        assert_eq!(route.hop_count, 5);
        assert_eq!(route.next_hop, hop2);
    }

    #[test]
    fn test_update_route_accepts_equal_hop_count() {
        let mut table = PathTable::new();
        let dest = ReticulumAddress::from_pubkey("equal_dest");
        let hop1 = ReticulumAddress::from_pubkey("equal_hop1");
        let hop2 = ReticulumAddress::from_pubkey("equal_hop2");

        assert!(table.update_route(dest, hop1, 2, 1000));

        // Equal hop count is accepted (<=) and refreshes the route
        assert!(table.update_route(dest, hop2, 2, 1005));
        let route = table.get_route(&dest).unwrap();
        assert_eq!(route.hop_count, 2);
        assert_eq!(route.next_hop, hop2);
    }

    #[test]
    fn test_entries_returns_all_routes() {
        let mut table = PathTable::new();
        table.update_route(
            ReticulumAddress::from_pubkey("entries_dest1"),
            ReticulumAddress::from_pubkey("entries_hop1"),
            1,
            1000,
        );
        table.update_route(
            ReticulumAddress::from_pubkey("entries_dest2"),
            ReticulumAddress::from_pubkey("entries_hop2"),
            3,
            1000,
        );

        let entries = table.entries();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.hop_count == 1));
        assert!(entries.iter().any(|e| e.hop_count == 3));
    }

    #[test]
    fn test_is_empty_reflects_table_state() {
        let mut table = PathTable::new();
        assert!(table.is_empty());

        table.update_route(
            ReticulumAddress::from_pubkey("empty_dest"),
            ReticulumAddress::from_pubkey("empty_hop"),
            1,
            1000,
        );
        assert!(!table.is_empty());
    }
}
