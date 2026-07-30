# scadman

A project-oriented package and dependency manager for OpenSCAD libraries, scripts, and reusable design assets.

> I'm a scad mannnnnnnnnnnnnnn ski-ba-bop-ba-dop-bop

## What it is

scadman treats an OpenSCAD project as a reproducible unit — a manifest plus a lockfile
that pin exact dependency versions — rather than a global pile of installed libraries.
Project-local reproducibility comes first; global installation is a convenience, not the
default.

It models three distinct things instead of forcing everything into one "package" shape:

- **Artifact** — any reusable OpenSCAD content (a single `.scad`, a repo, a tagged release), with or without metadata.
- **Package** — an artifact with an explicit contract: identity, version, license, declared roots, dependencies, integrity.
- **Project** — a reproducible environment that consumes packages and raw artifacts via a manifest and an exact lockfile.

## Install

`v0.1.0-alpha`. With a recent Rust toolchain (≥ 1.85) and `git` + `openscad` on `PATH`:

```sh
cargo install --git https://github.com/CameronBrooks11/scadman --tag v0.1.0-alpha scadman-cli
```

This installs the `scadman` binary. (Or, from a clone: `cargo build --release`.)

## Usage

In a project directory:

```sh
scadman init                 # create scadman.toml (and ignore .scadman/)
scadman add BOSL2 https://github.com/BelfrySCAD/BOSL2 --tag v2.0.700
scadman list                 # show declared dependencies and their locked state
scadman lock                 # resolve dependencies → scadman.lock (exact rev + content hash)
scadman sync                 # materialize the environment; warn about undeclared imports
scadman run -- model.scad -o out.stl   # run OpenSCAD with the project's dependencies
scadman remove BOSL2         # drop a dependency
scadman graph                # show the resolved dependency graph (--json for tooling)
scadman doctor               # check OpenSCAD, store, manifest, lock, and environment
```

`add` also accepts `--rev <commit>` or `--branch <name>` (branches are locked to a commit).
Because most OpenSCAD libraries don't publish releases, an exact git revision is a
first-class dependency form, not an afterthought.

To co-develop a project alongside a local library, depend on it by path instead of a git
source:

```sh
scadman add mylib --path ../mylib
```

A path dependency tracks the directory's *current* content — it is re-read on every `sync`,
so edits to the sibling show up immediately, while git dependencies alongside it stay pinned
to their locked commits (no re-fetch, so `sync` still works offline). It accepts
`--root`/`--on-path` like a git source, for co-developing a src-layout library. The whole
directory is copied into the store (minus `.git` and symlinks), so keep build output and
nested envs out of the library root. A path dependency is a local-development convenience,
not a reproducible pin: a lockfile that references one is not portable to another machine
(see *Status & scope*).

Libraries whose code lives under a subdir (e.g. `src/`) and import from that root — such as
dotSCAD — are added with `--root src --on-path` (see
[docs/library-roots.md](docs/library-roots.md)).

Many libraries pull in others without declaring it (e.g. `threadlib` uses `scad-utils`).
`sync` follows your project's imports into each dependency and warns about any *undeclared*
library they pull in, naming which library is missing — `scadman add <name> <url>` each,
then `lock`/`sync` again.

### Editor integration

`scadman env` prints the project's `OPENSCADPATH`, and `scadman env --json` emits a
machine-readable report of the resolved packages. Point an OpenSCAD language server
(`OPENSCADPATH` or its `searchPaths` setting) or a preview extension at it and editor
features resolve against the project's *pinned* dependency versions:

```sh
export OPENSCADPATH="$(scadman env)"
```

For VS Code, `scadman env --write-vscode` writes the path into `.vscode/settings.json` as
`openscad.search_paths` (for [openscad-LSP](https://github.com/Leathong/openscad-LSP)),
merging into any existing settings.

## How it works

A dependency is resolved to an exact commit, its content is hashed and stored immutably in
a content-addressed store (`~/.local/share/scadman/store/<hash>/`), and `sync` builds a
per-project environment (`.scadman/env/`) that exposes each package under its own name —
symlinked to the store — with `OPENSCADPATH` pointing at it. So a library's own
`include <BOSL2/…>` resolves to exactly the pinned content.

OpenSCAD has a flat namespace (`include` is textual, no module scoping), so scadman
enforces **one resolved version per package identity** and warns when installed code
imports a library you didn't declare. Design rationale and the ecosystem evidence behind
these choices are in [docs/](docs/) — see
[DECISIONS.md](docs/DECISIONS.md), [ecosystem-survey.md](docs/ecosystem-survey.md), and
[resolver-direction.md](docs/resolver-direction.md).

## Status & scope

**`v0.1.0-alpha`** — usable end-to-end for GitHub-hosted git dependencies
(`init → add → lock → sync → run`), validated against real libraries (BOSL2, NopSCADlib,
MCAD, dotSCAD, Round-Anything). Expect rough edges and breaking changes.

Deliberate limitations for this alpha:

- **GitHub-first.** Git sources plus local `path` dependencies (for co-developing a sibling library); no hosted registry, and registry/version dependencies are not yet supported.
- **Path dependencies are local, not reproducible.** They track a directory's current content and are re-read each `sync`; a lockfile that references one is not portable to another machine.
- **One version per identity.** OpenSCAD's flat namespace means a project resolves a single version of each library (no coexisting versions).
- **Lockfile hashes are per-OS.** A `scadman.lock` is reproducible across machines of the same OS; cross-OS sharing is not yet guaranteed.
- **`on_path` libraries share a flat namespace.** Opting a dependency into `on_path` places its root on `OPENSCADPATH`; two such libraries with a same-named top-level file collide (scadman warns).
- **No native OpenSCAD-GUI integration.** `OPENSCADPATH` is the integration seam (`scadman env`); GUI/registry work is future.

Roadmap: tracked on GitHub — see the [post-alpha roadmap epic](https://github.com/CameronBrooks11/scadman/issues/44).
