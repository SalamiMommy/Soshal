//! Relations FFI module
//! Friends, vouches, reviews

use flutter_rust_bridge::frb;

#[frb(sync, serialize)]
pub fn relations_send_friend_request(_pubkey: String) -> Result<bool, String> {
    Ok(true).into()
}
