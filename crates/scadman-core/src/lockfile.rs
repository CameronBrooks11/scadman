//! The lockfile (`scadman.lock`) — the exact, reproducible resolution of a manifest.
//!
//! Each entry pins a package to an exact source revision and a content hash (the key
//! into the content-addressed [`crate::store`]). Because most OpenSCAD libraries have
//! no releases, the revision — not a version number — is the primary identity.

use serde::{Deserialize, Serialize};

/// The canonical lockfile filename.
pub const LOCKFILE_FILE: &str = "scadman.lock";

/// The lockfile format version, bumped on incompatible layout changes.
pub const LOCKFILE_VERSION: u32 = 1;

/// Prefix marking a [`LockedPackage::source`] as a local path dependency (the remainder is
/// the canonicalized absolute path). Path sources are acquired from disk, not fetched.
pub const PATH_SOURCE_PREFIX: &str = "path:";

/// A parsed `scadman.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockfile {
    pub version: u32,
    #[serde(default, rename = "package", skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<LockedPackage>,
}

/// One resolved package, pinned to an exact revision and content hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedPackage {
    /// Canonical package name (the include-path prefix it is exposed under).
    pub name: String,
    /// Canonical, git-fetchable source URL, e.g. `https://github.com/owner/repo`.
    pub source: String,
    /// Exact resolved revision (a Git commit SHA for Git sources).
    pub rev: String,
    /// Content hash of the stored package tree — the [`crate::store`] key. Stable across
    /// machines of the same OS; cross-OS portability is not yet guaranteed (see
    /// [`crate::store::content_hash`]).
    pub hash: String,
    /// Names of this package's direct dependencies, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// Library-root subdir exposed under the package name (default: the repo root, `"."`).
    #[serde(default = "root_default", skip_serializing_if = "is_root_default")]
    pub root: String,
    /// Whether the exposed root is also placed on `OPENSCADPATH` (default false).
    #[serde(default, skip_serializing_if = "is_false")]
    pub on_path: bool,
}

fn root_default() -> String {
    ".".to_string()
}

fn is_root_default(root: &str) -> bool {
    root == "."
}

fn is_false(b: &bool) -> bool {
    !b
}

impl Lockfile {
    /// An empty lockfile at the current format version.
    pub fn new() -> Self {
        Self {
            version: LOCKFILE_VERSION,
            packages: Vec::new(),
        }
    }

    /// Parse a lockfile from TOML text.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Serialize this lockfile to TOML text, with packages in a stable order.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_package() -> LockedPackage {
        LockedPackage {
            name: "BOSL2".to_string(),
            source: "https://github.com/BelfrySCAD/BOSL2".to_string(),
            rev: "afe82db884ee4409aa76ecfcfbbf54d446964af1".to_string(),
            hash: "abc123".to_string(),
            dependencies: Vec::new(),
            root: ".".to_string(),
            on_path: false,
        }
    }

    #[test]
    fn new_lockfile_is_versioned_and_empty() {
        let lock = Lockfile::new();
        assert_eq!(lock.version, LOCKFILE_VERSION);
        assert!(lock.packages.is_empty());
    }

    #[test]
    fn round_trips_through_toml() {
        let mut lock = Lockfile::new();
        lock.packages.push(sample_package());
        let text = lock.to_toml().expect("serialize");
        assert!(text.contains("[[package]]"));
        let parsed = Lockfile::from_toml(&text).expect("reparse");
        assert_eq!(lock, parsed);
    }
}
