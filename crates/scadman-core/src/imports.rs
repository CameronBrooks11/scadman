//! Scanning installed OpenSCAD content for imports the project never declared.
//!
//! Most real transitive dependencies in the ecosystem are *manifest-less* (see
//! `docs/ecosystem-survey.md`): a library `use`s another by a qualified path like
//! `<scad-utils/…>` without declaring it anywhere scadman can read. Without this pass, a
//! user who installs such a library gets a raw OpenSCAD `can't open include file` error
//! that scadman caused and cannot explain. This pass reports the library roots the
//! installed content imports that the resolution does not provide, so scadman can say
//! exactly what to add.
//!
//! The scan is **reachability-based**: it starts from the project's own `.scad` files and
//! follows `use`/`include` edges through the installed packages, only visiting dependency
//! files the project actually reaches. A file a dependency ships but the project never
//! imports (a stray example, an optional module) is never scanned, so it cannot produce a
//! spurious "undeclared dependency" warning. An import is *internal* if it resolves within
//! its own package (relative to the importing file, or from the package root); such edges
//! are followed but never flagged. A qualified import (`<name/…>`) that resolves neither
//! internally nor to a provided package, and is reached from within a dependency, is
//! reported. Imports in the project's own files seed the walk but are not themselves
//! flagged (the project may lean on globally-installed libraries by choice).
//!
//! It also honestly bounds scadman's guarantee: `OPENSCADPATH` cannot stop OpenSCAD from
//! searching the user and installation library folders, so "declared deps shadow globals"
//! is the real promise — this scan is how an *undeclared* dependency is surfaced. Parsing
//! is comment- and string-aware.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

static IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:use|include)\s*<([^>]*)>").unwrap());

/// An installed package to scan: the name it is exposed under and its content root.
pub struct Installed {
    pub name: String,
    pub path: std::path::PathBuf,
}

/// A qualified import referencing a library root the resolution does not provide.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnmetImport {
    /// The installed package whose file makes the import.
    pub package: String,
    /// The `.scad` file (relative to the package root) where it appears.
    pub file: String,
    /// The referenced library root (first path segment of the import target).
    pub library: String,
}

/// One reachability scope: the project (name `""`) or an installed package.
struct Scope {
    name: String,
    root: PathBuf,
    files: BTreeSet<String>,
}

/// Find qualified imports that name a library root the resolution does not provide,
/// reachable from the project's own `.scad` files under `project`.
///
/// The walk starts at every project file and follows `use`/`include` edges through the
/// `installed` packages. Only files actually reached are scanned. An import is reported when
/// it is *qualified* (`<name/…>`), reached from **within a dependency**, and resolves
/// neither inside its own package nor to another installed package. Bare `<file.scad>`
/// imports rely on `OPENSCADPATH` and are too ambiguous to flag. Results are sorted and
/// de-duplicated. The scan is best-effort: unreadable files/dirs are skipped.
pub fn unmet_imports(project: &Path, installed: &[Installed]) -> Vec<UnmetImport> {
    // Scope 0 is the project (seeds the walk, never itself reported); 1.. are the packages.
    let mut scopes: Vec<Scope> = Vec::with_capacity(installed.len() + 1);
    scopes.push(Scope {
        name: String::new(),
        root: project.to_path_buf(),
        files: collect_scad(project),
    });
    for pkg in installed {
        scopes.push(Scope {
            name: pkg.name.clone(),
            root: pkg.path.clone(),
            files: collect_scad(&pkg.path),
        });
    }
    let by_name: HashMap<&str, usize> = scopes
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, s)| (s.name.as_str(), i))
        .collect();

    let mut visited: HashSet<(usize, String)> = HashSet::new();
    let mut queue: VecDeque<(usize, String)> = VecDeque::new();
    for rel in &scopes[0].files {
        if visited.insert((0, rel.clone())) {
            queue.push_back((0, rel.clone()));
        }
    }

    let mut out = Vec::new();
    while let Some((si, rel)) = queue.pop_front() {
        let Ok(bytes) = fs::read(scopes[si].root.join(&rel)) else {
            continue;
        };
        let src = String::from_utf8_lossy(&bytes);
        for target in import_targets(&src) {
            let target = target.trim();
            if target.is_empty() {
                continue;
            }
            // Internal to this scope: resolve relative to the importing file, or from root.
            let by_importer = join_norm(dir_of(&rel), target);
            let from_root = norm(target);
            let internal = if scopes[si].files.contains(&by_importer) {
                Some(by_importer)
            } else if scopes[si].files.contains(&from_root) {
                Some(from_root)
            } else {
                None
            };
            if let Some(next) = internal {
                enqueue(si, next, &mut visited, &mut queue);
                continue;
            }
            // A bare filename (no `/`) relies on OPENSCADPATH — too ambiguous to flag.
            let Some((first, rest)) = target.split_once('/') else {
                continue;
            };
            if matches!(first, "" | "." | "..") {
                continue;
            }
            // Provided by another installed package (or this one, self-rooted)? Follow it.
            if let Some(&j) = by_name.get(first) {
                let next = norm(rest);
                if scopes[j].files.contains(&next) {
                    enqueue(j, next, &mut visited, &mut queue);
                }
                continue;
            }
            // Unmet — but only surfaced from within a dependency, not the project's own code.
            if si != 0 {
                out.push(UnmetImport {
                    package: scopes[si].name.clone(),
                    file: rel.clone(),
                    library: first.to_string(),
                });
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn enqueue(
    scope: usize,
    rel: String,
    visited: &mut HashSet<(usize, String)>,
    queue: &mut VecDeque<(usize, String)>,
) {
    if visited.insert((scope, rel.clone())) {
        queue.push_back((scope, rel));
    }
}

/// The `.scad` files in a package, as sorted posix-relative paths (best-effort).
fn collect_scad(root: &Path) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    collect_scad_into(root, "", &mut files);
    files
}

fn collect_scad_into(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        // `.git` is noise; `.scadman/` is the project's own env of symlinks back into the
        // store — scanning it would re-walk the packages as if they were project files.
        if name == ".git" || name == ".scadman" {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let rel = if prefix.is_empty() {
            name.to_string_lossy().into_owned()
        } else {
            format!("{prefix}/{}", name.to_string_lossy())
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_scad_into(&entry.path(), &rel, out);
        } else if Path::new(&rel).extension().is_some_and(|e| e == "scad") {
            out.insert(rel);
        }
    }
}

fn dir_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn join_norm(dir: &str, target: &str) -> String {
    if dir.is_empty() {
        norm(target)
    } else {
        norm(&format!("{dir}/{target}"))
    }
}

/// Logically normalize a posix path (resolve `.` and `..` without touching the filesystem).
fn norm(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." if matches!(parts.last(), Some(&p) if p != "..") => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Extract the raw targets of `use`/`include` statements, ignoring commented-out or
/// string-embedded ones.
fn import_targets(src: &str) -> Vec<String> {
    let clean = strip_noncode(src);
    IMPORT
        .captures_iter(&clean)
        .map(|c| c[1].trim().to_string())
        .collect()
}

/// Blank comments and string literals in a single pass, preserving newlines.
fn strip_noncode(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c2 in chars.by_ref() {
                    if c2 == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for c2 in chars.by_ref() {
                    if prev == '*' && c2 == '/' {
                        break;
                    }
                    prev = c2;
                }
                out.push(' ');
            }
            '"' => {
                let mut escaped = false;
                for c2 in chars.by_ref() {
                    if escaped {
                        escaped = false;
                    } else if c2 == '\\' {
                        escaped = true;
                    } else if c2 == '"' {
                        break;
                    }
                }
                out.push(' ');
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// A project dir plus named package dirs, all under one tempdir.
    struct Fixture {
        dir: TempDir,
    }
    impl Fixture {
        fn new() -> Self {
            Fixture {
                dir: TempDir::new().unwrap(),
            }
        }
        fn project(&self) -> PathBuf {
            let p = self.dir.path().join("project");
            fs::create_dir_all(&p).unwrap();
            p
        }
        fn write_project(&self, rel: &str, contents: &str) -> PathBuf {
            let p = self.project();
            write(&p, rel, contents);
            p
        }
        fn package(&self, name: &str) -> Installed {
            let path = self.dir.path().join("pkgs").join(name);
            fs::create_dir_all(&path).unwrap();
            Installed {
                name: name.to_string(),
                path,
            }
        }
        fn write_pkg(&self, pkg: &Installed, rel: &str, contents: &str) {
            write(&pkg.path, rel, contents);
        }
    }

    #[test]
    fn import_targets_ignores_comments_and_strings() {
        let src = "// use <linecommented.scad>\nuse <BOSL2/std.scad>\n/* include <blockcommented.scad> */\necho(\"use <instring/x.scad>\");\nuse <bare.scad>\n";
        let targets = import_targets(src);
        assert!(targets.contains(&"BOSL2/std.scad".to_string()));
        assert!(targets.contains(&"bare.scad".to_string()));
        for dropped in ["linecommented", "blockcommented", "instring"] {
            assert!(!targets.iter().any(|t| t.contains(dropped)));
        }
    }

    #[test]
    fn norm_resolves_dot_and_dotdot() {
        assert_eq!(norm("a/./b"), "a/b");
        assert_eq!(norm("a/b/../c"), "a/c");
        assert_eq!(join_norm("lib", "../shared/x.scad"), "shared/x.scad");
        assert_eq!(join_norm("", "std.scad"), "std.scad");
    }

    #[test]
    fn a_genuine_external_library_is_reported() {
        // The project uses `agentscad`, whose reached file imports `scad-utils/` (nowhere
        // provided) and `BOSL2/` (provided) and a bare file.
        let fx = Fixture::new();
        let project = fx.write_project("main.scad", "use <agentscad/util.scad>\n");
        let agentscad = fx.package("agentscad");
        fx.write_pkg(
            &agentscad,
            "util.scad",
            "use <scad-utils/lists.scad>\ninclude <BOSL2/std.scad>\ninclude <bare.scad>\n",
        );
        let bosl2 = fx.package("BOSL2");
        fx.write_pkg(&bosl2, "std.scad", "// std\n");

        let unmet = unmet_imports(&project, &[agentscad, bosl2]);
        assert_eq!(unmet.len(), 1, "only scad-utils is unmet: {unmet:?}");
        assert_eq!(unmet[0].library, "scad-utils");
        assert_eq!(unmet[0].package, "agentscad");
    }

    #[test]
    fn unreached_dependency_files_are_not_scanned() {
        // The #32 fix: a dependency ships a file the project never imports; its external
        // import must NOT be reported, while the reached file's external import is.
        let fx = Fixture::new();
        let project = fx.write_project("main.scad", "use <threadlib/threadlib.scad>\n");
        let threadlib = fx.package("threadlib");
        fx.write_pkg(
            &threadlib,
            "threadlib.scad",
            "use <scad-utils/lists.scad>\n",
        );
        // Unreached: nothing imports draw-helpers.scad.
        fx.write_pkg(
            &threadlib,
            "draw-helpers.scad",
            "use <obiscad/attach.scad>\n",
        );

        let unmet = unmet_imports(&project, &[threadlib]);
        let libs: Vec<&str> = unmet.iter().map(|u| u.library.as_str()).collect();
        assert_eq!(libs, ["scad-utils"], "obiscad is unreached: {unmet:?}");
    }

    #[test]
    fn transitive_reach_through_internal_files_is_followed() {
        // project -> dep/root.scad -> dep/src/impl.scad (internal) -> scad-utils/ (unmet).
        let fx = Fixture::new();
        let project = fx.write_project("m.scad", "use <dep/root.scad>\n");
        let dep = fx.package("dep");
        fx.write_pkg(&dep, "root.scad", "use <src/impl.scad>\n");
        fx.write_pkg(&dep, "src/impl.scad", "use <scad-utils/lists.scad>\n");

        let unmet = unmet_imports(&project, &[dep]);
        assert_eq!(unmet.len(), 1);
        assert_eq!(unmet[0].library, "scad-utils");
        assert_eq!(unmet[0].file, "src/impl.scad");
    }

    #[test]
    fn internal_subdir_imports_are_not_flagged() {
        // A library whose reached file imports its own `utils/` subdir (like NopSCADlib).
        let fx = Fixture::new();
        let project = fx.write_project("m.scad", "use <NopSCADlib/core.scad>\n");
        let nop = fx.package("NopSCADlib");
        fx.write_pkg(&nop, "core.scad", "use <utils/helpers.scad>\n");
        fx.write_pkg(&nop, "utils/helpers.scad", "// helpers\n");
        assert!(unmet_imports(&project, &[nop]).is_empty());
    }

    #[test]
    fn self_rooted_import_is_not_flagged() {
        // BOSL2's reached file does `include <BOSL2/std.scad>` (first segment == own name).
        let fx = Fixture::new();
        let project = fx.write_project("m.scad", "use <BOSL2/shapes.scad>\n");
        let bosl2 = fx.package("BOSL2");
        fx.write_pkg(&bosl2, "shapes.scad", "include <BOSL2/std.scad>\n");
        fx.write_pkg(&bosl2, "std.scad", "// std\n");
        assert!(unmet_imports(&project, &[bosl2]).is_empty());
    }

    #[test]
    fn project_own_undeclared_imports_are_not_reported() {
        // A project importing an undeclared library directly is left alone (it may rely on
        // a globally installed copy); the scan only speaks for what dependencies pull in.
        let fx = Fixture::new();
        let project = fx.write_project("m.scad", "use <SomeGlobalLib/x.scad>\n");
        assert!(unmet_imports(&project, &[]).is_empty());
    }
}
