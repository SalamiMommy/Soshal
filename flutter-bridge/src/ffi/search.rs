//! Search FFI module
//!
//! Full-text search (FTS5) across posts, profiles, and hashtags via the
//! search-index table (populated by the relay sync loop). Query terms are
//! sanitized Rust-side before reaching the FTS parser.

use flutter_rust_bridge::frb;
use nostr_sdk::client::Client;
use nostr_sdk::prelude::{Filter, Kind};
use serde::{Deserialize, Serialize};
use soshal_db_core::repos::search_index::SearchIndexRepo;
use soshal_search_core::fts5::format_fts5_query;

/// Search result item
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SearchResult {
    pub id: String,
    pub result_type: String, // "post", "profile", "hashtag", "mention"
    pub title: String,
    pub description: String,
    pub pubkey: Option<String>,
    pub score: f32,
    pub created_at: u64,
}

fn run_search(query: &str, limit: i64, kind: Option<i64>) -> Result<Vec<SearchResult>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let fts_query = format_fts5_query(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        // `format_fts5_query` already produces a safe, prefix-matching AND
        // expression ("term* AND term*"). Feeding that through
        // `build_fts_query` again would re-split on whitespace, destroying
        // the AND/prefix semantics (and injecting a literal `AND` term).
        let fts_match = fts_query;
        let out = soshal_db_core::block_on(async {
            let mut stmt = conn
                .prepare(
                    "SELECT p.id, p.pubkey, p.content, p.kind, p.created_at FROM posts_fts f \
                     JOIN posts p ON f.rowid = p.rowid \
                     WHERE p.is_deleted = 0 AND posts_fts MATCH ?1 AND (?2 IS NULL OR p.kind = ?2) \
                     ORDER BY rank LIMIT ?3",
                )
                .await?;
            let mut rows = stmt
                .query(libsql::params![fts_match.as_str(), kind, limit])
                .await?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await? {
                out.push(SearchResult {
                    id: row.get(0)?,
                    result_type: match row.get::<i64>(3)? {
                        0 => "profile".to_string(),
                        _ => "post".to_string(),
                    },
                    title: truncate_preview(&row.get::<String>(2)?, 80),
                    description: truncate_preview(&row.get::<String>(2)?, 160),
                    pubkey: Some(row.get(1)?),
                    score: 1.0,
                    created_at: row.get::<i64>(4)?.max(0) as u64,
                });
            }
            Ok::<_, libsql::Error>(out)
        })
        .map_err(soshal_db_core::error::DbError::from)?;
        let mut out = out;
        out.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(out)
    })
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

/// Search posts by content (kind 1).
#[frb(sync, serialize)]
pub fn search_posts(query: String, limit: i32) -> Result<String, String> {
    super::util::json_ok(run_search(&query, limit.clamp(1, 100) as i64, Some(1))?)
}

/// Search profiles by name/about (kind 0).
#[frb(sync, serialize)]
pub fn search_profiles(query: String, limit: i32) -> Result<String, String> {
    super::util::json_ok(run_search(&query, limit.clamp(1, 100) as i64, Some(0))?)
}

/// Search hashtags (trending list fallback if no hash-index rows yet).
#[frb(sync, serialize)]
pub fn search_hashtags(query: String, limit: i32) -> Result<Vec<String>, String> {
    if query.trim().is_empty() {
        return search_trending_hashtags(limit);
    }
    let json = super::db::db_query_raw(format!(
        "SELECT tag FROM hashtags WHERE tag LIKE '{}%' GROUP BY tag ORDER BY SUM(count) DESC LIMIT {}",
        query.replace('\'', "''"),
        limit.clamp(1, 100)
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    Ok(rows
        .into_iter()
        .filter_map(|r| r["tag"].as_str().map(|s| s.to_string()))
        .collect())
    .into()
}

/// Search mentions (profiles matching the query prefix).
#[frb(sync, serialize)]
pub fn search_mentions(query: String, limit: i32) -> Result<String, String> {
    super::util::json_ok(run_search(&query, limit.clamp(1, 50) as i64, Some(0))?)
}

/// Global search across all indexed kinds.
#[frb(sync, serialize)]
pub fn search_global(query: String, limit: i32) -> Result<String, String> {
    super::util::json_ok(run_search(&query, limit.clamp(1, 100) as i64, None)?)
}

/// Remote NIP-50 search: query relays for matching text notes. Returns a
/// JSON array of {id, pubkey, content, created_at} for verified events.
#[frb(serialize)]
pub async fn search_remote_global(
    query: String,
    limit: u64,
    relays_json: String,
) -> Result<String, String> {
    let relays: Vec<String> =
        serde_json::from_str(&relays_json).map_err(|e| format!("invalid relays JSON: {e}"))?;
    if relays.is_empty() {
        return Err("no relay urls".to_string()).into();
    }
    if query.trim().is_empty() {
        return super::util::json_ok(Vec::<serde_json::Value>::new());
    }
    for url in &relays {
        let (valid, _blocked) = soshal_content_core::url::is_valid_relay_url(url);
        if !valid {
            return Err(format!("invalid or blocked relay URL: {url}")).into();
        }
    }
    let filter = Filter::new().search(query).limit(limit as usize);
    let client = Client::builder().build();
    let mut added = 0usize;
    for url in &relays {
        if let Ok(target) = nostr::types::RelayUrl::parse(url) {
            if client.add_relay(target).await.is_ok() {
                added += 1;
            }
        }
    }
    if added == 0 {
        return Err("no relays could be added".to_string()).into();
    }
    let _ = client.connect().await;
    let events = match client
        .fetch_events(vec![filter])
        .timeout(std::time::Duration::from_secs(8))
        .await
    {
        Ok(events) => events,
        Err(e) => return Err(format!("remote search failed: {e}")).into(),
    };
    let results: Vec<serde_json::Value> = events
        .into_iter()
        .filter(|e| e.verify().is_ok() && e.kind == Kind::TextNote)
        .map(|e| {
            serde_json::json!({
                "id": e.id.to_hex(),
                "pubkey": e.pubkey.to_hex(),
                "content": e.content,
                "created_at": e.created_at.as_secs(),
            })
        })
        .collect();
    super::util::json_ok(results)
}

/// Trending hashtags, cached briefly: the underlying GROUP BY aggregate scans
/// the whole hashtags table (tag × pubkey rows), so a 60 s TTL avoids
/// recomputing it on every search screen open.
const HASHTAGS_TTL_SECS: i64 = 60;

fn cached_trending_hashtags(limit: i64) -> Result<Vec<String>, String> {
    use std::sync::{Mutex, OnceLock};
    type HashtagCache = Option<(i64, Vec<String>)>;
    static CACHE: OnceLock<Mutex<HashtagCache>> = OnceLock::new();
    let now = soshal_common_core::format::now_secs();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((ts, tags)) = guard.as_ref() {
            if now.saturating_sub(*ts) < HASHTAGS_TTL_SECS && tags.len() >= limit as usize {
                return Ok(tags[..limit as usize].to_vec());
            }
        }
    }
    const TRENDING_HASHTAGS_SQL: &str =
        "SELECT tag FROM hashtags GROUP BY tag ORDER BY SUM(count) DESC LIMIT 100";
    let tags = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let out = soshal_db_core::block_on(async {
            let mut stmt = conn.prepare(TRENDING_HASHTAGS_SQL).await?;
            let mut rows = stmt.query(()).await?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await? {
                out.push(row.get::<String>(0)?);
            }
            Ok::<_, libsql::Error>(out)
        })
        .map_err(soshal_db_core::error::DbError::from)?;
        Ok(out)
    })?;
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((now, tags.clone()));
    Ok(tags.into_iter().take(limit as usize).collect())
}

/// Get trending hashtags from the hashtag index (cached).
#[frb(sync, serialize)]
pub fn search_trending_hashtags(limit: i32) -> Result<Vec<String>, String> {
    cached_trending_hashtags(limit.clamp(1, 100) as i64)
}

/// Get trending profiles (most followers in local graph, via the indexed
/// `follower_count` column, v012).
#[frb(sync, serialize)]
pub fn search_trending_profiles(limit: i32) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT pubkey, name, about FROM users ORDER BY follower_count DESC LIMIT {}",
        limit.clamp(1, 100)
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let profiles: Vec<SearchResult> = rows
        .into_iter()
        .filter_map(|r| {
            Some(SearchResult {
                id: r["pubkey"].as_str()?.to_string(),
                result_type: "profile".to_string(),
                title: r["name"].as_str().unwrap_or("").to_string(),
                description: truncate_preview(r["about"].as_str().unwrap_or(""), 160),
                pubkey: r["pubkey"].as_str().map(|s| s.to_string()),
                score: 1.0,
                created_at: 0,
            })
        })
        .collect();
    super::util::json_ok(profiles)
}

/// Index a post (or any kind) into the FTS search table.
#[frb(sync, serialize)]
pub fn search_index_post(
    event_id: String,
    pubkey: String,
    content: String,
    kind: i64,
) -> Result<bool, String> {
    let row = soshal_db_core::repos::search_index::SearchIndexRow {
        id: event_id,
        pubkey,
        content: truncate_preview(&content, 4096),
        kind,
        created_at: soshal_common_core::format::now_secs(),
    };
    super::db::with_db_result(|db| {
        SearchIndexRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Index many posts in one call. `rows_json` is a JSON array of
/// `{"id","pubkey","content","kind"}` objects; replaces N sequential
/// per-post index round-trips from the feed's index queue.
#[frb(sync, serialize)]
pub fn search_index_posts(rows_json: String) -> Result<bool, String> {
    #[derive(serde::Deserialize)]
    struct IndexInput {
        id: String,
        pubkey: String,
        content: String,
        kind: i64,
    }
    let rows: Vec<IndexInput> =
        serde_json::from_str(&rows_json).map_err(|e| format!("invalid rows JSON: {e}"))?;
    super::db::with_db_result(|db| {
        let repo = SearchIndexRepo::new(db);
        for r in rows {
            let row = soshal_db_core::repos::search_index::SearchIndexRow {
                id: r.id,
                pubkey: r.pubkey,
                content: truncate_preview(&r.content, 4096),
                kind: r.kind,
                created_at: soshal_common_core::format::now_secs(),
            };
            repo.upsert(&row)?;
        }
        Ok(true)
    })
}

/// Index a profile into the FTS search table (kind 0 row).
#[frb(sync, serialize)]
pub fn search_index_profile(pubkey: String, name: String, about: String) -> Result<bool, String> {
    let content = if name.is_empty() {
        about
    } else if about.is_empty() {
        name
    } else {
        format!("{name} {about}")
    };
    search_index_post(format!("profile:{pubkey}"), pubkey, content, 0)
}

/// Remove an entry from the search index.
#[frb(sync, serialize)]
pub fn search_remove_indexed(id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        SearchIndexRepo::new(db).delete(&id)?;
        Ok(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;

    fn tmp_db(label: &str) -> String {
        db::tmp_db(label, "search")
    }

    fn insert_user(pubkey: &str, name: &str) {
        db::db_execute_raw(format!(
            "INSERT INTO users (pubkey, npub, name) VALUES ('{pubkey}','npub1{pubkey}','{name}') ON CONFLICT DO UPDATE SET name='{name}'"
        ))
        .unwrap();
    }

    fn insert_post(id: &str, pubkey: &str, content: &str, kind: i64, created_at: i64) {
        insert_user(pubkey, "tester");
        db::db_execute_raw(format!(
            "INSERT INTO posts (id, pubkey, content, kind, created_at) VALUES ('{id}','{pubkey}','{content}',{kind},{created_at})"
        ))
        .unwrap();
    }

    fn parse_arr(json: &str) -> Vec<serde_json::Value> {
        serde_json::from_str::<serde_json::Value>(json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn test_search_posts_happy() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("posts");
        insert_post("p1", "pk1", "hello caveman world", 1, 1000);
        insert_post("p2", "pk1", "unrelated chatter", 1, 2000);
        let arr = parse_arr(&search_posts("caveman".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["id"], "p1");
        assert_eq!(arr[0]["result_type"], "post");
        assert_eq!(arr[0]["pubkey"], "pk1");
        assert!(arr[0]["title"].as_str().unwrap().contains("caveman"));
    }

    #[test]
    fn test_search_posts_newest_first_and_limit_clamp() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("order");
        insert_post("p1", "pk1", "soshal alpha", 1, 1000);
        insert_post("p2", "pk1", "soshal beta", 1, 2000);
        let arr = parse_arr(&search_posts("soshal".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "p2", "json: {arr:?}");
        let arr = parse_arr(&search_posts("soshal".to_string(), 0).unwrap());
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "p1");
    }

    #[test]
    fn test_search_profiles_filters_kind() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("profiles");
        insert_post("prof1", "pk1", "alice soshal builder", 0, 1000);
        insert_post("post1", "pk1", "soshal post text", 1, 2000);
        let arr = parse_arr(&search_profiles("soshal".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["id"], "prof1");
        assert_eq!(arr[0]["result_type"], "profile");
        let arr = parse_arr(&search_posts("soshal".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "post1");
    }

    #[test]
    fn test_search_global_matches_both_kinds() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("global");
        insert_post("prof1", "pk1", "soshal alice", 0, 1000);
        insert_post("post1", "pk1", "soshal post", 1, 2000);
        let arr = parse_arr(&search_global("soshal".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        assert!(arr.iter().any(|r| r["result_type"] == "post"));
        assert!(arr.iter().any(|r| r["result_type"] == "profile"));
    }

    #[test]
    fn test_search_empty_query_returns_empty() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("empty");
        insert_post("p1", "pk1", "soshal content", 1, 1000);
        assert!(parse_arr(&search_posts("   ".to_string(), 10).unwrap()).is_empty());
        assert!(parse_arr(&search_posts(String::new(), 10).unwrap()).is_empty());
        assert!(parse_arr(&search_global(String::new(), 10).unwrap()).is_empty());
    }

    #[test]
    fn test_search_no_match_returns_empty() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("miss");
        insert_post("p1", "pk1", "soshal content", 1, 1000);
        assert!(parse_arr(&search_posts("zzzmissing".to_string(), 10).unwrap()).is_empty());
        assert!(parse_arr(&search_global("zzzmissing".to_string(), 10).unwrap()).is_empty());
    }

    #[test]
    fn test_search_mentions_matches_profile() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("mentions");
        insert_post("prof1", "pk1", "alice soshal builder", 0, 1000);
        let arr = parse_arr(&search_mentions("alice".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["id"], "prof1");
        assert_eq!(arr[0]["result_type"], "profile");
    }

    #[test]
    fn test_search_hashtags_prefix_ordered() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("hashtags");
        db::db_execute_raw(
            "INSERT INTO hashtags (tag, pubkey, last_used_at, count) VALUES \
             ('soshal','pk1',100,3),('soshal','pk2',200,5),('rust','pk1',300,1)"
                .to_string(),
        )
        .unwrap();
        let arr = search_hashtags("so".to_string(), 10).unwrap();
        assert_eq!(arr, vec!["soshal".to_string()]);
    }

    #[test]
    fn test_hashtags_empty_falls_back_to_trending() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("trending");
        db::db_execute_raw(
            "INSERT INTO hashtags (tag, pubkey, last_used_at, count) VALUES \
             ('soshal','pk1',100,3),('soshal','pk2',200,5),('rust','pk1',300,1)"
                .to_string(),
        )
        .unwrap();
        let arr = search_trending_hashtags(10).unwrap();
        assert_eq!(arr, vec!["soshal".to_string(), "rust".to_string()]);
        let arr = search_hashtags(String::new(), 10).unwrap();
        assert_eq!(arr, vec!["soshal".to_string(), "rust".to_string()]);
    }

    #[test]
    fn test_search_trending_profiles_order() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("trendprof");
        db::db_execute_raw(
            "INSERT INTO users (pubkey, npub, name, contact_pubkeys) VALUES \
             ('pk1','npub1pk1','carl','[\"a\",\"b\",\"c\"]'),('pk2','npub1pk2','amy','[]')"
                .to_string(),
        )
        .unwrap();
        let arr = parse_arr(&search_trending_profiles(10).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        assert_eq!(arr[0]["id"], "pk1");
        assert_eq!(arr[0]["result_type"], "profile");
        assert_eq!(arr[0]["title"], "carl");
    }

    #[test]
    fn test_index_and_remove_roundtrip() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("index");
        insert_post("p1", "pk1", "seed body", 1, 1000);
        assert!(search_index_post(
            "p1".to_string(),
            "pk1".to_string(),
            "indexed content".to_string(),
            1
        )
        .unwrap());
        assert!(
            search_index_profile("pk9".to_string(), "carol".to_string(), String::new()).unwrap()
        );
        assert_eq!(
            parse_arr(&search_posts("indexed".to_string(), 10).unwrap()).len(),
            1
        );
        assert!(search_remove_indexed("p1".to_string()).unwrap());
        assert!(parse_arr(&search_posts("indexed".to_string(), 10).unwrap()).is_empty());
    }

    #[test]
    fn test_search_multi_term_keeps_and_and_prefix() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("multi");
        insert_post("p1", "pk1", "caveman world order", 1, 1000);
        insert_post("p2", "pk1", "caveman philosophy", 1, 2000);
        insert_post("p3", "pk1", "world wide web", 1, 3000);
        let arr = parse_arr(&search_posts("caveman world".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["id"], "p1", "both terms ANDed, prefix-matched");
        let arr = parse_arr(&search_posts("cave wor".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["id"], "p1", "prefix terms still ANDed");
    }

    #[test]
    fn test_search_errors_when_db_not_initialized() {
        if db::db_path().is_err() {
            assert!(search_posts("x".to_string(), 10)
                .unwrap_err()
                .contains("not initialized"));
        }
    }

    #[test]
    fn test_search_query_and_hashtag_edges() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("qed");
        insert_post("p1", "pk1", "soshal content", 1, 1000);
        // Punctuation-only query: FTS sanitizer strips all terms -> empty.
        assert!(parse_arr(&search_posts("!!!".to_string(), 10).unwrap()).is_empty());
        assert!(parse_arr(&search_global("!@#$%^".to_string(), 10).unwrap()).is_empty());
        // Quote escaping + limit clamp.
        db::db_execute_raw(
            "INSERT INTO hashtags (tag, pubkey, last_used_at, count) VALUES \
             ('sos''hal','pk1',100,5),('soshal','pk2',200,3),('rust','pk3',300,1)"
                .to_string(),
        )
        .unwrap();
        assert_eq!(
            search_hashtags("sos'hal".to_string(), 10).unwrap(),
            vec!["sos'hal".to_string()]
        );
        assert_eq!(search_hashtags("sos".to_string(), 0).unwrap().len(), 1);
        assert_eq!(search_hashtags("sos".to_string(), 200).unwrap().len(), 2);
        // Cached trending: limit 3 forces recompute (cache holds <=2 tags),
        // second call hits the 60s TTL after the rows are gone.
        assert_eq!(
            search_trending_hashtags(3).unwrap(),
            vec![
                "sos'hal".to_string(),
                "soshal".to_string(),
                "rust".to_string()
            ]
        );
        db::db_execute_raw("DELETE FROM hashtags".to_string()).unwrap();
        assert_eq!(
            search_trending_hashtags(2).unwrap(),
            vec!["sos'hal".to_string(), "soshal".to_string()]
        );
    }

    #[test]
    fn test_search_index_and_profile_edges() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("idxedge");
        // Long post content truncated to 4096 + ellipsis.
        let long = format!("needle{}", "x".repeat(5000));
        assert!(search_index_post("long1".to_string(), "pk1".to_string(), long, 1).unwrap());
        let json = db::db_query_raw("SELECT content FROM posts_fts WHERE id = 'long1'".to_string())
            .unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        let content = rows[0]["content"].as_str().unwrap();
        assert_eq!(content.chars().count(), 4097);
        assert!(content.ends_with('…'));
        // Profile index: empty name -> about only; both -> concat.
        assert!(
            search_index_profile("pkA".to_string(), String::new(), "about text".to_string())
                .unwrap()
        );
        assert!(search_index_profile(
            "pkB".to_string(),
            "name".to_string(),
            "about text".to_string()
        )
        .unwrap());
        let json =
            db::db_query_raw("SELECT content FROM posts_fts WHERE id = 'profile:pkA'".to_string())
                .unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(rows[0]["content"], "about text");
        let json =
            db::db_query_raw("SELECT content FROM posts_fts WHERE id = 'profile:pkB'".to_string())
                .unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(rows[0]["content"], "name about text");
        // Batch index: invalid JSON errors; multi-row loop indexes both.
        let err = search_index_posts("not json".to_string()).unwrap_err();
        assert!(err.starts_with("invalid rows JSON"), "err: {err}");
        insert_post("a1", "pk1", "seed", 1, 1000);
        insert_post("b1", "pk2", "seed", 1, 2000);
        let batch = r#"[{"id":"a1","pubkey":"pk1","content":"alpha beta","kind":1},{"id":"b1","pubkey":"pk2","content":"gamma delta","kind":1}]"#;
        assert!(search_index_posts(batch.to_string()).unwrap());
        let arr = parse_arr(&search_posts("alpha".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["id"], "a1");
        assert_eq!(
            parse_arr(&search_posts("delta".to_string(), 10).unwrap()).len(),
            1
        );
        // Removing a nonexistent index entry is Ok.
        assert!(search_remove_indexed("ghost".to_string()).unwrap());
        // Trending profile about truncated at 160.
        let about = "x".repeat(200);
        db::db_execute_raw(format!(
            "INSERT INTO users (pubkey, npub, name, about, contact_pubkeys) VALUES \
             ('pk9','npub1pk9','alice','{about}','[\"a\",\"b\",\"c\"]')"
        ))
        .unwrap();
        let arr = parse_arr(&search_trending_profiles(10).unwrap());
        assert_eq!(arr[0]["id"], "pk9", "json: {arr:?}");
        assert_eq!(arr[0]["title"], "alice");
        let desc = arr[0]["description"].as_str().unwrap();
        assert_eq!(desc.chars().count(), 161);
        assert!(desc.ends_with('…'));
        // truncate_preview: >80 chars -> 80 + ellipsis.
        let t = truncate_preview(&"a".repeat(81), 80);
        assert_eq!(t.chars().count(), 81);
        assert!(t.ends_with('…'));
        assert_eq!(truncate_preview(&"a".repeat(80), 80), "a".repeat(80));
        assert_eq!(truncate_preview("", 80), "");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_remote_global_error_paths() {
        let err = search_remote_global("q".to_string(), 10, "not-json".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("invalid relays JSON"), "err: {err}");
        let err = search_remote_global("q".to_string(), 10, "[]".to_string())
            .await
            .unwrap_err();
        assert_eq!(err, "no relay urls");
        let json = search_remote_global(
            String::new(),
            10,
            r#"["wss://relay.example.com"]"#.to_string(),
        )
        .await
        .unwrap();
        assert!(parse_arr(&json).is_empty());
        let err = search_remote_global(
            "q".to_string(),
            10,
            r#"["wss://localhost:8000"]"#.to_string(),
        )
        .await
        .unwrap_err();
        assert_eq!(err, "invalid or blocked relay URL: wss://localhost:8000");
    }
}
