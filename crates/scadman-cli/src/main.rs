//! The `scadman` command-line interface.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use scadman_core::{
    Dependency, Environment, GitDependency, GitFetcher, Installed, Lockfile, Manifest, Store,
    build_environment, fetch_git, lock_staleness, lockfile, manifest, resolve, unmet_imports,
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
    /// Add a git dependency to scadman.toml.
    Add {
        /// Name to expose the dependency under (the `<Name/…>` include prefix).
        name: String,
        /// Git URL of the dependency.
        git: String,
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
    /// Materialize the project environment from the lockfile (resolving if needed).
    Sync,
    /// Print the project's OPENSCADPATH (point your editor/OpenSCAD at it).
    Env {
        /// Emit a machine-readable JSON report instead of the path.
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
            rev,
            tag,
            branch,
            root,
            on_path,
        } => add_cmd(name, git, rev, tag, branch, root, on_path),
        Command::Remove { name } => remove_cmd(name),
        Command::List => list_cmd(),
        Command::Lock => lock_cmd(),
        Command::Sync => sync_cmd().map(|_| ()),
        Command::Env { json } => env_cmd(json),
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

fn env_cmd(json: bool) -> Result<()> {
    let store = open_store()?;
    let (lock, env) = prepare_environment(&store)?;
    let openscadpath = env
        .openscad_path_value()
        .context("assemble OPENSCADPATH")?
        .to_string_lossy()
        .into_owned();
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

#[allow(clippy::too_many_arguments)]
fn add_cmd(
    name: String,
    git: String,
    rev: Option<String>,
    tag: Option<String>,
    branch: Option<String>,
    root: Option<String>,
    on_path: bool,
) -> Result<()> {
    let mut manifest = load_manifest()?;
    let updated = add_dependency(&mut manifest, &name, git, rev, tag, branch, root, on_path)?;
    let text = manifest.to_toml().context("serialize manifest")?;
    fs::write(manifest::MANIFEST_FILE, text)
        .with_context(|| format!("write {}", manifest::MANIFEST_FILE))?;
    let verb = if updated { "Updated" } else { "Added" };
    println!(
        "{verb} `{name}` in {}. Run `scadman lock` to resolve.",
        manifest::MANIFEST_FILE
    );
    Ok(())
}

/// Insert (or replace) a git dependency in the manifest. Returns whether an existing entry
/// was replaced. Exactly one of `rev`/`tag`/`branch` must be given.
#[allow(clippy::too_many_arguments)]
fn add_dependency(
    manifest: &mut Manifest,
    name: &str,
    git: String,
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
    let refs = [&rev, &tag, &branch].iter().filter(|r| r.is_some()).count();
    if refs != 1 {
        bail!("specify exactly one of --rev, --tag, or --branch");
    }
    let dep = Dependency::Git(GitDependency {
        git,
        rev,
        tag,
        branch,
        root,
        on_path,
    });
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
    let resolved = resolve(manifest, &fetcher).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(resolved.to_lockfile())
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
    let provided: BTreeSet<String> = lock.packages.iter().map(|p| p.name.clone()).collect();

    for unmet in unmet_imports(&installed, &provided) {
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
    fn add_dependency_requires_exactly_one_ref() {
        let mut m = Manifest::new("p");
        assert!(add_dependency(&mut m, "A", "u".into(), None, None, None, None, false).is_err());
        assert!(
            add_dependency(
                &mut m,
                "A",
                "u".into(),
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
            url,
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
                "u".into(),
                Some("r".into()),
                None,
                None,
                Some("../etc".into()),
                false,
            )
            .is_err()
        );
    }
}
