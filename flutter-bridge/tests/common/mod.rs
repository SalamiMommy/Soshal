#![allow(dead_code)]

use std::sync::MutexGuard;

pub fn init_db(label: &str, name: &str) -> String {
    let path = soshal_test_util::tmp_path(label, name)
        .to_string_lossy()
        .to_string();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
    assert!(soshal_flutter_bridge::db::db_init(path.clone()).is_ok());
    path
}

pub fn unique_pubkey(tag: &str) -> String {
    let kp: soshal_flutter_bridge::KeyPairResult =
        serde_json::from_str(&soshal_flutter_bridge::auth::auth_generate_keypair().unwrap())
            .unwrap();
    format!("{}_{}", tag, &kp.public_key[..12])
}

pub fn lock() -> MutexGuard<'static, ()> {
    soshal_test_util::test_lock()
}

pub fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
}
