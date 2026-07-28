//! Acquiring package sources into the content store.
//!
//! Given a Git URL and a reference (a commit SHA, tag, or branch), fetch that revision
//! with the system `git` and land its content in the [`crate::store`]. Only the
//! requested revision is fetched (shallow), and the resolved commit SHA is returned so a
//! branch or tag can be pinned in the lockfile. The `.git` directory is dropped by the
//! store, so what lands is the working tree alone.

use std::io;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::store::{Store, StoreEntry};

/// A package fetched into the store.
#[derive(Debug, Clone)]
pub struct Acquired {
    /// Where the content landed in the store.
    pub entry: StoreEntry,
    /// The exact commit the reference resolved to.
    pub rev: String,
}

/// Fetch a Git `reference` from `url` and store its content.
///
/// `reference` may be a commit SHA, tag, or branch. Requires `git` on `PATH`. Fetching a
/// bare commit SHA shallowly requires the host to allow fetching unadvertised objects
/// (GitHub does; some self-hosted servers do not) — tags and branches work everywhere.
/// A non-shallow fallback for restrictive hosts is deferred until non-GitHub sources are
/// in scope.
pub fn fetch_git(store: &Store, url: &str, reference: &str) -> io::Result<Acquired> {
    let scratch = TempDir::new()?;
    let dir = scratch.path();

    run_git(dir, &["init", "--quiet"])?;
    // `--` guards a `-`-leading URL from being read as a git option (defense in depth;
    // the URL comes from a manifest/lockfile).
    run_git(dir, &["remote", "add", "origin", "--", url])?;
    // `--` before `reference` is load-bearing: without it a reference beginning with
    // `-` (e.g. `--upload-pack=…`) is parsed as a git option, which is arbitrary command
    // execution over local/ssh transports. References come from manifests/lockfiles.
    run_git(
        dir,
        &[
            "fetch", "--depth", "1", "--quiet", "origin", "--", reference,
        ],
    )?;
    run_git(dir, &["checkout", "--quiet", "--detach", "FETCH_HEAD"])?;

    let rev = capture_git(dir, &["rev-parse", "HEAD"])?.trim().to_string();
    let entry = store.insert(dir)?;
    Ok(Acquired { entry, rev })
}

fn git(cwd: &Path, args: &[&str]) -> io::Result<std::process::Output> {
    Command::new("git").arg("-C").arg(cwd).args(args).output()
}

fn run_git(cwd: &Path, args: &[&str]) -> io::Result<()> {
    let output = git(cwd, args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(git_error(args, &output.stderr))
    }
}

fn capture_git(cwd: &Path, args: &[&str]) -> io::Result<String> {
    let output = git(cwd, args)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(git_error(args, &output.stderr))
    }
}

fn git_error(args: &[&str], stderr: &[u8]) -> io::Error {
    io::Error::other(format!(
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(stderr).trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Run git in `dir`, asserting success (test setup only).
    fn setup_git(dir: &Path, args: &[&str]) {
        let output = git(dir, args).expect("spawn git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn make_source_repo() -> (TempDir, String) {
        let repo = TempDir::new().unwrap();
        let dir = repo.path();
        setup_git(dir, &["init", "--quiet"]);
        fs::write(dir.join("std.scad"), "x=1;").unwrap();
        setup_git(dir, &["add", "."]);
        setup_git(
            dir,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "commit",
                "--quiet",
                "-m",
                "init",
            ],
        );
        let rev = capture_git(dir, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();
        (repo, rev)
    }

    #[test]
    fn fetches_a_revision_into_the_store() {
        let (repo, rev) = make_source_repo();
        // file:// so git honors --depth against a local repo.
        let url = format!("file://{}", repo.path().display());

        let store_root = TempDir::new().unwrap();
        let store = Store::new(store_root.path());

        let acquired = fetch_git(&store, &url, &rev).unwrap();
        assert_eq!(acquired.rev, rev);
        assert_eq!(
            fs::read_to_string(acquired.entry.path.join("std.scad")).unwrap(),
            "x=1;"
        );
        // The store strips .git — only the working tree lands.
        assert!(!acquired.entry.path.join(".git").exists());
    }

    #[test]
    fn missing_revision_is_an_error() {
        let (repo, _) = make_source_repo();
        let url = format!("file://{}", repo.path().display());
        let store_root = TempDir::new().unwrap();
        let store = Store::new(store_root.path());

        let err = fetch_git(&store, &url, "0000000000000000000000000000000000000000");
        assert!(err.is_err());
    }

    #[test]
    fn flag_like_reference_is_treated_as_a_ref_not_an_option() {
        // With the `--` guard, a reference beginning with `-` is a (bogus) refspec that
        // fails cleanly, rather than a git option — the argument-injection guard.
        let (repo, _) = make_source_repo();
        let url = format!("file://{}", repo.path().display());
        let store_root = TempDir::new().unwrap();
        let store = Store::new(store_root.path());

        let err = fetch_git(&store, &url, "--not-a-real-ref");
        assert!(err.is_err());
    }
}
