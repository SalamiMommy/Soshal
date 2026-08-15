//! Analytics FFI module
//! Engagement stats, post analytics

use flutter_rust_bridge::frb;

#[frb(sync, serialize)]
pub fn analytics_compute_stats() -> Result<String, String> {
    let my_pubkey = super::signer::signer_pubkey().unwrap_or_default();
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let total_posts: i64 = soshal_db_core::query::query_first(
            &conn,
            "SELECT COUNT(*) FROM posts WHERE pubkey = ?1 AND kind = 1 AND is_deleted = 0",
            [my_pubkey.as_str()],
            |r| r.get(0),
        )?
        .unwrap_or(0);
        let total_reactions: i64 = soshal_db_core::query::query_first(
            &conn,
            "SELECT COUNT(*) FROM reactions r JOIN posts p ON p.id = r.event_id \
             WHERE p.pubkey = ?1 AND p.kind = 1 AND p.is_deleted = 0",
            [my_pubkey.as_str()],
            |r| r.get(0),
        )?
        .unwrap_or(0);
        let total_zap_msat: i64 = soshal_db_core::query::query_first(
            &conn,
            "SELECT COALESCE(SUM(amount_msat), 0) FROM zaps WHERE recipient_pubkey = ?1",
            [my_pubkey.as_str()],
            |r| r.get(0),
        )?
        .unwrap_or(0);
        Ok(serde_json::json!({
            "totalPosts": total_posts,
            "totalReactions": total_reactions,
            "totalZapMsat": total_zap_msat,
        })
        .to_string())
    })
}

/// Generate dense vector embedding using hardware-accelerated Burn SLM engine.
#[frb(sync, serialize)]
pub fn analytics_slm_generate_embedding(text: String) -> Result<String, String> {
    let engine = soshal_analytics_core::slm::BurnSlmEngine::new(
        soshal_analytics_core::slm::SlmBackend::WebGpu,
    );
    let embedding = engine.generate_embedding(&text)?;
    serde_json::to_string(&embedding)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

/// Classify post text for sentiment & automated spam detection locally via Burn SLM.
#[frb(sync, serialize)]
pub fn analytics_slm_classify_post(text: String) -> Result<String, String> {
    let engine = soshal_analytics_core::slm::BurnSlmEngine::new(
        soshal_analytics_core::slm::SlmBackend::WebGpu,
    );
    let classification = engine.classify_post(&text)?;
    serde_json::to_string(&classification)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use soshal_analytics_core::slm::{SlmClassification, SlmEmbedding};

    #[test]
    fn test_slm_embedding_fixed_dim() {
        let json = analytics_slm_generate_embedding("Soshal P2P mesh network".to_string()).unwrap();
        let embedding: SlmEmbedding = serde_json::from_str(&json).unwrap();
        assert_eq!(embedding.vector.len(), 384);
        assert_eq!(embedding.dimension, 384);
    }

    #[test]
    fn test_slm_embedding_deterministic() {
        let a = analytics_slm_generate_embedding("Soshal P2P mesh network".to_string()).unwrap();
        let b = analytics_slm_generate_embedding("Soshal P2P mesh network".to_string()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn test_slm_classify_spam_vs_clean() {
        let spam: SlmClassification = serde_json::from_str(
            &analytics_slm_classify_post("claim free crypto now!!!".to_string()).unwrap(),
        )
        .unwrap();
        assert!(spam.is_spam);
        assert_eq!(spam.label, "spam");

        let clean: SlmClassification = serde_json::from_str(
            &analytics_slm_classify_post("this is a great post about hiking".to_string()).unwrap(),
        )
        .unwrap();
        assert!(!clean.is_spam);
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
