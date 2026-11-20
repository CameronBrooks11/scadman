//! Git submodules a stored package declares but does not contain.
//!
//! scadman fetches a dependency's own commit and nothing it links to, so a submodule
//! arrives as an empty folder. A library that reads files from that folder then fails
//! inside OpenSCAD with nothing pointing back at the cause. The package's `.gitmodules`
//! is part of its stored content, so the gap can be found from the store alone.

use std::fs;
use std::path::{Component, Path};

/// A submodule declared in a package's `.gitmodules`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submodule {
    /// Where the package expects it, relative to the repository root.
    pub path: String,
    /// Where it is fetched from, as the package wrote it.
    pub url: Option<String>,
}

/// The submodules `package_root` declares whose folders hold nothing.
///
/// A folder that has content is not reported: a path dependency read from a working
/// tree has its submodules checked out. A package with no `.gitmodules` has none.
pub fn unfetched_submodules(package_root: &Path) -> Vec<Submodule> {
    let Ok(text) = fs::read_to_string(package_root.join(".gitmodules")) else {
        return Vec::new();
    };
    parse_gitmodules(&text)
        .into_iter()
        .filter(|submodule| stays_inside(&submodule.path))
        .filter(|submodule| is_empty(&package_root.join(&submodule.path)))
        .collect()
}

/// Read the `path` and `url` of each `[submodule "…"]` section. A section without a
/// `path` is dropped: there is no folder to speak of.
fn parse_gitmodules(text: &str) -> Vec<Submodule> {
    let mut found = Vec::new();
    let mut current: Option<(Option<String>, Option<String>)> = None;
    let mut finish = |section: Option<(Option<String>, Option<String>)>| {
        if let Some((Some(path), url)) = section {
            found.push(Submodule { path, url });
        }
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            finish(current.take());
            if line.to_ascii_lowercase().starts_with("[submodule") {
                current = Some((None, None));
            }
            continue;
        }
        let Some((path, url)) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim().to_ascii_lowercase().as_str() {
            "path" => *path = Some(value),
            "url" => *url = Some(value),
            _ => {}
        }
    }
    finish(current);
    found
}

/// Whether a declared path names somewhere under the package, so that checking it reads
/// nothing outside the package.
fn stays_inside(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn is_empty(dir: &Path) -> bool {
    match fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn package(gitmodules: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".gitmodules"), gitmodules).unwrap();
        dir
    }

    const ONE: &str = "[submodule \"framework\"]\n\tpath = lib/framework\n\turl = https://example.invalid/framework.git\n";

    #[test]
    fn a_declared_submodule_with_an_empty_folder_is_reported() {
        let dir = package(ONE);
        fs::create_dir_all(dir.path().join("lib/framework")).unwrap();
        assert_eq!(
            unfetched_submodules(dir.path()),
            vec![Submodule {
                path: "lib/framework".to_string(),
                url: Some("https://example.invalid/framework.git".to_string()),
            }]
        );
    }

    #[test]
    fn a_declared_submodule_with_no_folder_is_reported() {
        let dir = package(ONE);
        assert_eq!(unfetched_submodules(dir.path()).len(), 1);
    }

    #[test]
    fn a_submodule_whose_folder_has_content_is_not_reported() {
        let dir = package(ONE);
        fs::create_dir_all(dir.path().join("lib/framework")).unwrap();
        fs::write(dir.path().join("lib/framework/a.scad"), "").unwrap();
        assert_eq!(unfetched_submodules(dir.path()), Vec::new());
    }

    #[test]
    fn a_package_without_gitmodules_has_none() {
        let dir = TempDir::new().unwrap();
        assert_eq!(unfetched_submodules(dir.path()), Vec::new());
    }

    #[test]
    fn every_section_is_read_whatever_its_layout() {
        // Spaces for tabs, no indentation, a comment, a key git adds, and an unrelated
        // section between two submodules.
        let text = "# pinned\n[submodule \"a\"]\npath = a\nurl = git@example.invalid:o/a.git\n\
                    [core]\n\tpath = not-a-submodule\n\
                    [submodule \"b\"]\n    branch = main\n    path = deep/b\n";
        assert_eq!(
            parse_gitmodules(text),
            vec![
                Submodule {
                    path: "a".to_string(),
                    url: Some("git@example.invalid:o/a.git".to_string()),
                },
                Submodule {
                    path: "deep/b".to_string(),
                    url: None,
                },
            ]
        );
    }

    #[test]
    fn a_path_leading_out_of_the_package_is_not_checked() {
        let outside = TempDir::new().unwrap();
        let dir = package(&format!(
            "[submodule \"up\"]\n\tpath = ../elsewhere\n[submodule \"abs\"]\n\tpath = {}\n",
            outside.path().display()
        ));
        assert_eq!(unfetched_submodules(dir.path()), Vec::new());
    }
}
