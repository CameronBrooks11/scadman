//! The `scadman` command-line interface.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use scadman_core::{
    Dependency, Environment, Fetched, Fetcher, GitDependency, GitFetcher, GitRef, Installed,
    Lockfile, Manifest, Store, build_environment, canonical_url, fetch_git, lock_staleness,
    lockfile, manifest, resolve, unfetched_submodules, unmet_imports, validate_content_hash,
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
        /// Local directory to depend on instead of a git source (e.g. `../mylib`);
        /// `--root`/`--on-path` apply as with a git source.
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
        /// Also fetch the repository's git submodules, at the commits it records.
        #[arg(long, conflicts_with = "path")]
        submodules: bool,
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
    Sync {
        /// Re-hash every locked library in the store, and restore any that no longer
        /// matches its locked hash. Without this, only a library whose stored copy has
        /// lost its write protection is re-hashed.
        #[arg(long)]
        verify: bool,
        /// Use scadman.lock exactly as it is: fail if it is missing or would change.
        #[arg(long)]
        locked: bool,
    },
    /// Print the project's OPENSCADPATH (point your editor/OpenSCAD at it).
    Env {
        /// Emit a machine-readable JSON report instead of the path.
        #[arg(long)]
        json: bool,
        /// Write the search path into .vscode/settings.json for the OpenSCAD language server.
        #[arg(long, conflicts_with = "json")]
        write_vscode: bool,
        /// Use scadman.lock exactly as it is: fail if it is missing or would change.
        #[arg(long)]
        locked: bool,
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
        /// Use scadman.lock exactly as it is: fail if it is missing or would change.
        /// Give it before the OpenSCAD arguments.
        #[arg(long)]
        locked: bool,
        /// Arguments passed through to openscad (e.g. the .scad file).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

fn main() -> Result<()> {
    // Rust starts a program ignoring SIGPIPE, so writing to a pipe whose reader has gone
    // (`scadman doctor | head -1`) fails, and `println!` panics on the failure. Take the
    // signal as other command-line tools do: end quietly.
    #[cfg(unix)]
    // SAFETY: called once, before any thread is started or anything is written; it sets
    // one signal back to its default action.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    match Cli::parse().command {
        Command::Init { name } => init_cmd(name),
        Command::Add {
            name,
            git,
            path,
            rev,
            tag,
            branch,
            root,
            on_path,
            submodules,
        } => add_cmd(name, git, path, rev, tag, branch, root, on_path, submodules),
        Command::Remove { name } => remove_cmd(name),
        Command::List => list_cmd(),
        Command::Lock => lock_cmd(),
        Command::Update { names } => update_cmd(names),
        Command::Sync { verify, locked } => sync_cmd(verify, locked).map(|_| ()),
        Command::Env {
            json,
            write_vscode,
            locked,
        } => env_cmd(json, write_vscode, locked),
        Command::Doctor => {
            if !doctor_cmd()? {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Graph { json } => graph_cmd(json),
        Command::Run { locked, args } => {
            let code = run_cmd(args, locked).unwrap_or_else(|error| {
                eprintln!("Error: {error:?}");
                RUN_FAILED_BEFORE_OPENSCAD
            });
            std::process::exit(code)
        }
    }
}

fn init_cmd(name: Option<String>) -> Result<()> {
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
    write_dependencies(&manifest)?;
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
    let locked = locked_revs()?;
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

/// Locked name → resolved rev, from an existing lockfile (empty if none/unreadable). A
/// lockfile of an unknown version is an error, not "nothing locked".
fn locked_revs() -> Result<BTreeMap<String, String>> {
    let Ok(text) = fs::read_to_string(lockfile::LOCKFILE_FILE) else {
        return Ok(BTreeMap::new());
    };
    check_lockfile_version(&text)?;
    Ok(match Lockfile::from_toml(&text) {
        Ok(lock) => lock.packages.into_iter().map(|p| (p.name, p.rev)).collect(),
        Err(_) => BTreeMap::new(),
    })
}

fn short(rev: &str) -> &str {
    &rev[..rev.len().min(12)]
}

fn env_cmd(json: bool, write_vscode: bool, locked: bool) -> Result<()> {
    let store = open_store()?;
    let (lock, env) = prepare_environment(&store, false, locked)?;
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
/// `${workspaceFolder}` token, so the written settings are portable across checkouts. The
/// part after the token always uses `/`, which VS Code reads on every OS, so settings
/// written on Windows work for a collaborator on Linux or macOS. Paths outside the project
/// (unusual) are left absolute.
fn workspace_relative(openscadpath: &str) -> String {
    let Ok(cwd) = std::env::current_dir() else {
        return openscadpath.to_string();
    };
    workspace_relative_to(openscadpath, &cwd.to_string_lossy())
}

/// The separator between `OPENSCADPATH` entries on this platform.
const PATH_LIST_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

fn workspace_relative_to(openscadpath: &str, cwd: &str) -> String {
    openscadpath
        .split(PATH_LIST_SEPARATOR)
        .map(|p| match p.strip_prefix(cwd) {
            // Only at a path-component boundary, so a sibling like `/a/proj-shared` is left
            // absolute when cwd is `/a/proj` (a raw prefix would mangle it).
            Some(rest) if rest.is_empty() || rest.starts_with(std::path::is_separator) => {
                let rest = if cfg!(windows) {
                    rest.replace('\\', "/")
                } else {
                    rest.to_string()
                };
                format!("${{workspaceFolder}}{rest}")
            }
            _ => p.to_string(),
        })
        .collect::<Vec<_>>()
        .join(&PATH_LIST_SEPARATOR.to_string())
}

/// Print the report, and say whether the project is in order: `false` when something
/// in it needs fixing before it can be relied on.
fn doctor_cmd() -> Result<bool> {
    let mut in_order = true;
    let installed = openscad_version();
    let program = openscad_program();
    match &installed {
        Some(version) if program.as_os_str() == "openscad" => println!("openscad:     {version}"),
        Some(version) => println!("openscad:     {version} ({})", program.display()),
        None => println!("openscad:     not found ({OPENSCAD_SEARCH}) — `scadman run` will fail"),
    }
    match Store::default_root() {
        Some(root) => println!(
            "store:        {}{}",
            root.display(),
            if root.exists() {
                ""
            } else {
                " (not created yet)"
            }
        ),
        None if cfg!(windows) => {
            println!("store:        unavailable ($XDG_DATA_HOME and %LOCALAPPDATA% unset)")
        }
        None => println!("store:        unavailable ($XDG_DATA_HOME and $HOME unset)"),
    }

    // Deliberately not `load_manifest().ok()`. That collapses "absent" and
    // "present but invalid" into the same `None`, so a typo in scadman.toml was
    // reported as "not found — run `scadman init`" — advice that then fails
    // with "scadman.toml already exists". Diagnosing exactly this is doctor's
    // job, so the three cases are kept apart.
    let manifest = match fs::read_to_string(manifest::MANIFEST_FILE) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            println!(
                "manifest:     {} not found — run `scadman init`",
                manifest::MANIFEST_FILE
            );
            in_order = false;
            None
        }
        Err(e) => {
            println!(
                "manifest:     {} cannot be read — {e}",
                manifest::MANIFEST_FILE
            );
            in_order = false;
            None
        }
        Ok(text) => match Manifest::from_root_toml(&text) {
            Ok(m) => {
                let n = m.dependencies.len();
                println!(
                    "manifest:     {} ({n} {})",
                    manifest::MANIFEST_FILE,
                    plural(n, "dependency", "dependencies")
                );
                Some(m)
            }
            Err(e) => {
                println!(
                    "manifest:     {} failed to parse — {e}",
                    manifest::MANIFEST_FILE
                );
                in_order = false;
                None
            }
        },
    };

    if let Some(m) = &manifest {
        if let Some(required) = &m.project.openscad {
            let req = required.trim_start_matches(['>', '=', ' ']);
            let line = if manifest::parse_requirement(required).is_none() {
                format!("`{required}` unrecognized — use e.g. `2021.01` or `>=2021.01`")
            } else {
                match &installed {
                    None => format!("OpenSCAD ≥ {req} — but it was not found"),
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
        let (lock, lock_in_order) = report_lockfile(m);
        in_order &= lock_in_order;
        if let Some(lock) = lock
            && let Some(root) = Store::default_root()
        {
            in_order &= report_content(&lock, &Store::new(root));
        }
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
    Ok(in_order)
}

/// The singular or plural noun for a count (e.g. `plural(1, "package", "packages")`).
fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

/// Print the `lockfile:` line. Hands back the lockfile if it could be read, and whether
/// it is in order: there when the manifest needs one, readable, and up to date.
fn report_lockfile(manifest: &Manifest) -> (Option<Lockfile>, bool) {
    if !Path::new(lockfile::LOCKFILE_FILE).exists() {
        println!("lockfile:     not created — run `scadman lock`");
        // A project with no dependencies has nothing to lock yet.
        return (None, manifest.dependencies.is_empty());
    }
    let text = fs::read_to_string(lockfile::LOCKFILE_FILE).ok();
    if let Some(Err(e)) = text.as_deref().map(check_lockfile_version) {
        println!("lockfile:     {e}");
        return (None, false);
    }
    let lock = text.and_then(|t| Lockfile::from_toml(&t).ok());
    let in_order = match &lock {
        Some(lock) => match lock_staleness(manifest, lock) {
            Some(reason) => {
                println!("lockfile:     out of date ({reason}) — run `scadman lock`");
                false
            }
            None => {
                let n = lock.packages.len();
                println!(
                    "lockfile:     {} ({n} {}, up to date)",
                    lockfile::LOCKFILE_FILE,
                    plural(n, "package", "packages")
                );
                true
            }
        },
        None => {
            println!("lockfile:     present but unreadable");
            false
        }
    };
    (lock, in_order)
}

/// Print the `content:` line: whether each locked library has a copy in the store that
/// still hashes to what the lockfile pins. Returns whether all of them do.
fn report_content(lock: &Lockfile, store: &Store) -> bool {
    let mut checked = 0;
    let mut modified = Vec::new();
    let mut missing = Vec::new();
    for package in &lock.packages {
        // The hash becomes a path, and this lockfile has not been through `parse_lockfile`.
        if validate_content_hash(&package.hash).is_err() {
            continue;
        }
        if !store.path_for(&package.hash).exists() {
            missing.push(format!("`{}`", package.name));
            continue;
        }
        checked += 1;
        if !store.verify(&package.hash).unwrap_or(false) {
            modified.push(format!("`{}`", package.name));
        }
    }
    if !modified.is_empty() {
        println!(
            "content:      {} modified in the store — run `scadman sync --verify` to restore",
            modified.join(", ")
        );
    } else if !missing.is_empty() {
        println!(
            "content:      {} not in the store — run `scadman sync` to fetch",
            missing.join(", ")
        );
    } else if checked > 0 {
        let total = lock.packages.len();
        println!(
            "content:      {checked} of {total} locked {} in the store, unmodified",
            plural(total, "package", "packages")
        );
    }
    modified.is_empty() && missing.is_empty()
}

/// Where scadman looks for OpenSCAD, for messages that say it was not found.
const OPENSCAD_SEARCH: &str = if cfg!(windows) {
    "$OPENSCAD, PATH, or %ProgramFiles%\\OpenSCAD*"
} else {
    "$OPENSCAD or PATH"
};

/// The OpenSCAD to run: `$OPENSCAD` if it is set, else `openscad` on `PATH`. On Windows,
/// failing those, the installer's folder under `%ProgramFiles%`, which it does not add to
/// `PATH` (#133).
fn openscad_program() -> PathBuf {
    if let Some(program) = std::env::var_os("OPENSCAD").filter(|v| !v.is_empty()) {
        return PathBuf::from(program);
    }
    if cfg!(windows) && !on_path("openscad.exe") {
        let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|name| std::env::var_os(name).filter(|v| !v.is_empty()))
            .map(PathBuf::from)
            .collect();
        if let Some(found) = installed_openscad(&roots) {
            return found;
        }
    }
    PathBuf::from("openscad")
}

/// Whether a file named `exe` is in a directory on `PATH`.
fn on_path(exe: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(exe).is_file()))
}

/// `<root>/OpenSCAD*/openscad.exe` under the first root that has one. The folders are
/// tried in sorted order, so the installer's `OpenSCAD` comes before longer names such as
/// `OpenSCAD (Nightly)`, of which it is a prefix.
fn installed_openscad(roots: &[PathBuf]) -> Option<PathBuf> {
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let mut dirs: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("OpenSCAD"))
            .collect();
        dirs.sort();
        if let Some(exe) = dirs
            .iter()
            .map(|name| root.join(name).join("openscad.exe"))
            .find(|exe| exe.is_file())
        {
            return Some(exe);
        }
    }
    None
}

/// The `openscad --version` line, or `None` if OpenSCAD was not found.
fn openscad_version() -> Option<String> {
    // OpenSCAD prints its version to stderr.
    let output = ProcCommand::new(openscad_program())
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
    parse_lockfile(&text)
}

/// Refuse lockfile text that declares a version this scadman does not understand. Every
/// reader goes through this, so an older scadman never acts on — or rewrites — a newer
/// lockfile. Text with no readable version passes; parsing reports what is wrong with it.
fn check_lockfile_version(text: &str) -> Result<()> {
    if let Some(found) = lockfile::declared_version(text)
        && !(1..=lockfile::LOCKFILE_VERSION).contains(&found)
    {
        bail!(
            "{} is lockfile version {found} but this scadman understands up to {} — upgrade scadman",
            lockfile::LOCKFILE_FILE,
            lockfile::LOCKFILE_VERSION
        );
    }
    Ok(())
}

/// Parse lockfile text, checking its version first (see [`check_lockfile_version`]).
fn parse_lockfile(text: &str) -> Result<Lockfile> {
    check_lockfile_version(text)?;
    let lock =
        Lockfile::from_toml(text).with_context(|| format!("parse {}", lockfile::LOCKFILE_FILE))?;
    // A hash becomes a path under the store root. Refuse a malformed one here, where the
    // lockfile is read, so nothing probes or links a path built from it.
    for package in &lock.packages {
        validate_content_hash(&package.hash)
            .map_err(|reason| anyhow::anyhow!(reason))
            .with_context(|| format!("{}: package `{}`", lockfile::LOCKFILE_FILE, package.name))?;
    }
    Ok(lock)
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
    submodules: bool,
) -> Result<()> {
    // `scadman add <url>` is the natural first try for anyone coming from npm/cargo/pip.
    // The name-validation error it would otherwise hit reads as if the URL were malformed,
    // so say what is wrong instead: the URL is in the NAME position. No name is inferred —
    // it becomes the `<Name/…>` include prefix, which is the user's choice to make.
    if looks_like_git_url(&name) {
        let usage = match suggested_name(&name) {
            Some(n) => format!("scadman add {n} {name}"),
            None => format!("scadman add <NAME> {name}"),
        };
        bail!(
            "`{name}` looks like a git URL, not a package name — usage: `scadman add <NAME> <GIT>`, e.g. `{usage}`"
        );
    }

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
    if let Some(Dependency::Git(git)) = manifest.dependencies.get_mut(&name) {
        git.submodules = submodules;
    }
    write_dependencies(&manifest)?;
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

/// Whether `s` reads as a git remote rather than a package name: a URL with a scheme
/// (`https://…`, `file://…`, `ssh://…`) or scp-style `[user@]host:path`, where the `:`
/// comes before any `/`.
fn looks_like_git_url(s: &str) -> bool {
    if s.contains("://") {
        return true;
    }
    match (s.find(':'), s.find('/')) {
        (Some(colon), Some(slash)) => colon > 0 && colon < slash,
        _ => false,
    }
}

/// The repository basename of a git URL (`…/BOSL2.git` → `BOSL2`), if it is a valid
/// package name. Used only to show a corrected command, never to pick a name silently.
fn suggested_name(url: &str) -> Option<String> {
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    manifest::validate_package_name(name).ok()?;
    Some(name.to_string())
}

/// The remote's current default branch (the target of its `HEAD` symref), via
/// `git ls-remote`. Used when `scadman add <url>` is given no explicit ref.
fn default_branch(url: &str) -> Result<String> {
    let output = scadman_core::source::git_command()
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
            if let Some(rev) = &rev {
                manifest::validate_commit_hash(rev).map_err(|e| anyhow::anyhow!(e))?;
            }
            Dependency::Git(GitDependency {
                git,
                rev,
                tag,
                branch,
                root,
                on_path,
                submodules: false,
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
    // `lock` replaces the lockfile without reading it, so check what it is about to
    // replace: rewriting a newer scadman's lockfile in this format would downgrade it.
    // A file with no readable version is simply broken, and is regenerated.
    if let Ok(existing) = fs::read_to_string(lockfile::LOCKFILE_FILE) {
        check_lockfile_version(&existing).with_context(|| {
            format!(
                "not overwriting {} (delete it to write a fresh one)",
                lockfile::LOCKFILE_FILE
            )
        })?;
    }
    let store = open_store()?;
    // A lockfile that does not parse is broken, and is written afresh.
    let previous = fs::read_to_string(lockfile::LOCKFILE_FILE)
        .ok()
        .and_then(|text| parse_lockfile(&text).ok());
    let lock = match &previous {
        None => resolve_lock(&manifest, &store)?,
        Some(previous) => {
            // Commits already locked stay: `update` is what moves one. The exception is
            // a pin from a lockfile that did not record what it was resolved from. The
            // manifest may ask for something else by now and nothing would show it, so
            // those are resolved again, once.
            let unrecorded = previous
                .packages
                .iter()
                .filter(|p| !p.source.starts_with(lockfile::PATH_SOURCE_PREFIX))
                .filter(|p| p.requested.is_empty())
                .map(|p| p.source.clone())
                .collect();
            let base = std::env::current_dir().context("determine project directory")?;
            let fetcher = CachedGitFetcher::new(&store, previous, unrecorded, false);
            let lock = resolve(&manifest, &base, &fetcher)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .to_lockfile();
            warn_restored_by(&fetcher.inner, &lock);
            for package in &lock.packages {
                if !package.source.starts_with(lockfile::PATH_SOURCE_PREFIX)
                    && let Some(before) = previous.packages.iter().find(|p| p.name == package.name)
                    && before.rev != package.rev
                {
                    println!(
                        "Updated {}: {} → {}",
                        package.name,
                        short(&before.rev),
                        short(&package.rev)
                    );
                }
            }
            lock
        }
    };
    write_lock(&lock)?;
    warn_unfetched_submodules(&lock, &store);
    let n = lock.packages.len();
    println!(
        "Locked {n} {} → {}",
        plural(n, "package", "packages"),
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
        // Advance everything: a full fresh resolve, transitive dependencies included,
        // plus a report of what moved.
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
                        "`{name}` is a version dependency, which needs a package registry — registries aren't supported yet; use a git or path source"
                    )
                }
                None => bail!(
                    "`{name}` is not a dependency in {}",
                    manifest::MANIFEST_FILE
                ),
            }
        }
        let base_lock = previous.clone().unwrap_or_else(Lockfile::new);
        let fetcher = CachedGitFetcher::new(&store, &base_lock, refresh, false);
        let lock = resolve(&manifest, &base, &fetcher)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .to_lockfile();
        warn_restored_by(&fetcher.inner, &lock);
        lock
    };

    write_lock(&updated)?;
    warn_unfetched_submodules(&updated, &store);
    report_update(previous.as_ref(), &updated);
    Ok(())
}

/// Read `scadman.lock` if it exists and parses (else `None`).
fn read_lockfile_opt() -> Result<Option<Lockfile>> {
    if !Path::new(lockfile::LOCKFILE_FILE).exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(lockfile::LOCKFILE_FILE)?;
    Ok(Some(parse_lockfile(&text)?))
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
fn prepare_environment(
    store: &Store,
    verify: bool,
    locked: bool,
) -> Result<(Lockfile, Environment)> {
    let lock = read_or_lock(store, verify, locked)?;
    ensure_stored(&lock, store, verify)?;
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

fn sync_cmd(verify: bool, locked: bool) -> Result<Environment> {
    let store = open_store()?;
    let (lock, env) = prepare_environment(&store, verify, locked)?;
    warn_unfetched_submodules(&lock, &store);
    let n = env.exposed.len();
    println!(
        "Synced {n} {} → {}",
        plural(n, "package", "packages"),
        env.root.display()
    );
    Ok(env)
}

/// Run OpenSCAD with the project environment and return the code to exit with: the one
/// OpenSCAD ended with.
fn run_cmd(args: Vec<String>, locked: bool) -> Result<i32> {
    let store = open_store()?;
    warn_openscad_too_old();
    let (_, env) = prepare_environment(&store, false, locked)?;
    // Point OPENSCADPATH at the project env (plus any on_path library roots) so declared
    // dependencies shadow globals. (OpenSCAD still searches its built-in user/install
    // dirs — the include-scan surfaces undeclared imports.)
    let openscadpath = env.openscad_path_value().context("assemble OPENSCADPATH")?;
    let program = openscad_program();
    let status = ProcCommand::new(&program)
        .env("OPENSCADPATH", &openscadpath)
        .args(&args)
        .status()
        .with_context(|| {
            format!(
                "launch {} (looked in {OPENSCAD_SEARCH}; set OPENSCAD to its path)",
                program.display()
            )
        })?;
    Ok(exit_code_of(status))
}

/// The exit code that reports how a child ended: its own, or 128 plus the signal that
/// ended it, as a shell reports one.
fn exit_code_of(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(1)
}

/// What `run` exits with when scadman fails before OpenSCAD has run, so that a caller
/// can tell it from any code OpenSCAD returns. `env`, `timeout` and `nohup` use the same
/// number for the same case.
const RUN_FAILED_BEFORE_OPENSCAD: i32 = 125;

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

/// Write `manifest`'s dependencies into `scadman.toml`, changing only the entries that
/// differ from what the file holds.
fn write_dependencies(manifest: &Manifest) -> Result<()> {
    let text = fs::read_to_string(manifest::MANIFEST_FILE)
        .with_context(|| format!("read {}", manifest::MANIFEST_FILE))?;
    let edited = manifest::edit_dependencies(&text, manifest)
        .map_err(|reason| anyhow::anyhow!(reason))
        .with_context(|| format!("update {}", manifest::MANIFEST_FILE))?;
    fs::write(manifest::MANIFEST_FILE, edited)
        .with_context(|| format!("write {}", manifest::MANIFEST_FILE))
}

fn load_manifest() -> Result<Manifest> {
    let text = fs::read_to_string(manifest::MANIFEST_FILE)
        .with_context(|| format!("read {} (run `scadman init`?)", manifest::MANIFEST_FILE))?;
    Manifest::from_root_toml(&text).with_context(|| format!("parse {}", manifest::MANIFEST_FILE))
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
    let lock = resolved.to_lockfile();
    warn_restored_by(&fetcher, &lock);
    Ok(lock)
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
    /// Git identity (canonical URL) → the package the lockfile pins it to.
    locked: HashMap<String, lockfile::LockedPackage>,
    /// Git identities to force a fresh fetch for, bypassing the cache — how `scadman update`
    /// advances selected deps (empty for a path-triggered refresh, which holds all git deps).
    refresh: HashSet<String>,
    /// Re-hash every copy before serving it, protected or not (`sync --verify`).
    verify: bool,
}

impl CachedGitFetcher {
    fn new(store: &Store, lock: &Lockfile, refresh: HashSet<String>, verify: bool) -> Self {
        let locked = lock
            .packages
            .iter()
            .filter(|p| !p.source.starts_with(lockfile::PATH_SOURCE_PREFIX))
            .map(|p| (p.source.clone(), p.clone()))
            .collect();
        Self {
            inner: GitFetcher::new(store.clone()),
            store: store.clone(),
            locked,
            refresh,
            verify,
        }
    }
}

impl Fetcher for CachedGitFetcher {
    fn fetch(&self, url: &str, reference: &GitRef, submodules: bool) -> io::Result<Fetched> {
        if !self.refresh.contains(url)
            && let Some(package) = self.locked.get(url)
            && holds_reference(reference, package)
        {
            let path = self.store.path_for(&package.hash);
            // A copy locked without its submodules does not answer a request for them,
            // and one with them is not what a request without them locks.
            if package.submodules == submodules && path.exists() {
                // Checked here, at the moment the copy is handed over: the resolver reads
                // its `scadman.toml` next, and a modified copy must not get that far.
                // Being keyed by URL like the cache itself, the check covers exactly the
                // copies that are served, whatever the manifest calls them now.
                let sound = stored_copy_is_sound(package, &self.store, self.verify)
                    .map_err(|e| io::Error::other(format!("{e:#}")))?;
                if !sound {
                    fetch_locked(package, &self.store, true)
                        .map_err(|e| io::Error::other(format!("{e:#}")))?;
                }
                return Ok(Fetched {
                    rev: package.rev.clone(),
                    hash: package.hash.clone(),
                    path,
                });
            }
            // Held, with no copy to serve: fetch the locked commit. The reference itself
            // would be wherever a branch or tag points today.
            let fetched = self
                .inner
                .fetch(url, &GitRef::Rev(package.rev.clone()), submodules)?;
            // The same commit with the same setting is the content the lockfile pins.
            if package.submodules == submodules && fetched.hash != package.hash {
                return Err(io::Error::other(format!(
                    "integrity: {} @ {} hashed to {} but the lockfile expects {}",
                    package.name, package.rev, fetched.hash, package.hash
                )));
            }
            return Ok(fetched);
        }
        self.inner.fetch(url, reference, submodules)
    }

    fn fetch_path(&self, dir: &Path) -> io::Result<Fetched> {
        self.inner.fetch_path(dir)
    }
}

/// Whether serving the locked commit `rev` satisfies a request for `reference`, i.e. the
/// cache may hold this dep rather than re-fetch. A branch or tag is held at its locked
/// commit (scadman doesn't chase upstream movement unless the dep is named for `update`).
/// A request for a *different exact commit* — a changed `rev` pin, e.g. a transitive dep
/// whose parent moved — must be honored, or the lock would record a commit its requester
/// no longer asks for.
///
/// The kind comes from the manifest key, never from the spelling: a tag named `20210101`
/// looks like a commit hash and is not one.
///
/// A tag or branch is held while it is the one the commit was resolved from. A lockfile
/// written before that was recorded cannot say, and its pins are held as they always
/// were.
fn holds_reference(reference: &GitRef, package: &lockfile::LockedPackage) -> bool {
    match reference {
        GitRef::Rev(requested) => package.rev.eq_ignore_ascii_case(requested),
        GitRef::Tag(_) | GitRef::Branch(_) => {
            package.requested.is_empty() || package.requested.contains(&reference.requested())
        }
    }
}

/// The lockfile to build from. It is created when missing, and rewritten when a path
/// dependency's directory has changed, unless `locked` forbids writing it: then either
/// is an error and nothing is written.
fn read_or_lock(store: &Store, verify: bool, locked: bool) -> Result<Lockfile> {
    let manifest = load_manifest()?;
    if Path::new(lockfile::LOCKFILE_FILE).exists() {
        let text = fs::read_to_string(lockfile::LOCKFILE_FILE)?;
        let lock = parse_lockfile(&text)?;
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
            let fetcher = CachedGitFetcher::new(store, &lock, HashSet::new(), verify);
            let base = std::env::current_dir().context("determine project directory")?;
            let refreshed = resolve(&manifest, &base, &fetcher)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .to_lockfile();
            warn_restored_by(&fetcher.inner, &refreshed);
            if refreshed != lock {
                if locked {
                    let changed: Vec<String> = refreshed
                        .packages
                        .iter()
                        .filter(|package| !lock.packages.contains(package))
                        .map(|package| format!("`{}`", package.name))
                        .collect();
                    bail!(
                        "{} would change ({} no longer matches it) and --locked forbids writing it. Run `scadman sync` without --locked, and commit the result.",
                        lockfile::LOCKFILE_FILE,
                        changed.join(", ")
                    );
                }
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
    if locked {
        bail!(
            "{} is missing and --locked forbids writing it. Run `scadman lock`, and commit the result.",
            lockfile::LOCKFILE_FILE
        );
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

/// Ensure every locked package's content is in the store and is what the lockfile pins:
/// fetch what is missing (a fresh checkout), and replace what was modified.
fn ensure_stored(lock: &Lockfile, store: &Store, verify: bool) -> Result<()> {
    for package in &lock.packages {
        let present = store.path_for(&package.hash).exists();
        if present && stored_copy_is_sound(package, store, verify)? {
            continue;
        }
        // A path dependency has no remote to re-fetch from; its content is inserted at lock
        // time. If it is missing here (e.g. a store cleared out from under a committed lock),
        // re-locking re-reads the local directory — don't try to git-fetch a `path:` source.
        if package.source.starts_with(lockfile::PATH_SOURCE_PREFIX) {
            bail!(
                "path dependency `{}` content is {} the store — run `scadman lock`",
                package.name,
                if present { "modified in" } else { "not in" }
            );
        }
        fetch_locked(package, store, present)?;
    }
    Ok(())
}

/// Whether `package`'s stored copy can be used as it stands, protecting it if so.
///
/// A copy is write-protected once it is known to match its hash, so one that still is has
/// had no chance to change and is taken as it stands. One that is not — stored by a
/// scadman from before copies were protected, or with the protection since lifted — is
/// re-hashed, and so is every copy when `verify` is set.
fn stored_copy_is_sound(
    package: &lockfile::LockedPackage,
    store: &Store,
    verify: bool,
) -> Result<bool> {
    let inspect = || -> io::Result<bool> {
        if !verify && store.is_protected(&package.hash)? {
            return Ok(true);
        }
        store.verify(&package.hash)
    };
    let sound = inspect().with_context(|| format!("check `{}` in the store", package.name))?;
    if sound {
        // Best effort: where a file system cannot hold the protection, the copy is
        // simply re-hashed next time.
        store.protect(&package.hash).ok();
    }
    Ok(sound)
}

/// Fetch `package` at its locked commit into the store and check it hashes to the locked
/// value. With `replacing` set, a modified copy is in the way: it is swapped out only once
/// the right content is in hand, so a failure here (no network, say) leaves it where it
/// was.
fn fetch_locked(package: &lockfile::LockedPackage, store: &Store, replacing: bool) -> Result<()> {
    let acquired = fetch_git(store, &package.source, &package.rev, package.submodules)
        .with_context(|| {
            if replacing {
                format!(
                    "`{}` was modified in the store, and restoring it from {} failed",
                    package.name, package.rev
                )
            } else {
                format!("fetch {} @ {}", package.name, package.rev)
            }
        })?;
    if acquired.entry.hash != package.hash {
        bail!(
            "integrity: {} @ {} hashed to {} but the lockfile expects {}",
            package.name,
            package.rev,
            acquired.entry.hash,
            package.hash
        );
    }
    if replacing {
        warn_restored(&package.name, &package.hash);
    }
    Ok(())
}

/// Say that a stored copy had been changed and was put back. It was served in that state
/// until now, so anything rendered from it is worth a second look.
fn warn_restored(name: &str, hash: &str) {
    eprintln!(
        "warning: `{name}` had been modified in the store ({}); restored it to the locked content",
        short(hash)
    );
}

/// [`warn_restored`] for the copies a resolve replaced, named from the lock it produced.
fn warn_restored_by(fetcher: &GitFetcher, lock: &Lockfile) {
    for hash in fetcher.take_restored() {
        // What a resolve fetched is in the lock it produced, so the name is there.
        if let Some(package) = lock.packages.iter().find(|p| p.hash == hash) {
            warn_restored(&package.name, &hash);
        }
    }
}

/// Name each git submodule a locked package declares and does not hold, which is every
/// one of a dependency that did not ask for its submodules: the folder is empty, and a
/// library that reads from it fails inside OpenSCAD. One that is also a dependency in its
/// own right has been dealt with, and is not named.
fn warn_unfetched_submodules(lock: &Lockfile, store: &Store) {
    let sources: HashSet<String> = lock
        .packages
        .iter()
        .map(|p| canonical_url(&p.source))
        .collect();
    // A package locked with its submodules has all that git would fetch; anything its
    // `.gitmodules` still lists is a leftover section, and asking again changes nothing.
    for package in lock.packages.iter().filter(|p| !p.submodules) {
        for submodule in unfetched_submodules(&store.path_for(&package.hash)) {
            let url = submodule.url.as_deref();
            if url.is_some_and(|url| sources.contains(&canonical_url(url))) {
                continue;
            }
            let from = url.map(|url| format!(" ({url})")).unwrap_or_default();
            // A path dependency is read from a working tree as it stands, so the
            // submodule is the user's to check out there.
            let remedy = if package.source.starts_with(lockfile::PATH_SOURCE_PREFIX) {
                "check it out in that directory".to_string()
            } else {
                format!(
                    "set `submodules = true` on `{}` in {}",
                    package.name,
                    manifest::MANIFEST_FILE
                )
            };
            eprintln!(
                "warning: `{}` has a git submodule at `{}/`{from} that was not fetched, so that folder is empty — to fetch it, {remedy}",
                package.name, submodule.path
            );
        }
    }
}

fn warn_unmet_imports(lock: &Lockfile, env: &Environment) {
    let installed: Vec<Installed> = lock
        .packages
        .iter()
        .map(|p| Installed {
            name: p.name.clone(),
            path: env.root.join(&p.name),
            on_path: p.on_path,
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

    /// A well-formed `rev` for fixtures: `add` refuses anything shorter.
    const FULL_REV: &str = "061fef7c429628808e847696bb345a9b0ec6e279";

    #[test]
    fn installed_openscad_prefers_the_installers_folder() {
        let root = tempfile::tempdir().unwrap();
        for dir in ["OpenSCAD (Nightly)", "OpenSCAD", "Other"] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
            std::fs::write(root.path().join(dir).join("openscad.exe"), "").unwrap();
        }
        assert_eq!(
            installed_openscad(&[PathBuf::from("/nonexistent"), root.path().to_path_buf()]),
            Some(root.path().join("OpenSCAD").join("openscad.exe"))
        );
        std::fs::remove_dir_all(root.path().join("OpenSCAD")).unwrap();
        assert_eq!(
            installed_openscad(&[root.path().to_path_buf()]),
            Some(root.path().join("OpenSCAD (Nightly)").join("openscad.exe"))
        );
        assert_eq!(installed_openscad(&[root.path().join("Other")]), None);
    }

    #[test]
    fn workspace_relative_uses_the_workspace_token() {
        let cwd = "/home/u/proj";
        // A path under the project becomes `${workspaceFolder}`-relative.
        assert_eq!(
            workspace_relative_to("/home/u/proj/.scadman/env", cwd),
            "${workspaceFolder}/.scadman/env"
        );
        // Multiple (on_path) entries are each rewritten; the separator is preserved.
        let sep = PATH_LIST_SEPARATOR;
        assert_eq!(
            workspace_relative_to(
                &format!("/home/u/proj/.scadman/env{sep}/home/u/proj/.scadman/env/dotSCAD"),
                cwd
            ),
            format!(
                "${{workspaceFolder}}/.scadman/env{sep}${{workspaceFolder}}/.scadman/env/dotSCAD"
            )
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
    fn git_urls_are_told_apart_from_package_names() {
        for url in [
            "https://github.com/revarbat/BOSL",
            "file:///srv/git/lib",
            "ssh://git@host/o/r.git",
            "git@github.com:revarbat/BOSL.git",
        ] {
            assert!(looks_like_git_url(url), "{url} should read as a URL");
        }
        // Real names, including the forms the add command documents, are not URLs.
        for name in ["BOSL2", "dotSCAD", "my-lib_2", "lib.v2", "a:b"] {
            assert!(!looks_like_git_url(name), "{name} should read as a name");
        }
    }

    #[test]
    fn suggested_name_is_the_repo_basename() {
        assert_eq!(
            suggested_name("https://github.com/revarbat/BOSL").as_deref(),
            Some("BOSL")
        );
        assert_eq!(
            suggested_name("https://github.com/BelfrySCAD/BOSL2.git/").as_deref(),
            Some("BOSL2")
        );
        assert_eq!(
            suggested_name("git@github.com:revarbat/BOSL.git").as_deref(),
            Some("BOSL")
        );
        // Nothing usable to suggest.
        assert_eq!(suggested_name("https://"), None);
    }

    #[test]
    fn plural_picks_the_right_noun() {
        assert_eq!(plural(1, "package", "packages"), "package");
        assert_eq!(plural(0, "package", "packages"), "packages");
        assert_eq!(plural(2, "package", "packages"), "packages");
    }

    /// A package locked at `rev`, resolved from each of `requested`.
    fn locked_at(rev: &str, requested: &[&str]) -> lockfile::LockedPackage {
        lockfile::LockedPackage {
            name: "lib".to_string(),
            source: "https://h/lib".to_string(),
            rev: rev.to_string(),
            hash: String::new(),
            dependencies: Vec::new(),
            root: ".".to_string(),
            on_path: false,
            submodules: false,
            requested: requested.iter().map(|r| r.to_string()).collect(),
        }
    }

    const LOCKED_REV: &str = "061fef7abc0000000000000000000000000000ab";

    #[test]
    fn holds_reference_holds_the_same_pin_and_honors_a_changed_one() {
        let package = locked_at(
            LOCKED_REV,
            &["rev:061fef7abc0000000000000000000000000000ab"],
        );
        let r = |s: &str| GitRef::Rev(s.to_string());
        // The same exact pin is held — no needless re-fetch.
        assert!(holds_reference(&r(LOCKED_REV), &package));
        assert!(holds_reference(&r(&LOCKED_REV.to_uppercase()), &package));
        // A prefix of the locked commit is not the same pin (and cannot be fetched).
        assert!(!holds_reference(&r("061fef7"), &package));
        // A different exact commit must NOT be held — it has to be re-fetched.
        assert!(!holds_reference(
            &r("deadbeef1234567890000000000000000000cafe"),
            &package
        ));
        assert!(!holds_reference(&r("deadbee"), &package));
    }

    #[test]
    fn holds_reference_holds_the_tag_or_branch_the_commit_came_from() {
        let package = locked_at(LOCKED_REV, &["branch:main", "tag:v1"]);
        assert!(holds_reference(&GitRef::Branch("main".into()), &package));
        assert!(holds_reference(&GitRef::Tag("v1".into()), &package));
        // Another name is a different request, and so is the same name as the other kind.
        assert!(!holds_reference(&GitRef::Branch("dev".into()), &package));
        assert!(!holds_reference(&GitRef::Tag("v2".into()), &package));
        assert!(!holds_reference(&GitRef::Tag("main".into()), &package));
        assert!(!holds_reference(&GitRef::Branch("v1".into()), &package));
    }

    #[test]
    fn holds_reference_holds_every_tag_and_branch_of_a_lockfile_that_recorded_none() {
        // Written by an earlier release: there is nothing to compare against.
        let package = locked_at(LOCKED_REV, &[]);
        assert!(holds_reference(&GitRef::Branch("main".into()), &package));
        assert!(holds_reference(&GitRef::Tag("v2.0.700".into()), &package));
        // An exact commit is still compared.
        assert!(!holds_reference(
            &GitRef::Rev("deadbeef1234567890000000000000000000cafe".into()),
            &package
        ));
    }

    #[test]
    fn holds_reference_goes_by_kind_not_spelling() {
        // A tag or branch spelled like a commit hash is still a tag or branch: held.
        let package = locked_at(LOCKED_REV, &[]);
        assert!(holds_reference(&GitRef::Tag("20210101".into()), &package));
        assert!(holds_reference(
            &GitRef::Branch("deadbeef".into()),
            &package
        ));
        assert!(holds_reference(&GitRef::Tag("cafe2024".into()), &package));
        // The same spellings as a `rev` are exact pins, and a different one is not held.
        assert!(!holds_reference(&GitRef::Rev("20210101".into()), &package));
    }

    #[test]
    fn add_dependency_refuses_an_abbreviated_rev() {
        let mut m = Manifest::new("p");
        let err = add_dependency(
            &mut m,
            "A",
            Some("u".into()),
            None,
            Some("061fef7".into()),
            None,
            None,
            None,
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("full commit hash"), "{err}");
        assert!(m.dependencies.is_empty(), "nothing is added");
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
                Some(FULL_REV.into()),
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
            Some(FULL_REV.into()),
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
                Some(FULL_REV.into()),
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
