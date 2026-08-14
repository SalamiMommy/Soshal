//! Social FFI module
//! Compatibility, discovery

use flutter_rust_bridge::frb;

#[frb(sync, serialize)]
pub fn social_friend_suggestions() -> Result<Vec<String>, String> {
    Ok(vec![]).into()
}
