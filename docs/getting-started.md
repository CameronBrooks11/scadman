# Getting started

## Install

Grab the prebuilt binary and `checksums.txt` from the
[latest release](https://github.com/CameronBrooks11/scadman/releases/latest) (Linux x86_64
only). In the directory you downloaded both into:

```sh
sha256sum --ignore-missing -c checksums.txt
tar -xzf scadman-*-x86_64-unknown-linux-gnu.tar.gz
install -Dm755 scadman ~/.local/bin/scadman   # or anywhere on your PATH
```

On macOS, or another Linux architecture, build from source with a Rust toolchain
(≥ 1.88):

```sh
cargo install --locked --git https://github.com/CameronBrooks11/scadman --tag v0.1.0-alpha.3 scadman-cli
```

Either way, `git` and `openscad` need to be on `PATH`. `scadman doctor` reports what it
finds.

## A first project

In your project directory:

```sh
scadman init                                            # create scadman.toml
scadman add BOSL2 https://github.com/BelfrySCAD/BOSL2   # track its default branch
scadman lock                                            # pin it to an exact commit
scadman run -- model.scad -o out.stl                    # render with BOSL2 available
```

`model.scad` imports the library under the name you gave it:

```openscad
include <BOSL2/std.scad>
```

Commit `scadman.toml` and `scadman.lock`. The `.scadman/` directory is machine-local;
`init` adds it to `.gitignore`. Run scadman from the project root, where `scadman.toml`
lives.

## Commands

| Command | What it does |
|---|---|
| `scadman init [--name N]` | Create `scadman.toml` and ignore `.scadman/`. |
| `scadman add <name> <git-url>` | Declare a git dependency. `--tag`, `--branch` or `--rev` picks the ref; with none, it tracks the remote's default branch. |
| `scadman add <name> --path <dir>` | Declare a local directory as a dependency. |
| `scadman remove <name>` | Drop a dependency from `scadman.toml`. |
| `scadman list` | Show declared dependencies and the commit each is locked to. |
| `scadman lock` | Resolve every dependency and write `scadman.lock`. |
| `scadman update [name…]` | Move branch and tag dependencies to their newest commit, all or just the named ones, and report what moved. |
| `scadman sync` | Build the project environment from the lockfile and warn about undeclared imports. |
| `scadman run -- <openscad args>` | Sync, then run OpenSCAD with the environment on `OPENSCADPATH`. |
| `scadman env [--json \| --write-vscode]` | Sync, then print the `OPENSCADPATH` for editors and tools. |
| `scadman graph [--json]` | Print the locked dependency graph. |
| `scadman doctor` | Report on OpenSCAD, the store, the manifest, the lockfile and the environment. |

A branch or tag is pinned to an exact commit at lock time and stays there until you run
`scadman update`. A `--rev` pin never moves; give it the full 40-character commit hash
([#90](https://github.com/CameronBrooks11/scadman/issues/90)).

After editing `scadman.toml`, by hand or with `add` and `remove`, run `scadman lock`.
`sync` and `run` stop with an "out of date" error for some stale lockfiles, but they do
not catch every edit. The details are in [Manifest and lockfile](manifest.md).

## Local libraries and unusual layouts

- To develop a project alongside a library on your own disk, depend on it by path:
  `scadman add mylib --path ../mylib`. See [Path dependencies](path-dependencies.md).
- Some libraries, dotSCAD for example, keep their code under a subdirectory like `src/`
  and import relative to it. Add them with `--root src --on-path`. See
  [Library roots](library-roots.md).

## Undeclared imports

Many libraries pull in others without declaring it; `agentscad`, for example, uses
`scad-utils` and `list-comprehension-demos`. `sync` follows your project's imports into
each dependency and names any undeclared library it reaches:

```
warning: `agentscad` imports `scad-utils/` (in mesh.scad) but it is not in your dependencies
```

`scadman add` each named library, then `lock` and `sync` again. Imports of a bare
filename (`use <file.scad>`) can't be attributed to a library and aren't flagged.

## Minimum OpenSCAD version

Declare `openscad = "2021.01"` under `[project]` (a `>=` prefix is accepted; ranges and
other operators aren't) and `doctor` and `run` warn when the installed OpenSCAD is
older. Advisory only, not a hard gate.
