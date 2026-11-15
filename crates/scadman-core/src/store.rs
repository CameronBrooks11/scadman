//! A content-addressed store for immutable package content.
//!
//! Package trees are hashed (see [`content_hash`]) and landed at
//! `<root>/<hash>/`. Identical content deduplicates to one entry, entries are never
//! mutated in place, and inserts are atomic (staged then renamed), so a crash cannot
//! leave a half-written entry that looks complete. Project environments link into the
//! store rather than copying (see the `environment` module).
//!
//! Because every project on the machine links to the same entry, a write that reaches one
//! through an environment would change them all. So an entry is write-protected before it
//! appears (see [`Store::protect`]), and "write-protected" doubles as "known to match its
//! hash": an entry found writable is re-hashed before it is trusted
//! ([`Store::is_protected`], [`Store::verify`]).

use std::ffi::OsStr;
use std::fs;
use std::io;
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
    /// Whether an entry was already there under this hash with other content, and was
    /// replaced. It means something wrote into the store after the entry was made.
    pub restored: bool,
}

/// Check that `hash` has the shape [`content_hash`] produces: 64 lowercase hex digits.
///
/// A hash becomes a path segment under the store root ([`Store::path_for`]). One read
/// from a lockfile is untrusted, and a value like `/etc` or `../..` would name a
/// directory outside the store, so anything else is refused before it is joined.
pub fn validate_content_hash(hash: &str) -> Result<(), String> {
    let well_formed = hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if well_formed {
        Ok(())
    } else {
        Err(format!(
            "hash `{hash}` is not a content hash (64 lowercase hex digits)"
        ))
    }
}

impl Store {
    /// A store rooted at `root` (created lazily on first insert).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The default per-user store root: `$XDG_DATA_HOME/scadman/store`, falling back to
    /// `$HOME/.local/share/scadman/store`, or on Windows to
    /// `%LOCALAPPDATA%\scadman\store`. Returns `None` if none of them is set.
    pub fn default_root() -> Option<PathBuf> {
        // Per the XDG spec a relative $XDG_DATA_HOME is invalid and must be ignored.
        if let Some(dir) = absolute_var("XDG_DATA_HOME") {
            return Some(dir.join("scadman").join("store"));
        }
        if cfg!(windows) {
            return Some(absolute_var("LOCALAPPDATA")?.join("scadman").join("store"));
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

    /// Insert `src`'s content, returning its entry. Idempotent: if an entry with this hash
    /// exists and still matches it, that entry is kept and `src` is not copied again. One
    /// that no longer matches was modified after it was stored, and is replaced.
    pub fn insert(&self, src: &Path) -> io::Result<StoreEntry> {
        let hash = content_hash(src)?;
        let dest = self.path_for(&hash);
        let modified = fs::symlink_metadata(&dest).is_ok() && !self.verify(&hash)?;
        if dest.exists() && !modified {
            // Entries stored before write protection existed get it here.
            self.protect(&hash).ok();
            return Ok(StoreEntry {
                hash,
                path: dest,
                restored: false,
            });
        }

        fs::create_dir_all(&self.root)?;
        let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
        let staging = self.root.join(format!(
            ".staging-{}-{}-{}",
            std::process::id(),
            seq,
            &hash[..16]
        ));
        // The staging name is unique to this process and call, so no later insert reuses
        // or clears it: a failed copy has to remove its own partial tree.
        if let Err(err) = copy_tree(src, &staging) {
            fs::remove_dir_all(&staging).ok();
            return Err(err);
        }
        // Protect before the rename, so an entry is never visible unprotected. Best
        // effort: a file system that cannot drop write permission still gets the right
        // content, and its entries are re-hashed on use instead.
        protect_tree(&staging).ok();

        // Only now that the replacement is complete does the modified entry go.
        if modified && let Err(err) = remove_tree(&dest) {
            remove_tree(&staging).ok();
            return Err(err);
        }
        match fs::rename(&staging, &dest) {
            Ok(()) => {}
            // Another writer landed the same content first; keep theirs, drop staging.
            Err(_) if dest.exists() => {
                remove_tree(&staging).ok();
            }
            Err(err) => {
                remove_tree(&staging).ok();
                return Err(err);
            }
        }
        Ok(StoreEntry {
            hash,
            path: dest,
            restored: modified,
        })
    }

    /// Whether the entry for `hash` still holds exactly the content that hashes to it.
    /// False for a missing entry, and for one holding anything a stored tree never has
    /// (a symlink, a `.git`), which the hash itself would skip over.
    pub fn verify(&self, hash: &str) -> io::Result<bool> {
        let entry = self.path_for(hash);
        match fs::symlink_metadata(&entry) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => return Ok(false),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(err),
        }
        let (actual, skipped) = hash_tree(&entry)?;
        Ok(!skipped && actual == hash)
    }

    /// Take write permission off the entry for `hash`: every file, and on Unix every
    /// directory, so nothing can be added either. (A Windows directory has no such switch;
    /// there the environment's directories are its own, and only the files are shared.)
    ///
    /// Call it only for an entry known to match its hash: [`Store::is_protected`] is read
    /// as "already verified".
    pub fn protect(&self, hash: &str) -> io::Result<()> {
        protect_tree(&self.path_for(hash))
    }

    /// Whether the entry for `hash` is write-protected throughout, as [`Store::protect`]
    /// leaves it. An entry that is not has been open to writes since it was last verified.
    pub fn is_protected(&self, hash: &str) -> io::Result<bool> {
        is_protected_tree(&self.path_for(hash))
    }
}

/// A deterministic content hash of a directory tree.
///
/// Files are hashed in sorted relative-path order; each contributes its path, a NUL
/// separator, the content byte length, and the content bytes — so both content and layout
/// matter. A path is hashed as its components joined with `/` (see `hashed_path`), and a
/// nested `.git` directory is ignored, so a Git checkout hashes to its working-tree content
/// alone.
///
/// The hash is the same on Linux, macOS and Windows: the separator is always `/`, and file
/// mode is not hashed. Line endings are kept out of it by fetching with git's conversion
/// off (see [`crate::source`]). One residual cross-OS hazard is a filename differing only
/// by Unicode normalization form (NFC vs NFD), which git's default
/// `core.precomposeUnicode` avoids on macOS.
pub fn content_hash(dir: &Path) -> io::Result<String> {
    Ok(hash_tree(dir)?.0)
}

/// [`content_hash`], and whether anything under `dir` was skipped (a `.git`, a symlink).
fn hash_tree(dir: &Path) -> io::Result<(String, bool)> {
    let mut files = Vec::new();
    let mut skipped = false;
    collect_files(dir, PathBuf::new(), &mut files, &mut skipped)?;
    files.sort();

    let mut hasher = Sha256::new();
    for rel in files {
        let bytes = fs::read(dir.join(&rel))?;
        hasher.update(hashed_path(&rel)?);
        hasher.update([0u8]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok((hex(&hasher.finalize()), skipped))
}

/// An environment variable holding an absolute path, if it is set to one.
fn absolute_var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|d| !d.is_empty() && Path::new(d).is_absolute())
        .map(PathBuf::from)
}

/// A relative path as the hash sees it: its components joined with `/`, whatever the
/// platform's separator. On Unix a component is its raw bytes, so names differing only in
/// invalid UTF-8 stay distinct; elsewhere it must be valid Unicode, and is hashed as UTF-8.
fn hashed_path(rel: &Path) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    for (i, component) in rel.components().enumerate() {
        if i > 0 {
            out.push(b'/');
        }
        out.extend_from_slice(component_bytes(component.as_os_str())?);
    }
    Ok(out)
}

#[cfg(unix)]
fn component_bytes(name: &OsStr) -> io::Result<&[u8]> {
    use std::os::unix::ffi::OsStrExt;
    Ok(name.as_bytes())
}

#[cfg(not(unix))]
fn component_bytes(name: &OsStr) -> io::Result<&[u8]> {
    name.to_str().map(str::as_bytes).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file name {name:?} is not valid Unicode, so it cannot be hashed"),
        )
    })
}

fn collect_files(
    base: &Path,
    rel: PathBuf,
    out: &mut Vec<PathBuf>,
    skipped: &mut bool,
) -> io::Result<()> {
    for entry in fs::read_dir(base.join(&rel))? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            *skipped = true;
            continue;
        }
        let file_type = entry.file_type()?;
        // Skip symlinks: following them into the store would let a link like
        // `evil -> /etc/passwd` pull host content into an entry later exposed to OpenSCAD.
        if file_type.is_symlink() {
            *skipped = true;
            continue;
        }
        let child = rel.join(&name);
        if file_type.is_dir() {
            collect_files(base, child, out, skipped)?;
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
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue; // never copy symlinks into the store (see collect_files)
        }
        let from = entry.path();
        let to = dest.join(&name);
        if file_type.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Take write permission off every file under `dir`, and on Unix off every directory
/// including `dir`. Symlinks are left alone: changing one's permissions changes its target's.
fn protect_tree(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            protect_tree(&entry.path())?;
        } else if file_type.is_file() {
            set_readonly(&entry.path())?;
        }
    }
    if cfg!(unix) {
        set_readonly(dir)?;
    }
    Ok(())
}

fn set_readonly(path: &Path) -> io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    if !permissions.readonly() {
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

/// Whether [`protect_tree`] has nothing left to do under `dir`. A symlink counts as
/// unprotected: a stored tree has none, so one was put there afterwards.
fn is_protected_tree(dir: &Path) -> io::Result<bool> {
    if cfg!(unix) && !fs::metadata(dir)?.permissions().readonly() {
        return Ok(false);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let protected = if file_type.is_dir() {
            is_protected_tree(&entry.path())?
        } else {
            file_type.is_file() && entry.metadata()?.permissions().readonly()
        };
        if !protected {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Remove `path` and everything under it, write-protected or not.
fn remove_tree(path: &Path) -> io::Result<()> {
    if !fs::symlink_metadata(path)?.file_type().is_dir() {
        return fs::remove_file(path);
    }
    make_writable(path)?;
    fs::remove_dir_all(path)
}

/// Give back the write permission deleting needs: on Unix a directory's, to unlink what
/// it holds; elsewhere a file's own. Symlinks are not followed.
fn make_writable(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(dir)?.permissions();
        permissions.set_mode(permissions.mode() | 0o200);
        fs::set_permissions(dir, permissions)?;
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            make_writable(&entry.path())?;
        } else if cfg!(not(unix)) && file_type.is_file() {
            let mut permissions = entry.metadata()?.permissions();
            // Not the Unix trap the lint is about: this branch never runs there.
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(entry.path(), permissions)?;
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

/// A store in a temporary directory, for tests. Its entries are write-protected, which on
/// Unix a temporary directory cannot delete, so dropping this gives the permission back.
#[cfg(test)]
pub(crate) struct TempStore {
    pub(crate) store: Store,
    dir: tempfile::TempDir,
}

#[cfg(test)]
impl TempStore {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self {
            store: Store::new(dir.path()),
            dir,
        }
    }

    pub(crate) fn root(&self) -> &Path {
        self.dir.path()
    }
}

#[cfg(test)]
impl Drop for TempStore {
    fn drop(&mut self) {
        make_writable(self.dir.path()).ok();
    }
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
    fn content_hash_shape_is_validated() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "std.scad", "x=1;");
        // What content_hash produces is accepted.
        assert!(validate_content_hash(&content_hash(d.path()).unwrap()).is_ok());
        let good = "0123456789abcdef".repeat(4);
        assert!(validate_content_hash(&good).is_ok());
        for bad in [
            "",
            ".",
            "..",
            "/etc",
            "../../victim",
            "deadbeef",
            &good.to_uppercase(),
            &format!("{good}0"),
            &format!("{}/", &good[..63]),
        ] {
            assert!(validate_content_hash(bad).is_err(), "should reject `{bad}`");
        }
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

        let temp = TempStore::new();
        let store = &temp.store;

        let first = store.insert(src.path()).unwrap();
        assert!(first.path.join("std.scad").exists());
        assert_eq!(
            fs::read_to_string(first.path.join("std.scad")).unwrap(),
            "x=1;"
        );

        let second = store.insert(src.path()).unwrap();
        assert_eq!(first, second); // same hash + path, no error
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped_not_followed() {
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("secret"), "host-only").unwrap();

        let src = tempfile::tempdir().unwrap();
        write(src.path(), "std.scad", "x=1;");
        std::os::unix::fs::symlink(external.path().join("secret"), src.path().join("leak"))
            .unwrap();

        // The symlink neither appears in the store nor perturbs the hash.
        let plain = tempfile::tempdir().unwrap();
        write(plain.path(), "std.scad", "x=1;");
        assert_eq!(
            content_hash(src.path()).unwrap(),
            content_hash(plain.path()).unwrap()
        );

        let temp = TempStore::new();
        let entry = temp.store.insert(src.path()).unwrap();
        assert!(entry.path.join("std.scad").exists());
        assert!(!entry.path.join("leak").exists());
    }

    /// A source tree with a file at the root and one in a subdirectory.
    fn source() -> tempfile::TempDir {
        let src = tempfile::tempdir().unwrap();
        write(src.path(), "std.scad", "x=1;");
        write(src.path(), "lib/util.scad", "y=2;");
        src
    }

    /// Lift an entry's protection, as someone set on changing it would.
    fn unprotect(entry: &Path) {
        make_writable(entry).unwrap();
        #[cfg(unix)]
        for file in ["std.scad", "lib/util.scad"] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(entry.join(file), fs::Permissions::from_mode(0o644)).unwrap();
        }
    }

    #[test]
    fn an_inserted_entry_cannot_be_written_to() {
        // Every project links to the same entry, so a write through one project's
        // environment (an edit, or Python leaving `__pycache__` beside a library's
        // scripts) used to change the library for all of them.
        let src = source();
        let temp = TempStore::new();
        let entry = temp.store.insert(src.path()).unwrap();

        assert!(temp.store.is_protected(&entry.hash).unwrap());
        assert!(
            fs::write(entry.path.join("std.scad"), "x=9;").is_err(),
            "an existing file must not be writable"
        );
        assert!(
            fs::write(entry.path.join("lib").join("util.scad"), "y=9;").is_err(),
            "nor one in a subdirectory"
        );
        // Windows has no directory permission to withhold; there the environment's
        // directories are its own, so a new file never lands in the store.
        #[cfg(unix)]
        {
            assert!(fs::write(entry.path.join("new.scad"), "z").is_err());
            assert!(fs::create_dir(entry.path.join("lib").join("__pycache__")).is_err());
        }
        assert!(temp.store.verify(&entry.hash).unwrap());
    }

    #[test]
    fn verify_notices_every_kind_of_change() {
        type Change = fn(&Path);
        let changes: [(&str, Change); 4] = [
            ("an edited file", |e| {
                fs::write(e.join("std.scad"), "x=9;").unwrap()
            }),
            ("an added file", |e| {
                write(e, "lib/__pycache__/util.pyc", "bytecode")
            }),
            ("a removed file", |e| {
                fs::remove_file(e.join("lib").join("util.scad")).unwrap()
            }),
            // The content hash skips a `.git`, so equal hashes alone would miss this.
            ("an added .git", |e| write(e, ".git/config", "[core]")),
        ];
        for (what, change) in changes {
            let src = source();
            let temp = TempStore::new();
            let entry = temp.store.insert(src.path()).unwrap();
            unprotect(&entry.path);
            change(&entry.path);
            assert!(!temp.store.verify(&entry.hash).unwrap(), "{what}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn verify_notices_an_added_symlink() {
        // Skipped by the content hash, like `.git`; OpenSCAD would follow it.
        let src = source();
        let temp = TempStore::new();
        let entry = temp.store.insert(src.path()).unwrap();
        unprotect(&entry.path);
        std::os::unix::fs::symlink("/etc/hostname", entry.path.join("extra.scad")).unwrap();
        assert!(!temp.store.verify(&entry.hash).unwrap());
        assert!(!temp.store.is_protected(&entry.hash).unwrap());
    }

    #[test]
    fn verify_is_false_for_an_entry_that_is_not_there() {
        let temp = TempStore::new();
        assert!(!temp.store.verify(&"0".repeat(64)).unwrap());
    }

    #[test]
    fn unprotecting_an_entry_shows() {
        // Protection is what stands for "verified", so lifting it must be visible.
        let src = source();
        let temp = TempStore::new();
        let entry = temp.store.insert(src.path()).unwrap();
        unprotect(&entry.path);
        assert!(!temp.store.is_protected(&entry.hash).unwrap());
        temp.store.protect(&entry.hash).unwrap();
        assert!(temp.store.is_protected(&entry.hash).unwrap());
    }

    #[test]
    fn insert_replaces_an_entry_that_was_modified() {
        // Inserting used to return whatever sat under the hash, so a fresh `lock` of the
        // same commit handed out the modified copy under the original hash.
        let src = source();
        let temp = TempStore::new();
        let first = temp.store.insert(src.path()).unwrap();
        assert!(!first.restored);
        unprotect(&first.path);
        fs::write(first.path.join("std.scad"), "x=9;").unwrap();
        write(&first.path, "lib/__pycache__/util.pyc", "bytecode");

        let again = temp.store.insert(src.path()).unwrap();
        assert!(again.restored, "the replacement is reported");
        assert_eq!(again.path, first.path);
        assert_eq!(
            fs::read_to_string(again.path.join("std.scad")).unwrap(),
            "x=1;"
        );
        assert!(!again.path.join("lib").join("__pycache__").exists());
        assert!(temp.store.verify(&again.hash).unwrap());
        assert!(temp.store.is_protected(&again.hash).unwrap());
        let names: Vec<_> = fs::read_dir(temp.root())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            std::slice::from_ref(&again.hash),
            "no staging or old copy left"
        );
    }

    #[test]
    fn insert_protects_an_intact_entry_stored_without_protection() {
        // An entry written by a scadman from before entries were protected.
        let src = source();
        let temp = TempStore::new();
        let hash = content_hash(src.path()).unwrap();
        copy_tree(src.path(), &temp.store.path_for(&hash)).unwrap();
        assert!(!temp.store.is_protected(&hash).unwrap());

        let entry = temp.store.insert(src.path()).unwrap();
        assert!(!entry.restored);
        assert!(temp.store.is_protected(&hash).unwrap());
    }

    #[test]
    fn ascii_tree_hash_is_stable() {
        // A golden value that pins the hash scheme: a change to path serialization,
        // ordering, or framing would break existing lockfiles, so it must be deliberate.
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "std.scad", "x=1;");
        write(d.path(), "lib/util.scad", "y=2;");
        write(d.path(), "lib/sub/deep.scad", "z=3;");
        assert_eq!(
            content_hash(d.path()).unwrap(),
            "e6e5ec53c7350f3fd1c7f4d01d5eac2e8f8669931f7c8260eb5aaba9f28bae8a"
        );
    }

    #[test]
    fn hash_orders_paths_by_component_not_flat_bytes() {
        // A root file `lib.scad` sorts AFTER the directory `lib/` under component-wise
        // ordering (`lib` < `lib.scad`), but BEFORE it under flat-byte ordering (`.`=0x2e <
        // `/`=0x2f). This golden pins the component-wise order the scheme relies on.
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "lib.scad", "a=1;");
        write(d.path(), "lib/x.scad", "b=2;");
        assert_eq!(
            content_hash(d.path()).unwrap(),
            "1a67b2454b943ef5749246d609c0a3615326dee44db53879c433dff941805b11"
        );
    }

    #[test]
    fn a_path_is_hashed_with_forward_slashes() {
        // Built with the platform separator, so on Windows this holds `\`.
        let rel = PathBuf::from("lib").join("sub").join("deep.scad");
        assert_eq!(hashed_path(&rel).unwrap(), b"lib/sub/deep.scad");
    }

    #[cfg(unix)]
    #[test]
    fn executable_bit_does_not_affect_hash() {
        use std::os::unix::fs::PermissionsExt;
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "std.scad", "x=1;");
        let before = content_hash(a.path()).unwrap();
        fs::set_permissions(a.path().join("std.scad"), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(before, content_hash(a.path()).unwrap());
    }
}
