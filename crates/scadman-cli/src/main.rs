//! The `scadman` command-line interface.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use scadman_core::{
    Environment, GitFetcher, Installed, Lockfile, Manifest, Store, build_environment, fetch_git,
    lockfile, manifest, resolve, unmet_imports,
};

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
    /// Resolve dependencies and write scadman.lock.
    Lock,
    /// Materialize the project environment from the lockfile (resolving if needed).
    Sync,
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
        Command::Lock => lock_cmd(),
        Command::Sync => sync_cmd().map(|_| ()),
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
    Ok(())
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

fn sync_cmd() -> Result<Environment> {
    let store = open_store()?;
    let lock = read_or_lock(&store)?;
    ensure_stored(&lock, &store)?;

    let env = build_environment(&lock, &store, &env_root()?).context("build environment")?;
    warn_unmet_imports(&lock, &env);
    println!(
        "Synced {} package(s) → {}",
        env.exposed.len(),
        env.root.display()
    );
    Ok(env)
}

fn run_cmd(args: Vec<String>) -> Result<()> {
    let env = sync_cmd()?;
    // Replace (not prepend) OPENSCADPATH so only declared dependencies are visible.
    let status = ProcCommand::new("openscad")
        .env("OPENSCADPATH", &env.root)
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
    if Path::new(lockfile::LOCKFILE_FILE).exists() {
        let text = fs::read_to_string(lockfile::LOCKFILE_FILE)?;
        return Lockfile::from_toml(&text)
            .with_context(|| format!("parse {}", lockfile::LOCKFILE_FILE));
    }
    let manifest = load_manifest()?;
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
