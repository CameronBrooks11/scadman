//! End-to-end tests driving the built `scadman` binary against a local `file://` git repo
//! in an isolated `XDG_DATA_HOME`, so the whole lock/sync pipeline is exercised for real.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn scadman(project: &Path, store_home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scadman"))
        .current_dir(project)
        .env("XDG_DATA_HOME", store_home)
        .args(args)
        .output()
        .expect("run scadman")
}

const GREET: &str = "module greet() { cube(1); }\n";

/// A git repo exposing `greet.scad`; returns its path and HEAD commit.
fn make_lib(root: &Path) -> (PathBuf, String) {
    let lib = root.join("mylib");
    fs::create_dir_all(&lib).unwrap();
    fs::write(lib.join("greet.scad"), GREET).unwrap();
    git(&lib, &["init", "--quiet"]);
    git(&lib, &["add", "-A"]);
    git(
        &lib,
        &[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "--quiet",
            "-m",
            "init",
        ],
    );
    let out = Command::new("git")
        .arg("-C")
        .arg(&lib)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let rev = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (lib, rev)
}

fn project_with(root: &Path, toml: &str) -> PathBuf {
    let proj = root.join("proj");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("scadman.toml"), toml).unwrap();
    proj
}

#[test]
fn lock_then_sync_end_to_end() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"demo\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", rev = \"{rev}\" }}\n",
            lib.display()
        ),
    );

    let lock = scadman(&proj, store.path(), &["lock"]);
    assert!(
        lock.status.success(),
        "lock failed: {}",
        String::from_utf8_lossy(&lock.stderr)
    );
    let lockfile = fs::read_to_string(proj.join("scadman.lock")).unwrap();
    assert!(
        lockfile.contains(&rev),
        "lockfile should pin the resolved rev"
    );

    let sync = scadman(&proj, store.path(), &["sync"]);
    assert!(
        sync.status.success(),
        "sync failed: {}",
        String::from_utf8_lossy(&sync.stderr)
    );
    // `<mylib/greet.scad>` resolves through the env symlink to the stored content.
    let via_env = proj
        .join(".scadman")
        .join("env")
        .join("mylib")
        .join("greet.scad");
    assert_eq!(fs::read_to_string(via_env).unwrap(), GREET);
}

#[test]
fn sync_rejects_a_lockfile_with_a_wrong_hash() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let source = format!("file://{}", lib.display());
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"demo\"\n\n[dependencies]\nmylib = {{ git = \"{source}\", rev = \"{rev}\" }}\n"
        ),
    );
    // Correct source + rev, but a bogus content hash: a fresh sync fetches and must reject.
    fs::write(
        proj.join("scadman.lock"),
        format!(
            "version = 1\n\n[[package]]\nname = \"mylib\"\nsource = \"{source}\"\nrev = \"{rev}\"\nhash = \"deadbeef\"\n"
        ),
    )
    .unwrap();

    let sync = scadman(&proj, store.path(), &["sync"]);
    assert!(
        !sync.status.success(),
        "sync should fail on a hash mismatch"
    );
    assert!(
        String::from_utf8_lossy(&sync.stderr).contains("integrity"),
        "expected an integrity error"
    );
}

#[test]
fn sync_rejects_a_stale_lockfile() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let source = format!("file://{}", lib.display());
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"demo\"\n\n[dependencies]\nmylib = {{ git = \"{source}\", rev = \"{rev}\" }}\n"
        ),
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());

    // Declare a second dependency without re-locking → the lock is now stale.
    fs::write(
        proj.join("scadman.toml"),
        format!(
            "[project]\nname = \"demo\"\n\n[dependencies]\nmylib = {{ git = \"{source}\", rev = \"{rev}\" }}\nother = {{ git = \"file:///nonexistent\", rev = \"deadbeef\" }}\n"
        ),
    )
    .unwrap();

    let sync = scadman(&proj, store.path(), &["sync"]);
    assert!(!sync.status.success(), "sync should fail on a stale lock");
    assert!(
        String::from_utf8_lossy(&sync.stderr).contains("out of date"),
        "expected a staleness error"
    );
}

#[test]
fn init_scaffolds_gitignore() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = root.path().join("p");
    fs::create_dir_all(&proj).unwrap();

    assert!(
        scadman(&proj, store.path(), &["init", "--name", "demo"])
            .status
            .success()
    );
    let gitignore = fs::read_to_string(proj.join(".gitignore")).unwrap();
    assert!(
        gitignore.contains(".scadman/"),
        "init should ignore .scadman/"
    );
}

#[test]
fn add_remove_and_list() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = root.path().join("p");
    fs::create_dir_all(&proj).unwrap();
    assert!(
        scadman(&proj, store.path(), &["init", "--name", "demo"])
            .status
            .success()
    );
    assert!(
        scadman(
            &proj,
            store.path(),
            &[
                "add",
                "BOSL2",
                "https://github.com/BelfrySCAD/BOSL2",
                "--tag",
                "v2.0"
            ],
        )
        .status
        .success()
    );

    let list = scadman(&proj, store.path(), &["list"]);
    assert!(list.status.success());
    let out = String::from_utf8_lossy(&list.stdout);
    assert!(
        out.contains("BOSL2") && out.contains("not locked"),
        "list: {out}"
    );

    assert!(
        scadman(&proj, store.path(), &["remove", "BOSL2"])
            .status
            .success()
    );
    let list = scadman(&proj, store.path(), &["list"]);
    assert!(String::from_utf8_lossy(&list.stdout).contains("No dependencies"));

    // Removing a non-existent dependency errors.
    assert!(
        !scadman(&proj, store.path(), &["remove", "BOSL2"])
            .status
            .success()
    );
}

#[test]
fn env_reports_path_and_json() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"demo\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", rev = \"{rev}\" }}\n",
            lib.display()
        ),
    );

    let env = scadman(&proj, store.path(), &["env"]);
    assert!(
        env.status.success(),
        "env: {}",
        String::from_utf8_lossy(&env.stderr)
    );
    assert!(
        String::from_utf8_lossy(&env.stdout)
            .trim()
            .ends_with(".scadman/env"),
        "env should print the OPENSCADPATH dir on stdout"
    );

    let json = scadman(&proj, store.path(), &["env", "--json"]);
    assert!(json.status.success());
    let text = String::from_utf8_lossy(&json.stdout);
    assert!(
        text.contains("openscadpath") && text.contains("mylib") && text.contains(&rev),
        "env --json: {text}"
    );
}
