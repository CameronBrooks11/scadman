# Changelog

All notable changes to scadman are recorded here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[SemVer](https://semver.org/) (pre-1.0: expect breaking changes between minor versions).

## [Unreleased]

### Added

- **A documentation site** at <https://cameronbrooks11.github.io/scadman/>, built from
  `docs/`. The first-project walkthrough and the command table moved there from the
  README, which keeps install and a four-command summary; editor and GUI integration
  has its own page.
- **A manifest and lockfile reference** (`docs/manifest.md`): the `scadman.toml` keys,
  what `scadman.lock` records, which manifest edits `sync` detects, and where scadman
  keeps its files.
- **`CONTRIBUTING.md`**: setup, the check and test commands, pull-request conventions,
  and the release steps.

### Fixed

- **An unknown table or key in `scadman.toml` is now an error.** A misspelled
  `[dependencies]` header used to read as a project with no dependencies: `list`,
  `lock` and `doctor` all reported success. The same went for a misspelled key under
  `[project]`. The error names the table or key and its line. A dependency's own
  `scadman.toml` still tolerates unknown keys outside its dependency tables, so a
  library written for a newer scadman stays readable. **Breaking** for a manifest that
  carried extra keys: remove them.
- **A git dependency naming more than one of `rev`, `tag` and `branch` is now refused.**
  `lock` used to accept it and silently use `rev` over `tag`, and `tag` over `branch`, so
  adding `tag = "v2"` beside an old `rev` line locked the old commit without a word.
  `add` already rejected the combination; a hand-edited manifest, and a dependency's
  own, are now held to the same rule.
- **A tag or branch whose name looks like a commit hash is held at its locked commit.**
  In a project with a path dependency, a tag such as `20210101` (seven or more hex
  digits) was taken for a commit, so every `sync` re-fetched it: the lock moved on its
  own when the tag moved upstream, and `sync` failed without the network. Whether a
  reference is a commit now comes from the manifest key it was declared under, not
  from its spelling.
- **Every command now refuses a lockfile of an unknown version.** Only `sync`, `run` and
  `env` checked it. `graph`, `list` and `doctor` read a newer lockfile without comment,
  and `update <name>` used it as its base and wrote it back in the old format. `lock`
  and `update` no longer overwrite one either; the error says to upgrade scadman or
  delete the file. The version is read on its own, so a newer lockfile that does not
  parse is still reported by its version.
- **A removed dependency no longer stays available.** After `scadman remove`, the
  library stayed in the lockfile: `sync` and `run` went on exposing it, the project's
  own import of it kept rendering, and `doctor` called the lockfile up to date. `sync`,
  `run` and `env` now stop with the same "out of date … run `scadman lock`" error an
  added-but-unlocked dependency gets, and `doctor` reports it. Libraries that are in
  the lockfile because another one depends on them are unaffected.
- **A lockfile `hash` that is not a content hash is refused.** The hash is joined onto
  the store root, so `hash = "/some/dir"` or `hash = "../../dir"` made `sync` link the
  package to that directory and put it on `OPENSCADPATH`. A hash must now be 64
  lowercase hex digits, checked wherever the lockfile is read and again where the
  environment is built, before the existing environment is touched.
- **A failed copy into the store cleans up after itself.** When an insert failed
  partway (a full disk, for one), its `.staging-*` directory stayed in the store for
  good: the name is unique to the run, so nothing later reused or removed it.
- **`scadman add <url>` now says the URL is in the name position.** Passing only a
  git URL used to fail with the package-name character error, which read as if the
  URL were malformed. It now prints `` `<url>` looks like a git URL, not a package
  name — usage: `scadman add <NAME> <GIT>` `` with a corrected command using the
  repository's name, e.g. `scadman add BOSL https://github.com/revarbat/BOSL`.

### Changed

- **Release binaries are built from the committed `Cargo.lock`.** The release
  build now passes `--locked`, so a lockfile that disagrees with `Cargo.toml` fails
  the release rather than being rewritten inside the job. CI checks the same thing
  on every pull request, so a stale lock is caught before it reaches a tag.

## [0.1.0-alpha.3] — 2026-09-02

Third alpha. A `doctor` fix, a raised minimum Rust version, and the first release
built by a pipeline that ships checksums and publishes a full release rather than a
pre-release.

### Fixed

- **`scadman doctor` no longer reports an unparseable manifest as missing.** It
  loaded the manifest in a way that collapsed a parse error into the same result
  as an absent file, so a typo in `scadman.toml` printed
  `` scadman.toml not found — run `scadman init` `` — advice that then fails with
  `scadman.toml already exists`. Doctor now separates the cases and prints the
  TOML parse error, with the offending line, which is what it exists to do.

### Changed

- **The minimum supported Rust version is now 1.88** (was 1.85). `scadman-core`'s
  resolver uses let-chains, which stabilised in 1.88. This affects building from
  source only — the prebuilt binary is unaffected. CI now checks against the
  version declared in `Cargo.toml` rather than a separately pinned one, so the
  two cannot drift.
- **Polished CLI output.** `lock` and `sync` report `Locked 3 packages` rather
  than `Locked 3 package(s)`; `doctor` describes an absent store as
  `(not created yet)` rather than `(empty)`; and the error for a version
  dependency now gives the reason and the way out — registries aren't supported
  yet, use a git or path source.
- **Release archives ship with a `checksums.txt`** (SHA-256, `sha256sum -c`
  format) so a downloaded binary can be verified before it goes on `PATH`. The
  README's install step does exactly that.
- **Releases are no longer marked pre-release.** GitHub excludes pre-releases
  from `/releases/latest`, so that endpoint returned 404 and scripted installs
  had nothing to resolve against. The version string still carries the maturity
  signal — this is an alpha.

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

[0.1.0-alpha.3]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.2
[0.1.0-alpha]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha
