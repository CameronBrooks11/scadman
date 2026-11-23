//! Building a project's OpenSCAD environment from a lockfile.
//!
//! OpenSCAD resolves `use`/`include` against directories on `OPENSCADPATH`, and libraries
//! assume they live under their own name (a self-rooted `include <BOSL2/…>` only works if
//! `BOSL2/` is on the path — see `docs/ecosystem-survey.md`). So the environment is a
//! single directory containing one entry per package, named for the package, each a
//! view of that package's immutable [`crate::store`] entry: a symlink on Unix, and on
//! Windows a directory tree of hard links to the stored files (see `mirror_tree`), because
//! OpenSCAD 2021.01 there cannot read through a junction or a symlink. Pointing `OPENSCADPATH` at
//! that directory makes `<Name/…>` resolve to the pinned content.
//!
//! Each build brings the environment into line with the lockfile: an entry that is
//! already right is left alone, one that is not is replaced, and one the lockfile no
//! longer has is removed. Everything is checked before anything is changed, so a build
//! that cannot succeed leaves the environment as it was, and builds of one project wait
//! for each other, so commands run side by side do not undo one another's work.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::lockfile::Lockfile;
use crate::manifest::{validate_library_root, validate_package_name};
use crate::store::{Store, validate_content_hash};

/// A materialized project environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// The environment directory (one link per package, named for it).
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
    build_as(Exposure::HERE, lock, store, env_root)
}

fn build_as(
    exposure: Exposure,
    lock: &Lockfile,
    store: &Store,
    env_root: &Path,
) -> io::Result<Environment> {
    // The lockfile is untrusted input (it travels with a cloned repo). This is the single
    // chokepoint every package name, root and hash funnels through to the filesystem, so
    // validate here regardless of whether the resolver already did — and validate all of
    // it before touching the existing environment, so a bad entry changes nothing.
    for package in &lock.packages {
        validate_package_name(&package.name).map_err(io::Error::other)?;
        validate_library_root(&package.root).map_err(io::Error::other)?;
        validate_content_hash(&package.hash)
            .map_err(|reason| io::Error::other(format!("package `{}`: {reason}", package.name)))?;
    }

    // Find what each entry must point at, and that it is there, before changing anything.
    let mut targets = Vec::with_capacity(lock.packages.len());
    for package in &lock.packages {
        let entry = store.path_for(&package.hash);
        if !entry.exists() {
            return Err(io::Error::other(format!(
                "package `{}` ({}) is not in the store — acquire it first",
                package.name, package.hash
            )));
        }
        let target = entry.join(&package.root);
        if !target.exists() {
            return Err(io::Error::other(format!(
                "package `{}`: library root `{}` does not exist in its content",
                package.name, package.root
            )));
        }
        targets.push(target);
    }

    let _held = hold(env_root)?;
    fs::create_dir_all(env_root)?;

    let mut exposed = Vec::with_capacity(lock.packages.len());
    let mut on_path = Vec::new();
    for (package, target) in lock.packages.iter().zip(&targets) {
        let link = env_root.join(&package.name);
        if !exposure.is_current(env_root, &package.name, target) {
            exposure.expose(env_root, &package.name, target)?;
        }
        exposed.push(package.name.clone());
        if package.on_path {
            on_path.push(link);
        }
    }
    exposure.remove_all_but(env_root, &exposed)?;
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

/// A path beside the environment directory, named after it: where a build keeps what
/// must not be seen as a library.
fn beside(env_root: &Path, suffix: &str) -> PathBuf {
    let mut name = env_root.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    env_root.with_file_name(name)
}

/// Wait for any other build of this environment to finish, and keep others out until
/// the returned file is dropped. The operating system releases the hold if the process
/// dies. A file system that cannot lock is used without one.
fn hold(env_root: &Path) -> io::Result<fs::File> {
    let path = beside(env_root, ".lock");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    match file.lock() {
        Err(error) if error.kind() != io::ErrorKind::Unsupported => Err(error),
        _ => Ok(file),
    }
}

/// Remove whatever is at `path`: a link, a file or a directory tree. Nothing there is fine.
fn remove_entry(path: &Path) -> io::Result<()> {
    let removed = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
        Ok(found) if found.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
    };
    match removed {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// How a package's directory is made to appear in the environment.
#[derive(Debug, Clone, Copy)]
enum Exposure {
    /// A symlink to the stored directory.
    #[cfg(unix)]
    Symlink,
    /// A copy of the directory tree whose files are hard links to the stored ones.
    /// OpenSCAD 2021.01 on Windows crashes reading a junction or a symlink once a library
    /// includes its own files (#132).
    #[cfg_attr(unix, allow(dead_code))]
    Mirror,
}

impl Exposure {
    /// What this platform uses.
    #[cfg(unix)]
    const HERE: Self = Self::Symlink;
    #[cfg(windows)]
    const HERE: Self = Self::Mirror;

    /// Where a mirrored entry records the directory it was made from. A tree of hard
    /// links does not say where it came from, so this is how a later build knows.
    fn stamp(env_root: &Path, name: &str) -> PathBuf {
        beside(env_root, ".stamps").join(name)
    }

    /// Whether the entry `name` already shows `target`.
    fn is_current(self, env_root: &Path, name: &str, target: &Path) -> bool {
        let link = env_root.join(name);
        match self {
            #[cfg(unix)]
            Self::Symlink => fs::read_link(link).is_ok_and(|to| to == target),
            Self::Mirror => {
                fs::symlink_metadata(link).is_ok_and(|found| found.is_dir())
                    && fs::read_to_string(Self::stamp(env_root, name))
                        .is_ok_and(|from| Path::new(&from) == target)
            }
        }
    }

    /// Make the entry `name` show `target`, replacing whatever is there. The new entry is
    /// made beside the environment first, so the old one stays until its replacement is
    /// whole.
    fn expose(self, env_root: &Path, name: &str, target: &Path) -> io::Result<()> {
        let link = env_root.join(name);
        let new = beside(env_root, ".new");
        remove_entry(&new)?;
        match self {
            #[cfg(unix)]
            Self::Symlink => {
                std::os::unix::fs::symlink(target, &new)?;
                // A link moved onto a link replaces it in one step. Anything else that
                // has ended up there has to go first.
                if fs::symlink_metadata(&link).is_ok_and(|found| found.is_dir()) {
                    remove_entry(&link)?;
                }
                fs::rename(&new, &link)
            }
            Self::Mirror => {
                mirror_tree(target, &new)?;
                let stamp = Self::stamp(env_root, name);
                // No stamp while the entry is in between: a build cut short here leaves
                // an entry the next one replaces.
                remove_entry(&stamp)?;
                remove_entry(&link)?;
                fs::rename(&new, &link)?;
                if let Some(stamps) = stamp.parent() {
                    fs::create_dir_all(stamps)?;
                }
                fs::write(stamp, target.to_string_lossy().as_bytes())
            }
        }
    }

    /// Remove every entry whose name is not in `wanted`.
    fn remove_all_but(self, env_root: &Path, wanted: &[String]) -> io::Result<()> {
        let unwanted = |dir: &Path| -> io::Result<Vec<PathBuf>> {
            let mut found = Vec::new();
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                if !wanted
                    .iter()
                    .any(|name| *name.as_str() == entry.file_name())
                {
                    found.push(entry.path());
                }
            }
            Ok(found)
        };
        for path in unwanted(env_root)? {
            remove_entry(&path)?;
        }
        let stamps = beside(env_root, ".stamps");
        if matches!(self, Self::Mirror) && stamps.is_dir() {
            for path in unwanted(&stamps)? {
                remove_entry(&path)?;
            }
        }
        Ok(())
    }
}

/// Recreate the directory tree `target` at `link`, each file a hard link to the original,
/// or a copy where a hard link cannot be made (another volume, or a file system without
/// them). The store holds no symlinks, so only directories and files are mirrored.
fn mirror_tree(target: &Path, link: &Path) -> io::Result<()> {
    fs::create_dir_all(link)?;
    for entry in fs::read_dir(target)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let to = link.join(entry.file_name());
        if file_type.is_dir() {
            mirror_tree(&entry.path(), &to)?;
        } else if file_type.is_file() && fs::hard_link(entry.path(), &to).is_err() {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lockfile::LockedPackage;
    use tempfile::TempDir;

    // Well-formed content hashes (64 lowercase hex digits) for fixtures.
    const H_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const H_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const H_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

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
            submodules: false,
        }
    }

    #[test]
    fn exposes_each_package_under_its_name() {
        let (_root, store) = store_with(&[(H_A, "// bosl2"), (H_B, "// utils")]);
        let mut lock = Lockfile::new();
        lock.packages.push(locked("BOSL2", H_A));
        lock.packages.push(locked("scad-utils", H_B));

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
        let (_root, store) = store_with(&[(H_A, "// a"), (H_B, "// b")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");

        let mut first = Lockfile::new();
        first.packages.push(locked("A", H_A));
        first.packages.push(locked("B", H_B));
        build(&first, &store, &env_root).unwrap();

        // A later lockfile with only A must not leave B behind.
        let mut second = Lockfile::new();
        second.packages.push(locked("A", H_A));
        let env = build(&second, &store, &env_root).unwrap();

        assert_eq!(env.exposed, vec!["A".to_string()]);
        assert!(env_root.join("A").exists());
        assert!(!env_root.join("B").exists());
    }

    #[test]
    fn rebuilding_removes_the_links_not_the_store_content() {
        // Rebuilding deletes the environment. That must remove each link (hard links on
        // Windows) without touching the stored files.
        let (_root, store) = store_with(&[(H_A, "// a")]);
        let mut lock = Lockfile::new();
        lock.packages.push(locked("a", H_A));
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");
        build(&lock, &store, &env_root).unwrap();
        build(&lock, &store, &env_root).unwrap();
        fs::remove_dir_all(&env_root).unwrap();
        assert_eq!(
            fs::read_to_string(store.path_for(H_A).join("std.scad")).unwrap(),
            "// a"
        );
    }

    #[test]
    fn an_environment_over_protected_entries_can_be_rebuilt_and_removed() {
        // Stored files are write-protected. On Windows the environment's files are hard
        // links to them, so they carry the same read-only attribute, and replacing the
        // environment has to delete those links all the same, leaving the store as it was.
        let src = TempDir::new().unwrap();
        fs::create_dir_all(src.path().join("sub")).unwrap();
        fs::write(src.path().join("std.scad"), "// a").unwrap();
        fs::write(src.path().join("sub").join("more.scad"), "// b").unwrap();
        let temp = crate::store::TempStore::new();
        let entry = temp.store.insert(src.path()).unwrap();

        let mut lock = Lockfile::new();
        lock.packages.push(locked("a", &entry.hash));
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");
        build(&lock, &temp.store, &env_root).unwrap();
        build(&lock, &temp.store, &env_root).unwrap();
        assert_eq!(
            fs::read_to_string(env_root.join("a").join("sub").join("more.scad")).unwrap(),
            "// b"
        );
        assert!(
            fs::write(env_root.join("a").join("std.scad"), "// changed").is_err(),
            "a write through the environment must not reach the store"
        );
        fs::remove_dir_all(&env_root).unwrap();
        assert!(temp.store.verify(&entry.hash).unwrap());
        assert!(temp.store.is_protected(&entry.hash).unwrap());
    }

    #[test]
    fn mirror_tree_recreates_the_files_as_real_files() {
        let src = TempDir::new().unwrap();
        fs::create_dir_all(src.path().join("sub")).unwrap();
        fs::write(src.path().join("std.scad"), "a").unwrap();
        fs::write(src.path().join("sub").join("more.scad"), "b").unwrap();
        let out = TempDir::new().unwrap();
        let link = out.path().join("lib");
        mirror_tree(src.path(), &link).unwrap();
        for (rel, text) in [("std.scad", "a"), ("sub/more.scad", "b")] {
            let path = link.join(rel);
            assert!(
                !fs::symlink_metadata(&path)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
        assert!(
            !fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    /// A lockfile of one package, `lib`, with content hash `hash`.
    fn lock_of(hash: &str) -> Lockfile {
        let mut lock = Lockfile::new();
        lock.packages.push(locked("lib", hash));
        lock
    }

    #[test]
    fn a_mirrored_entry_that_is_already_right_is_left_alone() {
        let (_root, store) = store_with(&[(H_A, "// a")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");
        build_as(Exposure::Mirror, &lock_of(H_A), &store, &env_root).unwrap();
        // Something a rebuild of the entry would not put back.
        fs::write(env_root.join("lib").join("left.txt"), "").unwrap();

        build_as(Exposure::Mirror, &lock_of(H_A), &store, &env_root).unwrap();

        assert!(env_root.join("lib").join("left.txt").exists());
    }

    #[test]
    fn a_mirrored_entry_is_replaced_when_its_package_changes() {
        let (_root, store) = store_with(&[(H_A, "// a"), (H_B, "// b")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");
        build_as(Exposure::Mirror, &lock_of(H_A), &store, &env_root).unwrap();
        fs::write(env_root.join("lib").join("left.txt"), "").unwrap();

        build_as(Exposure::Mirror, &lock_of(H_B), &store, &env_root).unwrap();

        let entry = env_root.join("lib");
        assert_eq!(fs::read_to_string(entry.join("std.scad")).unwrap(), "// b");
        assert!(!entry.join("left.txt").exists());
        // And it is known to be right from then on.
        fs::write(entry.join("left.txt"), "").unwrap();
        build_as(Exposure::Mirror, &lock_of(H_B), &store, &env_root).unwrap();
        assert!(entry.join("left.txt").exists());
    }

    #[test]
    fn a_mirrored_entry_with_no_record_of_its_source_is_made_again() {
        // As after a build cut short between replacing the entry and recording it.
        let (_root, store) = store_with(&[(H_A, "// a")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");
        build_as(Exposure::Mirror, &lock_of(H_A), &store, &env_root).unwrap();
        fs::write(env_root.join("lib").join("left.txt"), "").unwrap();
        fs::remove_file(env_dir.path().join("env.stamps").join("lib")).unwrap();

        build_as(Exposure::Mirror, &lock_of(H_A), &store, &env_root).unwrap();

        assert!(!env_root.join("lib").join("left.txt").exists());
    }

    #[test]
    fn a_build_waits_for_another_build_of_the_same_environment() {
        let (_root, store) = store_with(&[(H_A, "// a")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");

        // Stand in for a build in progress, and start a second one.
        let first = hold(&env_root).unwrap();
        let (done, finished) = std::sync::mpsc::channel();
        let second = {
            let env_root = env_root.clone();
            std::thread::spawn(move || {
                let built = build(&lock_of(H_A), &store, &env_root);
                done.send(()).unwrap();
                built
            })
        };

        let wait = std::time::Duration::from_millis(300);
        assert!(
            finished.recv_timeout(wait).is_err(),
            "the second build went ahead while the first was running"
        );
        assert!(!env_root.exists());
        drop(first);
        second.join().unwrap().unwrap();
        assert!(env_root.join("lib").exists());
    }

    #[test]
    fn a_mirrored_environment_that_was_deleted_is_built_again() {
        // Deleting `.scadman/env` by hand leaves the records beside it.
        let (_root, store) = store_with(&[(H_A, "// a")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");
        build_as(Exposure::Mirror, &lock_of(H_A), &store, &env_root).unwrap();
        fs::remove_dir_all(&env_root).unwrap();

        build_as(Exposure::Mirror, &lock_of(H_A), &store, &env_root).unwrap();

        assert_eq!(
            fs::read_to_string(env_root.join("lib").join("std.scad")).unwrap(),
            "// a"
        );
    }

    #[test]
    fn a_mirrored_entry_the_lockfile_drops_is_removed_with_its_record() {
        let (_root, store) = store_with(&[(H_A, "// a")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");
        build_as(Exposure::Mirror, &lock_of(H_A), &store, &env_root).unwrap();

        build_as(Exposure::Mirror, &Lockfile::new(), &store, &env_root).unwrap();

        assert!(!env_root.join("lib").exists());
        assert!(!env_dir.path().join("env.stamps").join("lib").exists());
    }

    #[test]
    fn a_build_that_fails_leaves_the_environment_it_had() {
        let (_root, store) = store_with(&[(H_A, "// a")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");
        build(&lock_of(H_A), &store, &env_root).unwrap();

        // A second package that is not in the store: nothing may change for the first.
        let mut lock = lock_of(H_A);
        lock.packages.push(locked("Ghost", H_C));
        assert!(build(&lock, &store, &env_root).is_err());

        assert_eq!(
            fs::read_to_string(env_root.join("lib").join("std.scad")).unwrap(),
            "// a"
        );
    }

    #[test]
    fn something_else_standing_where_an_entry_goes_is_replaced() {
        // A directory where a link belongs, as an environment built another way leaves.
        let (_root, store) = store_with(&[(H_A, "// a")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");
        fs::create_dir_all(env_root.join("lib")).unwrap();
        fs::write(env_root.join("lib").join("old.scad"), "// old").unwrap();

        build(&lock_of(H_A), &store, &env_root).unwrap();

        assert_eq!(
            fs::read_to_string(env_root.join("lib").join("std.scad")).unwrap(),
            "// a"
        );
        assert!(!env_root.join("lib").join("old.scad").exists());
    }

    #[test]
    fn missing_store_entry_is_an_error() {
        let (_root, store) = store_with(&[]);
        let mut lock = Lockfile::new();
        lock.packages.push(locked("Ghost", H_C));
        let env_dir = TempDir::new().unwrap();
        assert!(build(&lock, &store, &env_dir.path().join("env")).is_err());
    }

    #[test]
    fn a_missing_root_is_told_apart_from_missing_content() {
        let (_root, store) = store_with(&[(H_A, "// a")]);
        let env_dir = TempDir::new().unwrap();

        // The package is in the store; the root inside it is not.
        let mut lock = Lockfile::new();
        let mut pkg = locked("A", H_A);
        pkg.root = "srcc".to_string();
        lock.packages.push(pkg);
        let err = build(&lock, &store, &env_dir.path().join("env"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("srcc") && err.contains("does not exist"),
            "should say the root is missing from the package: {err}"
        );
        assert!(
            !err.contains("not in the store"),
            "the content is in the store; only the root is wrong: {err}"
        );

        // The package itself is absent.
        let mut lock = Lockfile::new();
        lock.packages.push(locked("B", H_B));
        let err = build(&lock, &store, &env_dir.path().join("env"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not in the store"), "{err}");
    }

    #[test]
    fn exposes_a_subdir_root_and_adds_it_to_openscad_path() {
        let store_dir = TempDir::new().unwrap();
        let store = Store::new(store_dir.path());
        let entry = store.path_for(H_A);
        fs::create_dir_all(entry.join("src")).unwrap();
        fs::write(entry.join("src").join("lib.scad"), "// lib").unwrap();

        let mut lock = Lockfile::new();
        lock.packages.push(LockedPackage {
            name: "dotSCAD".to_string(),
            source: "s".to_string(),
            rev: "r".to_string(),
            hash: H_A.to_string(),
            dependencies: Vec::new(),
            root: "src".to_string(),
            on_path: true,
            submodules: false,
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
        let (_root, store) = store_with(&[(H_A, "// x")]);
        let mut lock = Lockfile::new();
        let mut pkg = locked("x", H_A);
        pkg.root = "../etc".to_string();
        lock.packages.push(pkg);
        let env_dir = TempDir::new().unwrap();
        assert!(build(&lock, &store, &env_dir.path().join("env")).is_err());
    }

    #[test]
    fn rejects_a_lockfile_hash_that_is_not_a_content_hash() {
        // The hash is joined onto the store root. An absolute path replaces the root
        // outright and `..` climbs out of it, so either would link the package to a
        // directory of the lockfile author's choosing.
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("private.txt"), "host-only").unwrap();
        let (_store_root, store) = store_with(&[]);
        let absolute = outside.path().to_string_lossy().into_owned();
        let climbing = format!("../../..{absolute}");
        for bad in [absolute.as_str(), climbing.as_str(), "", ".", "deadbeef"] {
            let mut lock = Lockfile::new();
            lock.packages.push(locked("mylib", bad));
            let env_dir = TempDir::new().unwrap();
            let env_root = env_dir.path().join("env");
            let err = build(&lock, &store, &env_root).unwrap_err().to_string();
            assert!(
                err.contains("hash"),
                "`{bad}` should be refused as a hash: {err}"
            );
            assert!(
                !env_root.join("mylib").exists(),
                "`{bad}`: nothing may be linked"
            );
        }
    }

    #[test]
    fn a_bad_entry_is_refused_before_the_environment_is_touched() {
        // Validation runs over the whole lockfile first: a bad second package must not
        // leave the old environment deleted and a new one half built.
        let (_root, store) = store_with(&[(H_A, "// a")]);
        let env_dir = TempDir::new().unwrap();
        let env_root = env_dir.path().join("env");
        let mut good = Lockfile::new();
        good.packages.push(locked("A", H_A));
        build(&good, &store, &env_root).unwrap();

        let mut bad = Lockfile::new();
        bad.packages.push(locked("A", H_A));
        bad.packages.push(locked("B", "/etc"));
        assert!(build(&bad, &store, &env_root).is_err());
        assert!(
            env_root.join("A").join("std.scad").exists(),
            "the existing environment must be left as it was"
        );
    }

    #[test]
    fn rejects_unsafe_package_name_from_the_lockfile() {
        // A tampered/committed lockfile must not be able to plant a symlink outside the env.
        let (_root, store) = store_with(&[]);
        let mut lock = Lockfile::new();
        lock.packages.push(locked("../evil", H_A));
        let env_dir = TempDir::new().unwrap();
        let err = build(&lock, &store, &env_dir.path().join("env")).unwrap_err();
        assert!(err.to_string().contains("valid package name"));
    }
}
