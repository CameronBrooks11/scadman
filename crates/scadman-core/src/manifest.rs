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

/// The shape [`Manifest::from_root_toml`] accepts: [`Manifest`] with unknown keys refused.
/// Kept beside it so the two change together; a test pins that they agree.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictManifest {
    project: StrictProject,
    #[serde(default)]
    dependencies: BTreeMap<String, Dependency>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictProject {
    name: String,
    #[serde(default)]
    openscad: Option<String>,
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

/// Validate that a `rev` is a full commit hash: 40 hex digits (SHA-1), or 64 (SHA-256).
///
/// git fetches a commit only by its full object name. An abbreviated one is taken for a
/// ref name and fails with "couldn't find remote ref", so it is refused here with a
/// message that says what to write instead.
pub fn validate_commit_hash(rev: &str) -> Result<(), String> {
    let full = matches!(rev.len(), 40 | 64) && rev.bytes().all(|b| b.is_ascii_hexdigit());
    if full {
        Ok(())
    } else {
        Err(format!(
            "rev `{rev}` is not a full commit hash — give all 40 hex digits, or use a tag or branch"
        ))
    }
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

/// Parse a project's `openscad` requirement into a minimum version. Only a bare version
/// (`"2021.01"`) or a `>=`/`>` prefix is recognized; anything else (`^`, `<`, `~>`, ranges)
/// returns `None` so the caller can flag it rather than silently treat it as a minimum.
pub fn parse_requirement(required: &str) -> Option<Vec<u32>> {
    let rest = required.trim();
    let rest = rest
        .strip_prefix(">=")
        .or_else(|| rest.strip_prefix('>'))
        .unwrap_or(rest)
        .trim();
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    parse_version(rest)
}

/// Whether an `installed` OpenSCAD version string satisfies the `required` minimum. `None`
/// if the installed version can't be parsed or the requirement is unrecognized (see
/// [`parse_requirement`]).
pub fn meets_openscad_requirement(installed: &str, required: &str) -> Option<bool> {
    let mut have = parse_version(installed)?;
    let mut need = parse_requirement(required)?;
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

    /// Parse a manifest from TOML text, ignoring tables and keys it does not know.
    ///
    /// This is the reader for a dependency's own `scadman.toml`: a library may carry keys
    /// from a newer scadman, and an older one must still be able to read it. For the
    /// project's own manifest use [`Manifest::from_root_toml`].
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Parse the project's own manifest, rejecting any table or key it does not know.
    ///
    /// A misspelled `[dependencies]` header would otherwise read as a project with no
    /// dependencies, and every command would report success.
    pub fn from_root_toml(text: &str) -> Result<Self, toml::de::Error> {
        let strict: StrictManifest = toml::from_str(text)?;
        Ok(Self {
            project: Project {
                name: strict.project.name,
                openscad: strict.project.openscad,
            },
            dependencies: strict.dependencies,
        })
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
        // Leading-zero month ordering: `2021.01` (Jan) is below `2021.10` (Oct).
        assert_eq!(
            meets_openscad_requirement("2021.01", "2021.10"),
            Some(false)
        );
        assert_eq!(meets_openscad_requirement("2021.10", "2021.01"), Some(true));
        // `2021.1` and `2021.01` are the same version (month 1).
        assert_eq!(meets_openscad_requirement("2021.1", "2021.01"), Some(true));
        // Unparseable installed / unrecognized requirement → can't decide.
        assert_eq!(meets_openscad_requirement("dev", "2021.01"), None);
        assert_eq!(meets_openscad_requirement("2021.01", "^2021.01"), None);
    }

    #[test]
    fn parse_requirement_only_accepts_bare_or_ge() {
        assert_eq!(parse_requirement("2021.01"), Some(vec![2021, 1]));
        assert_eq!(parse_requirement(">=2021.01"), Some(vec![2021, 1]));
        assert_eq!(parse_requirement("> 2021.01"), Some(vec![2021, 1]));
        // Unsupported operators / ranges are rejected, not silently treated as a minimum.
        for bad in ["^2021.01", "<2021.01", "~>2021", "2021.01 || 2022.0", ""] {
            assert_eq!(parse_requirement(bad), None, "should reject `{bad}`");
        }
    }

    #[test]
    fn parses_all_dependency_forms() {
        let text = r#"
[project]
name = "enclosure"
openscad = ">=2021.01"

[dependencies]
BOSL2 = "^2.0"
Round-Anything = { git = "https://github.com/Irev-Dev/Round-Anything", rev = "061fef7c429628808e847696bb345a9b0ec6e279" }
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
                rev: Some("061fef7c429628808e847696bb345a9b0ec6e279".to_string()),
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
    fn a_rev_must_be_a_full_commit_hash() {
        let sha1 = "061fef7c429628808e847696bb345a9b0ec6e279";
        assert!(validate_commit_hash(sha1).is_ok());
        assert!(validate_commit_hash(&sha1.to_uppercase()).is_ok());
        assert!(validate_commit_hash(&"a".repeat(64)).is_ok()); // SHA-256 repositories
        for bad in [
            "",
            "061fef7",
            &sha1[..39],
            &format!("{sha1}0"),
            "main",
            "v1.0",
        ] {
            let err = validate_commit_hash(bad).unwrap_err();
            assert!(err.contains("full commit hash"), "`{bad}`: {err}");
        }
        // 40 characters that are not hex is still not a hash.
        assert!(validate_commit_hash(&"z".repeat(40)).is_err());
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
    fn root_manifest_rejects_a_misspelled_table() {
        // `[dependancies]` used to parse as "no dependencies": every command succeeded
        // and the project silently had none.
        let text = "[project]\nname = \"p\"\n\n[dependancies]\nBOSL2 = { git = \"https://x/BOSL2\", tag = \"v1\" }\n";
        let err = Manifest::from_root_toml(text).unwrap_err().to_string();
        assert!(err.contains("dependancies"), "should name the table: {err}");
    }

    #[test]
    fn root_manifest_rejects_an_unknown_project_key() {
        let text = "[project]\nname = \"p\"\nopenscadd = \">=2021.01\"\n";
        let err = Manifest::from_root_toml(text).unwrap_err().to_string();
        assert!(err.contains("openscadd"), "should name the key: {err}");
    }

    #[test]
    fn root_manifest_accepts_everything_a_manifest_serializes_to() {
        // Guards the strict parser against drifting from `Manifest`: a field added to
        // one and not the other would make scadman reject its own output.
        let mut m = Manifest::new("widget");
        m.project.openscad = Some(">=2021.01".to_string());
        m.dependencies.insert(
            "A".to_string(),
            Dependency::Git(GitDependency {
                git: "https://h/A".to_string(),
                rev: None,
                tag: Some("v1".to_string()),
                branch: None,
                root: Some("src".to_string()),
                on_path: true,
            }),
        );
        m.dependencies.insert(
            "B".to_string(),
            Dependency::Path(PathDependency {
                path: "../b".to_string(),
                root: None,
                on_path: false,
            }),
        );
        let text = m.to_toml().unwrap();
        assert_eq!(Manifest::from_root_toml(&text).unwrap(), m);
    }

    #[test]
    fn dependency_manifest_tolerates_unknown_keys() {
        // A library's own manifest may carry keys from a newer scadman; `from_toml` (used
        // for fetched manifests) must keep reading it.
        let text =
            "future = 1\n\n[project]\nname = \"lib\"\nlicense = \"MIT\"\n\n[future-table]\nx = 1\n";
        let m = Manifest::from_toml(text).expect("tolerant parse");
        assert_eq!(m.project.name, "lib");
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
