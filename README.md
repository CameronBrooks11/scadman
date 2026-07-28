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

## Usage

Build the CLI with a recent Rust toolchain (`cargo build --release`; the binary is
`scadman`), then, in a project directory:

```sh
scadman init                 # create scadman.toml
scadman add BOSL2 https://github.com/BelfrySCAD/BOSL2 --tag v2.0.700
scadman lock                 # resolve dependencies → scadman.lock (exact rev + content hash)
scadman sync                 # materialize the environment; warn about undeclared imports
scadman run -- model.scad -o out.stl   # run OpenSCAD with the project's dependencies
```

`add` also accepts `--rev <commit>` or `--branch <name>` (branches are locked to a commit).
Because most OpenSCAD libraries don't publish releases, an exact git revision is a
first-class dependency form, not an afterthought.

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

## Status

Early but usable end-to-end for git dependencies (`init → add → lock → sync → run`). A
hosted registry, richer package metadata, and native OpenSCAD-editor integration are
future work.
