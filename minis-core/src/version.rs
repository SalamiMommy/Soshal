//! Semantic versioning and capability negotiation for Mini-Apps and plugins.
//!
//! Provides strict SemVer 2.0.0 parsing, display, comparison, and requirement
//! evaluation using `semver`.

use semver::{Version, VersionReq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A parsed Semantic Version (Major.Minor.Patch-Prerelease+Build).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginVersion(pub Version);

impl PluginVersion {
    /// Parse a semantic version string (e.g. "1.2.3", "0.4.0-beta.1").
    pub fn parse(text: &str) -> Result<Self, String> {
        Version::parse(text.trim())
            .map(Self)
            .map_err(|e| format!("invalid semver '{text}': {e}"))
    }

    /// Major version number (breaking API changes).
    pub fn major(&self) -> u64 {
        self.0.major
    }

    /// Minor version number (backwards-compatible features).
    pub fn minor(&self) -> u64 {
        self.0.minor
    }

    /// Patch version number (backwards-compatible bug fixes).
    pub fn patch(&self) -> u64 {
        self.0.patch
    }

    /// Returns `true` if this version is a pre-release (e.g. "-alpha", "-rc.1").
    pub fn is_prerelease(&self) -> bool {
        !self.0.pre.is_empty()
    }

    /// Borrow the underlying [`semver::Version`].
    pub fn as_version(&self) -> &Version {
        &self.0
    }
}

impl fmt::Display for PluginVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for PluginVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PluginVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// A Semantic Version requirement expression (e.g. `^1.2.0`, `>=0.4.0, <1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRequirement(pub VersionReq);

impl VersionRequirement {
    /// Parse a SemVer requirement string.
    pub fn parse(expr: &str) -> Result<Self, String> {
        VersionReq::parse(expr.trim())
            .map(Self)
            .map_err(|e| format!("invalid version requirement '{expr}': {e}"))
    }

    /// Check if a given [`PluginVersion`] satisfies this requirement.
    pub fn matches(&self, version: &PluginVersion) -> bool {
        self.0.matches(&version.0)
    }
}

impl fmt::Display for VersionRequirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for VersionRequirement {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for VersionRequirement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Check if a plugin version string satisfies a required SemVer expression.
pub fn is_version_compatible(plugin_version: &str, required_range: &str) -> Result<bool, String> {
    let ver = PluginVersion::parse(plugin_version)?;
    let req = VersionRequirement::parse(required_range)?;
    Ok(req.matches(&ver))
}

/// Check if the host runtime engine satisfies a plugin's minimum (and optional maximum) version bounds.
pub fn check_host_compatibility(
    min_engine_version: &str,
    max_engine_version: Option<&str>,
    host_version: &str,
) -> Result<bool, String> {
    let host = PluginVersion::parse(host_version)?;
    let min = PluginVersion::parse(min_engine_version)?;
    if host < min {
        return Ok(false);
    }
    if let Some(max_str) = max_engine_version {
        let max = PluginVersion::parse(max_str)?;
        if host > max {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parsing_and_components() {
        let v = PluginVersion::parse("2.4.1-beta.2").unwrap();
        assert_eq!(v.major(), 2);
        assert_eq!(v.minor(), 4);
        assert_eq!(v.patch(), 1);
        assert!(v.is_prerelease());
        assert_eq!(v.to_string(), "2.4.1-beta.2");

        let v_release = PluginVersion::parse("1.0.0").unwrap();
        assert!(!v_release.is_prerelease());
    }

    #[test]
    fn semver_ordering() {
        let v1 = PluginVersion::parse("1.0.0").unwrap();
        let v2 = PluginVersion::parse("1.1.0").unwrap();
        let v3 = PluginVersion::parse("2.0.0").unwrap();
        let v_pre = PluginVersion::parse("1.0.0-alpha").unwrap();

        assert!(v_pre < v1);
        assert!(v1 < v2);
        assert!(v2 < v3);
    }

    #[test]
    fn requirement_matching() {
        assert!(is_version_compatible("1.2.3", "^1.0.0").unwrap());
        assert!(is_version_compatible("1.9.9", "^1.2.0").unwrap());
        assert!(!is_version_compatible("2.0.0", "^1.2.0").unwrap()); // Major bump incompatible

        assert!(is_version_compatible("0.4.5", ">=0.4.0, <0.5.0").unwrap());
        assert!(!is_version_compatible("0.5.0", ">=0.4.0, <0.5.0").unwrap());

        assert!(is_version_compatible("3.1.4", "~3.1.0").unwrap());
        assert!(!is_version_compatible("3.2.0", "~3.1.0").unwrap());
    }

    #[test]
    fn host_engine_compatibility() {
        // Host 1.5.0 against min 1.2.0, max 2.0.0 -> OK
        assert!(check_host_compatibility("1.2.0", Some("2.0.0"), "1.5.0").unwrap());

        // Host 1.1.0 too old for min 1.2.0
        assert!(!check_host_compatibility("1.2.0", Some("2.0.0"), "1.1.0").unwrap());

        // Host 2.1.0 exceeds max 2.0.0
        assert!(!check_host_compatibility("1.2.0", Some("2.0.0"), "2.1.0").unwrap());

        // No max bound
        assert!(check_host_compatibility("1.0.0", None, "5.0.0").unwrap());
    }

    #[test]
    fn serde_roundtrip() {
        let ver = PluginVersion::parse("3.2.1").unwrap();
        let json = serde_json::to_string(&ver).unwrap();
        assert_eq!(json, "\"3.2.1\"");
        let deserialized: PluginVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(ver, deserialized);
    }
}
