//! Analytics FFI module
//! Engagement stats, post analytics

use flutter_rust_bridge::frb;

#[frb(sync, serialize)]
pub fn analytics_compute_stats() -> Result<String, String> {
    Ok("stats".to_string()).into()
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
        assert_eq!(analytics_compute_stats().unwrap(), "stats");
    }
}
