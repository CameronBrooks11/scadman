# AGENTS.md

Instructions for AI coding agents working in this repository.

## Project

scadman is a project-oriented package and dependency manager for OpenSCAD — a Rust
core plus a single-binary CLI. It treats an OpenSCAD project as a reproducible unit
(manifest + lockfile) rather than a global pile of installed libraries. See
[README.md](README.md) for the model, [docs/DECISIONS.md](docs/DECISIONS.md) for the
foundational decisions, and [docs/ecosystem-survey.md](docs/ecosystem-survey.md) for
the evidence the design is grounded in.

## Stack

- **Language** — Rust (edition 2024), Cargo workspace.
- **Tooling** — rustfmt, clippy, cargo test, `just`, pre-commit (gitleaks).

## Layout

```
crates/
  scadman-core/    # data model + logic: manifest, lockfile, resolver, environment
  scadman-cli/     # the `scadman` binary
docs/              # decisions, ecosystem survey
fixtures/          # archetype libraries pinned for tests (references, not vendored)
```

## Commands

```sh
just setup        # rustup components (rustfmt, clippy)
just fmt          # format
just check        # fmt-check + clippy -D warnings (CI-equivalent)
just test         # cargo test
just build        # build all crates
just run -- init  # run the CLI
```

## Conventions

- Format before committing; `just check && just test` must pass. Never bypass hooks.
- Conventional Commits (`type(scope): subject`). One logical change per commit.
- Keep the core library free of CLI/IO concerns where practical; the CLI is a thin shell.

## Constraints (design guardrails from the survey + decisions)

- **Do not rewrite third-party library source** to resolve imports — the environment
  exposes libraries under their own names on the search path instead.
- Assume **one resolved version per package identity per environment** (flat OpenSCAD
  namespace; collisions are real).
- Treat unversioned / exact-revision Git sources as first-class — most libraries do
  not release.
- Do not commit secrets or credentials.
