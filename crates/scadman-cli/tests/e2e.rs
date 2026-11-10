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
    // Correct source + rev, but the wrong (well-formed) content hash: a fresh sync fetches
    // and must reject.
    fs::write(
        proj.join("scadman.lock"),
        format!(
            "version = 1\n\n[[package]]\nname = \"mylib\"\nsource = \"{source}\"\nrev = \"{rev}\"\nhash = \"{}\"\n",
            "0".repeat(64)
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
fn a_lockfile_hash_that_is_a_path_is_refused() {
    // `hash` is joined onto the store root, so a lockfile could point a package at any
    // directory on the machine: sync linked it into the environment and put it on
    // OPENSCADPATH.
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let source = format!("file://{}", lib.display());
    let victim = root.path().join("victim");
    fs::create_dir_all(&victim).unwrap();
    fs::write(victim.join("private.txt"), "host-only\n").unwrap();
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"demo\"\n\n[dependencies]\nmylib = {{ git = \"{source}\", rev = \"{rev}\" }}\n"
        ),
    );

    for cmd in ["sync", "env", "graph"] {
        fs::write(
            proj.join("scadman.lock"),
            format!(
                "version = 1\n\n[[package]]\nname = \"mylib\"\nsource = \"{source}\"\nrev = \"{rev}\"\nhash = \"{}\"\n",
                victim.display()
            ),
        )
        .unwrap();
        let out = scadman(&proj, store.path(), &[cmd]);
        assert!(!out.status.success(), "`{cmd}` should refuse the lockfile");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("hash") && err.contains("mylib"),
            "`{cmd}` should name the field and the package: {err}"
        );
        assert!(
            !proj.join(".scadman/env/mylib").exists(),
            "`{cmd}`: nothing may be linked into the environment"
        );
    }
}

#[test]
fn lock_rejects_a_bad_library_root_on_a_git_dependency() {
    // An unsafe or misspelled root on a git dependency used to lock cleanly, write itself
    // into scadman.lock, and fail only at sync — the misspelled case with "not in the
    // store — run a sync".
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let proj = root.path().join("proj");
    fs::create_dir_all(&proj).unwrap();

    for (bad, expect) in [("../..", ".."), ("/etc", "relative"), ("srcc", "srcc")] {
        fs::write(
            proj.join("scadman.toml"),
            format!(
                "[project]\nname = \"demo\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", rev = \"{rev}\", root = \"{bad}\" }}\n",
                lib.display()
            ),
        )
        .unwrap();
        let out = scadman(&proj, store.path(), &["lock"]);
        assert!(!out.status.success(), "lock should refuse root `{bad}`");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("library root") && err.contains("mylib") && err.contains(expect),
            "root `{bad}`: should name the dependency and what is wrong: {err}"
        );
        assert!(
            !err.contains("dependency name") && !err.contains("not in the store"),
            "root `{bad}`: misleading message: {err}"
        );
        assert!(
            !proj.join("scadman.lock").exists(),
            "root `{bad}`: no lockfile may be written"
        );
    }
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
fn sync_refuses_a_lockfile_holding_a_removed_dependency() {
    // After `remove`, the library stayed locked: sync kept exposing it, so the project's
    // own import of it kept rendering, and doctor called the lockfile up to date.
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
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
    assert!(scadman(&proj, store.path(), &["sync"]).status.success());
    assert!(proj.join(".scadman/env/mylib").exists());

    assert!(
        scadman(&proj, store.path(), &["remove", "mylib"])
            .status
            .success()
    );

    let sync = scadman(&proj, store.path(), &["sync"]);
    assert!(
        !sync.status.success(),
        "sync must refuse while the lock still holds the removed dependency"
    );
    let err = String::from_utf8_lossy(&sync.stderr);
    assert!(
        err.contains("out of date") && err.contains("mylib") && err.contains("scadman lock"),
        "should say the lock is out of date, name the library and the fix: {err}"
    );

    let doctor = scadman(&proj, store.path(), &["doctor"]);
    let text = String::from_utf8_lossy(&doctor.stdout);
    let line = text
        .lines()
        .find(|l| l.starts_with("lockfile:"))
        .expect("doctor prints a lockfile line");
    assert!(
        line.contains("out of date") && !line.contains("up to date"),
        "doctor should not call it up to date: {line}"
    );

    // Re-locking clears it, and the library is gone from the environment.
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
    let sync = scadman(&proj, store.path(), &["sync"]);
    assert!(
        sync.status.success(),
        "sync after lock: {}",
        String::from_utf8_lossy(&sync.stderr)
    );
    assert!(
        !proj.join(".scadman/env/mylib").exists(),
        "the removed library must no longer be exposed"
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
fn add_with_a_url_as_the_name_points_at_the_argument_order() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = root.path().join("p");
    fs::create_dir_all(&proj).unwrap();
    assert!(
        scadman(&proj, store.path(), &["init", "--name", "demo"])
            .status
            .success()
    );
    let before = fs::read_to_string(proj.join("scadman.toml")).unwrap();

    for (url, suggested) in [
        ("https://github.com/revarbat/BOSL", "BOSL"),
        ("https://github.com/BelfrySCAD/BOSL2.git", "BOSL2"),
        ("git@github.com:revarbat/BOSL.git", "BOSL"),
    ] {
        let out = scadman(&proj, store.path(), &["add", url]);
        assert!(!out.status.success(), "`add {url}` should fail");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("looks like a git URL"), "stderr: {err}");
        assert!(
            err.contains(&format!("scadman add {suggested} {url}")),
            "stderr should suggest the corrected command: {err}"
        );
    }

    // Nothing was written for any of the rejected forms.
    assert_eq!(
        fs::read_to_string(proj.join("scadman.toml")).unwrap(),
        before
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

/// Real-ecosystem validation (#21): fetch BOSL2 at a pinned rev and render a model that
/// `include`s it end-to-end. Requires network access and `openscad` on PATH, so it is
/// ignored by default — run with `cargo test -- --ignored`.
#[test]
#[ignore = "needs network + openscad; real-ecosystem validation"]
fn bosl2_renders_end_to_end() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = project_with(
        root.path(),
        "[project]\nname = \"v\"\n\n[dependencies]\nBOSL2 = { git = \"https://github.com/BelfrySCAD/BOSL2\", rev = \"afe82db884ee4409aa76ecfcfbbf54d446964af1\" }\n",
    );
    fs::write(
        proj.join("model.scad"),
        "include <BOSL2/std.scad>\ncuboid([10, 20, 30], rounding = 3);\n",
    )
    .unwrap();

    assert!(
        scadman(&proj, store.path(), &["lock"]).status.success(),
        "lock BOSL2"
    );
    let run = scadman(
        &proj,
        store.path(),
        &["run", "--", "-o", "out.stl", "model.scad"],
    );
    assert!(
        run.status.success(),
        "render: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(proj.join("out.stl").exists(), "BOSL2 should render an STL");
}

/// Real-ecosystem validation (#25): dotSCAD is src-layout and needs its `src/` on
/// OPENSCADPATH; with `root = "src"` + `on_path = true` a deep module renders. Ignored
/// (network + openscad); run with `cargo test -- --ignored`.
#[test]
#[ignore = "needs network + openscad; real-ecosystem validation"]
fn dotscad_renders_with_root_and_on_path() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = project_with(
        root.path(),
        "[project]\nname = \"v\"\n\n[dependencies.dotSCAD]\ngit = \"https://github.com/JustinSDK/dotSCAD\"\nrev = \"bb33edfd75cba0edbb7971606a077b9c69d0b7d2\"\nroot = \"src\"\non_path = true\n",
    );
    fs::write(
        proj.join("model.scad"),
        "use <dotSCAD/line3d.scad>\nline3d([[0, 0, 0], [10, 0, 0], [10, 10, 5]], 2);\n",
    )
    .unwrap();
    assert!(
        scadman(&proj, store.path(), &["lock"]).status.success(),
        "lock dotSCAD"
    );
    let run = scadman(
        &proj,
        store.path(),
        &["run", "--", "-o", "out.stl", "model.scad"],
    );
    assert!(
        run.status.success(),
        "render: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        proj.join("out.stl").exists(),
        "dotSCAD should render an STL"
    );
}

/// The `manifest:` line of a `doctor` report.
///
/// Assertions here must be scoped to this line, not the whole report. The
/// `openscad:` line legitimately says "not found on PATH" wherever OpenSCAD is
/// absent, so a whole-text check for "not found" passes on a developer machine
/// and fails in CI for a reason that has nothing to do with the manifest.
fn manifest_line(text: &str) -> &str {
    text.lines()
        .find(|l| l.starts_with("manifest:"))
        .expect("doctor always prints a manifest line")
}

#[test]
fn doctor_distinguishes_a_broken_manifest_from_a_missing_one() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();

    // A manifest that exists but does not parse. Diagnosing exactly this is
    // doctor's job, and reporting it as "not found" sends the user to
    // `scadman init`, which then refuses because the file is right there.
    let broken = project_with(
        root.path(),
        "[project]\nname = \"d\"\n\n[dependencies]\nmylib = { git = 42 }\n",
    );
    let out = scadman(&broken, store.path(), &["doctor"]);
    assert!(
        out.status.success(),
        "doctor is a diagnostic; it should still report"
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let line = manifest_line(&text);
    assert!(
        !line.contains("not found"),
        "a manifest that exists must not be reported as missing:\n{line}"
    );
    assert!(
        !line.contains("scadman init"),
        "must not advise init when the file already exists:\n{line}"
    );
    assert!(
        line.contains("scadman.toml") && line.to_lowercase().contains("parse"),
        "should name the file and say it failed to parse:\n{line}"
    );

    // The missing case keeps its existing advice.
    let empty = TempDir::new().unwrap();
    let out = scadman(empty.path(), store.path(), &["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout);
    let line = manifest_line(&text);
    assert!(
        line.contains("not found") && line.contains("scadman init"),
        "a genuinely absent manifest should still point at init:\n{line}"
    );
}

#[test]
fn every_command_refuses_a_lockfile_of_an_unknown_version() {
    // Only sync/run/env used to check the version. graph, list and doctor read a newer
    // lockfile without comment, and `update <name>` rewrote it in the old format.
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, _) = make_lib(root.path());
    let branch = branch_of(&lib);
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"demo\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", branch = \"{branch}\" }}\n",
            lib.display()
        ),
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
    let lock_path = proj.join("scadman.lock");
    let newer = fs::read_to_string(&lock_path)
        .unwrap()
        .replacen("version = 1", "version = 99", 1);
    assert!(newer.starts_with("version = 99"), "fixture: {newer}");
    fs::write(&lock_path, &newer).unwrap();

    // Give `update` something it would otherwise move.
    fs::write(lib.join("more.scad"), "// more\n").unwrap();
    commit_all(&lib, "more");

    for args in [
        &["sync"][..],
        &["graph"],
        &["list"],
        &["update", "mylib"],
        &["update"],
        &["lock"],
    ] {
        let out = scadman(&proj, store.path(), args);
        assert!(!out.status.success(), "`{args:?}` should refuse");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("version 99"),
            "`{args:?}` should name the version: {err}"
        );
        assert_eq!(
            fs::read_to_string(&lock_path).unwrap(),
            newer,
            "`{args:?}` must leave the newer lockfile untouched"
        );
    }

    let out = scadman(&proj, store.path(), &["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.starts_with("lockfile:"))
        .expect("doctor prints a lockfile line");
    assert!(
        line.contains("version 99") && !line.contains("up to date"),
        "doctor should report the version, not `up to date`: {line}"
    );
}

#[test]
fn a_newer_lockfile_that_does_not_parse_is_reported_by_version() {
    // A later format may not fit today's shape at all. The version is still readable,
    // and "upgrade scadman" is more use than a parse error about a field.
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = project_with(root.path(), "[project]\nname = \"demo\"\n");
    fs::write(
        proj.join("scadman.lock"),
        "version = 2\npackage = \"not a list any more\"\n",
    )
    .unwrap();
    let out = scadman(&proj, store.path(), &["graph"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("version 2"), "should name the version: {err}");
}

#[test]
fn lock_replaces_a_lockfile_that_is_not_a_lockfile() {
    // Refusing to overwrite is for a lockfile from a newer scadman. A file with no
    // readable version is just broken, and regenerating it is the fix.
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = project_with(root.path(), "[project]\nname = \"demo\"\n");
    fs::write(proj.join("scadman.lock"), "this is not toml = = =\n").unwrap();
    let out = scadman(&proj, store.path(), &["lock"]);
    assert!(
        out.status.success(),
        "lock: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        fs::read_to_string(proj.join("scadman.lock"))
            .unwrap()
            .starts_with("version = 1")
    );
}

#[test]
fn lock_rejects_a_git_dependency_with_two_refs() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    git(&lib, &["tag", "v1"]);
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"demo\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", rev = \"{rev}\", tag = \"v1\" }}\n",
            lib.display()
        ),
    );

    let out = scadman(&proj, store.path(), &["lock"]);
    assert!(
        !out.status.success(),
        "lock should refuse an entry with both rev and tag"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("mylib") && err.contains("rev") && err.contains("tag"),
        "should name the dependency and the keys: {err}"
    );
    assert!(!proj.join("scadman.lock").exists(), "no lockfile written");
}

#[test]
fn a_misspelled_dependencies_table_is_an_error_not_an_empty_project() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = project_with(
        root.path(),
        "[project]\nname = \"d\"\n\n[dependancies]\nmylib = { git = \"file:///x\", tag = \"v1\" }\n",
    );

    for cmd in ["list", "lock", "sync"] {
        let out = scadman(&proj, store.path(), &[cmd]);
        assert!(!out.status.success(), "`{cmd}` should fail");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("dependancies"),
            "`{cmd}` should name the table: {err}"
        );
    }
    assert!(
        !proj.join("scadman.lock").exists(),
        "no lockfile may be written for a manifest that does not parse"
    );

    let out = scadman(&proj, store.path(), &["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        manifest_line(&text).contains("failed to parse"),
        "doctor should report the parse failure:\n{text}"
    );
    // The TOML error spans several lines; the table is named below the `manifest:` line.
    assert!(
        text.contains("dependancies"),
        "doctor should name the table:\n{text}"
    );
}

#[test]
fn doctor_reports_setup() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"d\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", rev = \"{rev}\" }}\n",
            lib.display()
        ),
    );
    let out = scadman(&proj, store.path(), &["doctor"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("manifest:") && text.contains("lockfile:") && text.contains("environment:")
    );
    assert!(
        text.contains("scadman lock"),
        "should nudge to lock when unlocked"
    );
}

#[test]
fn env_write_vscode_merges_settings() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"d\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", rev = \"{rev}\" }}\n",
            lib.display()
        ),
    );
    fs::create_dir_all(proj.join(".vscode")).unwrap();
    fs::write(
        proj.join(".vscode").join("settings.json"),
        "{\n  \"editor.tabSize\": 2\n}\n",
    )
    .unwrap();

    let out = scadman(&proj, store.path(), &["env", "--write-vscode"]);
    assert!(
        out.status.success(),
        "env --write-vscode: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let settings = fs::read_to_string(proj.join(".vscode").join("settings.json")).unwrap();
    assert!(
        settings.contains("openscad.search_paths"),
        "sets the LSP search path"
    );
    assert!(
        settings.contains("${workspaceFolder}/.scadman/env"),
        "path is workspace-relative (committable), not absolute: {settings}"
    );
    assert!(
        settings.contains("editor.tabSize"),
        "preserves existing settings"
    );
}

#[test]
fn graph_prints_tree_and_json() {
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
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());

    let tree = scadman(&proj, store.path(), &["graph"]);
    assert!(tree.status.success());
    let text = String::from_utf8_lossy(&tree.stdout);
    assert!(text.contains("demo"), "prints the project name");
    assert!(text.contains("mylib"), "prints the dependency");

    let json = scadman(&proj, store.path(), &["graph", "--json"]);
    assert!(json.status.success());
    let text = String::from_utf8_lossy(&json.stdout);
    assert!(text.contains("\"project\": \"demo\""), "json has project");
    assert!(text.contains("\"roots\""), "json has roots");
    assert!(text.contains("\"mylib\""), "json has the node");
}

#[test]
fn graph_without_lock_nudges() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = project_with(root.path(), "[project]\nname = \"demo\"\n");
    let out = scadman(&proj, store.path(), &["graph"]);
    assert!(
        !out.status.success(),
        "graph without a lockfile should fail"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("scadman lock"),
        "nudges to run lock"
    );
}

#[test]
fn path_dependency_syncs_and_picks_up_edits() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();

    // A plain local sibling library (no git).
    let lib = root.path().join("lib");
    fs::create_dir_all(&lib).unwrap();
    fs::write(lib.join("greet.scad"), "module greet() { cube(1); }\n").unwrap();

    let proj = project_with(
        root.path(),
        "[project]\nname = \"app\"\n\n[dependencies]\nmylib = { path = \"../lib\" }\n",
    );

    let lock = scadman(&proj, store.path(), &["lock"]);
    assert!(
        lock.status.success(),
        "lock failed: {}",
        String::from_utf8_lossy(&lock.stderr)
    );

    let sync = scadman(&proj, store.path(), &["sync"]);
    assert!(sync.status.success());
    // The env exposes the local lib under its name.
    let exposed = proj
        .join(".scadman")
        .join("env")
        .join("mylib")
        .join("greet.scad");
    assert_eq!(
        fs::read_to_string(&exposed).unwrap(),
        "module greet() { cube(1); }\n"
    );

    // Editing the sibling and re-syncing picks up the change (the point of path deps).
    fs::write(lib.join("greet.scad"), "module greet() { sphere(2); }\n").unwrap();
    let resync = scadman(&proj, store.path(), &["sync"]);
    assert!(
        resync.status.success(),
        "resync failed: {}",
        String::from_utf8_lossy(&resync.stderr)
    );
    assert_eq!(
        fs::read_to_string(&exposed).unwrap(),
        "module greet() { sphere(2); }\n",
        "sync should re-read the local path dependency"
    );
}

#[test]
fn a_failed_store_insert_leaves_no_staging_directory() {
    // Make the copy into the store fail partway, the way a full disk would: cap the size
    // of any file the process may write, below the size of a file in the dependency. The
    // content hash is computed first (reads only), so the failure lands in the copy.
    // `trap '' XFSZ` makes the limit surface as an EFBIG error rather than killing the
    // process; `ulimit -f` counts 512- or 1024-byte blocks depending on the shell, and
    // either way 512 blocks is far below the 4 MiB file.
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let lib = root.path().join("sib");
    fs::create_dir_all(&lib).unwrap();
    fs::write(lib.join("s.scad"), "// s\n").unwrap();
    fs::write(lib.join("big.bin"), vec![0u8; 4 * 1024 * 1024]).unwrap();
    let proj = project_with(
        root.path(),
        "[project]\nname = \"app\"\n\n[dependencies]\nsib = { path = \"../sib\" }\n",
    );

    let out = Command::new("sh")
        .arg("-c")
        .arg("trap '' XFSZ; ulimit -f 512; exec \"$0\" lock")
        .arg(env!("CARGO_BIN_EXE_scadman"))
        .current_dir(&proj)
        .env("XDG_DATA_HOME", store.path())
        .output()
        .expect("run scadman under a file-size limit");
    assert!(
        !out.status.success(),
        "the capped lock should fail; if it passed, the limit did not apply: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    let store_dir = store.path().join("scadman").join("store");
    let leftovers: Vec<String> = fs::read_dir(&store_dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "a failed insert must leave nothing in the store: {leftovers:?}"
    );

    // Without the limit the same lock succeeds, and the store holds exactly the entry.
    let ok = scadman(&proj, store.path(), &["lock"]);
    assert!(
        ok.status.success(),
        "lock: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let entries: Vec<String> = fs::read_dir(&store_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries.len(), 1, "one store entry, no staging: {entries:?}");
    assert!(!entries[0].starts_with(".staging"), "{entries:?}");
}

#[test]
fn add_path_then_lock_end_to_end() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let lib = root.path().join("sib");
    fs::create_dir_all(&lib).unwrap();
    fs::write(lib.join("x.scad"), "// x\n").unwrap();

    let proj = project_with(root.path(), "[project]\nname = \"app\"\n");
    let add = scadman(&proj, store.path(), &["add", "sib", "--path", "../sib"]);
    assert!(
        add.status.success(),
        "add --path failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let manifest = fs::read_to_string(proj.join("scadman.toml")).unwrap();
    assert!(
        manifest.contains("path = \"../sib\""),
        "manifest: {manifest}"
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
}

#[test]
fn missing_path_dependency_errors_clearly() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = project_with(
        root.path(),
        "[project]\nname = \"app\"\n\n[dependencies]\ngone = { path = \"../nope\" }\n",
    );
    let out = scadman(&proj, store.path(), &["lock"]);
    assert!(
        !out.status.success(),
        "lock of a missing path dep should fail"
    );
}

#[test]
fn path_dep_does_not_force_refetch_of_git_deps() {
    // A path dep triggers a re-resolve on every sync; git deps alongside it must be served
    // from the store at their locked rev, NOT re-fetched — so sync keeps working offline and
    // branch/tag deps don't silently move. Proven by deleting the git remote after locking.
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (gitlib, rev) = make_lib(root.path()); // a git "remote" at <root>/mylib

    let sib = root.path().join("sib");
    fs::create_dir_all(&sib).unwrap();
    fs::write(sib.join("s.scad"), "// s\n").unwrap();

    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"app\"\n\n[dependencies]\nmygit = {{ git = \"file://{}\", rev = \"{rev}\" }}\nmysib = {{ path = \"../sib\" }}\n",
            gitlib.display()
        ),
    );

    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
    let lock_after_lock = fs::read_to_string(proj.join("scadman.lock")).unwrap();

    // Take the git remote away entirely — a re-fetch would now fail.
    fs::remove_dir_all(&gitlib).unwrap();

    let sync = scadman(&proj, store.path(), &["sync"]);
    assert!(
        sync.status.success(),
        "sync must serve the git dep from the store cache: {}",
        String::from_utf8_lossy(&sync.stderr)
    );
    assert!(
        proj.join(".scadman/env/mygit").exists(),
        "git dep still exposed"
    );
    assert!(proj.join(".scadman/env/mysib").exists(), "path dep exposed");
    // Nothing changed, so the lock must not have been rewritten.
    let lock_after_sync = fs::read_to_string(proj.join("scadman.lock")).unwrap();
    assert_eq!(
        lock_after_lock, lock_after_sync,
        "an unchanged path-dep project must not rewrite the lock"
    );
}

#[test]
fn a_hex_named_tag_is_held_at_its_locked_commit_beside_a_path_dep() {
    // A tag whose name is all hex digits (a date stamp like `20210101`) used to be taken
    // for a commit hash. The locked commit never "started with" it, so every sync beside
    // a path dep re-fetched it: the lock moved when the tag moved, and sync needed the
    // network. It must behave like any other tag.
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (gitlib, _) = make_lib(root.path());
    git(&gitlib, &["tag", "20210101"]);

    let sib = root.path().join("sib");
    fs::create_dir_all(&sib).unwrap();
    fs::write(sib.join("s.scad"), "// s\n").unwrap();

    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"app\"\n\n[dependencies]\nmygit = {{ git = \"file://{}\", tag = \"20210101\" }}\nmysib = {{ path = \"../sib\" }}\n",
            gitlib.display()
        ),
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
    let locked = fs::read_to_string(proj.join("scadman.lock")).unwrap();

    // Move the tag upstream. A sync must not follow it; only `update` does.
    fs::write(gitlib.join("more.scad"), "// more\n").unwrap();
    commit_all(&gitlib, "more");
    git(&gitlib, &["tag", "-f", "20210101"]);
    let sync = scadman(&proj, store.path(), &["sync"]);
    assert!(
        sync.status.success(),
        "sync: {}",
        String::from_utf8_lossy(&sync.stderr)
    );
    assert_eq!(
        fs::read_to_string(proj.join("scadman.lock")).unwrap(),
        locked,
        "sync must not move a tag dependency off its locked commit"
    );

    // And with the remote gone entirely, sync still works from the store.
    fs::remove_dir_all(&gitlib).unwrap();
    let offline = scadman(&proj, store.path(), &["sync"]);
    assert!(
        offline.status.success(),
        "sync must not need the remote for a locked tag: {}",
        String::from_utf8_lossy(&offline.stderr)
    );
}

#[test]
fn changing_a_git_pin_still_nudges_even_with_a_path_dep() {
    // A path dep auto-refreshes, but it must NOT disable the staleness nudge for a git dep
    // beside it: editing the git pin in the manifest requires an explicit `scadman lock`.
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (gitlib, rev) = make_lib(root.path());
    let sib = root.path().join("sib");
    fs::create_dir_all(&sib).unwrap();
    fs::write(sib.join("s.scad"), "// s\n").unwrap();

    let manifest = |git_rev: &str| {
        format!(
            "[project]\nname = \"app\"\n\n[dependencies]\nmygit = {{ git = \"file://{}\", rev = \"{git_rev}\" }}\nmysib = {{ path = \"../sib\" }}\n",
            gitlib.display()
        )
    };
    let proj = project_with(root.path(), &manifest(&rev));
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());

    // Point the git dep at a different revision without re-locking, then sync.
    let other = "0123456789abcdef0123456789abcdef01234567";
    fs::write(proj.join("scadman.toml"), manifest(other)).unwrap();
    let out = scadman(&proj, store.path(), &["sync"]);
    assert!(
        !out.status.success(),
        "sync must not silently ignore a changed git pin when a path dep coexists"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("scadman lock"),
        "should nudge to re-lock: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn path_dependency_with_root_and_on_path_exposes_the_subdir() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    // A src-layout sibling: code under src/.
    let lib = root.path().join("srclib");
    fs::create_dir_all(lib.join("src")).unwrap();
    fs::write(lib.join("src").join("core.scad"), "// core\n").unwrap();

    let proj = project_with(
        root.path(),
        "[project]\nname = \"app\"\n\n[dependencies]\nsrclib = { path = \"../srclib\", root = \"src\", on_path = true }\n",
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
    assert!(scadman(&proj, store.path(), &["sync"]).status.success());

    // Exposed under its name → the src/ contents (not the repo root).
    let exposed = proj.join(".scadman/env/srclib/core.scad");
    assert_eq!(fs::read_to_string(&exposed).unwrap(), "// core\n");
    // on_path places the library's own dir on OPENSCADPATH.
    let env = scadman(&proj, store.path(), &["env"]);
    let path = String::from_utf8_lossy(&env.stdout);
    assert!(
        path.contains(".scadman/env/srclib"),
        "on_path should add the lib dir to OPENSCADPATH: {path}"
    );
}

#[test]
fn add_path_rejects_a_conflicting_ref_flag() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = project_with(root.path(), "[project]\nname = \"app\"\n");
    let out = scadman(
        &proj,
        store.path(),
        &["add", "x", "--path", "../x", "--rev", "abc"],
    );
    assert!(!out.status.success(), "--path with --rev must be rejected");
}

/// Commit all changes in `dir` and return the new HEAD sha.
fn commit_all(dir: &Path, msg: &str) -> String {
    git(dir, &["add", "-A"]);
    git(
        dir,
        &[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "--quiet",
            "-m",
            msg,
        ],
    );
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn branch_of(dir: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn update_advances_a_branch_dependency() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev_a) = make_lib(root.path());
    let branch = branch_of(&lib);
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"app\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", branch = \"{branch}\" }}\n",
            lib.display()
        ),
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
    let lock_before = fs::read_to_string(proj.join("scadman.lock")).unwrap();
    assert!(lock_before.contains(&rev_a));

    // Advance the branch, then update.
    fs::write(lib.join("more.scad"), "// more\n").unwrap();
    let rev_b = commit_all(&lib, "more");
    assert_ne!(rev_a, rev_b);

    let out = scadman(&proj, store.path(), &["update"]);
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Updated mylib"),
        "should report the move: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let lock_after = fs::read_to_string(proj.join("scadman.lock")).unwrap();
    assert!(
        lock_after.contains(&rev_b),
        "lock advanced to the new commit"
    );
    assert!(!lock_after.contains(&rev_a), "old commit gone");
}

#[test]
fn update_leaves_a_rev_pin_untouched() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"app\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", rev = \"{rev}\" }}\n",
            lib.display()
        ),
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
    // Even a new upstream commit can't move a rev pin.
    fs::write(lib.join("more.scad"), "// more\n").unwrap();
    commit_all(&lib, "more");

    let out = scadman(&proj, store.path(), &["update"]);
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Already up to date"),
        "a rev pin must not move: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn update_scopes_to_named_dependencies() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib_a, _) = make_lib(root.path());
    // A second library dir with its own git repo.
    let lib_b = root.path().join("libb");
    fs::create_dir_all(&lib_b).unwrap();
    fs::write(lib_b.join("b.scad"), "// b\n").unwrap();
    git(&lib_b, &["init", "--quiet"]);
    let b_rev0 = commit_all(&lib_b, "init");

    let (ba, bb) = (branch_of(&lib_a), branch_of(&lib_b));
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"app\"\n\n[dependencies]\na = {{ git = \"file://{}\", branch = \"{ba}\" }}\nb = {{ git = \"file://{}\", branch = \"{bb}\" }}\n",
            lib_a.display(),
            lib_b.display()
        ),
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());

    // Advance BOTH remotes, then update only `a`.
    fs::write(lib_a.join("x.scad"), "// x\n").unwrap();
    let a_new = commit_all(&lib_a, "x");
    fs::write(lib_b.join("y.scad"), "// y\n").unwrap();
    let b_new = commit_all(&lib_b, "y");

    let out = scadman(&proj, store.path(), &["update", "a"]);
    assert!(out.status.success());
    let after = fs::read_to_string(proj.join("scadman.lock")).unwrap();
    assert!(after.contains(&a_new), "`a` advanced");
    assert!(
        !after.contains(&b_new),
        "`b` must NOT advance (held at its locked rev)"
    );
    assert!(
        after.contains(&b_rev0),
        "`b` held at exactly its originally-locked rev"
    );
    assert_eq!(after.matches("rev =").count(), 2, "both deps still locked");
}

#[test]
fn update_rejects_a_path_dependency_name() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let sib = root.path().join("sib");
    fs::create_dir_all(&sib).unwrap();
    fs::write(sib.join("s.scad"), "// s\n").unwrap();
    let proj = project_with(
        root.path(),
        "[project]\nname = \"app\"\n\n[dependencies]\nmysib = { path = \"../sib\" }\n",
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
    let out = scadman(&proj, store.path(), &["update", "mysib"]);
    assert!(
        !out.status.success(),
        "updating a path dep should be rejected"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("path dependency"));
}

#[test]
fn full_update_advances_a_transitive_branch_dep() {
    // Regression: `scadman update` (no names) must advance transitive branch/tag deps too,
    // matching `scadman lock` — not hold them at their locked rev.
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();

    // Leaf lib B (transitive), on a branch.
    let libb = root.path().join("b");
    fs::create_dir_all(&libb).unwrap();
    fs::write(libb.join("b.scad"), "// b1\n").unwrap();
    git(&libb, &["init", "--quiet"]);
    commit_all(&libb, "b1");
    let bb = branch_of(&libb);

    // Lib A depends on B (transitive), and is itself a branch dep of the project.
    let liba = root.path().join("a");
    fs::create_dir_all(&liba).unwrap();
    fs::write(liba.join("a.scad"), "// a1\n").unwrap();
    fs::write(
        liba.join("scadman.toml"),
        format!(
            "[project]\nname = \"a\"\n\n[dependencies]\nb = {{ git = \"file://{}\", branch = \"{bb}\" }}\n",
            libb.display()
        ),
    )
    .unwrap();
    git(&liba, &["init", "--quiet"]);
    commit_all(&liba, "a1");
    let ba = branch_of(&liba);

    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"app\"\n\n[dependencies]\na = {{ git = \"file://{}\", branch = \"{ba}\" }}\n",
            liba.display()
        ),
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());

    // Advance both the leaf B and the parent A.
    fs::write(libb.join("more.scad"), "// b2\n").unwrap();
    let b2 = commit_all(&libb, "b2");
    fs::write(liba.join("more.scad"), "// a2\n").unwrap();
    commit_all(&liba, "a2");

    assert!(scadman(&proj, store.path(), &["update"]).status.success());
    let after = fs::read_to_string(proj.join("scadman.lock")).unwrap();
    assert!(
        after.contains(&b2),
        "the transitive branch dep B must advance on a full `update`"
    );
}

#[test]
fn doctor_notes_branch_tracking_only_when_present() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let branch = branch_of(&lib);

    // Branch dep → doctor should nudge toward update.
    let tracking = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"app\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", branch = \"{branch}\" }}\n",
            lib.display()
        ),
    );
    let out = scadman(&tracking, store.path(), &["doctor"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("tracking:"));

    // Rev pin only → no tracking nudge.
    let pinned = root.path().join("pinned");
    fs::create_dir_all(&pinned).unwrap();
    fs::write(
        pinned.join("scadman.toml"),
        format!(
            "[project]\nname = \"pinned\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", rev = \"{rev}\" }}\n",
            lib.display()
        ),
    )
    .unwrap();
    let out = scadman(&pinned, store.path(), &["doctor"]);
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("tracking:"),
        "a rev-pin-only project must not show the tracking hint"
    );
}

#[test]
fn update_with_no_prior_lock_reports_added() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"app\"\n\n[dependencies]\nmylib = {{ git = \"file://{}\", rev = \"{rev}\" }}\n",
            lib.display()
        ),
    );
    let out = scadman(&proj, store.path(), &["update"]);
    assert!(out.status.success());
    assert!(
        proj.join("scadman.lock").exists(),
        "update creates the lock"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Added mylib"),
        "no prior lock → reports Added: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn update_reports_a_removed_dependency() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, rev) = make_lib(root.path());
    let src = format!("file://{}", lib.display());
    let proj = project_with(
        root.path(),
        &format!(
            "[project]\nname = \"app\"\n\n[dependencies]\nmylib = {{ git = \"{src}\", rev = \"{rev}\" }}\n"
        ),
    );
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
    // Remove the only dependency, then update.
    fs::write(proj.join("scadman.toml"), "[project]\nname = \"app\"\n").unwrap();
    let out = scadman(&proj, store.path(), &["update"]);
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Removed mylib"),
        "should report the dropped dep: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn update_unknown_dependency_errors() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = project_with(root.path(), "[project]\nname = \"app\"\n");
    let out = scadman(&proj, store.path(), &["update", "ghost"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not a dependency"),
        "clear unknown-name error: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn add_without_a_ref_tracks_the_default_branch() {
    // A newcomer's `scadman add <name> <url>` with no ref should just work: track the
    // remote's default branch (like `git clone`), then lock to a commit.
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let (lib, _rev) = make_lib(root.path());
    let branch = branch_of(&lib);
    let proj = root.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    assert!(
        scadman(&proj, store.path(), &["init", "--name", "app"])
            .status
            .success()
    );

    let out = scadman(
        &proj,
        store.path(),
        &["add", "mylib", &format!("file://{}", lib.display())],
    );
    assert!(
        out.status.success(),
        "bare add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("default branch"),
        "should report it defaulted: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let manifest = fs::read_to_string(proj.join("scadman.toml")).unwrap();
    assert!(
        manifest.contains(&format!("branch = \"{branch}\"")),
        "manifest tracks the default branch: {manifest}"
    );
    // And it locks to a commit.
    assert!(scadman(&proj, store.path(), &["lock"]).status.success());
}

#[test]
fn doctor_reports_an_openscad_requirement() {
    let root = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let proj = project_with(
        root.path(),
        "[project]\nname = \"app\"\nopenscad = \"2099.01\"\n",
    );
    let out = scadman(&proj, store.path(), &["doctor"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("requires:"),
        "shows the requirement line: {text}"
    );
    assert!(text.contains("OpenSCAD ≥ 2099.01"));

    // A project without the field shows no `requires:` line.
    let bare = root.path().join("bare");
    fs::create_dir_all(&bare).unwrap();
    fs::write(bare.join("scadman.toml"), "[project]\nname = \"bare\"\n").unwrap();
    let out2 = scadman(&bare, store.path(), &["doctor"]);
    assert!(!String::from_utf8_lossy(&out2.stdout).contains("requires:"));
}
