# Changelog

All notable changes to scadman are recorded here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[SemVer](https://semver.org/) (pre-1.0: expect breaking changes between minor versions).

## [Unreleased]

### Fixed

- **`scadman doctor` no longer reports an unparseable manifest as missing.** It
  loaded the manifest in a way that collapsed a parse error into the same result
  as an absent file, so a typo in `scadman.toml` printed
  `scadman.toml not found — run \`scadman init\`` — advice that then fails with
  `scadman.toml already exists`. Doctor now separates the cases and prints the
  TOML parse error, with the offending line, which is what it exists to do.

## [0.1.0-alpha.2] — 2026-07-30

Second alpha. Rounds out the CLI (`graph`, `doctor`, `update`), adds local path
dependencies and editor integration, makes the include-scan precise, and smooths first-run
onboarding. Validated against 12 real projects (a dogfooding pass).

### Added

- **`scadman graph`** — print the resolved dependency graph as a tree, or `--json` for tooling.
- **`scadman doctor`** — a setup report: OpenSCAD, store, manifest, lockfile freshness, and
  the built environment, each line nudging the next step.
- **`scadman update [name…]`** — advance dependencies to newer commits and report what moved.
  With no arguments it re-resolves everything (like `lock`, but with a delta report); with
  names it advances only those and holds the rest at their locked revisions. Exact `rev` pins
  never move; `doctor` notes when dependencies track a branch/tag.
- **Path dependencies** — depend on a local sibling library with `Name = { path = "../lib" }`
  (or `scadman add Name --path ../lib`; `--root`/`--on-path` for a src-layout sibling).
  `sync`/`run`/`env` re-read it so code edits show up immediately, while git dependencies
  beside it stay pinned and are served from the store (offline-capable). Only the root
  manifest may declare one, and a lockfile referencing one is local, not reproducible.
- **Minimum OpenSCAD version** — declare `openscad = "2021.01"` under `[project]` (a `>=`
  prefix is accepted) and `doctor`/`run` warn when the installed OpenSCAD is older. Advisory
  only; OpenSCAD compatibility is really feature-based.
- **Editor integration** — `scadman env --write-vscode` writes `openscad.search_paths` (for
  openscad-LSP), relative to `${workspaceFolder}` so `.vscode/settings.json` is committable;
  and `scadman run -- model.scad` opens the OpenSCAD GUI with dependencies resolved.

### Changed

- **`scadman add <url>` with no ref now tracks the remote's default branch** (like
  `git clone`) — pass `--tag`/`--branch`/`--rev` to pin otherwise.
- **The include-scan is reachability-based** — it follows the project's imports and warns
  only about undeclared libraries the files you actually use reach, instead of every file a
  dependency ships (far fewer false positives on real libraries).
- Content hashes are documented as reproducible across Linux and macOS (path-separator- and
  file-mode-independent).

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

[0.1.0-alpha.2]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.2
[0.1.0-alpha]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha
