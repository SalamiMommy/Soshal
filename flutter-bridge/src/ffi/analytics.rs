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
