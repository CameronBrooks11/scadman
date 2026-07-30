//! The `scadman` command-line interface.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use scadman_core::{
    Dependency, Environment, Fetched, Fetcher, GitDependency, GitFetcher, Installed, Lockfile,
    Manifest, Store, build_environment, canonical_url, fetch_git, lock_staleness, lockfile,
    manifest, resolve, unmet_imports,
};
use serde::Serialize;

#[derive(Parser)]
#[command(
    name = "scadman",
    version,
    about = "Project-oriented OpenSCAD dependency manager"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a scadman.toml in the current directory.
    Init {
        /// Project name (defaults to the current directory name).
        #[arg(long)]
        name: Option<String>,
    },
    /// Add a git or local-path dependency to scadman.toml.
    Add {
        /// Name to expose the dependency under (the `<Name/…>` include prefix).
        name: String,
        /// Git URL of the dependency (omit when using --path).
        git: Option<String>,
        /// Local directory to depend on instead of a git source (e.g. `../mylib`). Accepts
        /// `--root`/`--on-path` like a git source; not a git URL or ref.
        #[arg(long, conflicts_with_all = ["git", "rev", "tag", "branch"])]
        path: Option<String>,
        /// Pin to an exact commit.
        #[arg(long)]
        rev: Option<String>,
        /// Pin to a tag.
        #[arg(long)]
        tag: Option<String>,
        /// Track a branch (locked to a commit at lock time).
        #[arg(long)]
        branch: Option<String>,
        /// Library-root subdir to expose (for libraries whose code lives under e.g. `src/`).
        #[arg(long)]
        root: Option<String>,
        /// Also place the exposed root on OPENSCADPATH (opt in for libraries like dotSCAD
        /// whose files import from their own root).
        #[arg(long)]
        on_path: bool,
    },
    /// Remove a dependency from scadman.toml.
    Remove {
        /// Name of the dependency to remove.
        name: String,
    },
    /// List declared dependencies and their locked state.
    List,
    /// Resolve dependencies and write scadman.lock.
    Lock,
    /// Advance dependencies to newer commits and rewrite scadman.lock, reporting what moved.
    ///
    /// With no arguments, re-resolves every dependency (branch/tag deps move to the current
    /// tip; exact `rev` pins can't move). With names, advances only those and holds the rest
    /// at their locked revisions.
    Update {
        /// Dependencies to advance (default: all).
        names: Vec<String>,
    },
    /// Materialize the project environment from the lockfile (resolving if needed).
    Sync,
    /// Print the project's OPENSCADPATH (point your editor/OpenSCAD at it).
    Env {
        /// Emit a machine-readable JSON report instead of the path.
        #[arg(long)]
        json: bool,
        /// Write the search path into .vscode/settings.json for the OpenSCAD language server.
        #[arg(long, conflicts_with = "json")]
        write_vscode: bool,
    },
    /// Report on the project setup (OpenSCAD, store, manifest, lock, environment).
    Doctor,
    /// Print the resolved dependency graph (from scadman.lock).
    Graph {
        /// Emit a machine-readable JSON graph instead of a tree.
        #[arg(long)]
        json: bool,
    },
    /// Sync, then run OpenSCAD with the project environment on OPENSCADPATH.
    Run {
        /// Arguments passed through to openscad (e.g. the .scad file).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Init { name } => init(name),
        Command::Add {
            name,
            git,
            path,
            rev,
            tag,
            branch,
            root,
            on_path,
        } => add_cmd(name, git, path, rev, tag, branch, root, on_path),
        Command::Remove { name } => remove_cmd(name),
        Command::List => list_cmd(),
        Command::Lock => lock_cmd(),
        Command::Update { names } => update_cmd(names),
        Command::Sync => sync_cmd().map(|_| ()),
        Command::Env { json, write_vscode } => env_cmd(json, write_vscode),
        Command::Doctor => doctor_cmd(),
        Command::Graph { json } => graph_cmd(json),
        Command::Run { args } => run_cmd(args),
    }
}

fn init(name: Option<String>) -> Result<()> {
    let path = Path::new(manifest::MANIFEST_FILE);
    if path.exists() {
        bail!("{} already exists", manifest::MANIFEST_FILE);
    }

    let name = match name {
        Some(name) => name,
        None => current_dir_name().unwrap_or_else(|| "project".to_string()),
    };

    let manifest = Manifest::new(name);
    let text = manifest.to_toml().context("serialize manifest")?;
    fs::write(path, text).with_context(|| format!("write {}", manifest::MANIFEST_FILE))?;
    println!("Created {}", manifest::MANIFEST_FILE);
    if ensure_gitignore()? {
        println!("Added `.scadman/` to .gitignore");
    }
    Ok(())
}

/// Ensure `.gitignore` ignores the machine-local `.scadman/` env dir. Returns whether the
/// file was modified.
fn ensure_gitignore() -> Result<bool> {
    let path = Path::new(".gitignore");
    let current = fs::read_to_string(path).unwrap_or_default();
    let already = current
        .lines()
        .map(str::trim)
        .any(|l| l == ".scadman/" || l == ".scadman" || l == "/.scadman/");
    if already {
        return Ok(false);
    }
    let mut content = current;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(".scadman/\n");
    fs::write(path, content).context("update .gitignore")?;
    Ok(true)
}

fn remove_cmd(name: String) -> Result<()> {
    let mut manifest = load_manifest()?;
    if manifest.dependencies.remove(&name).is_none() {
        bail!(
            "`{name}` is not a dependency in {}",
            manifest::MANIFEST_FILE
        );
    }
    let text = manifest.to_toml().context("serialize manifest")?;
    fs::write(manifest::MANIFEST_FILE, text)
        .with_context(|| format!("write {}", manifest::MANIFEST_FILE))?;
    println!(
        "Removed `{name}` from {}. Run `scadman lock`.",
        manifest::MANIFEST_FILE
    );
    Ok(())
}

fn list_cmd() -> Result<()> {
    let manifest = load_manifest()?;
    if manifest.dependencies.is_empty() {
        println!("No dependencies declared.");
        return Ok(());
    }
    let locked = locked_revs();
    for (name, dep) in &manifest.dependencies {
        let spec = match dep {
            Dependency::Git(g) => {
                let reference = g
                    .rev
                    .as_deref()
                    .or(g.tag.as_deref())
                    .or(g.branch.as_deref())
                    .unwrap_or("?");
                format!("{} @ {reference}", g.git)
            }
            Dependency::Version(v) => format!("version {v}"),
            Dependency::Path(p) => format!("path {}", p.path),
        };
        match locked.get(name) {
            Some(rev) => println!("{name}  {spec}  (locked {})", short(rev)),
            None => println!("{name}  {spec}  (not locked)"),
        }
    }
    Ok(())
}

/// Locked name → resolved rev, from an existing lockfile (empty if none/unreadable).
fn locked_revs() -> BTreeMap<String, String> {
    if !Path::new(lockfile::LOCKFILE_FILE).exists() {
        return BTreeMap::new();
    }
    let Ok(text) = fs::read_to_string(lockfile::LOCKFILE_FILE) else {
        return BTreeMap::new();
    };
    match Lockfile::from_toml(&text) {
        Ok(lock) => lock.packages.into_iter().map(|p| (p.name, p.rev)).collect(),
        Err(_) => BTreeMap::new(),
    }
}

fn short(rev: &str) -> &str {
    &rev[..rev.len().min(12)]
}

fn env_cmd(json: bool, write_vscode: bool) -> Result<()> {
    let store = open_store()?;
    let (lock, env) = prepare_environment(&store)?;
    let openscadpath = env
        .openscad_path_value()
        .context("assemble OPENSCADPATH")?
        .to_string_lossy()
        .into_owned();
    if write_vscode {
        write_vscode_settings(&openscadpath)?;
        return Ok(());
    }
    if json {
        let report = EnvReport {
            openscadpath,
            packages: lock
                .packages
                .iter()
                .map(|p| EnvPackage {
                    name: p.name.clone(),
                    source: p.source.clone(),
                    rev: p.rev.clone(),
                    hash: p.hash.clone(),
                    store_path: store.path_for(&p.hash).join(&p.root).display().to_string(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{openscadpath}");
    }
    Ok(())
}

/// A machine-readable view of the project environment, for editors/tooling.
#[derive(Serialize)]
struct EnvReport {
    openscadpath: String,
    packages: Vec<EnvPackage>,
}

#[derive(Serialize)]
struct EnvPackage {
    name: String,
    source: String,
    rev: String,
    hash: String,
    store_path: String,
}

/// Write the project search path into `.vscode/settings.json` for the OpenSCAD language
/// server (`openscad.search_paths`), merging into any existing settings.
fn write_vscode_settings(openscadpath: &str) -> Result<()> {
    let dir = Path::new(".vscode");
    fs::create_dir_all(dir).context("create .vscode")?;
    let path = dir.join("settings.json");

    let mut settings = if path.exists() {
        let text = fs::read_to_string(&path)?;
        serde_json::from_str(&text).with_context(|| {
            format!(
                "{} is not plain JSON (comments/JSONC aren't supported here) — add \
                 `openscad.search_paths` manually",
                path.display()
            )
        })?
    } else {
        serde_json::json!({})
    };
    let object = settings
        .as_object_mut()
        .context(".vscode/settings.json is not a JSON object")?;
    // Write the path relative to the workspace (VS Code expands `${workspaceFolder}`), so the
    // committed settings work on any checkout instead of pinning this machine's absolute path.
    let value = workspace_relative(openscadpath);
    object.insert(
        "openscad.search_paths".to_string(),
        serde_json::Value::String(value),
    );

    fs::write(
        &path,
        format!("{}\n", serde_json::to_string_pretty(&settings)?),
    )
    .with_context(|| format!("write {}", path.display()))?;
    println!("Wrote `openscad.search_paths` to {}", path.display());
    Ok(())
}

/// Rewrite an `OPENSCADPATH` (absolute, under the current project) to use VS Code's
/// `${workspaceFolder}` token, so the written settings are portable across checkouts. Paths
/// outside the project (unusual) are left absolute.
fn workspace_relative(openscadpath: &str) -> String {
    let Ok(cwd) = std::env::current_dir() else {
        return openscadpath.to_string();
    };
    workspace_relative_to(openscadpath, &cwd.to_string_lossy())
}

fn workspace_relative_to(openscadpath: &str, cwd: &str) -> String {
    openscadpath
        .split(':')
        .map(|p| match p.strip_prefix(cwd) {
            // Only at a path-component boundary, so a sibling like `/a/proj-shared` is left
            // absolute when cwd is `/a/proj` (a raw prefix would mangle it).
            Some(rest) if rest.is_empty() || rest.starts_with('/') => {
                format!("${{workspaceFolder}}{rest}")
            }
            _ => p.to_string(),
        })
        .collect::<Vec<_>>()
        .join(":")
}

fn doctor_cmd() -> Result<()> {
    let installed = openscad_version();
    match &installed {
        Some(version) => println!("openscad:     {version}"),
        None => println!("openscad:     not found on PATH — `scadman run` will fail"),
    }
    match Store::default_root() {
        Some(root) => println!(
            "store:        {}{}",
            root.display(),
            if root.exists() { "" } else { " (empty)" }
        ),
        None => println!("store:        unavailable ($XDG_DATA_HOME and $HOME unset)"),
    }

    let manifest = load_manifest().ok();
    match &manifest {
        Some(m) => {
            let n = m.dependencies.len();
            println!(
                "manifest:     {} ({n} {})",
                manifest::MANIFEST_FILE,
                plural(n, "dependency", "dependencies")
            )
        }
        None => println!(
            "manifest:     {} not found — run `scadman init`",
            manifest::MANIFEST_FILE
        ),
    }

    if let Some(m) = &manifest {
        if let Some(required) = &m.project.openscad {
            let req = required.trim_start_matches(['>', '=', ' ']);
            let line = if manifest::parse_requirement(required).is_none() {
                format!(
                    "OpenSCAD requirement `{required}` unrecognized — use e.g. `2021.01` or `>=2021.01`"
                )
            } else {
                match &installed {
                    None => format!("OpenSCAD ≥ {req} — but it is not on PATH"),
                    Some(have) => match manifest::meets_openscad_requirement(have, required) {
                        Some(true) => format!("OpenSCAD ≥ {req} (ok)"),
                        Some(false) => {
                            format!("OpenSCAD ≥ {req} — installed is older, update OpenSCAD")
                        }
                        None => format!("OpenSCAD ≥ {req} (couldn't read the installed version)"),
                    },
                }
            };
            println!("requires:     {line}");
        }
        report_lockfile(m);
        let tracking = m
            .dependencies
            .values()
            .filter(|d| matches!(d, Dependency::Git(g) if g.branch.is_some() || g.tag.is_some()))
            .count();
        if tracking > 0 {
            println!(
                "tracking:     {tracking} {} track{} a branch/tag — `scadman update` to advance",
                plural(tracking, "dependency", "dependencies"),
                if tracking == 1 { "s" } else { "" }
            );
        }
    }

    let env_dir = env_root()?;
    if env_dir.exists() {
        let count = fs::read_dir(&env_dir).map(Iterator::count).unwrap_or(0);
        println!(
            "environment:  {count} {} exposed at {}",
            plural(count, "package", "packages"),
            env_dir.display()
        );
    } else if manifest.is_some() {
        println!("environment:  not built — run `scadman sync`");
    } else {
        println!("environment:  not built");
    }
    Ok(())
}

/// The singular or plural noun for a count (e.g. `plural(1, "package", "packages")`).
fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

fn report_lockfile(manifest: &Manifest) {
    if !Path::new(lockfile::LOCKFILE_FILE).exists() {
        println!("lockfile:     not created — run `scadman lock`");
        return;
    }
    let lock = fs::read_to_string(lockfile::LOCKFILE_FILE)
        .ok()
        .and_then(|t| Lockfile::from_toml(&t).ok());
    match lock {
        Some(lock) => match lock_staleness(manifest, &lock) {
            Some(reason) => {
                println!("lockfile:     out of date ({reason}) — run `scadman lock`")
            }
            None => {
                let n = lock.packages.len();
                println!(
                    "lockfile:     {} ({n} {}, up to date)",
                    lockfile::LOCKFILE_FILE,
                    plural(n, "package", "packages")
                )
            }
        },
        None => println!("lockfile:     present but unreadable"),
    }
}

/// The `openscad --version` line, or `None` if it is not on `PATH`.
fn openscad_version() -> Option<String> {
    // OpenSCAD prints its version to stderr.
    let output = ProcCommand::new("openscad")
        .arg("--version")
        .output()
        .ok()?;
    let text = if output.stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout)
    } else {
        String::from_utf8_lossy(&output.stderr)
    };
    let line = text.lines().next().unwrap_or("").trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// The resolved dependency graph, as a tree rooted at the project (or JSON).
fn graph_cmd(json: bool) -> Result<()> {
    let manifest = load_manifest()?;
    let lock = load_lockfile()?;
    let packages: BTreeMap<&str, &lockfile::LockedPackage> =
        lock.packages.iter().map(|p| (p.name.as_str(), p)).collect();
    let roots: Vec<String> = manifest.dependencies.keys().cloned().collect();

    if json {
        let graph = GraphReport {
            project: manifest.project.name,
            roots,
            nodes: lock
                .packages
                .iter()
                .map(|p| GraphNode {
                    name: p.name.clone(),
                    source: p.source.clone(),
                    rev: p.rev.clone(),
                    dependencies: p.dependencies.clone(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&graph)?);
        return Ok(());
    }

    println!("{}", manifest.project.name);
    if roots.is_empty() {
        println!("(no dependencies)");
        return Ok(());
    }
    let mut visited = BTreeSet::new();
    let last = roots.len() - 1;
    for (i, root) in roots.iter().enumerate() {
        print_graph_node(root, "", i == last, &packages, &mut visited);
    }
    Ok(())
}

/// Recursively print one graph node and its dependencies as an indented tree. A node whose
/// subtree was already printed is shown as `(*)` and not re-expanded (also breaks cycles).
fn print_graph_node(
    name: &str,
    prefix: &str,
    last: bool,
    packages: &BTreeMap<&str, &lockfile::LockedPackage>,
    visited: &mut BTreeSet<String>,
) {
    let connector = if last { "└── " } else { "├── " };
    let Some(pkg) = packages.get(name) else {
        println!("{prefix}{connector}{name} (not locked)");
        return;
    };
    if visited.contains(name) && !pkg.dependencies.is_empty() {
        println!("{prefix}{connector}{name} @ {} (*)", short(&pkg.rev));
        return;
    }
    visited.insert(name.to_string());
    println!("{prefix}{connector}{name} @ {}", short(&pkg.rev));

    let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });
    let last_dep = pkg.dependencies.len().saturating_sub(1);
    for (i, dep) in pkg.dependencies.iter().enumerate() {
        print_graph_node(dep, &child_prefix, i == last_dep, packages, visited);
    }
}

/// The dependency graph as data: every locked node plus the project's direct roots.
#[derive(Serialize)]
struct GraphReport {
    project: String,
    roots: Vec<String>,
    nodes: Vec<GraphNode>,
}

#[derive(Serialize)]
struct GraphNode {
    name: String,
    source: String,
    rev: String,
    dependencies: Vec<String>,
}

/// Load `scadman.lock`, erroring with a nudge to `scadman lock` if it is missing.
fn load_lockfile() -> Result<Lockfile> {
    let text = fs::read_to_string(lockfile::LOCKFILE_FILE)
        .with_context(|| format!("{} not found — run `scadman lock`", lockfile::LOCKFILE_FILE))?;
    Lockfile::from_toml(&text).with_context(|| format!("parse {}", lockfile::LOCKFILE_FILE))
}

#[allow(clippy::too_many_arguments)]
fn add_cmd(
    name: String,
    git: Option<String>,
    path: Option<String>,
    rev: Option<String>,
    tag: Option<String>,
    branch: Option<String>,
    root: Option<String>,
    on_path: bool,
) -> Result<()> {
    // Load the manifest first, so a bare `add` in a directory with no `scadman.toml` fails
    // fast rather than after a network round-trip to resolve the default branch.
    let mut manifest = load_manifest()?;

    // A git source given with no ref defaults to the remote's current default branch, like
    // `git clone` — most OpenSCAD libraries publish no releases, so branch-tracking is the
    // common case (see docs/ecosystem-survey.md). The branch is locked to a commit at lock
    // time, and `scadman update` advances it.
    let mut defaulted = None;
    let branch =
        if git.is_some() && path.is_none() && rev.is_none() && tag.is_none() && branch.is_none() {
            let url = git.as_deref().unwrap();
            let resolved = default_branch(url).with_context(|| {
                format!("determine the default branch of `{url}` — pass --branch, --tag, or --rev")
            })?;
            defaulted = Some(resolved.clone());
            Some(resolved)
        } else {
            branch
        };

    let updated = add_dependency(
        &mut manifest,
        &name,
        git,
        path,
        rev,
        tag,
        branch,
        root,
        on_path,
    )?;
    let text = manifest.to_toml().context("serialize manifest")?;
    fs::write(manifest::MANIFEST_FILE, text)
        .with_context(|| format!("write {}", manifest::MANIFEST_FILE))?;
    let verb = if updated { "Updated" } else { "Added" };
    match &defaulted {
        Some(b) => println!(
            "{verb} `{name}` in {} (tracking default branch `{b}`). Run `scadman lock` to resolve.",
            manifest::MANIFEST_FILE
        ),
        None => println!(
            "{verb} `{name}` in {}. Run `scadman lock` to resolve.",
            manifest::MANIFEST_FILE
        ),
    }
    Ok(())
}

/// The remote's current default branch (the target of its `HEAD` symref), via
/// `git ls-remote`. Used when `scadman add <url>` is given no explicit ref.
fn default_branch(url: &str) -> Result<String> {
    let output = ProcCommand::new("git")
        .args(["ls-remote", "--symref", "--", url, "HEAD"])
        .output()
        .context("run `git ls-remote` (is git on PATH?)")?;
    if !output.status.success() {
        bail!(
            "git ls-remote failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // A symref line looks like: `ref: refs/heads/main\tHEAD`.
    text.lines()
        .find_map(|line| line.strip_prefix("ref: refs/heads/"))
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("`{url}` advertises no default branch"))
}

/// Insert (or replace) a dependency in the manifest. Returns whether an existing entry was
/// replaced. Either `--path` (a local directory) or a git URL with exactly one of
/// `rev`/`tag`/`branch` must be given, not both.
#[allow(clippy::too_many_arguments)]
fn add_dependency(
    manifest: &mut Manifest,
    name: &str,
    git: Option<String>,
    path: Option<String>,
    rev: Option<String>,
    tag: Option<String>,
    branch: Option<String>,
    root: Option<String>,
    on_path: bool,
) -> Result<bool> {
    manifest::validate_package_name(name).map_err(|e| anyhow::anyhow!(e))?;

    if let Some(root) = &root {
        manifest::validate_library_root(root).map_err(|e| anyhow::anyhow!(e))?;
    }
    let dep = match (git, path) {
        (Some(_), Some(_)) => bail!("specify a git URL or --path, not both"),
        (None, Some(path)) => Dependency::Path(manifest::PathDependency {
            path,
            root,
            on_path,
        }),
        (Some(git), None) => {
            let refs = [&rev, &tag, &branch].iter().filter(|r| r.is_some()).count();
            if refs != 1 {
                bail!("specify exactly one of --rev, --tag, or --branch");
            }
            Dependency::Git(GitDependency {
                git,
                rev,
                tag,
                branch,
                root,
                on_path,
            })
        }
        (None, None) => bail!("provide a git URL or --path <dir>"),
    };
    Ok(manifest
        .dependencies
        .insert(name.to_string(), dep)
        .is_some())
}

fn lock_cmd() -> Result<()> {
    let manifest = load_manifest()?;
    let store = open_store()?;
    let lock = resolve_lock(&manifest, &store)?;
    write_lock(&lock)?;
    println!(
        "Locked {} package(s) → {}",
        lock.packages.len(),
        lockfile::LOCKFILE_FILE
    );
    Ok(())
}

/// Advance dependencies to newer commits, holding the ones not named at their locked revs,
/// and report what moved. `names` empty means advance all.
fn update_cmd(names: Vec<String>) -> Result<()> {
    let manifest = load_manifest()?;
    let store = open_store()?;
    let base = std::env::current_dir().context("determine project directory")?;
    let previous = read_lockfile_opt()?;

    let updated = if names.is_empty() {
        // Advance everything — a full fresh re-resolve, identical to `scadman lock` (which
        // already moves branch/tag deps, transitive ones included), plus a delta report.
        resolve_lock(&manifest, &store)?
    } else {
        // Advance only the named deps; hold the rest at their locked revisions. Named deps
        // must exist and be git sources (a path dep is re-read on every sync, not "updated";
        // a rev pin is accepted but simply can't move).
        let mut refresh = HashSet::new();
        for name in &names {
            match manifest.dependencies.get(name) {
                Some(Dependency::Git(g)) => {
                    refresh.insert(canonical_url(&g.git));
                }
                Some(Dependency::Path(_)) => bail!(
                    "`{name}` is a path dependency — its content is re-read on every `sync`, not updated"
                ),
                Some(Dependency::Version(_)) => {
                    bail!(
                        "`{name}` is a registry (version) dependency, which needs a registry (not in v1)"
                    )
                }
                None => bail!(
                    "`{name}` is not a dependency in {}",
                    manifest::MANIFEST_FILE
                ),
            }
        }
        let base_lock = previous.clone().unwrap_or_else(Lockfile::new);
        let fetcher = CachedGitFetcher::new(&store, &base_lock, refresh);
        resolve(&manifest, &base, &fetcher)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .to_lockfile()
    };

    write_lock(&updated)?;
    report_update(previous.as_ref(), &updated);
    Ok(())
}

/// Read `scadman.lock` if it exists and parses (else `None`).
fn read_lockfile_opt() -> Result<Option<Lockfile>> {
    if !Path::new(lockfile::LOCKFILE_FILE).exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(lockfile::LOCKFILE_FILE)?;
    Ok(Some(Lockfile::from_toml(&text).with_context(|| {
        format!("parse {}", lockfile::LOCKFILE_FILE)
    })?))
}

/// Print the per-package rev deltas between the previous lock and the updated one.
fn report_update(previous: Option<&Lockfile>, updated: &Lockfile) {
    let prior = |name: &str| previous.and_then(|l| l.packages.iter().find(|p| p.name == name));
    let mut moved = 0;
    for pkg in &updated.packages {
        match prior(&pkg.name) {
            Some(before) if before.rev != pkg.rev => {
                println!(
                    "Updated {}: {} → {}",
                    pkg.name,
                    short(&before.rev),
                    short(&pkg.rev)
                );
                moved += 1;
            }
            Some(_) => {}
            None => {
                println!("Added {} @ {}", pkg.name, short(&pkg.rev));
                moved += 1;
            }
        }
    }
    if let Some(previous) = previous {
        for before in &previous.packages {
            if !updated.packages.iter().any(|p| p.name == before.name) {
                println!("Removed {}", before.name);
                moved += 1;
            }
        }
    }
    if moved == 0 {
        println!("Already up to date.");
    }
}

/// Resolve-or-read the lock, ensure content is stored, build the environment, and warn
/// about undeclared imports (to stderr). Prints nothing to stdout, so `env` can emit only
/// its report.
fn prepare_environment(store: &Store) -> Result<(Lockfile, Environment)> {
    let lock = read_or_lock(store)?;
    ensure_stored(&lock, store)?;
    let env = build_environment(&lock, store, &env_root()?).context("build environment")?;
    warn_unmet_imports(&lock, &env);
    warn_on_path_collisions(&env);
    Ok((lock, env))
}

/// Warn if two `on_path` libraries expose a top-level file of the same name — a bare
/// import of it is ambiguous (the flat-namespace risk `on_path` opts into).
fn warn_on_path_collisions(env: &Environment) {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    // openscad_path[0] is the env root; the rest are the on_path package dirs.
    for dir in env.openscad_path.iter().skip(1) {
        let label = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let file = entry.file_name();
            let Some(file) = file.to_str() else { continue };
            if !file.ends_with(".scad") {
                continue;
            }
            match seen.get(file) {
                Some(prev) if prev != &label => eprintln!(
                    "warning: on-path libraries `{prev}` and `{label}` both provide `{file}` — a bare import is ambiguous"
                ),
                Some(_) => {}
                None => {
                    seen.insert(file.to_string(), label.clone());
                }
            }
        }
    }
}

fn sync_cmd() -> Result<Environment> {
    let store = open_store()?;
    let (_, env) = prepare_environment(&store)?;
    println!(
        "Synced {} package(s) → {}",
        env.exposed.len(),
        env.root.display()
    );
    Ok(env)
}

fn run_cmd(args: Vec<String>) -> Result<()> {
    let store = open_store()?;
    warn_openscad_too_old();
    let (_, env) = prepare_environment(&store)?;
    // Point OPENSCADPATH at the project env (plus any on_path library roots) so declared
    // dependencies shadow globals. (OpenSCAD still searches its built-in user/install
    // dirs — the include-scan surfaces undeclared imports.)
    let openscadpath = env.openscad_path_value().context("assemble OPENSCADPATH")?;
    let status = ProcCommand::new("openscad")
        .env("OPENSCADPATH", &openscadpath)
        .args(&args)
        .status()
        .context("launch openscad (is it on PATH?)")?;
    if !status.success() {
        bail!("openscad exited with {status}");
    }
    Ok(())
}

/// Warn (to stderr) if the project declares a minimum OpenSCAD version the installed one
/// doesn't meet. Advisory only — the render may still work, and compatibility is really
/// feature-based (see `docs/ecosystem-survey.md`), so this never blocks the run.
fn warn_openscad_too_old() {
    let Ok(manifest) = load_manifest() else {
        return;
    };
    let Some(required) = manifest.project.openscad else {
        return;
    };
    if let Some(have) = openscad_version()
        && manifest::meets_openscad_requirement(&have, &required) == Some(false)
    {
        let req = required.trim_start_matches(['>', '=', ' ']);
        eprintln!(
            "warning: this project asks for OpenSCAD ≥ {req}, but you have `{have}` — update if a render fails"
        );
    }
}

fn load_manifest() -> Result<Manifest> {
    let text = fs::read_to_string(manifest::MANIFEST_FILE)
        .with_context(|| format!("read {} (run `scadman init`?)", manifest::MANIFEST_FILE))?;
    Manifest::from_toml(&text).with_context(|| format!("parse {}", manifest::MANIFEST_FILE))
}

fn open_store() -> Result<Store> {
    let root = Store::default_root()
        .context("cannot locate a store ($XDG_DATA_HOME and $HOME are unset)")?;
    Ok(Store::new(root))
}

fn env_root() -> Result<PathBuf> {
    Ok(std::env::current_dir()?.join(".scadman").join("env"))
}

fn resolve_lock(manifest: &Manifest, store: &Store) -> Result<Lockfile> {
    let fetcher = GitFetcher::new(store.clone());
    let base = std::env::current_dir().context("determine project directory")?;
    let resolved = resolve(manifest, &base, &fetcher).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(resolved.to_lockfile())
}

/// Whether the manifest has any path dependency (their content is mutable, so a lockfile
/// entry can silently go stale — see [`read_or_lock`]).
fn has_path_dependency(manifest: &Manifest) -> bool {
    manifest
        .dependencies
        .values()
        .any(|d| matches!(d, Dependency::Path(_)))
}

/// A fetcher that holds already-locked git sources at their pinned revision — served from
/// the store without touching the network — and only performs a real fetch for a git source
/// the lockfile doesn't already pin. Path sources always re-read from disk.
///
/// This is what a path-triggered refresh uses (see [`read_or_lock`]): a path dependency's
/// content must be re-read, but the git dependencies alongside it must NOT be re-fetched or
/// silently moved off their locked commits — so `sync`/`run` keep working offline.
struct CachedGitFetcher {
    inner: GitFetcher,
    store: Store,
    /// Git identity (canonical URL) → its locked `(rev, hash)`.
    locked: HashMap<String, (String, String)>,
    /// Git identities to force a fresh fetch for, bypassing the cache — how `scadman update`
    /// advances selected deps (empty for a path-triggered refresh, which holds all git deps).
    refresh: HashSet<String>,
}

impl CachedGitFetcher {
    fn new(store: &Store, lock: &Lockfile, refresh: HashSet<String>) -> Self {
        let locked = lock
            .packages
            .iter()
            .filter(|p| !p.source.starts_with(lockfile::PATH_SOURCE_PREFIX))
            .map(|p| (p.source.clone(), (p.rev.clone(), p.hash.clone())))
            .collect();
        Self {
            inner: GitFetcher::new(store.clone()),
            store: store.clone(),
            locked,
            refresh,
        }
    }
}

impl Fetcher for CachedGitFetcher {
    fn fetch(&self, url: &str, reference: &str) -> io::Result<Fetched> {
        if !self.refresh.contains(url)
            && let Some((rev, hash)) = self.locked.get(url)
            && holds_reference(reference, rev)
        {
            let path = self.store.path_for(hash);
            if path.exists() {
                return Ok(Fetched {
                    rev: rev.clone(),
                    hash: hash.clone(),
                    path,
                });
            }
        }
        self.inner.fetch(url, reference)
    }

    fn fetch_path(&self, dir: &Path) -> io::Result<Fetched> {
        self.inner.fetch_path(dir)
    }
}

/// Whether serving the locked commit `rev` satisfies a request for `reference`, i.e. the
/// cache may hold this dep rather than re-fetch. A branch/tag name is intentionally held at
/// its locked commit (scadman doesn't chase upstream movement unless the dep is named for
/// `update`). But a request for a *different exact commit* — a changed `rev` pin, e.g. a
/// transitive dep whose parent moved — must be honored, or the lock would record a commit
/// its requester no longer asks for.
fn holds_reference(reference: &str, rev: &str) -> bool {
    let is_commit = reference.len() >= 7 && reference.chars().all(|c| c.is_ascii_hexdigit());
    !is_commit || rev.starts_with(reference)
}

fn read_or_lock(store: &Store) -> Result<Lockfile> {
    let manifest = load_manifest()?;
    if Path::new(lockfile::LOCKFILE_FILE).exists() {
        let text = fs::read_to_string(lockfile::LOCKFILE_FILE)?;
        let lock = Lockfile::from_toml(&text)
            .with_context(|| format!("parse {}", lockfile::LOCKFILE_FILE))?;
        if lock.version != lockfile::LOCKFILE_VERSION {
            bail!(
                "{} is lockfile version {} but this scadman understands {} — upgrade scadman",
                lockfile::LOCKFILE_FILE,
                lock.version,
                lockfile::LOCKFILE_VERSION
            );
        }
        // A path dependency tracks a local directory's current content, which the lockfile
        // cannot pin. Re-resolve so an edit to a sibling library is picked up (the point of
        // co-developing with path deps). Git dependencies are held at their locked revisions
        // and served from the store (CachedGitFetcher) — a path dep must not force the
        // network or silently move a branch/tag dep, so `sync`/`run` still work offline. The
        // lock is rewritten only when the resolution actually changed.
        if has_path_dependency(&manifest) {
            // Git deps still obey the lock: a changed git pin in the manifest needs an
            // explicit `scadman lock`, exactly as in a git-only project — a path dep must not
            // quietly disable that staleness check for the git deps beside it. (Branch/tag
            // upstream movement isn't chased here either, also as in a git-only project.)
            if let Some(reason) = lock_staleness(&manifest, &lock) {
                bail!(
                    "{} is out of date with {}: {reason}. Run `scadman lock`.",
                    lockfile::LOCKFILE_FILE,
                    manifest::MANIFEST_FILE
                );
            }
            let fetcher = CachedGitFetcher::new(store, &lock, HashSet::new());
            let base = std::env::current_dir().context("determine project directory")?;
            let refreshed = resolve(&manifest, &base, &fetcher)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .to_lockfile();
            if refreshed != lock {
                write_lock(&refreshed)?;
            }
            return Ok(refreshed);
        }
        if let Some(reason) = lock_staleness(&manifest, &lock) {
            bail!(
                "{} is out of date with {}: {reason}. Run `scadman lock`.",
                lockfile::LOCKFILE_FILE,
                manifest::MANIFEST_FILE
            );
        }
        return Ok(lock);
    }
    let lock = resolve_lock(&manifest, store)?;
    write_lock(&lock)?;
    Ok(lock)
}

fn write_lock(lock: &Lockfile) -> Result<()> {
    let text = lock.to_toml().context("serialize lockfile")?;
    fs::write(lockfile::LOCKFILE_FILE, text)
        .with_context(|| format!("write {}", lockfile::LOCKFILE_FILE))
}

/// Ensure every locked package's content is present in the store, fetching any that are
/// missing (a fresh checkout) and verifying it hashes to the pinned value.
fn ensure_stored(lock: &Lockfile, store: &Store) -> Result<()> {
    for package in &lock.packages {
        if store.path_for(&package.hash).exists() {
            continue;
        }
        // A path dependency has no remote to re-fetch from; its content is inserted at lock
        // time. If it is missing here (e.g. a store cleared out from under a committed lock),
        // re-locking re-reads the local directory — don't try to git-fetch a `path:` source.
        if package.source.starts_with(lockfile::PATH_SOURCE_PREFIX) {
            bail!(
                "path dependency `{}` content is not in the store — run `scadman lock`",
                package.name
            );
        }
        let acquired = fetch_git(store, &package.source, &package.rev)
            .with_context(|| format!("fetch {} @ {}", package.name, package.rev))?;
        if acquired.entry.hash != package.hash {
            bail!(
                "integrity: {} @ {} hashed to {} but the lockfile expects {}",
                package.name,
                package.rev,
                acquired.entry.hash,
                package.hash
            );
        }
    }
    Ok(())
}

fn warn_unmet_imports(lock: &Lockfile, env: &Environment) {
    let installed: Vec<Installed> = lock
        .packages
        .iter()
        .map(|p| Installed {
            name: p.name.clone(),
            path: env.root.join(&p.name),
        })
        .collect();
    for unmet in unmet_imports(Path::new("."), &installed) {
        eprintln!(
            "warning: `{}` imports `{}/` (in {}) but it is not in your dependencies",
            unmet.package, unmet.library, unmet.file
        );
    }
}

fn current_dir_name() -> Option<String> {
    std::env::current_dir()
        .ok()?
        .file_name()?
        .to_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_relative_uses_the_workspace_token() {
        let cwd = "/home/u/proj";
        // A path under the project becomes `${workspaceFolder}`-relative.
        assert_eq!(
            workspace_relative_to("/home/u/proj/.scadman/env", cwd),
            "${workspaceFolder}/.scadman/env"
        );
        // Multiple (on_path) entries are each rewritten; the separator is preserved.
        assert_eq!(
            workspace_relative_to(
                "/home/u/proj/.scadman/env:/home/u/proj/.scadman/env/dotSCAD",
                cwd
            ),
            "${workspaceFolder}/.scadman/env:${workspaceFolder}/.scadman/env/dotSCAD"
        );
        // A path outside the project is left absolute.
        assert_eq!(workspace_relative_to("/opt/global", cwd), "/opt/global");
        // A sibling that merely shares a string prefix must NOT be rewritten.
        assert_eq!(
            workspace_relative_to("/home/u/proj-shared/lib", cwd),
            "/home/u/proj-shared/lib"
        );
    }

    #[test]
    fn default_branch_errors_on_an_unreachable_url() {
        // A bad/offline URL must surface an error (add_cmd turns it into an actionable
        // "pass --branch/--tag/--rev"), not a wrong branch.
        assert!(default_branch("file:///nonexistent-scadman-repo-xyz").is_err());
    }

    #[test]
    fn plural_picks_the_right_noun() {
        assert_eq!(plural(1, "package", "packages"), "package");
        assert_eq!(plural(0, "package", "packages"), "packages");
        assert_eq!(plural(2, "package", "packages"), "packages");
    }

    #[test]
    fn holds_reference_holds_branches_but_honors_changed_pins() {
        let rev = "061fef7abc0000000000000000000000000000ab";
        // Branch/tag names are held at the locked commit.
        assert!(holds_reference("main", rev));
        assert!(holds_reference("v2.0.700", rev));
        // The same exact pin (full or abbreviated) is held — no needless re-fetch.
        assert!(holds_reference(rev, rev));
        assert!(holds_reference("061fef7", rev));
        // A different exact commit must NOT be held — it has to be re-fetched.
        assert!(!holds_reference(
            "deadbeef1234567890000000000000000000cafe",
            rev
        ));
        assert!(!holds_reference("deadbee", rev));
    }

    #[test]
    fn add_dependency_requires_exactly_one_ref() {
        let mut m = Manifest::new("p");
        assert!(
            add_dependency(
                &mut m,
                "A",
                Some("u".into()),
                None,
                None,
                None,
                None,
                None,
                false
            )
            .is_err()
        );
        assert!(
            add_dependency(
                &mut m,
                "A",
                Some("u".into()),
                None,
                Some("r".into()),
                Some("t".into()),
                None,
                None,
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn add_dependency_records_root_and_on_path() {
        let mut m = Manifest::new("p");
        let url = "https://github.com/JustinSDK/dotSCAD".to_string();
        add_dependency(
            &mut m,
            "dotSCAD",
            Some(url),
            None,
            Some("abc123".into()),
            None,
            None,
            Some("src".into()),
            true,
        )
        .unwrap();
        match &m.dependencies["dotSCAD"] {
            Dependency::Git(g) => {
                assert_eq!(g.root.as_deref(), Some("src"));
                assert!(g.on_path);
            }
            other => panic!("expected git dependency, got {other:?}"),
        }
        // A traversing library root is rejected.
        assert!(
            add_dependency(
                &mut m,
                "bad",
                Some("u".into()),
                None,
                Some("r".into()),
                None,
                None,
                Some("../etc".into()),
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn add_dependency_supports_path_form() {
        let mut m = Manifest::new("p");
        add_dependency(
            &mut m,
            "local",
            None,
            Some("../local".into()),
            None,
            None,
            None,
            None,
            false,
        )
        .unwrap();
        match &m.dependencies["local"] {
            Dependency::Path(p) => assert_eq!(p.path, "../local"),
            other => panic!("expected path dependency, got {other:?}"),
        }
        // Neither git nor path is an error; both is an error.
        assert!(add_dependency(&mut m, "x", None, None, None, None, None, None, false).is_err());
        assert!(
            add_dependency(
                &mut m,
                "x",
                Some("u".into()),
                Some("../p".into()),
                None,
                None,
                None,
                None,
                false,
            )
            .is_err()
        );
    }
}
