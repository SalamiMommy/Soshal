//! Analytics FFI module
//! Engagement stats, post analytics

use flutter_rust_bridge::frb;
use std::sync::OnceLock;

#[derive(serde::Serialize)]
struct AnalyticsStatsDto {
    #[serde(rename = "totalPosts")]
    total_posts: i64,
    #[serde(rename = "totalReactions")]
    total_reactions: i64,
    #[serde(rename = "totalZapMsat")]
    total_zap_msat: i64,
}

#[frb(serialize)]
pub async fn analytics_compute_stats() -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let my_pubkey = super::signer::signer_pubkey().unwrap_or_default();
        super::db::with_db_result(|db| {
            let conn = db.conn()?;
            let (total_posts, total_reactions): (i64, i64) = soshal_db_core::query::query_first(
                &conn,
                "SELECT \
                 (SELECT COUNT(*) FROM posts WHERE pubkey = ?1 AND kind = 1 AND is_deleted = 0), \
                 (SELECT COUNT(*) FROM reactions r JOIN posts p ON r.event_id = p.id WHERE p.pubkey = ?1 AND p.kind = 1 AND p.is_deleted = 0)",
                [my_pubkey.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?
            .unwrap_or((0, 0));
            let total_zap_msat: i64 = soshal_db_core::query::query_first(
                &conn,
                "SELECT COALESCE(SUM(amount_msat), 0) FROM zaps WHERE recipient_pubkey = ?1",
                [my_pubkey.as_str()],
                |r| r.get(0),
            )?
            .unwrap_or(0);
            let json = serde_json::to_string(&AnalyticsStatsDto {
                total_posts,
                total_reactions,
                total_zap_msat,
            })
            .unwrap_or_else(|_| "{}".to_string());
            Ok(json)
        })
    })
    .await
    .map_err(|e| format!("analytics stats join: {e}"))?
}

static ENGINE: OnceLock<soshal_analytics_core::slm::BurnSlmEngine> = OnceLock::new();

/// Get-or-init the Burn SLM engine inside a panic guard. Init has no in-band
/// Result; a panicking GPU backend init must not cross the frb async boundary
/// (release profile aborts, killing the whole process).
fn engine() -> Result<&'static soshal_analytics_core::slm::BurnSlmEngine, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ENGINE.get_or_init(|| {
            soshal_analytics_core::slm::BurnSlmEngine::new(
                soshal_analytics_core::slm::SlmBackend::WebGpu,
            )
        })
    }))
    .map_err(|_| "SLM engine init panicked (hardware backend unavailable)".to_string())
}

/// Generate dense vector embedding using hardware-accelerated Burn SLM engine.
#[frb(serialize)]
pub async fn analytics_slm_generate_embedding(text: String) -> Result<String, String> {
    let engine = engine()?;
    let embedding = engine.generate_embedding(&text)?;
    serde_json::to_string(&embedding)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

/// Classify post text for sentiment & automated spam detection locally via Burn SLM.
#[frb(serialize)]
pub async fn analytics_slm_classify_post(text: String) -> Result<String, String> {
    let engine = engine()?;
    let classification = engine.classify_post(&text)?;
    serde_json::to_string(&classification)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_slm_embedding_unavailable() {
        let err = analytics_slm_generate_embedding("Soshal P2P mesh network".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("unavailable"), "{err}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_slm_embedding_unavailable_message() {
        let err = analytics_slm_generate_embedding("Soshal P2P mesh network".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("(roadmap)"), "{err}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_slm_classify_unavailable() {
        let err = analytics_slm_classify_post("claim free crypto now!!!".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("unavailable"), "{err}");
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_compute_stats_ok() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
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
        let out = analytics_compute_stats().await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["totalPosts"], 0);
        assert_eq!(v["totalReactions"], 0);
        assert_eq!(v["totalZapMsat"], 0);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }
}
