//! Scanning installed OpenSCAD content for imports the project never declared.
//!
//! Most real transitive dependencies in the ecosystem are *manifest-less* (see
//! `docs/ecosystem-survey.md`): a library `use`s another by a qualified path like
//! `<scad-utils/…>` without declaring it anywhere scadman can read. Without this pass, a
//! user who installs such a library gets a raw OpenSCAD `can't open include file` error
//! that scadman caused and cannot explain. This pass diffs the qualified library roots the
//! installed content imports against what the resolution provides, so scadman can say
//! exactly what to add.
//!
//! It also honestly bounds scadman's guarantee: `OPENSCADPATH` cannot stop OpenSCAD from
//! searching the user and installation library folders, so "declared deps shadow globals"
//! is the real promise — this scan is how an *undeclared* dependency is surfaced.
//!
//! Parsing is heuristic but comment- and string-aware: a single pass blanks comments and
//! string literals so import-like text inside them is never matched.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

static IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:use|include)\s*<([^>]*)>").unwrap());

/// An installed package to scan: the name it is exposed under and its content root.
pub struct Installed {
    pub name: String,
    pub path: PathBuf,
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
/// neither the importing package itself nor present in `provided`.
///
/// Only *qualified* imports (`<name/…>`) are reported — a bare `<file.scad>` relies on
/// `OPENSCADPATH` and is too ambiguous to flag. Results are sorted and de-duplicated. The
/// scan is best-effort: an unreadable file or directory is skipped, not fatal.
pub fn unmet_imports(installed: &[Installed], provided: &BTreeSet<String>) -> Vec<UnmetImport> {
    let mut out = Vec::new();
    for pkg in installed {
        scan_package(pkg, &pkg.path, provided, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn scan_package(
    pkg: &Installed,
    dir: &Path,
    provided: &BTreeSet<String>,
    out: &mut Vec<UnmetImport>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() == ".git" {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            scan_package(pkg, &path, provided, out);
        } else if path.extension().is_some_and(|e| e == "scad") {
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let src = String::from_utf8_lossy(&bytes).into_owned();
            let rel = path
                .strip_prefix(&pkg.path)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            for target in import_targets(&src) {
                if let Some(library) = qualified_library(&target)
                    && library != pkg.name
                    && !provided.contains(library)
                {
                    out.push(UnmetImport {
                        package: pkg.name.clone(),
                        file: rel.clone(),
                        library: library.to_string(),
                    });
                }
            }
        }
    }
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

/// Blank comments and string literals in a single pass, preserving newlines. This keeps
/// `use`/`include` text inside a `"…"` string or a `//`/`/* */` comment from matching.
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

/// The referenced library root of a qualified import, or `None` for a bare filename or a
/// path-relative target (`./`, `../`, absolute).
fn qualified_library(target: &str) -> Option<&str> {
    let (first, _) = target.split_once('/')?;
    match first {
        "" | "." | ".." => None,
        lib => Some(lib),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn import_targets_ignores_comments_and_strings() {
        let src = "\
// use <linecommented.scad>
use <BOSL2/std.scad>
include <../rel/util.scad>
/* include <blockcommented.scad> */
echo(\"use <instring/x.scad>\");
use <bare.scad>
";
        let targets = import_targets(src);
        assert!(targets.contains(&"BOSL2/std.scad".to_string()));
        assert!(targets.contains(&"../rel/util.scad".to_string()));
        assert!(targets.contains(&"bare.scad".to_string()));
        for dropped in ["linecommented", "blockcommented", "instring"] {
            assert!(
                !targets.iter().any(|t| t.contains(dropped)),
                "should not match `{dropped}`"
            );
        }
    }

    #[test]
    fn block_marker_inside_line_comment_does_not_swallow_code() {
        // A `/*` living in a `//` comment must not open a block comment.
        let src = "// disabled /* old\nuse <real/lib.scad>\n";
        let targets = import_targets(src);
        assert_eq!(targets, vec!["real/lib.scad".to_string()]);
    }

    #[test]
    fn qualified_library_classification() {
        assert_eq!(qualified_library("BOSL2/std.scad"), Some("BOSL2"));
        assert_eq!(qualified_library("bare.scad"), None);
        assert_eq!(qualified_library("../rel/util.scad"), None);
        assert_eq!(qualified_library("./x.scad"), None);
        assert_eq!(qualified_library("/abs/x.scad"), None);
    }

    #[test]
    fn reports_only_undeclared_qualified_libraries() {
        let dir = TempDir::new().unwrap();
        // "agentscad" imports BOSL2 (provided), scad-utils (not), its own name (self,
        // ignored), a bare import (ignored), and one inside a string (ignored).
        fs::write(
            dir.path().join("main.scad"),
            "use <BOSL2/std.scad>\nuse <scad-utils/lists.scad>\nuse <agentscad/util.scad>\ninclude <bare.scad>\necho(\"use <ghost/x.scad>\");\n",
        )
        .unwrap();
        let installed = vec![Installed {
            name: "agentscad".to_string(),
            path: dir.path().to_path_buf(),
        }];
        let provided: BTreeSet<String> = ["BOSL2".to_string()].into_iter().collect();

        let unmet = unmet_imports(&installed, &provided);
        assert_eq!(unmet.len(), 1);
        assert_eq!(unmet[0].package, "agentscad");
        assert_eq!(unmet[0].library, "scad-utils");
        assert_eq!(unmet[0].file, "main.scad");
    }

    #[test]
    fn empty_when_all_imports_are_provided_or_internal() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(
            dir.path().join("sub/a.scad"),
            "use <BOSL2/std.scad>\nuse <self/inner.scad>\n",
        )
        .unwrap();
        let installed = vec![Installed {
            name: "self".to_string(),
            path: dir.path().to_path_buf(),
        }];
        let provided: BTreeSet<String> = ["BOSL2".to_string()].into_iter().collect();
        assert!(unmet_imports(&installed, &provided).is_empty());
    }
}
