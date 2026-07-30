# Changelog

All notable changes to scadman are recorded here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[SemVer](https://semver.org/) (pre-1.0: expect breaking changes between minor versions).

## [Unreleased]

### Changed

- **OS-independent content hashes** — the store/lockfile hash now serializes paths
  canonically (components joined with `/`, each Unicode-normalized to NFC) and ignores file
  mode, so a `scadman.lock` is reproducible across Linux and macOS (e.g. an NFD filename on
  a macOS filesystem hashes the same as its NFC form on Linux). For ASCII content — all real
  OpenSCAD libraries — the hash is unchanged, so existing lockfiles stay valid.

### Added

- **`scadman update [name…]`** — advance dependencies to newer commits and report what
  moved. With no arguments it re-resolves everything (like `scadman lock`, but with a delta
  report); with names it advances only those and holds the rest at their locked revisions
  (path deps are always re-read). Exact `rev` pins never move. `doctor` now notes when
  dependencies track a branch/tag.

- **Path dependencies** — depend on a local sibling library with
  `Name = { path = "../lib" }` (or `scadman add Name --path ../lib`, with `--root`/`--on-path`
  for a src-layout sibling). `sync`/`run`/`env` re-read the directory so code edits are
  picked up immediately, while git dependencies beside it stay pinned and are served from
  the store (offline-capable); a changed git pin still needs `scadman lock`. Only the root
  manifest may declare a path dependency, and a lockfile referencing one is local, not
  reproducible.

## [0.1.0-alpha] — 2026-07-28

First public alpha. A working, project-oriented OpenSCAD dependency manager, grounded in a
228-repo ecosystem survey and validated against real libraries.

### Added

- **Project model** — `scadman.toml` manifest + `scadman.lock` lockfile pinning each
  dependency to an exact commit and content hash.
- **CLI** — `init`, `add`, `remove`, `list`, `lock`, `sync`, `run`, `env`.
- **Content-addressed store** — immutable, deduplicated package content with atomic,
  symlink-skipping inserts.
- **Collect-and-unify resolver** — one resolved version per package identity, structured
  conflict diagnostics, no version-range solving (none is needed yet — see
  `docs/resolver-direction.md`).
- **Per-project environment** — exposes each package under its own name via `OPENSCADPATH`,
  so a library's self-rooted `include <Name/…>` resolves to the pinned content.
- **Library roots** — `root` (expose a subdir as the library root) and `on_path` (place it
  on `OPENSCADPATH`) support src-layout libraries like dotSCAD.
- **Include-scan** — warns when installed code imports a library you did not declare.
- **Editor integration** — `scadman env` / `env --json` for openscad-LSP and preview
  extensions to resolve against pinned versions.

### Known limitations

GitHub-hosted git sources only; one version per identity; per-OS lockfile hashes; no hosted
registry or native OpenSCAD-GUI integration yet. See the README's *Status & scope*.

[0.1.0-alpha]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha
