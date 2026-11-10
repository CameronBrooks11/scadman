# scadman

[![CI](https://github.com/CameronBrooks11/scadman/actions/workflows/ci.yml/badge.svg)](https://github.com/CameronBrooks11/scadman/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/CameronBrooks11/scadman?include_prereleases)](https://github.com/CameronBrooks11/scadman/releases)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A project-oriented package manager for OpenSCAD libraries and scripts.

Instead of copying libraries into OpenSCAD's global `libraries/` folder and hoping every
project still works when one of them changes, scadman gives each project a manifest
(`scadman.toml`) and a lockfile (`scadman.lock`) that pin exact dependency versions. Most
OpenSCAD libraries are git repositories that never publish releases, so scadman is
GitHub-first: add a library by URL, track its branch, and pin it to an exact commit at
lock time.

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

## Quick start

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
not catch every edit.

The manifest format, the lockfile and where scadman keeps its files are described in
[docs/manifest.md](docs/manifest.md).

## Path dependencies

To co-develop a project alongside a local library, depend on it by path:

```sh
scadman add mylib --path ../mylib
```

`sync`, `run` and `env` re-read the directory, so edits to the sibling's code show up
immediately, while git dependencies beside it stay pinned. Details and caveats:
[docs/path-dependencies.md](docs/path-dependencies.md).

## Library roots

Some libraries, dotSCAD for example, keep their code under a subdirectory like `src/`
and import relative to it. Add them with `--root src --on-path`; see
[docs/library-roots.md](docs/library-roots.md).

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

## Editor and GUI integration

`scadman env` prints the project's `OPENSCADPATH`, and `scadman env --json` emits a
machine-readable report of the resolved libraries. Point an OpenSCAD language server or
preview extension at that path and editor features resolve against the project's pinned
dependency versions:

```sh
export OPENSCADPATH="$(scadman env)"
```

For VS Code, `scadman env --write-vscode` writes the path into `.vscode/settings.json` as
`openscad.search_paths` (for [openscad-LSP](https://github.com/Leathong/openscad-LSP)),
keeping the other settings. Paths are written relative to `${workspaceFolder}`, so the
file is safe to commit. An existing `settings.json` must be plain JSON: one with comments
is refused, and you add the setting by hand.

For the OpenSCAD GUI, `scadman run -- model.scad` opens the model with `OPENSCADPATH`
set. Or export the variable, as above, in the shell you launch OpenSCAD from.

## How it works

A dependency is resolved to an exact commit, its content is hashed and stored immutably
under that hash (`~/.local/share/scadman/store/<hash>/`), and `sync` builds a per-project
environment (`.scadman/env/`) that exposes each library under its own name, symlinked to
the store, with `OPENSCADPATH` pointing at it. So a library's own `include <BOSL2/…>`
resolves to exactly the pinned content.

OpenSCAD has a flat namespace (`include` is textual, no module scoping), so scadman
enforces one resolved version per library and warns when installed code imports a
library you didn't declare. OpenSCAD still searches its own user and installation
library folders after `OPENSCADPATH`: declared dependencies shadow globally installed
ones, but scadman cannot hide them.

Design rationale and the ecosystem evidence behind these choices are in [docs/](docs/).

## Status and scope

An alpha, usable end-to-end (`init → add → lock → update → sync → run`) and validated
against real libraries (BOSL2, NopSCADlib, MCAD, dotSCAD, Round-Anything) plus a
12-project dogfooding pass. Expect rough edges and breaking changes. Current limits:

- Git and local path sources only; no hosted registry or version-range dependencies yet.
- One resolved version per library, a consequence of OpenSCAD's flat `include` namespace.
- Linux and macOS; Windows isn't supported yet. Lockfiles are reproducible across both,
  though git's line-ending conversion or Unicode filename normalization can still change
  a content hash in edge cases.
- Path dependencies are a local-development convenience, not a reproducible pin.
- Nothing prunes the store yet
  ([#96](https://github.com/CameronBrooks11/scadman/issues/96)); it is safe to delete,
  and the next `sync` fetches what the lockfile needs.

Known bugs are on the [issue tracker](https://github.com/CameronBrooks11/scadman/issues);
planned work is in the
[post-alpha roadmap](https://github.com/CameronBrooks11/scadman/issues/44).

## Contributing

Bug reports and feedback are welcome on the
[issue tracker](https://github.com/CameronBrooks11/scadman/issues). To build, test and
send a change, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache-2.0.

> I'm a scad mannnnnnnnnnnnnnn ski-ba-bop-ba-dop-bop
