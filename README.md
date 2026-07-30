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

- **GitHub-first.** Git sources only — no hosted registry; `path` and registry/version dependencies are not yet supported.
- **One version per identity.** OpenSCAD's flat namespace means a project resolves a single version of each library (no coexisting versions).
- **Lockfile hashes are per-OS.** A `scadman.lock` is reproducible across machines of the same OS; cross-OS sharing is not yet guaranteed.
- **`on_path` libraries share a flat namespace.** Opting a dependency into `on_path` places its root on `OPENSCADPATH`; two such libraries with a same-named top-level file collide (scadman warns).
- **No native OpenSCAD-GUI integration.** `OPENSCADPATH` is the integration seam (`scadman env`); GUI/registry work is future.

Roadmap: tracked on GitHub — see the [post-alpha roadmap epic](https://github.com/CameronBrooks11/scadman/issues/44).
