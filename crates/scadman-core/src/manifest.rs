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

/// Project identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    /// Minimum OpenSCAD version the project needs, e.g. `"2021.01"` (a leading `>=` is
    /// accepted). scadman warns at `doctor`/`run` when the installed OpenSCAD is older.
    /// OpenSCAD compatibility is really feature-based (see `docs/ecosystem-survey.md`), so
    /// this is an advisory floor, not a guarantee.
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
#[serde(deny_unknown_fields)]
pub struct GitDependency {
    pub git: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Library-root subdir within the repo to expose (default: the repo root). For
    /// libraries that keep their code under e.g. `src/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Also place the exposed root directly on `OPENSCADPATH`, so the library's own
    /// root-relative imports (e.g. dotSCAD's `<util/…>`) resolve. Off by default; opt in
    /// only for libraries that need it — it globalizes that library's top-level filenames.
    #[serde(default, skip_serializing_if = "is_false")]
    pub on_path: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

/// A local path dependency. `root`/`on_path` mirror [`GitDependency`], so a src-layout
/// sibling library can be co-developed the same way it would be consumed from git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathDependency {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub on_path: bool,
}

/// Validate that a package name is a safe single path component.
///
/// A name becomes an OpenSCAD include-path directory and a store/env path segment, so a
/// value containing a separator or `..` (which can arrive from an attacker-influenced
/// transitive `scadman.toml`) must be rejected before it reaches the filesystem.
pub fn validate_package_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("package name is empty".to_string());
    }
    if name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(format!(
            "`{name}` is not a valid package name (no `/`, `\\`, NUL, `.`, or `..`)"
        ));
    }
    Ok(())
}

/// Validate that a library-root subdir stays within the package (it is joined onto the
/// store path and exposed to OpenSCAD).
pub fn validate_library_root(root: &str) -> Result<(), String> {
    if root.starts_with('/') || root.contains('\0') {
        return Err(format!("library root `{root}` must be a relative path"));
    }
    if root.split('/').any(|c| c == "..") {
        return Err(format!("library root `{root}` must not contain `..`"));
    }
    Ok(())
}

/// Parse the leading dotted-numeric run of a version string — e.g. `[2021, 1]` from
/// `"OpenSCAD version 2021.01 (git abc)"` or from a requirement like `">=2021.01"`.
pub fn parse_version(s: &str) -> Option<Vec<u32>> {
    let run: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let nums: Vec<u32> = run
        .split('.')
        .filter(|p| !p.is_empty())
        .map_while(|p| p.parse().ok())
        .collect();
    (!nums.is_empty()).then_some(nums)
}

/// Whether an `installed` OpenSCAD version string satisfies the `required` minimum. `None`
/// if either can't be parsed (the caller reports that it couldn't check).
pub fn meets_openscad_requirement(installed: &str, required: &str) -> Option<bool> {
    let mut have = parse_version(installed)?;
    let mut need = parse_version(required)?;
    let len = have.len().max(need.len());
    have.resize(len, 0);
    need.resize(len, 0);
    Some(have >= need)
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
    fn parses_openscad_versions_from_messy_strings() {
        assert_eq!(
            parse_version("OpenSCAD version 2021.01"),
            Some(vec![2021, 1])
        );
        assert_eq!(
            parse_version("OpenSCAD version 2023.10.30 (git 1a2b3c)"),
            Some(vec![2023, 10, 30])
        );
        assert_eq!(parse_version(">=2021.01"), Some(vec![2021, 1]));
        assert_eq!(parse_version("2019.05"), Some(vec![2019, 5]));
        assert_eq!(parse_version("no digits here"), None);
    }

    #[test]
    fn openscad_requirement_comparison() {
        // Met: same, newer year, newer month, extra patch component.
        assert_eq!(meets_openscad_requirement("2021.01", "2021.01"), Some(true));
        assert_eq!(meets_openscad_requirement("2023.06", "2021.01"), Some(true));
        assert_eq!(meets_openscad_requirement("2021.03", "2021.01"), Some(true));
        assert_eq!(
            meets_openscad_requirement("OpenSCAD version 2021.01.5", "2021.01"),
            Some(true)
        );
        // Unmet: older month, older year.
        assert_eq!(
            meets_openscad_requirement("2020.12", "2021.01"),
            Some(false)
        );
        assert_eq!(
            meets_openscad_requirement("2019.05", ">=2021.01"),
            Some(false)
        );
        // Unparseable → can't decide.
        assert_eq!(meets_openscad_requirement("dev", "2021.01"), None);
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
        assert_eq!(m.project.name, "enclosure");
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
                root: None,
                on_path: false,
            })
        );
        assert_eq!(
            m.dependencies["local-lib"],
            Dependency::Path(PathDependency {
                path: "../local-lib".to_string(),
                root: None,
                on_path: false,
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

    #[test]
    fn rejects_unsafe_package_names() {
        assert!(validate_package_name("BOSL2").is_ok());
        assert!(validate_package_name("Round-Anything").is_ok());
        for bad in ["", ".", "..", "../evil", "a/b", "a\\b", "x\0y"] {
            assert!(validate_package_name(bad).is_err(), "should reject `{bad}`");
        }
    }

    #[test]
    fn misspelled_dependency_key_is_rejected() {
        // `brunch` is a typo for `branch`; deny_unknown_fields must surface it as an error
        // rather than silently dropping it and pinning nothing.
        let text = r#"
[project]
name = "p"

[dependencies]
BOSL2 = { git = "https://x/BOSL2", brunch = "main" }
"#;
        assert!(Manifest::from_toml(text).is_err());
    }
}
