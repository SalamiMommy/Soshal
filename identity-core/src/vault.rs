//! Secret vault service interface for platform secure key storage.

/// Keyring vault service trait for managing identity keys securely.
pub trait KeyringVaultService {
    /// Save a secret string (e.g., nsec) under the specified key name.
    fn save_secret(&self, key_name: &str, secret: &str) -> Result<(), String>;

    /// Retrieve a secret string by key name.
    fn get_secret(&self, key_name: &str) -> Result<Option<String>, String>;

    /// Delete a secret entry by key name.
    fn delete_secret(&self, key_name: &str) -> Result<(), String>;
}
