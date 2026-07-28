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
//! To avoid false positives (validated against BOSL2, NopSCADlib, dotSCAD, …) it:
//! - resolves an import against the package's own files first (an import like `<utils/…>`
//!   from a file that resolves to the package's own `utils/` is internal, not a dep), and
//! - skips the package's non-library directories (`examples/`, `test/`, `docs/`), whose
//!   imports are not on a consumer's include path.
//!
//! It also honestly bounds scadman's guarantee: `OPENSCADPATH` cannot stop OpenSCAD from
//! searching the user and installation library folders, so "declared deps shadow globals"
//! is the real promise — this scan is how an *undeclared* dependency is surfaced. Parsing
//! is comment- and string-aware.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

static IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:use|include)\s*<([^>]*)>").unwrap());

/// Top-level directories that hold a library's own examples/tests/docs rather than the
/// code a consumer imports; their imports are not the user's dependencies.
const NON_LIBRARY_DIRS: &[&str] = &["examples", "example", "test", "tests", "docs", "doc"];

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

/// Find qualified imports across `installed` packages that name a library root which is
/// neither resolvable within the package itself nor present in `provided`.
///
/// Only *qualified* imports (`<name/…>`) that don't resolve internally are reported — a
/// bare `<file.scad>` relies on `OPENSCADPATH` and is too ambiguous to flag. Results are
/// sorted and de-duplicated. The scan is best-effort: unreadable files/dirs are skipped.
pub fn unmet_imports(installed: &[Installed], provided: &BTreeSet<String>) -> Vec<UnmetImport> {
    let mut out = Vec::new();
    for pkg in installed {
        let files = collect_scad(&pkg.path);
        for rel in &files {
            if in_non_library_dir(rel) {
                continue;
            }
            let Ok(bytes) = fs::read(pkg.path.join(rel)) else {
                continue;
            };
            let src = String::from_utf8_lossy(&bytes);
            for target in import_targets(&src) {
                if let Some(library) = unmet_library(rel, &target, &files, &pkg.name, provided) {
                    out.push(UnmetImport {
                        package: pkg.name.clone(),
                        file: rel.clone(),
                        library,
                    });
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
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
        if name == ".git" {
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

fn in_non_library_dir(rel: &str) -> bool {
    rel.split('/')
        .next()
        .is_some_and(|top| NON_LIBRARY_DIRS.contains(&top))
}

/// If `target` (imported from `importer`) names a library the package does not itself
/// provide, return that library root; otherwise `None`.
fn unmet_library(
    importer: &str,
    target: &str,
    files: &BTreeSet<String>,
    package: &str,
    provided: &BTreeSet<String>,
) -> Option<String> {
    let target = target.trim();
    // Resolves within the package (relative to the importing file, or from its root)?
    if files.contains(&join_norm(dir_of(importer), target)) || files.contains(&norm(target)) {
        return None;
    }
    // A bare filename (no `/`) relies on OPENSCADPATH — too ambiguous to flag.
    let (first, _) = target.split_once('/')?;
    if matches!(first, "" | "." | "..") || first == package || provided.contains(first) {
        return None;
    }
    Some(first.to_string())
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
    fn internal_subdir_imports_are_not_flagged() {
        // A library whose root file imports its own `utils/` subdir (like NopSCADlib) must
        // not be reported — the import resolves within the package.
        let dir = TempDir::new().unwrap();
        write(dir.path(), "core.scad", "use <utils/helpers.scad>\n");
        write(dir.path(), "utils/helpers.scad", "// helpers\n");
        let installed = vec![Installed {
            name: "NopSCADlib".to_string(),
            path: dir.path().to_path_buf(),
        }];
        assert!(unmet_imports(&installed, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn example_and_test_dirs_are_skipped() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "examples/demo.scad",
            "use <Something-1.0/x.scad>\n",
        );
        write(dir.path(), "test/t.scad", "use <voxel/x.scad>\n");
        let installed = vec![Installed {
            name: "Lib".to_string(),
            path: dir.path().to_path_buf(),
        }];
        assert!(unmet_imports(&installed, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn a_genuine_external_library_is_reported() {
        // `agentscad` imports `scad-utils/` which resolves nowhere in the package.
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "main.scad",
            "use <scad-utils/lists.scad>\nuse <agentscad/util.scad>\ninclude <bare.scad>\n",
        );
        let installed = vec![Installed {
            name: "agentscad".to_string(),
            path: dir.path().to_path_buf(),
        }];
        // BOSL2 provided; scad-utils not; agentscad is self; bare ignored.
        let provided: BTreeSet<String> = ["BOSL2".to_string()].into_iter().collect();
        let unmet = unmet_imports(&installed, &provided);
        assert_eq!(unmet.len(), 1);
        assert_eq!(unmet[0].library, "scad-utils");
    }

    #[test]
    fn self_rooted_import_is_not_flagged() {
        // BOSL2's own `include <BOSL2/std.scad>` (first segment == package name).
        let dir = TempDir::new().unwrap();
        write(dir.path(), "shapes.scad", "include <BOSL2/std.scad>\n");
        let installed = vec![Installed {
            name: "BOSL2".to_string(),
            path: dir.path().to_path_buf(),
        }];
        assert!(unmet_imports(&installed, &BTreeSet::new()).is_empty());
    }
}
