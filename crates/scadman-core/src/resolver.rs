//! The dependency resolver — "collect-and-unify" (see `docs/resolver-direction.md`).
//!
//! The ecosystem has no version ranges to negotiate (most dependencies are exact git
//! revisions, and no library declares more than a handful of leaf deps), so this is not a
//! constraint solver. It walks the dependency graph, resolves each reference to an exact
//! commit, and enforces **one resolved version per identity**. Two different revisions for
//! one identity is a hard [`Conflict`] — never an automatic join. Provenance is retained
//! so a conflict can name the two chains that caused it; the data model is deliberately
//! shaped so a future swap to a real solver (pubgrub-rs) is a contained change.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::lockfile::{LOCKFILE_VERSION, LockedPackage, Lockfile, PATH_SOURCE_PREFIX};
use crate::manifest::{
    Dependency, GitDependency, MANIFEST_FILE, Manifest, PathDependency, validate_library_root,
    validate_package_name,
};
use crate::source::fetch_git;
use crate::store::Store;

/// Backstop against pathological depth (cycles already terminate via identity unification).
const MAX_DEPTH: usize = 64;

/// Content fetched for one dependency reference.
pub struct Fetched {
    /// The exact commit the reference resolved to.
    pub rev: String,
    /// Content hash of the stored tree.
    pub hash: String,
    /// Local path to the fetched content (where a transitive manifest is read from).
    pub path: std::path::PathBuf,
}

/// The reference a git dependency names, with the manifest key it was declared under.
///
/// The kind travels with the name because the name alone cannot tell them apart: a tag
/// called `20210101` or a branch called `deadbeef` is spelled like a commit hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRef {
    /// `rev` — an exact commit. Never moves.
    Rev(String),
    /// `tag` — locked to the commit it points at.
    Tag(String),
    /// `branch` — locked to its tip at lock time.
    Branch(String),
}

impl GitRef {
    /// The reference as git spells it.
    pub fn as_str(&self) -> &str {
        match self {
            GitRef::Rev(s) | GitRef::Tag(s) | GitRef::Branch(s) => s,
        }
    }
}

/// Acquires dependency content into the store. Abstracted so the resolver is testable
/// without a network or real git.
pub trait Fetcher {
    /// Acquire a git source at a reference.
    fn fetch(&self, url: &str, reference: &GitRef) -> io::Result<Fetched>;
    /// Acquire a local directory (a path dependency). The `rev` of the result is the
    /// content hash — a path dependency has no revision, only whatever it contains now.
    fn fetch_path(&self, dir: &Path) -> io::Result<Fetched>;
}

/// The production fetcher: git acquisition into a content-addressed store.
pub struct GitFetcher {
    store: Store,
}

impl GitFetcher {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

impl Fetcher for GitFetcher {
    fn fetch(&self, url: &str, reference: &GitRef) -> io::Result<Fetched> {
        let acquired = fetch_git(&self.store, url, reference.as_str())?;
        Ok(Fetched {
            rev: acquired.rev,
            hash: acquired.entry.hash,
            path: acquired.entry.path,
        })
    }

    fn fetch_path(&self, dir: &Path) -> io::Result<Fetched> {
        let entry = self.store.insert(dir)?;
        Ok(Fetched {
            rev: entry.hash.clone(),
            hash: entry.hash,
            path: entry.path,
        })
    }
}

/// One resolved package, pinned to an exact revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub name: String,
    /// Canonical source URL — the package identity.
    pub source: String,
    pub rev: String,
    pub hash: String,
    /// Names of this package's direct dependencies (sorted).
    pub dependencies: Vec<String>,
    /// Library-root subdir exposed under the package name (default `"."`).
    pub root: String,
    /// Whether the exposed root is also placed on `OPENSCADPATH`.
    pub on_path: bool,
    /// The chain of names that first introduced this package (for conflict diagnostics).
    pub required_by: Vec<String>,
}

/// The complete resolution of a manifest: exactly one version per identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSet {
    pub packages: Vec<Resolved>,
}

impl ResolvedSet {
    /// Project the resolution into a lockfile.
    pub fn to_lockfile(&self) -> Lockfile {
        Lockfile {
            version: LOCKFILE_VERSION,
            packages: self
                .packages
                .iter()
                .map(|p| LockedPackage {
                    name: p.name.clone(),
                    source: p.source.clone(),
                    rev: p.rev.clone(),
                    hash: p.hash.clone(),
                    dependencies: p.dependencies.clone(),
                    root: p.root.clone(),
                    on_path: p.on_path,
                })
                .collect(),
        }
    }
}

/// A resolution failure the user must act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    /// The same identity is pinned to two different revisions.
    Version {
        identity: String,
        rev_a: String,
        via_a: Vec<String>,
        rev_b: String,
        via_b: Vec<String>,
    },
    /// Two different sources both claim the same exposed name (same env directory).
    Name {
        name: String,
        source_a: String,
        source_b: String,
    },
    /// One source is referenced under two different names (aliasing; unsupported in v1).
    Alias {
        identity: String,
        name_a: String,
        name_b: String,
    },
}

/// Any resolver error.
#[derive(Debug)]
pub enum ResolveError {
    /// A dependency conflict the user must resolve.
    Conflict(Conflict),
    /// A dependency form not supported in v1 (registry versions; a transitive path dep).
    Unsupported(String),
    /// Fetching a source failed.
    Fetch {
        name: String,
        source: String,
        error: io::Error,
    },
    /// A transitive manifest failed to parse.
    Manifest {
        name: String,
        error: toml::de::Error,
    },
    /// The dependency graph exceeded the depth backstop.
    TooDeep { name: String, depth: usize },
    /// A dependency name is not a safe path component (from the root or a transitive manifest).
    InvalidName { name: String, reason: String },
    /// A dependency's library root is unsafe, or does not exist in the package's content.
    InvalidRoot { name: String, reason: String },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::Conflict(c) => write!(f, "{c}"),
            ResolveError::Unsupported(msg) => write!(f, "unsupported dependency: {msg}"),
            ResolveError::Fetch {
                name,
                source,
                error,
            } => write!(f, "failed to fetch `{name}` ({source}): {error}"),
            ResolveError::Manifest { name, error } => {
                write!(f, "invalid scadman.toml in dependency `{name}`: {error}")
            }
            ResolveError::TooDeep { name, depth } => {
                write!(f, "dependency graph too deep (>{depth}) at `{name}`")
            }
            ResolveError::InvalidName { name, reason } => {
                write!(f, "invalid dependency name `{name}`: {reason}")
            }
            ResolveError::InvalidRoot { name, reason } => {
                write!(f, "invalid library root for `{name}`: {reason}")
            }
        }
    }
}

impl std::error::Error for ResolveError {}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Conflict::Version {
                identity,
                rev_a,
                via_a,
                rev_b,
                via_b,
            } => write!(
                f,
                "cannot resolve `{identity}`: {} requires {rev_a}, but {} requires {rev_b}",
                render_chain(via_a),
                render_chain(via_b),
            ),
            Conflict::Name {
                name,
                source_a,
                source_b,
            } => write!(
                f,
                "two dependencies both claim the name `{name}`: `{source_a}` and `{source_b}`"
            ),
            Conflict::Alias {
                identity,
                name_a,
                name_b,
            } => write!(
                f,
                "source `{identity}` is referenced under two names (`{name_a}` and `{name_b}`); aliasing is not supported"
            ),
        }
    }
}

/// Normalize a source URL to a canonical identity.
///
/// v1 strips a trailing slash and a `.git` suffix. Scheme/host casing and ssh↔https
/// unification are deferred (GitHub-first; documented in `docs/resolver-direction.md`).
pub fn canonical_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    trimmed.strip_suffix(".git").unwrap_or(trimmed).to_string()
}

/// Report why `lock` is out of date with `manifest`, if it is.
///
/// Checks that every declared git dependency has a lock entry with the same identity and,
/// for an exact `rev` pin, the same commit; and that the lock holds no package the
/// manifest no longer leads to (a removed dependency). It does not detect a changed
/// tag/branch on an unchanged URL (that needs re-resolution).
pub fn lock_staleness(manifest: &Manifest, lock: &Lockfile) -> Option<String> {
    for (name, dep) in &manifest.dependencies {
        let Dependency::Git(git) = dep else {
            // Path deps are re-resolved every sync (their content is mutable), so staleness
            // is moot; registry deps don't resolve in v1.
            continue;
        };
        let identity = canonical_url(&git.git);
        match lock.packages.iter().find(|p| &p.name == name) {
            None => return Some(format!("`{name}` is declared but not in the lockfile")),
            Some(pkg) if pkg.source != identity => {
                return Some(format!(
                    "`{name}` source changed ({} → {})",
                    pkg.source, identity
                ));
            }
            Some(pkg) => {
                // An exact `rev` change is knowable without re-resolving. The lock stores
                // the full SHA and the manifest rev may be abbreviated, so a prefix match
                // is fresh.
                if let Some(requested) = &git.rev
                    && !pkg.rev.starts_with(requested)
                {
                    return Some(format!(
                        "`{name}` rev changed (manifest wants {requested}, lock has {})",
                        pkg.rev
                    ));
                }
                // Exposure knobs also affect the environment, so a change makes the lock stale.
                let want_root = git.root.as_deref().unwrap_or(".");
                if pkg.root != want_root {
                    return Some(format!(
                        "`{name}` library root changed ({} → {want_root})",
                        pkg.root
                    ));
                }
                if pkg.on_path != git.on_path {
                    return Some(format!("`{name}` on_path changed to {}", git.on_path));
                }
            }
        }
    }
    orphaned_package(manifest, lock)
        .map(|name| format!("`{name}` is locked but no longer declared"))
}

/// The first locked package (in lockfile order) that nothing requires: neither a manifest
/// dependency nor reachable from one through the lock's own `dependencies` edges. That is
/// what `scadman remove` leaves behind until the next `lock`. A package another one
/// depends on is not an orphan, however deep.
fn orphaned_package<'a>(manifest: &Manifest, lock: &'a Lockfile) -> Option<&'a str> {
    let by_name: BTreeMap<&str, &LockedPackage> =
        lock.packages.iter().map(|p| (p.name.as_str(), p)).collect();
    let mut required: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut pending: Vec<&str> = manifest.dependencies.keys().map(String::as_str).collect();
    while let Some(name) = pending.pop() {
        if !required.insert(name) {
            continue; // already walked (also what stops a dependency cycle)
        }
        if let Some(pkg) = by_name.get(name) {
            pending.extend(pkg.dependencies.iter().map(String::as_str));
        }
    }
    lock.packages
        .iter()
        .map(|p| p.name.as_str())
        .find(|name| !required.contains(name))
}

/// Resolve a manifest into a pinned set — one version per identity.
///
/// `base` is the directory the root manifest lives in; relative path dependencies resolve
/// against it. (Path dependencies are only permitted in the root manifest — see
/// [`Resolution::visit_one`].)
pub fn resolve(
    root: &Manifest,
    base: &Path,
    fetcher: &dyn Fetcher,
) -> Result<ResolvedSet, ResolveError> {
    let mut resolution = Resolution {
        fetcher,
        base,
        by_identity: BTreeMap::new(),
        name_to_identity: BTreeMap::new(),
    };
    resolution.visit(&root.dependencies, &[], 0)?;

    let mut packages: Vec<Resolved> = resolution.by_identity.into_values().collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.source.cmp(&b.source)));
    Ok(ResolvedSet { packages })
}

struct Resolution<'a> {
    fetcher: &'a dyn Fetcher,
    base: &'a Path,
    by_identity: BTreeMap<String, Resolved>,
    name_to_identity: BTreeMap<String, String>,
}

impl Resolution<'_> {
    fn visit(
        &mut self,
        deps: &BTreeMap<String, Dependency>,
        via: &[String],
        depth: usize,
    ) -> Result<(), ResolveError> {
        for (name, dep) in deps {
            self.visit_one(name, dep, via, depth)?;
        }
        Ok(())
    }

    fn visit_one(
        &mut self,
        name: &str,
        dep: &Dependency,
        via: &[String],
        depth: usize,
    ) -> Result<(), ResolveError> {
        if depth > MAX_DEPTH {
            return Err(ResolveError::TooDeep {
                name: name.to_string(),
                depth: MAX_DEPTH,
            });
        }

        // A name from any manifest (root or a fetched transitive one) becomes a filesystem
        // path segment — reject traversal before it reaches the store/environment.
        validate_package_name(name).map_err(|reason| ResolveError::InvalidName {
            name: name.to_string(),
            reason,
        })?;

        // A declared library root is joined onto the package's content and exposed to
        // OpenSCAD. Check it for every kind of dependency, from any manifest, before
        // anything is fetched: the environment build rejects an unsafe one too, but only
        // after it has been written into the lockfile.
        let declared_root = match dep {
            Dependency::Git(git) => git.root.as_deref(),
            Dependency::Path(p) => p.root.as_deref(),
            Dependency::Version(_) => None,
        };
        if let Some(root) = declared_root {
            validate_library_root(root).map_err(|reason| ResolveError::InvalidRoot {
                name: name.to_string(),
                reason,
            })?;
        }

        // How to acquire this dependency's content, kept owned so the fetch happens after
        // the name-binding check (below) without borrowing `dep`.
        enum FetchInput {
            Git { url: String, reference: GitRef },
            Path { dir: PathBuf },
        }

        // Identity (the "one version per identity" key and lockfile source), exposure knobs,
        // and how to acquire it, per dependency kind.
        let (identity, root, on_path, input) = match dep {
            Dependency::Git(git) => {
                let identity = canonical_url(&git.git);
                let input = FetchInput::Git {
                    url: identity.clone(),
                    reference: git_reference(name, git)?,
                };
                (
                    identity,
                    git.root.clone().unwrap_or_else(|| ".".to_string()),
                    git.on_path,
                    input,
                )
            }
            Dependency::Path(p) => {
                // A path dependency names a directory on this machine — meaningless when it
                // arrives via a fetched (untrusted) transitive manifest, so only the root
                // manifest may declare one.
                if !via.is_empty() {
                    return Err(ResolveError::Unsupported(format!(
                        "path dependency `{name}` is only supported in the root manifest (found via {})",
                        render_chain(via)
                    )));
                }
                let dir = self.resolve_path_dep(name, p)?;
                let identity = format!("{PATH_SOURCE_PREFIX}{}", dir.display());
                (
                    identity,
                    p.root.clone().unwrap_or_else(|| ".".to_string()),
                    p.on_path,
                    FetchInput::Path { dir },
                )
            }
            Dependency::Version(_) => {
                return Err(ResolveError::Unsupported(format!(
                    "version dependency `{name}` needs a package registry — registries aren't supported yet; use a git or path source"
                )));
            }
        };

        // One exposed name must map to one identity. Record the binding on ALL paths —
        // including the unify early-return below — so a later use of the name is checked.
        if let Some(existing) = self.name_to_identity.get(name)
            && existing != &identity
        {
            return Err(ResolveError::Conflict(Conflict::Name {
                name: name.to_string(),
                source_a: existing.clone(),
                source_b: identity,
            }));
        }
        self.name_to_identity
            .insert(name.to_string(), identity.clone());

        let fetched = match input {
            FetchInput::Git { url, reference } => self.fetcher.fetch(&url, &reference),
            FetchInput::Path { dir } => self.fetcher.fetch_path(&dir),
        }
        .map_err(|error| ResolveError::Fetch {
            name: name.to_string(),
            source: identity.clone(),
            error,
        })?;

        // The root must exist in what was fetched. A misspelled one would otherwise lock,
        // and only fail when the environment is built.
        if !fetched.path.join(&root).is_dir() {
            return Err(ResolveError::InvalidRoot {
                name: name.to_string(),
                reason: format!("`{root}` does not exist in the package"),
            });
        }

        // Already resolved this identity — must agree on revision and on exposed name.
        if let Some(existing) = self.by_identity.get(&identity) {
            if existing.rev != fetched.rev {
                return Err(ResolveError::Conflict(Conflict::Version {
                    identity,
                    rev_a: existing.rev.clone(),
                    via_a: existing.required_by.clone(),
                    rev_b: fetched.rev,
                    via_b: chain(via, name),
                }));
            }
            if existing.name != name {
                return Err(ResolveError::Conflict(Conflict::Alias {
                    identity,
                    name_a: existing.name.clone(),
                    name_b: name.to_string(),
                }));
            }
            return Ok(());
        }

        let dep_deps = read_manifest_deps(name, &fetched.path)?;
        let dep_names: Vec<String> = dep_deps.keys().cloned().collect();
        let required_by = chain(via, name);

        self.by_identity.insert(
            identity.clone(),
            Resolved {
                name: name.to_string(),
                source: identity,
                rev: fetched.rev,
                hash: fetched.hash,
                dependencies: dep_names,
                root,
                on_path,
                required_by: required_by.clone(),
            },
        );

        self.visit(&dep_deps, &required_by, depth + 1)
    }
}

impl Resolution<'_> {
    /// Resolve a path dependency's directory against the base dir and canonicalize it.
    fn resolve_path_dep(&self, name: &str, p: &PathDependency) -> Result<PathBuf, ResolveError> {
        let joined = self.base.join(&p.path);
        let dir = joined.canonicalize().map_err(|error| ResolveError::Fetch {
            name: name.to_string(),
            source: format!("{PATH_SOURCE_PREFIX}{}", joined.display()),
            error,
        })?;
        if !dir.is_dir() {
            return Err(ResolveError::Unsupported(format!(
                "path dependency `{name}` ({}) is not a directory",
                dir.display()
            )));
        }
        Ok(dir)
    }
}

/// The one reference a git dependency names. Exactly one of `rev`, `tag` and `branch`
/// must be set: an entry with two says two different things, and picking one silently
/// locks code the author may not have meant.
fn git_reference(name: &str, git: &GitDependency) -> Result<GitRef, ResolveError> {
    let given: Vec<(&str, GitRef)> = [
        ("rev", git.rev.clone().map(GitRef::Rev)),
        ("tag", git.tag.clone().map(GitRef::Tag)),
        ("branch", git.branch.clone().map(GitRef::Branch)),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|v| (key, v)))
    .collect();

    match given.as_slice() {
        [(_, reference)] => Ok(reference.clone()),
        [] => Err(ResolveError::Unsupported(format!(
            "git dependency `{name}` needs a rev, tag, or branch"
        ))),
        several => {
            let keys: Vec<&str> = several.iter().map(|(key, _)| *key).collect();
            Err(ResolveError::Unsupported(format!(
                "git dependency `{name}` sets {} — give exactly one of rev, tag, or branch",
                keys.join(" and ")
            )))
        }
    }
}

fn read_manifest_deps(
    name: &str,
    path: &Path,
) -> Result<BTreeMap<String, Dependency>, ResolveError> {
    let manifest_path = path.join(MANIFEST_FILE);
    if !manifest_path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = std::fs::read_to_string(&manifest_path).map_err(|error| ResolveError::Fetch {
        name: name.to_string(),
        source: manifest_path.display().to_string(),
        error,
    })?;
    let manifest = Manifest::from_toml(&text).map_err(|error| ResolveError::Manifest {
        name: name.to_string(),
        error,
    })?;
    Ok(manifest.dependencies)
}

fn chain(via: &[String], name: &str) -> Vec<String> {
    let mut result = via.to_vec();
    result.push(name.to_string());
    result
}

fn render_chain(via: &[String]) -> String {
    let mut parts = vec!["root".to_string()];
    parts.extend(via.iter().cloned());
    parts.join(" → ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A package the fake fetcher knows: reference→rev, plus its own manifest (if any).
    struct FakePkg {
        revs: HashMap<String, String>,
        manifest: Option<String>,
    }

    struct FakeFetcher {
        pkgs: HashMap<String, FakePkg>,
        base: TempDir,
        dirs: RefCell<HashMap<String, PathBuf>>,
    }

    impl FakeFetcher {
        fn new() -> Self {
            Self {
                pkgs: HashMap::new(),
                base: TempDir::new().unwrap(),
                dirs: RefCell::new(HashMap::new()),
            }
        }

        /// Register a package: `url`, its `(reference, rev)` pairs, and optional manifest.
        fn pkg(mut self, url: &str, revs: &[(&str, &str)], manifest: Option<&str>) -> Self {
            self.pkgs.insert(
                url.to_string(),
                FakePkg {
                    revs: revs
                        .iter()
                        .map(|(r, rev)| (r.to_string(), rev.to_string()))
                        .collect(),
                    manifest: manifest.map(str::to_string),
                },
            );
            self
        }
    }

    impl Fetcher for FakeFetcher {
        fn fetch(&self, url: &str, reference: &GitRef) -> io::Result<Fetched> {
            let reference = reference.as_str();
            let pkg = self
                .pkgs
                .get(url)
                .ok_or_else(|| io::Error::other(format!("unknown url {url}")))?;
            let rev = pkg
                .revs
                .get(reference)
                .ok_or_else(|| io::Error::other(format!("unknown ref {reference} for {url}")))?
                .clone();

            let mut dirs = self.dirs.borrow_mut();
            let path = dirs
                .entry(url.to_string())
                .or_insert_with(|| {
                    let safe: String = url
                        .chars()
                        .map(|c| if c.is_alphanumeric() { c } else { '_' })
                        .collect();
                    let dir = self.base.path().join(safe);
                    std::fs::create_dir_all(&dir).unwrap();
                    if let Some(manifest) = &pkg.manifest {
                        std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
                    }
                    dir
                })
                .clone();

            Ok(Fetched {
                rev: rev.clone(),
                hash: format!("hash-{rev}"),
                path,
            })
        }

        fn fetch_path(&self, dir: &Path) -> io::Result<Fetched> {
            // A path dep is acquired straight from disk; derive a deterministic hash from the
            // directory and read any transitive deps from the real dir (as production does).
            let key: String = dir
                .to_string_lossy()
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect();
            let hash = format!("pathhash-{key}");
            Ok(Fetched {
                rev: hash.clone(),
                hash,
                path: dir.to_path_buf(),
            })
        }
    }

    fn manifest(toml: &str) -> Manifest {
        Manifest::from_toml(toml).unwrap()
    }

    #[test]
    fn canonical_url_strips_git_and_slash() {
        assert_eq!(canonical_url("https://x/repo.git"), "https://x/repo");
        assert_eq!(canonical_url("https://x/repo/"), "https://x/repo");
        assert_eq!(canonical_url(" https://x/repo.git/ "), "https://x/repo");
    }

    #[test]
    fn empty_manifest_resolves_to_nothing() {
        let root = manifest("[project]\nname = \"p\"\n");
        let set = resolve(&root, Path::new("."), &FakeFetcher::new()).unwrap();
        assert!(set.packages.is_empty());
    }

    #[test]
    fn single_dep_with_no_transitive_manifest() {
        let fetcher = FakeFetcher::new().pkg("https://h/A", &[("r1", "sha_a")], None);
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"r1\" }\n",
        );
        let set = resolve(&root, Path::new("."), &fetcher).unwrap();
        assert_eq!(set.packages.len(), 1);
        assert_eq!(set.packages[0].rev, "sha_a");
        assert!(set.packages[0].dependencies.is_empty());
        // to_lockfile mirrors the resolution.
        let lock = set.to_lockfile();
        assert_eq!(lock.packages[0].rev, "sha_a");
    }

    #[test]
    fn transitive_dependency_is_collected() {
        // A ships a scadman.toml declaring B.
        let a_manifest = "[project]\nname = \"A\"\n[dependencies]\nB = { git = \"https://h/B\", rev = \"r1\" }\n";
        let fetcher = FakeFetcher::new()
            .pkg("https://h/A", &[("ra", "sha_a")], Some(a_manifest))
            .pkg("https://h/B", &[("r1", "sha_b")], None);
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"ra\" }\n",
        );
        let set = resolve(&root, Path::new("."), &fetcher).unwrap();
        let names: Vec<&str> = set.packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["A", "B"]);
        let a = set.packages.iter().find(|p| p.name == "A").unwrap();
        assert_eq!(a.dependencies, vec!["B".to_string()]);
    }

    #[test]
    fn diamond_at_same_rev_unifies() {
        // A and C both depend on B at the same reference → B resolved once.
        let dep_b = "[project]\nname = \"x\"\n[dependencies]\nB = { git = \"https://h/B\", rev = \"r1\" }\n";
        let fetcher = FakeFetcher::new()
            .pkg("https://h/A", &[("ra", "sha_a")], Some(dep_b))
            .pkg("https://h/C", &[("rc", "sha_c")], Some(dep_b))
            .pkg("https://h/B", &[("r1", "sha_b")], None);
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"ra\" }\nC = { git = \"https://h/C\", rev = \"rc\" }\n",
        );
        let set = resolve(&root, Path::new("."), &fetcher).unwrap();
        assert_eq!(set.packages.iter().filter(|p| p.name == "B").count(), 1);
        assert_eq!(set.packages.len(), 3);
    }

    #[test]
    fn version_conflict_is_reported() {
        // A wants B@r1→sha_b1, C wants B@r2→sha_b2.
        let a_dep = "[project]\nname = \"A\"\n[dependencies]\nB = { git = \"https://h/B\", rev = \"r1\" }\n";
        let c_dep = "[project]\nname = \"C\"\n[dependencies]\nB = { git = \"https://h/B\", rev = \"r2\" }\n";
        let fetcher = FakeFetcher::new()
            .pkg("https://h/A", &[("ra", "sha_a")], Some(a_dep))
            .pkg("https://h/C", &[("rc", "sha_c")], Some(c_dep))
            .pkg("https://h/B", &[("r1", "sha_b1"), ("r2", "sha_b2")], None);
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"ra\" }\nC = { git = \"https://h/C\", rev = \"rc\" }\n",
        );
        let err = resolve(&root, Path::new("."), &fetcher).unwrap_err();
        match err {
            ResolveError::Conflict(Conflict::Version {
                identity,
                rev_a,
                via_a,
                rev_b,
                via_b,
            }) => {
                assert_eq!(identity, "https://h/B");
                assert!(
                    (rev_a == "sha_b1" && rev_b == "sha_b2")
                        || (rev_a == "sha_b2" && rev_b == "sha_b1")
                );
                // Both provenance chains are complete root→leaf paths (a diagnostic prior
                // tools dropped — see docs/ecosystem-survey.md on olman).
                assert_eq!(via_a.last().unwrap(), "B");
                assert_eq!(via_b.last().unwrap(), "B");
                assert!(via_a[0] == "A" || via_a[0] == "C");
                assert!(via_b[0] == "A" || via_b[0] == "C");
                assert_ne!(via_a[0], via_b[0]);
            }
            other => panic!("expected version conflict, got {other:?}"),
        }
    }

    #[test]
    fn same_source_under_two_names_is_an_alias_error() {
        let fetcher = FakeFetcher::new().pkg("https://h/A", &[("r1", "sha_a")], None);
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nOne = { git = \"https://h/A\", rev = \"r1\" }\nTwo = { git = \"https://h/A\", rev = \"r1\" }\n",
        );
        let err = resolve(&root, Path::new("."), &fetcher).unwrap_err();
        assert!(matches!(
            err,
            ResolveError::Conflict(Conflict::Alias { .. })
        ));
    }

    #[test]
    fn malformed_transitive_manifest_is_reported() {
        let fetcher = FakeFetcher::new().pkg(
            "https://h/A",
            &[("r1", "sha_a")],
            Some("not valid = = toml"),
        );
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"r1\" }\n",
        );
        let err = resolve(&root, Path::new("."), &fetcher).unwrap_err();
        assert!(matches!(err, ResolveError::Manifest { .. }));
    }

    #[test]
    fn a_git_dependency_with_more_than_one_ref_is_rejected() {
        // `rev` used to win silently over `tag`, and `tag` over `branch`, so an entry
        // saying two things locked whichever came first in that order.
        let fetcher = FakeFetcher::new().pkg(
            "https://h/A",
            &[("r1", "sha_1"), ("v2", "sha_2"), ("main", "sha_3")],
            None,
        );
        for (refs, named) in [
            ("rev = \"r1\", tag = \"v2\"", ["rev", "tag"]),
            ("rev = \"r1\", branch = \"main\"", ["rev", "branch"]),
            ("tag = \"v2\", branch = \"main\"", ["tag", "branch"]),
        ] {
            let root = manifest(&format!(
                "[project]\nname = \"p\"\n[dependencies]\nA = {{ git = \"https://h/A\", {refs} }}\n"
            ));
            let err = resolve(&root, Path::new("."), &fetcher).unwrap_err();
            let msg = err.to_string();
            assert!(matches!(err, ResolveError::Unsupported(_)), "{refs}: {msg}");
            assert!(msg.contains("`A`"), "names the dependency: {msg}");
            for key in named {
                assert!(msg.contains(key), "names `{key}`: {msg}");
            }
        }
    }

    #[test]
    fn a_transitive_git_dependency_with_more_than_one_ref_is_rejected() {
        let a_manifest = "[project]\nname = \"A\"\n[dependencies]\nB = { git = \"https://h/B\", rev = \"r1\", tag = \"v2\" }\n";
        let fetcher = FakeFetcher::new()
            .pkg("https://h/A", &[("ra", "sha_a")], Some(a_manifest))
            .pkg("https://h/B", &[("r1", "sha_b1"), ("v2", "sha_b2")], None);
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"ra\" }\n",
        );
        let err = resolve(&root, Path::new("."), &fetcher).unwrap_err();
        assert!(matches!(err, ResolveError::Unsupported(_)), "{err}");
    }

    #[test]
    fn unknown_keys_in_a_transitive_manifest_are_ignored() {
        // A dependency's manifest may use keys from a newer scadman; it still resolves.
        let a_manifest = "future = 1\n[project]\nname = \"A\"\nlicense = \"MIT\"\n[dependencies]\nB = { git = \"https://h/B\", rev = \"r1\" }\n";
        let fetcher = FakeFetcher::new()
            .pkg("https://h/A", &[("ra", "sha_a")], Some(a_manifest))
            .pkg("https://h/B", &[("r1", "sha_b")], None);
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"ra\" }\n",
        );
        let set = resolve(&root, Path::new("."), &fetcher).unwrap();
        assert_eq!(set.packages.len(), 2);
    }

    #[test]
    fn name_collision_is_reported() {
        // A and C both introduce a dep named `B` from different URLs.
        let a_dep = "[project]\nname = \"A\"\n[dependencies]\nB = { git = \"https://h/B1\", rev = \"r1\" }\n";
        let c_dep = "[project]\nname = \"C\"\n[dependencies]\nB = { git = \"https://h/B2\", rev = \"r1\" }\n";
        let fetcher = FakeFetcher::new()
            .pkg("https://h/A", &[("ra", "sha_a")], Some(a_dep))
            .pkg("https://h/C", &[("rc", "sha_c")], Some(c_dep))
            .pkg("https://h/B1", &[("r1", "sha_b1")], None)
            .pkg("https://h/B2", &[("r1", "sha_b2")], None);
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"ra\" }\nC = { git = \"https://h/C\", rev = \"rc\" }\n",
        );
        let err = resolve(&root, Path::new("."), &fetcher).unwrap_err();
        assert!(matches!(err, ResolveError::Conflict(Conflict::Name { .. })));
    }

    #[test]
    fn cycle_terminates_via_unification() {
        // A depends on B, B depends on A (same identity/rev) → terminates.
        let a_dep = "[project]\nname = \"A\"\n[dependencies]\nB = { git = \"https://h/B\", rev = \"rb\" }\n";
        let b_dep = "[project]\nname = \"B\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"ra\" }\n";
        let fetcher = FakeFetcher::new()
            .pkg("https://h/A", &[("ra", "sha_a")], Some(a_dep))
            .pkg("https://h/B", &[("rb", "sha_b")], Some(b_dep));
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"ra\" }\n",
        );
        let set = resolve(&root, Path::new("."), &fetcher).unwrap();
        assert_eq!(set.packages.len(), 2);
    }

    #[test]
    fn registry_version_dependency_is_unsupported() {
        let root = manifest("[project]\nname = \"p\"\n[dependencies]\nA = \"^1.0\"\n");
        let err = resolve(&root, Path::new("."), &FakeFetcher::new()).unwrap_err();
        assert!(matches!(err, ResolveError::Unsupported(_)));
    }

    #[test]
    fn lock_staleness_detects_missing_source_and_rev_changes() {
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"abc123\" }\n",
        );

        // Empty lock: A is declared but absent → stale.
        assert!(lock_staleness(&root, &Lockfile::new()).is_some());

        let one = |source: &str, rev: &str| {
            let mut lock = Lockfile::new();
            lock.packages.push(LockedPackage {
                name: "A".to_string(),
                source: source.to_string(),
                rev: rev.to_string(),
                hash: "h".to_string(),
                dependencies: Vec::new(),
                root: ".".to_string(),
                on_path: false,
            });
            lock
        };

        // Matching source, and the manifest rev is a prefix of the locked SHA → fresh.
        assert!(lock_staleness(&root, &one("https://h/A", "abc123def456")).is_none());
        // Different source for the same name → stale.
        assert!(lock_staleness(&root, &one("https://h/OTHER", "abc123")).is_some());
        // Same source but a different rev → stale.
        assert!(lock_staleness(&root, &one("https://h/A", "999999")).is_some());
    }

    /// A lock entry with the given direct dependencies (for the orphan tests).
    fn entry(name: &str, deps: &[&str]) -> LockedPackage {
        LockedPackage {
            name: name.to_string(),
            source: format!("https://h/{name}"),
            rev: "abc123".to_string(),
            hash: "h".to_string(),
            dependencies: deps.iter().map(|d| d.to_string()).collect(),
            root: ".".to_string(),
            on_path: false,
        }
    }

    #[test]
    fn lock_staleness_detects_an_entry_the_manifest_no_longer_declares() {
        // `A` was removed from the manifest; the lock still pins it. That used to count
        // as fresh ("a harmless extra pin"), so sync went on exposing the library.
        let root = manifest("[project]\nname = \"p\"\n");
        let mut lock = Lockfile::new();
        lock.packages.push(entry("A", &[]));
        let reason = lock_staleness(&root, &lock).expect("an orphan makes the lock stale");
        assert!(reason.contains("`A`"), "names the orphan: {reason}");
    }

    #[test]
    fn lock_staleness_does_not_mistake_a_transitive_package_for_an_orphan() {
        // The manifest declares only A. B and C are in the lock because A needs B and B
        // needs C; neither is an orphan. D is reachable from nothing.
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"abc123\" }\n",
        );
        let mut lock = Lockfile::new();
        lock.packages.push(entry("A", &["B"]));
        lock.packages.push(entry("B", &["C"]));
        lock.packages.push(entry("C", &["A"])); // a cycle must not hang the walk
        assert_eq!(lock_staleness(&root, &lock), None);

        lock.packages.push(entry("D", &[]));
        let reason = lock_staleness(&root, &lock).expect("D is an orphan");
        assert!(reason.contains("`D`"), "{reason}");
    }

    #[test]
    fn lock_staleness_counts_path_dependencies_as_roots() {
        // A path dependency's own entry, and what it pulls in, are not orphans.
        let root =
            manifest("[project]\nname = \"p\"\n[dependencies]\nsib = { path = \"../sib\" }\n");
        let mut lock = Lockfile::new();
        lock.packages.push(entry("sib", &["Util"]));
        lock.packages.push(entry("Util", &[]));
        assert_eq!(lock_staleness(&root, &lock), None);
    }

    #[test]
    fn lock_staleness_detects_root_and_on_path_changes() {
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"abc123\", root = \"src\", on_path = true }\n",
        );
        let mut lock = Lockfile::new();
        lock.packages.push(LockedPackage {
            name: "A".to_string(),
            source: "https://h/A".to_string(),
            rev: "abc123".to_string(),
            hash: "h".to_string(),
            dependencies: Vec::new(),
            root: ".".to_string(), // manifest now wants `src` → stale
            on_path: false,
        });
        assert!(lock_staleness(&root, &lock).is_some());
    }

    #[test]
    fn rejects_path_traversal_dependency_name() {
        // A name like `../evil` — as could arrive from a transitive manifest — must be
        // rejected before it becomes a filesystem path.
        let mut root = Manifest::new("p");
        root.dependencies.insert(
            "../evil".to_string(),
            Dependency::Git(GitDependency {
                git: "https://h/x".to_string(),
                rev: Some("r".to_string()),
                tag: None,
                branch: None,
                root: None,
                on_path: false,
            }),
        );
        let err = resolve(&root, Path::new("."), &FakeFetcher::new()).unwrap_err();
        assert!(matches!(err, ResolveError::InvalidName { .. }));
    }

    #[test]
    fn path_dependency_is_resolved_relative_to_base() {
        // A local sibling library, with its own transitive git dependency.
        let base = TempDir::new().unwrap();
        let lib = base.path().join("locallib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(
            lib.join(MANIFEST_FILE),
            "[project]\nname = \"locallib\"\n\n[dependencies]\nUtil = { git = \"https://h/util\", rev = \"u1\" }\n",
        )
        .unwrap();

        let root = manifest(
            "[project]\nname = \"p\"\n\n[dependencies]\nmylib = { path = \"locallib\" }\n",
        );
        let fetcher = FakeFetcher::new().pkg("https://h/util", &[("u1", "u1")], None);
        let set = resolve(&root, base.path(), &fetcher).unwrap();

        let mylib = set.packages.iter().find(|p| p.name == "mylib").unwrap();
        assert!(
            mylib.source.starts_with(PATH_SOURCE_PREFIX),
            "path source: {}",
            mylib.source
        );
        assert_eq!(
            mylib.rev, mylib.hash,
            "a path dep's rev is its content hash"
        );
        assert_eq!(mylib.dependencies, vec!["Util".to_string()]);
        // The transitive git dep resolved too.
        assert!(set.packages.iter().any(|p| p.name == "Util"));
    }

    #[test]
    fn two_path_deps_to_the_same_dir_are_an_alias_conflict() {
        let base = TempDir::new().unwrap();
        std::fs::create_dir_all(base.path().join("lib")).unwrap();
        let root = manifest(
            "[project]\nname = \"p\"\n\n[dependencies]\na = { path = \"lib\" }\nb = { path = \"lib\" }\n",
        );
        let err = resolve(&root, base.path(), &FakeFetcher::new()).unwrap_err();
        assert!(
            matches!(err, ResolveError::Conflict(Conflict::Alias { .. })),
            "{err:?}"
        );
    }

    #[test]
    fn a_git_dependency_with_an_unsafe_root_is_rejected_before_any_fetch() {
        // Only path dependencies had their root checked here; a git one locked cleanly
        // and failed later, at sync. No package is registered with the fetcher, so
        // reaching the fetch would surface as a Fetch error instead.
        for bad in ["../..", "/etc", "src/../../x"] {
            let root = manifest(&format!(
                "[project]\nname = \"p\"\n[dependencies]\nA = {{ git = \"https://h/A\", rev = \"r1\", root = \"{bad}\" }}\n"
            ));
            let err = resolve(&root, Path::new("."), &FakeFetcher::new()).unwrap_err();
            assert!(
                matches!(&err, ResolveError::InvalidRoot { name, .. } if name == "A"),
                "`{bad}`: {err:?}"
            );
        }
    }

    #[test]
    fn a_root_that_does_not_exist_in_the_package_is_rejected() {
        // A misspelled root used to lock, then fail at sync with "not in the store".
        let fetcher = FakeFetcher::new().pkg("https://h/A", &[("r1", "sha_a")], None);
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"r1\", root = \"srcc\" }\n",
        );
        let err = resolve(&root, Path::new("."), &fetcher).unwrap_err();
        assert!(
            matches!(&err, ResolveError::InvalidRoot { name, .. } if name == "A"),
            "{err:?}"
        );
        assert!(err.to_string().contains("srcc"), "names the root: {err}");

        // The same for a path dependency.
        let base = TempDir::new().unwrap();
        std::fs::create_dir_all(base.path().join("lib")).unwrap();
        let root = manifest(
            "[project]\nname = \"p\"\n\n[dependencies]\nlib = { path = \"lib\", root = \"srcc\" }\n",
        );
        let err = resolve(&root, base.path(), &FakeFetcher::new()).unwrap_err();
        assert!(
            matches!(&err, ResolveError::InvalidRoot { name, .. } if name == "lib"),
            "{err:?}"
        );
    }

    #[test]
    fn an_unsafe_path_dependency_root_is_reported_as_a_root_not_a_name() {
        let base = TempDir::new().unwrap();
        std::fs::create_dir_all(base.path().join("lib")).unwrap();
        let root = manifest(
            "[project]\nname = \"p\"\n\n[dependencies]\nlib = { path = \"lib\", root = \"../..\" }\n",
        );
        let err = resolve(&root, base.path(), &FakeFetcher::new()).unwrap_err();
        assert!(matches!(err, ResolveError::InvalidRoot { .. }), "{err:?}");
        assert!(
            !err.to_string().contains("dependency name"),
            "a bad root is not a bad name: {err}"
        );
    }

    #[test]
    fn a_transitive_git_dependency_with_an_unsafe_root_is_rejected() {
        let a_manifest = "[project]\nname = \"A\"\n[dependencies]\nB = { git = \"https://h/B\", rev = \"r1\", root = \"../..\" }\n";
        let fetcher = FakeFetcher::new()
            .pkg("https://h/A", &[("ra", "sha_a")], Some(a_manifest))
            .pkg("https://h/B", &[("r1", "sha_b")], None);
        let root = manifest(
            "[project]\nname = \"p\"\n[dependencies]\nA = { git = \"https://h/A\", rev = \"ra\" }\n",
        );
        let err = resolve(&root, Path::new("."), &fetcher).unwrap_err();
        assert!(
            matches!(&err, ResolveError::InvalidRoot { name, .. } if name == "B"),
            "{err:?}"
        );
    }

    #[test]
    fn path_dependency_carries_root_and_on_path() {
        let base = TempDir::new().unwrap();
        std::fs::create_dir_all(base.path().join("lib").join("src")).unwrap();
        let root = manifest(
            "[project]\nname = \"p\"\n\n[dependencies]\nlib = { path = \"lib\", root = \"src\", on_path = true }\n",
        );
        let set = resolve(&root, base.path(), &FakeFetcher::new()).unwrap();
        let lib = set.packages.iter().find(|p| p.name == "lib").unwrap();
        assert_eq!(lib.root, "src");
        assert!(lib.on_path);
    }

    #[test]
    fn missing_path_dependency_is_an_error() {
        let base = TempDir::new().unwrap();
        let root =
            manifest("[project]\nname = \"p\"\n\n[dependencies]\nmissing = { path = \"nope\" }\n");
        let err = resolve(&root, base.path(), &FakeFetcher::new()).unwrap_err();
        assert!(matches!(err, ResolveError::Fetch { .. }), "{err:?}");
    }

    #[test]
    fn transitive_path_dependency_is_rejected() {
        // A fetched (untrusted) manifest must not be able to reference a local path.
        let root = manifest(
            "[project]\nname = \"p\"\n\n[dependencies]\nA = { git = \"https://h/a\", rev = \"a1\" }\n",
        );
        let fetcher = FakeFetcher::new().pkg(
            "https://h/a",
            &[("a1", "a1")],
            Some("[project]\nname = \"a\"\n\n[dependencies]\nevil = { path = \"../../etc\" }\n"),
        );
        let err = resolve(&root, Path::new("."), &fetcher).unwrap_err();
        assert!(
            matches!(&err, ResolveError::Unsupported(m) if m.contains("only supported in the root")),
            "{err:?}"
        );
    }
}
