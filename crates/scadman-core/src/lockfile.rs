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
    /// Content hash of the stored package tree — the [`crate::store`] key. Reproducible
    /// across Linux and macOS; [`crate::store::content_hash`] names the two residual
    /// cross-OS hazards.
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

/// The format version a lockfile's text declares, if it has a readable one.
///
/// Read on its own, without requiring the rest of the file to fit today's shape: a
/// lockfile from a newer scadman may not parse as a [`Lockfile`] at all, and "this is
/// version 3" is the useful thing to say about it.
pub fn declared_version(text: &str) -> Option<u32> {
    #[derive(Deserialize)]
    struct VersionOnly {
        version: u32,
    }
    toml::from_str::<VersionOnly>(text).ok().map(|v| v.version)
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
    fn declared_version_is_read_even_when_the_rest_does_not_fit() {
        assert_eq!(declared_version("version = 1\n"), Some(1));
        // A shape this scadman cannot parse as a Lockfile still yields its version.
        let newer = "version = 7\npackage = \"not a list\"\n";
        assert!(Lockfile::from_toml(newer).is_err());
        assert_eq!(declared_version(newer), Some(7));
        // No readable version: not a lockfile we can say anything about.
        assert_eq!(declared_version("not toml = = ="), None);
        assert_eq!(declared_version("package = []\n"), None);
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
