//! Windowed feed engine for streaming bounded post windows to Dart.

use serde::{Deserialize, Serialize};
use soshal_db_core::{block_on, Database};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedPostItem {
    pub event_id: String,
    pub pubkey: String,
    pub content: String,
    pub created_at: i64,
    pub reactions: i64,
    pub replies: i64,
    pub reposts: i64,
    pub liked: bool,
    pub profile_name: Option<String>,
    pub profile_picture: Option<String>,
}

/// Fetch a bounded slice/window of posts directly from SQLite.
pub fn fetch_feed_window(
    db: &Database,
    start_index: usize,
    limit: usize,
) -> Result<Vec<FeedPostItem>, String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    block_on(async {
        let mut stmt = conn
            .prepare(
                "SELECT p.id, p.pubkey, p.content, p.created_at,
                        COALESCE(u.name, u.display_name), u.picture
                 FROM posts p
                 LEFT JOIN users u ON p.pubkey = u.pubkey
                 WHERE p.kind = 1 AND p.is_deleted = 0
                 ORDER BY p.created_at DESC
                 LIMIT ?1 OFFSET ?2",
            )
            .await
            .map_err(|e| e.to_string())?;

        let mut rows = stmt
            .query((limit as i64, start_index as i64))
            .await
            .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            let event_id: String = row.get(0).map_err(|e| e.to_string())?;
            let pubkey: String = row.get(1).map_err(|e| e.to_string())?;
            let content: String = row.get(2).map_err(|e| e.to_string())?;
            let created_at: i64 = row.get(3).map_err(|e| e.to_string())?;
            let profile_name: Option<String> = row.get(4).map_err(|e| e.to_string())?;
            let profile_picture: Option<String> = row.get(5).map_err(|e| e.to_string())?;
            out.push(FeedPostItem {
                event_id,
                pubkey,
                content,
                created_at,
                reactions: 0,
                replies: 0,
                reposts: 0,
                liked: false,
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

    #[test]
    fn test_feed_window_query() {
        let db = Database::open_in_memory().unwrap();
        db.migrate().unwrap();
        let posts = fetch_feed_window(&db, 0, 10).unwrap();
        assert_eq!(posts.len(), 0);
    }
}
