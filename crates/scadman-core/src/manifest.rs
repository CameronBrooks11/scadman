//! The project manifest (`scadman.toml`) — what a project declares it depends on.
//!
//! The survey (`docs/ecosystem-survey.md`) found that most OpenSCAD libraries have no
//! releases, so a Git dependency pinned to an exact revision is a first-class form here,
//! not an afterthought. A bare version string is also accepted for future registry use.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The canonical project manifest filename.
pub const MANIFEST_FILE: &str = "scadman.toml";

/// A parsed `scadman.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub project: Project,
    /// Declared dependencies, keyed by the name the project refers to them as.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, Dependency>,
}

/// Project identity and compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    /// Optional OpenSCAD version constraint (e.g. `">=2021.01"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openscad: Option<String>,
}

/// A single dependency, accepting either a bare version string or a detailed table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    /// `Name = "^2.0"` — a version constraint, resolved via a registry (future).
    Version(String),
    /// `Name = { git = "…", rev = "…" }` — a Git source, ideally pinned to a revision.
    Git(GitDependency),
    /// `Name = { path = "…" }` — a local path source.
    Path(PathDependency),
}

/// A Git dependency. Prefer `rev` for reproducibility; `tag`/`branch` are resolved to an
/// exact revision at lock time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitDependency {
    pub git: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// A local path dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathDependency {
    pub path: String,
}

impl Manifest {
    /// A fresh manifest for a project with the given name and no dependencies.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            project: Project {
                name: name.into(),
                openscad: None,
            },
            dependencies: BTreeMap::new(),
        }
    }

    /// Parse a manifest from TOML text.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Serialize this manifest to TOML text.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manifest_has_name_and_no_deps() {
        let m = Manifest::new("enclosure");
        assert_eq!(m.project.name, "enclosure");
        assert!(m.dependencies.is_empty());
    }

    #[test]
    fn parses_all_dependency_forms() {
        let text = r#"
[project]
name = "enclosure"
openscad = ">=2021.01"

[dependencies]
BOSL2 = "^2.0"
Round-Anything = { git = "https://github.com/Irev-Dev/Round-Anything", rev = "061fef7" }
local-lib = { path = "../local-lib" }
"#;
        let m = Manifest::from_toml(text).expect("parse");
        assert_eq!(m.project.openscad.as_deref(), Some(">=2021.01"));
        assert_eq!(
            m.dependencies["BOSL2"],
            Dependency::Version("^2.0".to_string())
        );
        assert_eq!(
            m.dependencies["Round-Anything"],
            Dependency::Git(GitDependency {
                git: "https://github.com/Irev-Dev/Round-Anything".to_string(),
                rev: Some("061fef7".to_string()),
                tag: None,
                branch: None,
            })
        );
        assert_eq!(
            m.dependencies["local-lib"],
            Dependency::Path(PathDependency {
                path: "../local-lib".to_string(),
            })
        );
    }

    #[test]
    fn round_trips_through_toml() {
        let mut m = Manifest::new("widget");
        m.dependencies
            .insert("BOSL2".to_string(), Dependency::Version("^2.0".to_string()));
        let text = m.to_toml().expect("serialize");
        let parsed = Manifest::from_toml(&text).expect("reparse");
        assert_eq!(m, parsed);
    }
}
