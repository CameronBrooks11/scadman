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

## Status

Early. Direction and foundational decisions are recorded in [docs/DECISIONS.md](docs/DECISIONS.md).
The next concrete step is a Rust core with a project-local environment model, informed by an
ecosystem survey of real OpenSCAD libraries.
