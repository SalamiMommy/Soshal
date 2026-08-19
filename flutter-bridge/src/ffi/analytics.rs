//! Analytics FFI module
//! Engagement stats, post analytics

use flutter_rust_bridge::frb;
use std::sync::OnceLock;

#[frb(sync, serialize)]
pub fn analytics_compute_stats() -> Result<String, String> {
    let my_pubkey = super::signer::signer_pubkey().unwrap_or_default();
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let rows: Vec<(String, i64)> = soshal_db_core::query::query(
            &conn,
            "SELECT p.pubkey, COUNT(r.event_id) FROM posts p \
             LEFT JOIN reactions r ON r.event_id = p.id \
             WHERE p.pubkey = ?1 AND p.kind = 1 AND p.is_deleted = 0 GROUP BY p.id",
            [my_pubkey.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let posts: Vec<soshal_analytics_core::PostInput> = rows
            .into_iter()
            .map(|(pubkey, likes)| soshal_analytics_core::PostInput {
                pubkey,
                local_stats: Some(soshal_analytics_core::LocalStats {
                    likes_count: Some(likes as u64),
                    reposts_count: None,
                }),
            })
            .collect();
        let out = soshal_analytics_core::posts::compute_analytics_posts(&posts, &my_pubkey);
        let total_zap_msat: i64 = soshal_db_core::query::query_first(
            &conn,
            "SELECT COALESCE(SUM(amount_msat), 0) FROM zaps WHERE recipient_pubkey = ?1",
            [my_pubkey.as_str()],
            |r| r.get(0),
        )?
        .unwrap_or(0);
        Ok(serde_json::json!({
            "totalPosts": out.total_posts,
            "totalReactions": out.total_reactions,
            "totalZapMsat": total_zap_msat,
        })
        .to_string())
    })
}

static ENGINE: OnceLock<soshal_analytics_core::slm::BurnSlmEngine> = OnceLock::new();

/// Generate dense vector embedding using hardware-accelerated Burn SLM engine.
#[frb(serialize)]
pub async fn analytics_slm_generate_embedding(text: String) -> Result<String, String> {
    let engine = ENGINE.get_or_init(|| {
        soshal_analytics_core::slm::BurnSlmEngine::new(
            soshal_analytics_core::slm::SlmBackend::WebGpu,
        )
    });
    let embedding = engine.generate_embedding(&text)?;
    serde_json::to_string(&embedding)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

/// Classify post text for sentiment & automated spam detection locally via Burn SLM.
#[frb(serialize)]
pub async fn analytics_slm_classify_post(text: String) -> Result<String, String> {
    let engine = ENGINE.get_or_init(|| {
        soshal_analytics_core::slm::BurnSlmEngine::new(
            soshal_analytics_core::slm::SlmBackend::WebGpu,
        )
    });
    let classification = engine.classify_post(&text)?;
    serde_json::to_string(&classification)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_slm_embedding_unavailable() {
        let err = analytics_slm_generate_embedding("Soshal P2P mesh network".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("unavailable"), "{err}");
    }

    #[tokio::test]
    async fn test_slm_embedding_unavailable_message() {
        let err = analytics_slm_generate_embedding("Soshal P2P mesh network".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("(roadmap)"), "{err}");
    }

    #[tokio::test]
    async fn test_slm_classify_unavailable() {
        let err = analytics_slm_classify_post("claim free crypto now!!!".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("unavailable"), "{err}");
    }

    #[test]
    fn test_compute_stats_ok() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = format!(
            "{}/soshal_analytics_{}_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id(),
            "stats"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(super::super::db::db_init(path.clone()).is_ok());
        let out = analytics_compute_stats().unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["totalPosts"], 0);
        assert_eq!(v["totalReactions"], 0);
        assert_eq!(v["totalZapMsat"], 0);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }
}
