use soshal_crypto_core::nip44::{clear_conversation_key_cache, derive_conversation_key};
use soshal_crypto_core::pqc_ratchet::ratchet_crypto::{derive_chain, derive_msg_key_bytes};

#[test]
fn conversation_key_derivation_cached_and_stable() {
    let key = [7u8; 32];
    let first = derive_conversation_key(&key);
    let second = derive_conversation_key(&key);
    assert_eq!(first, second, "cached derivation is stable");
    let other = derive_conversation_key(&[8u8; 32]);
    assert_ne!(
        first, other,
        "distinct keys derive distinct conversation keys"
    );
    clear_conversation_key_cache();
    let after_clear = derive_conversation_key(&key);
    assert_eq!(
        after_clear, first,
        "clear must not change the derived value"
    );
    clear_conversation_key_cache();
    clear_conversation_key_cache();
}

#[test]
fn derive_chain_valid_and_invalid_root() {
    let chain = derive_chain(&"ab".repeat(32), "session-1").unwrap();
    assert_eq!(chain.len(), 32);
    let same = derive_chain(&"ab".repeat(32), "session-1").unwrap();
    assert_eq!(chain, same, "deterministic");
    let other_ctx = derive_chain(&"ab".repeat(32), "session-2").unwrap();
    assert_ne!(chain, other_ctx, "context separates chains");
    let e = derive_chain("not-hex", "ctx").unwrap_err();
    assert_eq!(e, "bad root hex");
}

#[test]
fn derive_msg_key_bytes_advances_chain() {
    let chain_key = [3u8; 32];
    let (msg_key, next_chain) = derive_msg_key_bytes(&chain_key, "ctx").unwrap();
    assert_eq!(msg_key.len(), 32);
    assert_eq!(next_chain.len(), 32);
    let (again, next2) = derive_msg_key_bytes(&chain_key, "ctx").unwrap();
    assert_eq!(again, msg_key, "same chain key yields same msg key");
    assert_eq!(next2, next_chain);
    let (diff_msg, diff_next) = derive_msg_key_bytes(&chain_key, "other-ctx").unwrap();
    assert_ne!(diff_msg, msg_key, "context changes the message key");
    assert_ne!(diff_next, next_chain);
    let (_, advanced) = derive_msg_key_bytes(&next_chain, "ctx").unwrap();
    assert_ne!(advanced, next_chain, "ratchet advances the chain");
}
