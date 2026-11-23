# Changelog

All notable changes to scadman are recorded here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[SemVer](https://semver.org/) (pre-1.0: expect breaking changes between minor versions).

## [Unreleased]

### Fixed

- **`scadman run` exits with OpenSCAD's exit code** (#151). It exited 1 for any failure
  of OpenSCAD and printed an error of its own. It now passes the code through (128 plus
  the signal number if a signal ended OpenSCAD) and stays quiet. If scadman fails before
  OpenSCAD has started, it exits 125.
- **scadman's git commands no longer act on the repository git names in its
  environment** (#148). With `GIT_DIR` set, as git sets it for some of the commands it
  runs, `lock` and `sync` fetched and checked out a dependency inside that repository:
  its HEAD was moved, an `origin` remote added and its index replaced. `add` likewise
  read that repository's config when looking up a default branch.
- **`add` and `remove` keep your comments and layout in `scadman.toml`** (#150). They
  rewrote the whole file from its parsed contents, which deleted every comment. They now
  change only the entry asked about. A new entry is written as one line in
  `[dependencies]`, the form the documentation shows, unless the file already gives each
  dependency its own section.

## [0.1.0-alpha.9] — 2026-11-19

Ninth alpha. A dependency's git submodules can be fetched, and one that is left out is
named instead of arriving as a silently empty folder.

### Added

- **`submodules = true` on a git dependency fetches its git submodules** (#121), also
  as `scadman add --submodules`. Each submodule is fetched at the commit the dependency
  records for it, nested ones included, so the locked `rev` still fixes the whole tree.
  One that cannot be fetched fails `lock`. A request from any manifest in the graph
  applies to that library for the whole project.
- **Lockfile version 2**, written only when a library is locked with its submodules.
  Other lockfiles are still written as version 1, which earlier releases read. A
  version 2 lockfile is refused by 0.1.0-alpha.8 and earlier with a message to upgrade.

### Fixed

- **A branch or tag dependency whose stored copy is missing stays at its locked
  commit.** In a project with a path dependency, `sync` resolves again while holding
  git dependencies at their locked commits. A held dependency with no copy in the store
  (a new machine, a cleared store) was fetched by its branch or tag name instead, so it
  moved to the current tip and the lockfile was rewritten. It is now fetched at the
  locked commit, and refused if the content does not match the locked hash.
- **A dependency's unfetched git submodules are named** (#121). Submodules are not
  fetched unless asked for, so a submodule arrived as an empty folder with nothing
  said, and a library that reads from it failed inside OpenSCAD. `lock`, `update` and
  `sync` now warn with the folder, the submodule's URL and how to fetch it. A submodule
  whose repository is also a dependency is not warned about.

## [0.1.0-alpha.8] — 2026-11-15

Eighth alpha. Stored libraries are read-only, and a copy that was changed is found and
replaced. Check each project once with `scadman sync --verify` after upgrading, since
earlier versions let a write through `.scadman/env/` reach the store unnoticed.

### Fixed

- **A library's stored copy can no longer be changed through a project** (#140). Every
  project that locks the same content shares one copy in the store, and
  `.scadman/env/` leads straight to it, so a write there changed the library for all of
  them: an edit, or Python leaving `__pycache__` beside a library's scripts when they
  ran from its folder. `sync` and `doctor` reported nothing, and a fresh `lock` of the
  same commit in another project was handed the modified copy under the original hash.
  - Stored copies are now read-only: their files on every OS, and on Linux and macOS
    their directories too, so nothing can be added. A write fails where it is made;
    Python skips its bytecode and carries on.
  - A copy found writable is re-hashed before `sync`, `run` and `env` use it, and one
    that no longer matches is fetched again and replaced, with a warning. A store filled
    by an earlier scadman is checked this way on its first sync. `lock` and `update`
    replace a modified copy the same way instead of reusing it. The check comes before
    anything reads a copy's own `scadman.toml`.
  - `scadman sync --verify` re-hashes every copy, protected or not, and `doctor` has a
    `content:` line naming any that no longer matches. Use `--verify` where scadman
    runs as root, which file permissions do not bind.

### Changed

- **Deleting the store takes one more step.** Its entries are read-only, so on Linux and
  macOS `rm -rf` alone fails: run `chmod -R u+w` on the store first. On Windows use
  `Remove-Item -Recurse -Force`. `docs/manifest.md` has both commands.
- **`scadman-core`:** `StoreEntry` has a `restored` field, and `Store` gains `verify`,
  `protect` and `is_protected`.

## [0.1.0-alpha.7] — 2026-11-15

Seventh alpha. A setup action, so a project's CI installs scadman with one line.

### Added

- **A setup action for GitHub Actions** (#123): `CameronBrooks11/scadman/setup@<tag>`
  installs that release on Linux, macOS or Windows runners, checked against its
  `checksums.txt`, and with `sync: "true"` syncs the project and exports `OPENSCADPATH`.
  It reads its version from its own path, so it works nested inside another composite
  action, and a `version` input overrides it. CI runs it on all three OS.

## [0.1.0-alpha.6] — 2026-11-15

Sixth alpha. Two Windows fixes found on a real Windows 11 machine with OpenSCAD 2021.01:
the environment is now real files there, and scadman finds OpenSCAD where its installer
puts it.

### Added

- **scadman finds OpenSCAD where its Windows installer puts it** (#133). The installer
  doesn't add `C:\Program Files\OpenSCAD\` to `PATH`, so `doctor` said "not found" and
  `run` failed on a default install. scadman now tries `$OPENSCAD` (set it to the
  program's path, on any OS), then `PATH`, then on Windows `%ProgramFiles%\OpenSCAD*`.
  `doctor` names the binary when it isn't the one on `PATH`.

### Fixed

- **OpenSCAD 2021.01 crashed on Windows rendering through the environment** (#132).
  It cannot read a junction or a symlink, so once a library included its own files,
  `scadman run` failed with `read_symlink: The request is not supported`. On Windows
  each library in `.scadman/env/` is now a directory tree of hard links to the stored
  files, or copies where a hard link can't be made (another drive, or a file system
  without them). Editing a file there changes the stored copy, as with the symlinks on
  Linux and macOS. CI now renders a library with nested includes through `scadman run`
  on Windows with OpenSCAD 2021.01.

## [0.1.0-alpha.5] — 2026-11-15

Fifth alpha. scadman runs on Windows and macOS as well as Linux, with a prebuilt binary
for each, and a lockfile written on any of them verifies on the others.

### Added

- **macOS binaries** for Apple Silicon (`aarch64-apple-darwin`) and Intel
  (`x86_64-apple-darwin`) in every release, beside the Linux one, all listed in the one
  `checksums.txt` (#54). The install docs cover them. CI runs the tests on macOS too.
- **Windows support** (#53). The environment links each library with a directory
  junction, which needs no admin rights or Developer Mode; the store defaults to
  `%LOCALAPPDATA%\scadman\store` unless `XDG_DATA_HOME` is set. CI runs the tests on
  Windows, where the hash goldens give the same values as on Linux and macOS.
- **A Windows binary** in every release, `scadman-<version>-x86_64-pc-windows-msvc.zip`,
  listed in `checksums.txt`, with PowerShell install steps in the docs (#126). Before
  publishing, the release workflow installs each binary as the docs say on Linux,
  macOS and Windows, and locks tessella v0.1.0 with it; the hash has to match on all
  three.

### Changed

- **`env --write-vscode` writes `/` after `${workspaceFolder}` on every OS**, so settings
  written on Windows work for a collaborator on Linux or macOS.

### Fixed

- **A user's line-ending settings no longer change what is stored** (#125). scadman's
  git commands run with `core.autocrlf=false` and `core.eol=lf`, so a library is stored
  as its commit's bytes. Before, `core.autocrlf=true` (Git for Windows' default) or a
  CRLF `core.eol` under a library's `text=auto` attribute checked files out with CRLF,
  and the content hash no longer matched a lockfile written elsewhere. Paths are now
  hashed with `/` on every platform; every existing hash is unchanged.

## [0.1.0-alpha.4] — 2026-11-10

Fourth alpha. A documentation site, and a run of fixes to how the manifest and the
lockfile are read, most of them cases where scadman did the wrong thing without saying
so. Some can stop a project that used to pass: a `scadman.toml` with unknown keys, a git
dependency naming two refs, an abbreviated `rev`, and `sync` straight after `remove`.
Each now fails with a message that says what to change.

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
- **A git dependency's `root` is checked at lock time.** An absolute or `..` root on a
  git dependency locked cleanly, was written into `scadman.lock`, and failed only at
  `sync`; only path dependencies and `add --root` were checked earlier. A root that
  did not exist in the library (a typo) also locked, and `sync` then said the package
  was "not in the store — run a sync". `lock` now refuses both, for any dependency
  from any manifest, and says which root is wrong. A bad root is also no longer
  reported as an invalid dependency name.
- **An abbreviated `rev` is refused with a message that says so.** git fetches a
  commit only by its full name, so `rev = "061fef7"` could never lock: it failed with
  git's `couldn't find remote ref`. `add --rev` accepted it anyway, and `sync`
  prefix-matched it against the lockfile as if it were supported. `add` and `lock` now
  ask for the full 40-digit hash, or a tag or branch, and a `rev` is compared with the
  lockfile whole.
- **The include-scan knows what an `on_path` library provides.** When a dependency
  imported a path such as `<util/x.scad>` that an `on_path` library ships at its root,
  OpenSCAD resolved it and `sync` still warned that the dependency "imports `util/`
  but it is not in your dependencies", naming a library that does not exist. Such an
  import is now followed instead of reported, so scadman's warnings match what
  OpenSCAD can and cannot open.
- **Both crates declare the minimum Rust version.** `rust-version = "1.88"` was set
  for the workspace but neither crate inherited it. An older toolchain was already
  turned away, but only because a dependency (`sha2`) requires 1.88 too; scadman's own
  requirement was not stated where cargo reads it. cargo now names `scadman-cli` and
  `scadman-core` in that message.
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

[0.1.0-alpha.9]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.9
[0.1.0-alpha.8]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.8
[0.1.0-alpha.7]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.7
[0.1.0-alpha.6]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.6
[0.1.0-alpha.5]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.5
[0.1.0-alpha.4]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.4
[0.1.0-alpha.3]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha.2
[0.1.0-alpha]: https://github.com/CameronBrooks11/scadman/releases/tag/v0.1.0-alpha
