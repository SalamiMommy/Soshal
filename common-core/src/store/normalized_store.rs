//! Normalized in-memory entity graph store with zero duplication.

use crate::store::delta::EntityDelta;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserEntity {
    pub pubkey: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub nip05: Option<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostEntity {
    pub id: String,
    pub author_pubkey: String,
    pub content: String,
    pub created_at: u64,
    pub like_count: u32,
    pub repost_count: u32,
    pub zap_amount_sats: u64,
    pub user_liked: bool,
    pub bookmarked: bool,
}

type DeltaCallback = Box<dyn Fn(EntityDelta) + Send + Sync + 'static>;

pub struct NormalizedStore {
    users: RwLock<HashMap<String, UserEntity>>,
    posts: RwLock<HashMap<String, PostEntity>>,
    listeners: Mutex<Vec<DeltaCallback>>,
}

impl Default for NormalizedStore {
    fn default() -> Self {
        Self::new()
    }
}

impl NormalizedStore {
    pub fn new() -> Self {
        Self {
            users: RwLock::new(HashMap::new()),
            posts: RwLock::new(HashMap::new()),
            listeners: Mutex::new(Vec::new()),
        }
    }

    pub fn subscribe<F>(&self, callback: F)
    where
        F: Fn(EntityDelta) + Send + Sync + 'static,
    {
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.push(Box::new(callback));
        }
    }

    fn notify(&self, delta: EntityDelta) {
        // Take the listener list out of the lock, then release the lock
        // before invoking callbacks. Holding the Mutex across a callback that
        // re-entered notify() (via upsert_user/update_post_reaction/…) would
        // deadlock a non-reentrant std::sync::Mutex. Taking the Vec out means
        // re-entrant notify() calls (from a callback) see an empty list and
        // return immediately instead of deadlocking.
        let mut taken = {
            let mut listeners = match self.listeners.lock() {
                Ok(l) => l,
                Err(_) => return,
            };
            std::mem::take(&mut *listeners)
        };
        for listener in taken.iter() {
            listener(delta.clone());
        }
        let mut listeners = self.listeners.lock().unwrap_or_else(|e| e.into_inner());
        // Merge, not overwrite: a subscribe() that ran while callbacks were
        // executing pushed onto the live list; overwriting it would lose that
        // listener. Same poison-recovery convention as the store RwLocks.
        let mut live = std::mem::take(&mut *listeners);
        taken.append(&mut live);
        *listeners = taken;
    }

    pub fn upsert_user(&self, user: UserEntity) -> EntityDelta {
        let pubkey = user.pubkey.clone();
        let delta = EntityDelta::UserUpdated {
            pubkey: pubkey.clone(),
            name: user.name.clone(),
            avatar_url: user.avatar_url.clone(),
            nip05: user.nip05.clone(),
        };

        {
            let mut users = self.users.write().unwrap_or_else(|e| e.into_inner());
            users.insert(pubkey, user);
        }

        self.notify(delta.clone());
        delta
    }

    pub fn get_user(&self, pubkey: &str) -> Option<UserEntity> {
        let users = self.users.read().unwrap_or_else(|e| e.into_inner());
        users.get(pubkey).cloned()
    }

    pub fn update_post_reaction(
        &self,
        post_id: &str,
        user_liked: bool,
        like_count_delta: i32,
    ) -> Option<EntityDelta> {
        let mut posts = self.posts.write().unwrap_or_else(|e| e.into_inner());
        if let Some(post) = posts.get_mut(post_id) {
            post.user_liked = user_liked;
            if like_count_delta > 0 {
                post.like_count = post.like_count.saturating_add(like_count_delta as u32);
            } else if like_count_delta < 0 {
                post.like_count = post
                    .like_count
                    .saturating_sub(like_count_delta.unsigned_abs());
            }

            let delta = EntityDelta::PostReactionAdded {
                post_id: post_id.to_string(),
                like_count: post.like_count,
                repost_count: post.repost_count,
                zap_amount_sats: post.zap_amount_sats,
                user_liked,
            };

            self.notify(delta.clone());
            Some(delta)
        } else {
            None
        }
    }

    pub fn clear(&self) {
        let cleared = {
            let mut users = self.users.write().unwrap_or_else(|e| e.into_inner());
            let mut posts = self.posts.write().unwrap_or_else(|e| e.into_inner());
            let non_empty = !users.is_empty() || !posts.is_empty();
            users.clear();
            posts.clear();
            non_empty
        };
        if cleared {
            self.notify(EntityDelta::StoreCleared);
        }
    }
}

pub static GLOBAL_STORE: std::sync::OnceLock<Arc<NormalizedStore>> = std::sync::OnceLock::new();

pub fn global_store() -> &'static Arc<NormalizedStore> {
    GLOBAL_STORE.get_or_init(|| Arc::new(NormalizedStore::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn user(pubkey: &str) -> UserEntity {
        UserEntity {
            pubkey: pubkey.to_string(),
            name: Some("alice".to_string()),
            avatar_url: None,
            nip05: Some("alice@example.com".to_string()),
            updated_at: 1000,
        }
    }

    fn post(id: &str) -> PostEntity {
        PostEntity {
            id: id.to_string(),
            author_pubkey: "pk".to_string(),
            content: "hi".to_string(),
            created_at: 1000,
            like_count: 3,
            repost_count: 1,
            zap_amount_sats: 0,
            user_liked: false,
            bookmarked: false,
        }
    }

    #[test]
    fn test_upsert_and_get_user() {
        let store = NormalizedStore::new();
        assert!(store.get_user("pk1").is_none());
        store.upsert_user(user("pk1"));
        let got = store.get_user("pk1").unwrap();
        assert_eq!(got.name.as_deref(), Some("alice"));
        assert_eq!(got.nip05.as_deref(), Some("alice@example.com"));

        let mut updated = user("pk1");
        updated.name = Some("bob".to_string());
        store.upsert_user(updated);
        assert_eq!(store.get_user("pk1").unwrap().name.as_deref(), Some("bob"));
    }

    #[test]
    fn test_subscribe_receives_deltas() {
        let store = NormalizedStore::new();
        let seen: Arc<Mutex<Vec<EntityDelta>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        store.subscribe(move |d| seen_clone.lock().unwrap().push(d));

        store.upsert_user(user("pk1"));
        store
            .posts
            .write()
            .unwrap()
            .insert("p1".to_string(), post("p1"));
        store.update_post_reaction("p1", true, 1);

        let deltas = seen.lock().unwrap().clone();
        assert!(deltas.contains(&EntityDelta::UserUpdated {
            pubkey: "pk1".to_string(),
            name: Some("alice".to_string()),
            avatar_url: None,
            nip05: Some("alice@example.com".to_string()),
        }));
        assert!(deltas.contains(&EntityDelta::PostReactionAdded {
            post_id: "p1".to_string(),
            like_count: 4,
            repost_count: 1,
            zap_amount_sats: 0,
            user_liked: true,
        }));
    }

    #[test]
    fn test_update_post_reaction() {
        let store = NormalizedStore::new();
        assert_eq!(store.update_post_reaction("missing", true, 1), None);

        store.upsert_user(user("pk1"));
        store
            .posts
            .write()
            .unwrap()
            .insert("p1".to_string(), post("p1"));

        let delta = store.update_post_reaction("p1", true, 1).unwrap();
        match delta {
            EntityDelta::PostReactionAdded {
                like_count,
                user_liked,
                ..
            } => {
                assert_eq!(like_count, 4);
                assert!(user_liked);
            }
            _ => panic!("wrong delta"),
        }
        let stored = store.posts.read().unwrap().get("p1").unwrap().clone();
        assert_eq!(stored.like_count, 4);
        assert!(stored.user_liked);

        store.update_post_reaction("p1", false, -1);
        let stored = store.posts.read().unwrap().get("p1").unwrap().clone();
        assert_eq!(stored.like_count, 3);
        assert!(!stored.user_liked);
    }
}
