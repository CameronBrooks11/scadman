//! A content-addressed store for immutable package content.
//!
//! Package trees are hashed (see [`content_hash`]) and landed at
//! `<root>/<hash>/`. Identical content deduplicates to one entry, entries are never
//! mutated in place, and inserts are atomic (staged then renamed), so a crash cannot
//! leave a half-written entry that looks complete. Project environments link into the
//! store rather than copying (a later slice).

use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

/// Per-process counter making each staging directory name unique, so concurrent
/// inserts of identical content never share (and clobber) a staging path.
static STAGING_SEQ: AtomicU64 = AtomicU64::new(0);

/// A content-addressed store rooted at a directory.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

/// A stored entry: its content hash and the path it lives at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreEntry {
    pub hash: String,
    pub path: PathBuf,
}

impl Store {
    /// A store rooted at `root` (created lazily on first insert).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The default per-user store root: `$XDG_DATA_HOME/scadman/store`, falling back to
    /// `$HOME/.local/share/scadman/store`. Returns `None` if neither is set.
    pub fn default_root() -> Option<PathBuf> {
        // Per the XDG spec a relative $XDG_DATA_HOME is invalid and must be ignored.
        if let Some(dir) = std::env::var_os("XDG_DATA_HOME")
            .filter(|d| !d.is_empty() && Path::new(d).is_absolute())
        {
            return Some(PathBuf::from(dir).join("scadman").join("store"));
        }
        let home = std::env::var_os("HOME").filter(|d| !d.is_empty())?;
        Some(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("scadman")
                .join("store"),
        )
    }

    /// The path an entry with `hash` occupies (whether or not it exists yet).
    pub fn path_for(&self, hash: &str) -> PathBuf {
        self.root.join(hash)
    }

    /// Insert `src`'s content, returning its entry. Idempotent: if the hash already
    /// exists, the existing (immutable) entry is kept and `src` is not copied again.
    pub fn insert(&self, src: &Path) -> io::Result<StoreEntry> {
        let hash = content_hash(src)?;
        let dest = self.path_for(&hash);
        if dest.exists() {
            return Ok(StoreEntry { hash, path: dest });
        }

        fs::create_dir_all(&self.root)?;
        let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
        let staging = self.root.join(format!(
            ".staging-{}-{}-{}",
            std::process::id(),
            seq,
            &hash[..16]
        ));
        copy_tree(src, &staging)?;

        match fs::rename(&staging, &dest) {
            Ok(()) => {}
            // Another writer landed the same content first; keep theirs, drop staging.
            Err(_) if dest.exists() => {
                fs::remove_dir_all(&staging).ok();
            }
            Err(err) => {
                fs::remove_dir_all(&staging).ok();
                return Err(err);
            }
        }
        Ok(StoreEntry { hash, path: dest })
    }
}

/// A deterministic content hash of a directory tree.
///
/// Files are hashed in sorted relative-path order, each contribution covering the path,
/// a NUL separator, the byte length, and the bytes — so both content and layout matter.
/// Paths are hashed as their raw OS bytes (not lossily decoded), so names differing only
/// in invalid UTF-8 stay distinct. A nested `.git` directory is ignored, so a Git
/// checkout hashes to its content alone. The hash is stable per machine (the store key
/// only needs that); path-separator conventions make it OS-specific.
pub fn content_hash(dir: &Path) -> io::Result<String> {
    let mut files = Vec::new();
    collect_files(dir, PathBuf::new(), &mut files)?;
    files.sort();

    let mut hasher = Sha256::new();
    for rel in files {
        let bytes = fs::read(dir.join(&rel))?;
        hasher.update(rel.as_os_str().as_bytes());
        hasher.update([0u8]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hex(&hasher.finalize()))
}

fn collect_files(base: &Path, rel: PathBuf, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(base.join(&rel))? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let child = rel.join(&name);
        if entry.file_type()?.is_dir() {
            collect_files(base, child, out)?;
        } else {
            out.push(child);
        }
    }
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dest.join(&name);
        if entry.file_type()?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0xf) as u32, 16).unwrap());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn hash_is_deterministic_and_content_sensitive() {
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "std.scad", "x=1;");
        write(a.path(), "lib/util.scad", "y=2;");

        let b = tempfile::tempdir().unwrap();
        write(b.path(), "std.scad", "x=1;");
        write(b.path(), "lib/util.scad", "y=2;");

        let c = tempfile::tempdir().unwrap();
        write(c.path(), "std.scad", "x=1;");
        write(c.path(), "lib/util.scad", "y=3;"); // differs

        assert_eq!(
            content_hash(a.path()).unwrap(),
            content_hash(b.path()).unwrap()
        );
        assert_ne!(
            content_hash(a.path()).unwrap(),
            content_hash(c.path()).unwrap()
        );
    }

    #[test]
    fn git_dir_does_not_affect_hash() {
        let plain = tempfile::tempdir().unwrap();
        write(plain.path(), "std.scad", "x=1;");

        let with_git = tempfile::tempdir().unwrap();
        write(with_git.path(), "std.scad", "x=1;");
        write(with_git.path(), ".git/config", "[core]");

        assert_eq!(
            content_hash(plain.path()).unwrap(),
            content_hash(with_git.path()).unwrap()
        );
    }

    #[test]
    fn insert_is_idempotent_and_copies_content() {
        let src = tempfile::tempdir().unwrap();
        write(src.path(), "std.scad", "x=1;");

        let store_root = tempfile::tempdir().unwrap();
        let store = Store::new(store_root.path());

        let first = store.insert(src.path()).unwrap();
        assert!(first.path.join("std.scad").exists());
        assert_eq!(
            fs::read_to_string(first.path.join("std.scad")).unwrap(),
            "x=1;"
        );

        let second = store.insert(src.path()).unwrap();
        assert_eq!(first, second); // same hash + path, no error
    }
}
