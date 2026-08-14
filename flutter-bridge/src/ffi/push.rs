//! Push FFI module
//! Push token management

use flutter_rust_bridge::frb;

#[frb(sync, serialize)]
pub fn push_register_token(_token: String) -> Result<bool, String> {
    Ok(true).into()
}
