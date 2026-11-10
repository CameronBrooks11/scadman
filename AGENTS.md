# AGENTS.md

Instructions for AI coding agents working in this repository.

## Project

scadman is a project-oriented package and dependency manager for OpenSCAD — a Rust
core plus a single-binary CLI. It treats an OpenSCAD project as a reproducible unit
(manifest + lockfile) rather than a global pile of installed libraries. See
[README.md](README.md) for usage, [docs/manifest.md](docs/manifest.md) for the file
formats, [docs/DECISIONS.md](docs/DECISIONS.md) for the product model and foundational
decisions, and [docs/ecosystem-survey.md](docs/ecosystem-survey.md) for the evidence the
design is grounded in. [CONTRIBUTING.md](CONTRIBUTING.md) covers the pull-request,
changelog and release steps.

## Stack

- **Language** — Rust (edition 2024), Cargo workspace.
- **Tooling** — rustfmt, clippy, cargo test, `just`, pre-commit (gitleaks).

## Layout

```
crates/
  scadman-core/    # data model + logic: manifest, lockfile, resolver, store, environment, include-scan
  scadman-cli/     # the `scadman` binary + end-to-end tests
docs/              # user guides + design records; source of the docs site (book.toml, docs/SUMMARY.md)
fixtures/          # archetype libraries pinned for tests (references, not vendored)
```

## Commands

```sh
just setup        # rustup components (rustfmt, clippy) + pre-commit git hooks
just fmt          # format
just check        # fmt-check + clippy -D warnings (stricter than CI, where clippy is advisory)
just test         # cargo test
just build        # build all crates
just run init     # run the CLI (args pass through, e.g. `just run add BOSL2 <url>`)
```

## Conventions

- Format before committing; `just check && just test` must pass. Never bypass hooks.
- Conventional Commits (`type(scope): subject`). One logical change per commit.
- A user-visible change gets a line under `## [Unreleased]` in `CHANGELOG.md`, and the
  docs that describe the behaviour move in the same change.
- A new page under `docs/` is published only once it is listed in `docs/SUMMARY.md`.
  Link between docs pages with relative `.md` links; link out of `docs/` with full URLs.
- Keep the core library free of CLI/IO concerns where practical; the CLI is a thin shell.

## Constraints (design guardrails from the survey + decisions)

- **Do not rewrite third-party library source** to resolve imports — the environment
  exposes libraries under their own names on the search path instead.
- Assume **one resolved version per package identity per environment** (flat OpenSCAD
  namespace; collisions are real).
- Treat unversioned / exact-revision Git sources as first-class — most libraries do
  not release.
- Do not commit secrets or credentials.
