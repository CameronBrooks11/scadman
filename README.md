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
[latest release](https://github.com/CameronBrooks11/scadman/releases/latest) (Linux x86_64).
In the directory you downloaded both into:

```sh
sha256sum --ignore-missing -c checksums.txt
tar -xzf scadman-*-x86_64-unknown-linux-gnu.tar.gz
install -Dm755 scadman ~/.local/bin/scadman   # or anywhere on your PATH
```

On macOS (or a different Linux architecture), build from source with a Rust toolchain
(≥ 1.88):

```sh
cargo install --git https://github.com/CameronBrooks11/scadman --tag v0.1.0-alpha.3 scadman-cli
```

Either way, `git` and `openscad` need to be on `PATH`.

## Usage

In a project directory:

```sh
scadman init                 # create scadman.toml (and ignore .scadman/)
scadman add BOSL2 https://github.com/BelfrySCAD/BOSL2   # tracks the default branch
scadman list                 # show declared dependencies and their locked state
scadman lock                 # resolve dependencies → scadman.lock (exact rev + content hash)
scadman update [name…]       # advance branch/tag deps to newer commits (all, or just some)
scadman sync                 # materialize the environment; warn about undeclared imports
scadman run -- model.scad -o out.stl   # run OpenSCAD with the project's dependencies
scadman remove BOSL2         # drop a dependency
scadman graph                # show the resolved dependency graph (--json for tooling)
scadman doctor               # check OpenSCAD, store, manifest, lock, and environment
```

With no ref, `add` tracks the remote's default branch, like `git clone`; pass
`--tag <name>`, `--branch <name>`, or `--rev <commit>` to pin a specific tag, branch, or
commit. A branch or tag
is pinned to an exact commit at lock time. Advance it later with `scadman update` (all
dependencies) or `scadman update <name>` (just one, holding the rest); it reports which
commits moved. Exact `rev` pins never move.

### Path dependencies

To co-develop a project alongside a local library, depend on it by path:

```sh
scadman add mylib --path ../mylib
```

`sync`, `run`, and `env` re-read the directory, so edits to the sibling's code show up
immediately, while git dependencies beside it stay pinned. Details and caveats:
[docs/path-dependencies.md](docs/path-dependencies.md).

### Library roots

Some libraries — dotSCAD, for example — keep their code under a subdirectory like `src/`
and import relative to it. Add them with `--root src --on-path`; see
[docs/library-roots.md](docs/library-roots.md).

### Undeclared imports

Many libraries pull in others without declaring it; `agentscad`, for example, uses
`scad-utils` and `list-comprehension-demos`. `sync` follows your project's imports into
each dependency and names any undeclared library it reaches:

```
warning: `agentscad` imports `scad-utils/` (in mesh.scad) but it is not in your dependencies
```

`scadman add` each named library, then `lock` and `sync` again. Imports of a bare
filename (`use <file.scad>`) can't be attributed to a library and aren't flagged.

### Minimum OpenSCAD version

Declare `openscad = "2021.01"` under `[project]` (a `>=` prefix is accepted; other
operators aren't) and `doctor` and `run` warn when the installed OpenSCAD is older.
Advisory only, not a hard gate.

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
merging into any existing settings. Paths are written relative to `${workspaceFolder}`,
so the file is safe to commit.

For the OpenSCAD GUI, `scadman run -- model.scad` opens the model with `OPENSCADPATH`
set. Or export the variable (as above) in the shell you launch OpenSCAD from.

## How it works

A dependency is resolved to an exact commit, its content is hashed and stored immutably
under that hash (`~/.local/share/scadman/store/<hash>/`), and `sync` builds a per-project
environment (`.scadman/env/`) that exposes each library under its own name — symlinked to
the store — with `OPENSCADPATH` pointing at it. So a library's own `include <BOSL2/…>`
resolves to exactly the pinned content.

OpenSCAD has a flat namespace (`include` is textual, no module scoping), so scadman
enforces one resolved version per library and warns when installed code imports a
library you didn't declare. Design rationale and the ecosystem evidence behind these
choices are in [docs/](docs/).

## Status & scope

An alpha, usable end-to-end (`init → add → lock → update → sync → run`) and validated
against real libraries (BOSL2, NopSCADlib, MCAD, dotSCAD, Round-Anything) plus a
12-project dogfooding pass. Expect rough edges and breaking changes. Current limits:

- Git and local path sources only; no hosted registry or version-range dependencies yet.
- One resolved version per library — a consequence of OpenSCAD's flat `include` namespace.
- Linux and macOS; Windows isn't supported yet. Lockfiles are reproducible across both,
  though git's line-ending conversion or Unicode filename normalization can still change
  a content hash in edge cases.
- Path dependencies are a local-development convenience, not a reproducible pin.

Roadmap: [post-alpha roadmap epic](https://github.com/CameronBrooks11/scadman/issues/44).

## License

Apache-2.0. Bug reports and feedback are welcome on the
[issue tracker](https://github.com/CameronBrooks11/scadman/issues).

> I'm a scad mannnnnnnnnnnnnnn ski-ba-bop-ba-dop-bop
