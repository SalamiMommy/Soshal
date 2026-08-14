//! Relations FFI module
//! Friends, vouches, reviews

use flutter_rust_bridge::frb;

/// Send a friend request by following the target (signed kind-3 contact
/// list). There is no standard NIP friend-request event; follow is the
/// correct wire format.
#[frb(sync, serialize)]
pub fn relations_send_friend_request(pubkey: String) -> Result<bool, String> {
    super::identity::identity_follow_user(pubkey)?;
    Ok(true).into()
}
