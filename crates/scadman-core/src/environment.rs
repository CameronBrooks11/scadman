//! Building a project's OpenSCAD environment from a lockfile.
//!
//! OpenSCAD resolves `use`/`include` against directories on `OPENSCADPATH`, and libraries
//! assume they live under their own name (a self-rooted `include <BOSL2/…>` only works if
//! `BOSL2/` is on the path — see `docs/ecosystem-survey.md`). So the environment is a
//! single directory containing one entry per package, named for the package, each a
//! symlink to that package's immutable [`crate::store`] entry. Pointing `OPENSCADPATH` at
//! that directory makes `<Name/…>` resolve to the pinned content.
//!
//! The environment is rebuilt from scratch each time, so it always reflects the lockfile
//! exactly and never accumulates stale entries.

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use crate::lockfile::Lockfile;
use crate::manifest::{validate_library_root, validate_package_name};
use crate::store::Store;

/// A materialized project environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// The environment directory (one symlink per package, named for it).
    pub root: PathBuf,
    /// The package names exposed, sorted.
    pub exposed: Vec<String>,
    /// The `OPENSCADPATH` entries, in search order: the env root (so `<name/…>` resolves),
    /// followed by each `on_path` package's dir (so those libraries' own root-relative
    /// imports resolve). Declared deps shadow globals; OpenSCAD still also searches its
    /// built-in user/install dirs.
    pub openscad_path: Vec<PathBuf>,
}

impl Environment {
    /// The `OPENSCADPATH` value (entries joined with the platform separator). Errors only
    /// if a path contains the separator itself (rather than silently emptying the path).
    pub fn openscad_path_value(&self) -> Result<std::ffi::OsString, std::env::JoinPathsError> {
        std::env::join_paths(&self.openscad_path)
    }
}

/// Build (or rebuild) the environment at `env_root` for `lock`, linking each package to its
/// content in `store`. Returns an error if a locked package is missing from the store
/// (acquire it first) — the environment must reflect the lockfile exactly.
pub fn build(lock: &Lockfile, store: &Store, env_root: &Path) -> io::Result<Environment> {
    if env_root.exists() {
        fs::remove_dir_all(env_root)?;
    }
    fs::create_dir_all(env_root)?;

    let mut exposed = Vec::with_capacity(lock.packages.len());
    let mut on_path = Vec::new();
    for package in &lock.packages {
        // The lockfile is untrusted input (it travels with a cloned repo). This is the
        // single chokepoint every package name / root funnels through to the filesystem,
        // so validate here regardless of whether the resolver already did.
        validate_package_name(&package.name).map_err(io::Error::other)?;
        validate_library_root(&package.root).map_err(io::Error::other)?;

        let target = store.path_for(&package.hash).join(&package.root);
        if !target.exists() {
            return Err(io::Error::other(format!(
                "package `{}` root `{}` ({}) is not in the store — run a sync/acquire first",
                package.name, package.root, package.hash
            )));
        }
        let link = env_root.join(&package.name);
        symlink(&target, &link)?;
        exposed.push(package.name.clone());
        if package.on_path {
            on_path.push(link);
        }
    }
    exposed.sort();
    on_path.sort();

    let mut openscad_path = vec![env_root.to_path_buf()];
    openscad_path.extend(on_path);

    Ok(Environment {
        root: env_root.to_path_buf(),
        exposed,
        openscad_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lockfile::LockedPackage;
    use tempfile::TempDir;

    fn store_with(entries: &[(&str, &str)]) -> (TempDir, Store) {
        let root = TempDir::new().unwrap();
        let store = Store::new(root.path());
        for (hash, content) in entries {
            let dir = store.path_for(hash);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("std.scad"), content).unwrap();
        }
        (root, store)
    }

    fn locked(name: &str, hash: &str) -> LockedPackage {
        LockedPackage {
            name: name.to_string(),
            source: format!("https://h/{name}"),
            rev: "rev".to_string(),
            hash: hash.to_string(),
            dependencies: Vec::new(),
            root: ".".to_string(),
            on_path: false,
        }
    }

    #[test]
    fn exposes_each_package_under_its_name() {
        let (_root, store) = store_with(&[("h_bosl2", "// bosl2"), ("h_utils", "// utils")]);
        let mut lock = Lockfile::new();
        lock.packages.push(locked("BOSL2", "h_bosl2"));
        lock.packages.push(locked("scad-utils", "h_utils"));

        let env_dir = TempDir::new().unwrap();
        let env = build(&lock, &store, &env_dir.path().join("env")).unwrap();

        assert_eq!(
            env.exposed,
            vec!["BOSL2".to_string(), "scad-utils".to_string()]
        );
        // `<BOSL2/std.scad>` resolves through the symlink to the store content.
        let via_env = env.root.join("BOSL2").join("std.scad");
        assert_eq!(fs::read_to_string(via_env).unwrap(), "// bosl2");
    }

    #[test]
    fn rebuild_is_clean() {
        let (_root, store) = store_with(&[("h_a", "// a"), ("h_b", "// b")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");

        let mut first = Lockfile::new();
        first.packages.push(locked("A", "h_a"));
        first.packages.push(locked("B", "h_b"));
        build(&first, &store, &env_root).unwrap();

        // A later lockfile with only A must not leave B behind.
        let mut second = Lockfile::new();
        second.packages.push(locked("A", "h_a"));
        let env = build(&second, &store, &env_root).unwrap();

        assert_eq!(env.exposed, vec!["A".to_string()]);
        assert!(env_root.join("A").exists());
        assert!(!env_root.join("B").exists());
    }

    #[test]
    fn missing_store_entry_is_an_error() {
        let (_root, store) = store_with(&[]);
        let mut lock = Lockfile::new();
        lock.packages.push(locked("Ghost", "nonexistent"));
        let env_dir = TempDir::new().unwrap();
        assert!(build(&lock, &store, &env_dir.path().join("env")).is_err());
    }

    #[test]
    fn exposes_a_subdir_root_and_adds_it_to_openscad_path() {
        let store_dir = TempDir::new().unwrap();
        let store = Store::new(store_dir.path());
        let entry = store.path_for("h1");
        fs::create_dir_all(entry.join("src")).unwrap();
        fs::write(entry.join("src").join("lib.scad"), "// lib").unwrap();

        let mut lock = Lockfile::new();
        lock.packages.push(LockedPackage {
            name: "dotSCAD".to_string(),
            source: "s".to_string(),
            rev: "r".to_string(),
            hash: "h1".to_string(),
            dependencies: Vec::new(),
            root: "src".to_string(),
            on_path: true,
        });

        let env_dir = TempDir::new().unwrap();
        let env = build(&lock, &store, &env_dir.path().join("env")).unwrap();

        // Exposed under its name → the src/ content (not the repo root).
        assert_eq!(
            fs::read_to_string(env.root.join("dotSCAD").join("lib.scad")).unwrap(),
            "// lib"
        );
        // OPENSCADPATH = [env root, env/dotSCAD] because on_path is set.
        assert_eq!(env.openscad_path.len(), 2);
        assert_eq!(env.openscad_path[0], env.root);
        assert_eq!(env.openscad_path[1], env.root.join("dotSCAD"));
    }

    #[test]
    fn rejects_traversing_library_root() {
        let (_root, store) = store_with(&[("h", "// x")]);
        let mut lock = Lockfile::new();
        let mut pkg = locked("x", "h");
        pkg.root = "../etc".to_string();
        lock.packages.push(pkg);
        let env_dir = TempDir::new().unwrap();
        assert!(build(&lock, &store, &env_dir.path().join("env")).is_err());
    }

    #[test]
    fn rejects_unsafe_package_name_from_the_lockfile() {
        // A tampered/committed lockfile must not be able to plant a symlink outside the env.
        let (_root, store) = store_with(&[]);
        let mut lock = Lockfile::new();
        lock.packages.push(locked("../evil", "anyhash"));
        let env_dir = TempDir::new().unwrap();
        let err = build(&lock, &store, &env_dir.path().join("env")).unwrap_err();
        assert!(err.to_string().contains("valid package name"));
    }
}
