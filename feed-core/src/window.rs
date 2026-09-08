//! Windowed feed engine for streaming bounded post windows to Dart.

use serde::{Deserialize, Serialize};
use soshal_db_core::{block_on, Database};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedPostItem {
    pub event_id: String,
    pub pubkey: String,
    pub content: String,
    /// JSON `{"type","url","blob_hash","size"}` when the post carries a
    /// `["media", ...]` tag (LAN blob-sharing), else `None`.
    pub media_json: Option<String>,
    pub created_at: i64,
    pub reactions: i64,
    pub replies: i64,
    pub reposts: i64,
    pub liked: bool,
    pub profile_name: Option<String>,
    pub profile_picture: Option<String>,
}

/// Fetch a bounded slice/window of posts directly from SQLite.
///
/// `authors`: `None` = every kind-1 post (public); `Some` = only posts whose
/// author is in the set (audience filter; empty set matches nothing).
pub fn fetch_feed_window(
    db: &Database,
    start_index: usize,
    limit: usize,
    authors: Option<&[String]>,
) -> Result<Vec<FeedPostItem>, String> {
    let limit = if limit == 0 {
        0
    } else {
        soshal_db_core::repos::clamp_limit(limit as i64) as usize
    };
    let conn = db.conn().map_err(|e| e.to_string())?;
    block_on(async {
        let active_pubkey: Option<String> = {
            let stmt = conn
                .prepare("SELECT value FROM settings WHERE key = 'active_pubkey'")
                .await
                .map_err(|e| e.to_string())?;
            let mut rows = stmt.query(()).await.map_err(|e| e.to_string())?;
            if let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                row.get(0).ok()
            } else {
                None
            }
        };

        let single_author = match authors {
            Some(list) if list.len() == 1 => Some(&list[0]),
            _ => None,
        };
        let author_clause = match authors {
            Some(_) if single_author.is_some() => " AND p.pubkey = ?4",
            Some(_) => " AND p.pubkey IN (SELECT value FROM json_each(?4))",
            None => "",
        };
        let sql = format!(
            "SELECT p.id, p.pubkey, p.content, p.tags_json, p.created_at,
                    COALESCE(u.name, u.display_name), u.picture,
                    COALESCE((SELECT COUNT(*) FROM reactions r WHERE r.event_id = p.id), 0),
                    COALESCE((SELECT COUNT(*) FROM posts rp WHERE rp.root_id = p.id AND rp.kind = 1 AND rp.is_deleted = 0), 0),
                    COALESCE((SELECT COUNT(*) FROM reposts rt WHERE rt.event_id = p.id), 0),
                    COALESCE((SELECT MAX(CASE WHEN rl.pubkey = ?3 THEN 1 ELSE 0 END) FROM reactions rl WHERE rl.event_id = p.id), 0)
             FROM posts p
             LEFT JOIN users u ON p.pubkey = u.pubkey
             WHERE p.kind = 1 AND p.is_deleted = 0{author_clause}
             ORDER BY p.created_at DESC, p.id DESC
             LIMIT ?1 OFFSET ?2"
        );
        let stmt = conn.prepare(&sql).await.map_err(|e| e.to_string())?;

        let offset = i64::try_from(start_index).unwrap_or(i64::MAX).max(0);
        let mut rows = match authors {
            Some(_) if single_author.is_some() => {
                let single_pk = single_author.unwrap().as_str();
                stmt.query((limit as i64, offset, active_pubkey, single_pk))
                    .await
            }
            Some(author_list) => {
                let authors_json = serde_json::to_string(author_list)
                    .map_err(|e| format!("serialize authors: {e}"))?;
                stmt.query((limit as i64, offset, active_pubkey, authors_json))
                    .await
            }
            None => stmt.query((limit as i64, offset, active_pubkey)).await,
        }
        .map_err(|e| e.to_string())?;

        let mut out = Vec::with_capacity(limit);
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            let event_id: String = row.get(0).map_err(|e| e.to_string())?;
            let pubkey: String = row.get(1).map_err(|e| e.to_string())?;
            let content: String = row.get(2).map_err(|e| e.to_string())?;
            let tags_json: String = row.get(3).map_err(|e| e.to_string())?;
            let created_at: i64 = row.get(4).map_err(|e| e.to_string())?;
            let profile_name: Option<String> = row.get(5).map_err(|e| e.to_string())?;
            let profile_picture: Option<String> = row.get(6).map_err(|e| e.to_string())?;
            let reactions: i64 = row.get(7).map_err(|e| e.to_string())?;
            let replies: i64 = row.get(8).map_err(|e| e.to_string())?;
            let reposts: i64 = row.get(9).map_err(|e| e.to_string())?;
            let liked: i64 = row.get(10).map_err(|e| e.to_string())?;
            out.push(FeedPostItem {
                event_id,
                pubkey,
                content,
                media_json: super::query::media_json_from_tags(&tags_json),
                created_at,
                reactions,
                replies,
                reposts,
                liked: liked != 0,
                profile_name,
                profile_picture,
            });
        }
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use soshal_db_core::repos::post::{PostRepo, PostRow};
    use soshal_db_core::repos::user::{UserRepo, UserRow};

    fn insert_test_user(db: &Database, pubkey: &str) {
        let user = UserRow {
            pubkey: pubkey.into(),
            npub: format!("npub_{pubkey}"),
            name: Some("Alice".into()),
            display_name: None,
            about: None,
            picture: Some("pic.png".into()),
            banner: None,
            nip05: None,
            lud16: None,
            created_at: 1000,
            updated_at: 1000,
            metadata_json: None,
            contact_pubkeys: "[]".into(),
            relay_list: "[]".into(),
            follower_count: 0,
        };
        UserRepo::new(db).upsert(&user).unwrap();
    }

    fn insert_test_post(db: &Database, id: &str, pubkey: &str, content: &str, created_at: i64) {
        let post = PostRow {
            id: id.into(),
            pubkey: pubkey.into(),
            content: content.into(),
            kind: 1,
            created_at,
            tags_json: "[]".into(),
            sig: None,
            reply_to: None,
            root_id: None,
            mentioned_pubkeys: "[]".into(),
            mentioned_hashtags: "[]".into(),
            subject: None,
            sync_status: "synced".into(),
            is_deleted: false,
            scheduled_at: None,
            freenet_key: None,
            is_freenet_native: false,
            rsvp_event_id: None,
        };
        PostRepo::new(db).upsert(&post).unwrap();
    }

    #[test]
    fn test_feed_window_query() {
        let db = soshal_test_util::test_db();
        let posts = fetch_feed_window(&db, 0, 10, None).unwrap();
        assert_eq!(posts.len(), 0);
    }

    #[test]
    fn feed_window_orders_desc_and_joins_profile() {
        let db = soshal_test_util::test_db();
        insert_test_user(&db, "pk-alice");
        insert_test_post(&db, "old", "pk-alice", "first", 100);
        insert_test_post(&db, "new", "pk-alice", "second", 200);

        let posts = fetch_feed_window(&db, 0, 10, None).unwrap();
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].event_id, "new");
        assert_eq!(posts[1].event_id, "old");
        assert_eq!(posts[0].profile_name.as_deref(), Some("Alice"));
        assert_eq!(posts[0].profile_picture.as_deref(), Some("pic.png"));
    }

    #[test]
    fn feed_window_honors_offset_and_limit() {
        let db = soshal_test_util::test_db();
        insert_test_user(&db, "pk-a");
        for i in 0..5 {
            insert_test_post(&db, &format!("p{i}"), "pk-a", &format!("c{i}"), i as i64);
        }
        let page = fetch_feed_window(&db, 2, 2, None).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].event_id, "p2");
        assert_eq!(page[1].event_id, "p1");
        let past_end = fetch_feed_window(&db, 100, 10, None).unwrap();
        assert!(past_end.is_empty());
    }

    #[test]
    fn feed_window_excludes_deleted_and_non_kind1() {
        let db = soshal_test_util::test_db();
        insert_test_user(&db, "pk-a");
        insert_test_post(&db, "live", "pk-a", "visible", 300);
        let deleted = PostRow {
            id: "gone".into(),
            pubkey: "pk-a".into(),
            content: "hidden".into(),
            kind: 1,
            created_at: 400,
            tags_json: "[]".into(),
            sig: None,
            reply_to: None,
            root_id: None,
            mentioned_pubkeys: "[]".into(),
            mentioned_hashtags: "[]".into(),
            subject: None,
            sync_status: "synced".into(),
            is_deleted: true,
            scheduled_at: None,
            freenet_key: None,
            is_freenet_native: false,
            rsvp_event_id: None,
        };
        PostRepo::new(&db).upsert(&deleted).unwrap();
        let repost = PostRow {
            id: "repost".into(),
            pubkey: "pk-a".into(),
            content: "re".into(),
            kind: 6,
            created_at: 500,
            tags_json: "[]".into(),
            sig: None,
            reply_to: None,
            root_id: None,
            mentioned_pubkeys: "[]".into(),
            mentioned_hashtags: "[]".into(),
            subject: None,
            sync_status: "synced".into(),
            is_deleted: false,
            scheduled_at: None,
            freenet_key: None,
            is_freenet_native: false,
            rsvp_event_id: None,
        };
        PostRepo::new(&db).upsert(&repost).unwrap();

        let posts = fetch_feed_window(&db, 0, 10, None).unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].event_id, "live");
    }

    #[test]
    fn feed_window_item_roundtrips_serde() {
        let item = FeedPostItem {
            event_id: "e1".into(),
            pubkey: "pk".into(),
            content: "hi".into(),
            media_json: None,
            created_at: 42,
            reactions: 1,
            replies: 2,
            reposts: 3,
            liked: true,
            profile_name: Some("n".into()),
            profile_picture: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert_eq!(serde_json::from_str::<FeedPostItem>(&json).unwrap(), item);
    }
}
