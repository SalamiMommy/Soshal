//! Strict RFC 5322 identifier validation for NIP-05 and Lightning Addresses (LUD-16).

use email_address::EmailAddress;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// Validation error for Internet identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentifierError {
    InvalidSyntax(String),
    InvalidDomain(String),
    LocalPartTooLong,
    DomainTooLong,
    MissingTld,
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentifierError::InvalidSyntax(s) => write!(f, "Invalid email-like syntax: {s}"),
            IdentifierError::InvalidDomain(s) => {
                write!(f, "Missing or invalid domain component: {s}")
            }
            IdentifierError::LocalPartTooLong => {
                write!(f, "Local part cannot exceed 64 characters")
            }
            IdentifierError::DomainTooLong => write!(f, "Domain cannot exceed 255 characters"),
            IdentifierError::MissingTld => {
                write!(f, "Domain must contain at least one dot and a valid TLD")
            }
        }
    }
}

impl std::error::Error for IdentifierError {}

/// NIP-05 human-readable identifier (e.g. `alice@example.com` or `_@example.com`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Nip05Identifier {
    email: EmailAddress,
}

impl Nip05Identifier {
    /// Parse and validate a NIP-05 identifier string.
    pub fn parse(s: &str) -> Result<Self, IdentifierError> {
        let trimmed = s.trim();

        // NIP-05 local part allows `_` as a root identifier (`_@domain.com`)
        let email = EmailAddress::from_str(trimmed)
            .map_err(|e| IdentifierError::InvalidSyntax(e.to_string()))?;

        let local = email.local_part();
        let domain = email.domain();

        if local.len() > 64 {
            return Err(IdentifierError::LocalPartTooLong);
        }
        if domain.len() > 255 {
            return Err(IdentifierError::DomainTooLong);
        }
        if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
            return Err(IdentifierError::MissingTld);
        }

        Ok(Self { email })
    }

    /// The name/local part (e.g. `alice` in `alice@example.com`).
    pub fn name(&self) -> &str {
        self.email.local_part()
    }

    /// The domain part (e.g. `example.com` in `alice@example.com`).
    pub fn domain(&self) -> &str {
        self.email.domain()
    }

    /// Full canonical identifier string.
    pub fn as_str(&self) -> &str {
        self.email.as_str()
    }

    /// Standard NIP-05 query URL: `https://<domain>/.well-known/nostr.json?name=<name>`.
    pub fn well_known_url(&self) -> String {
        format!(
            "https://{}/.well-known/nostr.json?name={}",
            self.domain(),
            self.name()
        )
    }
}

impl fmt::Display for Nip05Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.email)
    }
}

impl FromStr for Nip05Identifier {
    type Err = IdentifierError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for Nip05Identifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Nip05Identifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Lightning Address (LUD-16) (e.g. `satoshi@fountain.fm`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LightningAddress {
    email: EmailAddress,
}

impl LightningAddress {
    /// Parse and validate a Lightning Address string.
    pub fn parse(s: &str) -> Result<Self, IdentifierError> {
        let trimmed = s.trim();
        let email = EmailAddress::from_str(trimmed)
            .map_err(|e| IdentifierError::InvalidSyntax(e.to_string()))?;

        let local = email.local_part();
        let domain = email.domain();

        if local.is_empty() || local.len() > 64 {
            return Err(IdentifierError::LocalPartTooLong);
        }
        if domain.len() > 255 {
            return Err(IdentifierError::DomainTooLong);
        }
        if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
            return Err(IdentifierError::MissingTld);
        }

        Ok(Self { email })
    }

    /// Username part of the Lightning Address.
    pub fn username(&self) -> &str {
        self.email.local_part()
    }

    /// Domain part of the Lightning Address.
    pub fn domain(&self) -> &str {
        self.email.domain()
    }

    /// Full canonical identifier string.
    pub fn as_str(&self) -> &str {
        self.email.as_str()
    }

    /// LNURL-pay endpoint: `https://<domain>/.well-known/lnurlp/<username>`.
    pub fn lnurlp_endpoint(&self) -> String {
        format!(
            "https://{}/.well-known/lnurlp/{}",
            self.domain(),
            self.username()
        )
    }
}

impl fmt::Display for LightningAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.email)
    }
}

impl FromStr for LightningAddress {
    type Err = IdentifierError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for LightningAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LightningAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_nip05() {
        let id = Nip05Identifier::parse("bob@example.com").expect("valid");
        assert_eq!(id.name(), "bob");
        assert_eq!(id.domain(), "example.com");
        assert_eq!(id.as_str(), "bob@example.com");
        assert_eq!(
            id.well_known_url(),
            "https://example.com/.well-known/nostr.json?name=bob"
        );

        let root_id = Nip05Identifier::parse("_@domain.org").expect("root valid");
        assert_eq!(root_id.name(), "_");
        assert_eq!(root_id.domain(), "domain.org");
    }

    #[test]
    fn test_invalid_nip05() {
        assert!(Nip05Identifier::parse("notanemail").is_err());
        assert!(Nip05Identifier::parse("user@").is_err());
        assert!(Nip05Identifier::parse("@domain.com").is_err());
        assert!(Nip05Identifier::parse("user@localhost").is_err()); // No dot
        assert!(Nip05Identifier::parse("user@.domain.com").is_err());
    }

    #[test]
    fn test_valid_lightning_address() {
        let lud16 = LightningAddress::parse("satoshi@fountain.fm").expect("valid");
        assert_eq!(lud16.username(), "satoshi");
        assert_eq!(lud16.domain(), "fountain.fm");
        assert_eq!(
            lud16.lnurlp_endpoint(),
            "https://fountain.fm/.well-known/lnurlp/satoshi"
        );
    }

    #[test]
    fn test_serde_roundtrip() {
        let id = Nip05Identifier::parse("alice@nostr.net").expect("valid");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, "\"alice@nostr.net\"");

        let deserialized: Nip05Identifier = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized, id);
    }
}
