# scadman

A project-oriented package manager for OpenSCAD libraries and scripts.

Instead of copying libraries into OpenSCAD's global `libraries/` folder and hoping every
project still works when one of them changes, scadman gives each project a manifest
(`scadman.toml`) and a lockfile (`scadman.lock`) that pin exact dependency versions. Most
OpenSCAD libraries are git repositories that never publish releases, so scadman is
GitHub-first: add a library by URL, track its branch, and pin it to an exact commit at
lock time.

New here? Start with [Getting started](getting-started.md).

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

## In these docs

**Using scadman**

- [Getting started](getting-started.md) — install, a first project, and every command
- [Manifest and lockfile](manifest.md) — `scadman.toml` and `scadman.lock` reference, and where scadman keeps its files
- [Path dependencies](path-dependencies.md) — co-developing with a local sibling library
- [Library roots](library-roots.md) — consuming src-layout libraries (`--root`, `--on-path`)
- [Editor and GUI integration](editor-integration.md) — language servers, VS Code, the OpenSCAD GUI

**Project**

- [Changelog](https://github.com/CameronBrooks11/scadman/blob/main/CHANGELOG.md)
- [Contributing](https://github.com/CameronBrooks11/scadman/blob/main/CONTRIBUTING.md)

**Design records** (background and history, not user documentation)

- [Foundational decisions](DECISIONS.md) — the load-bearing choices and their rationale
- [Ecosystem survey](ecosystem-survey.md) — the survey evidence the design rests on
- [Resolver direction](resolver-direction.md) — why v1 resolution is collect-and-unify, not a solver

These pages are published at <https://cameronbrooks11.github.io/scadman/>.
