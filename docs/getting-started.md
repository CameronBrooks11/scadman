# Getting started

## Install

Grab the archive for your platform and `checksums.txt` from the
[latest release](https://github.com/CameronBrooks11/scadman/releases/latest):

| Platform | Archive |
| --- | --- |
| Linux x86_64 | `scadman-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| macOS, Apple Silicon | `scadman-<version>-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `scadman-<version>-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `scadman-<version>-x86_64-pc-windows-msvc.zip` |

In the directory you downloaded both into, on Linux:

```sh
sha256sum --ignore-missing -c checksums.txt
tar -xzf scadman-*-x86_64-unknown-linux-gnu.tar.gz
install -Dm755 scadman ~/.local/bin/scadman   # or anywhere on your PATH
```

On macOS (use `x86_64-apple-darwin` on an Intel Mac):

```sh
shasum -a 256 --ignore-missing -c checksums.txt
tar -xzf scadman-*-aarch64-apple-darwin.tar.gz
xattr -d com.apple.quarantine scadman 2>/dev/null || true   # the binary is not notarized
mkdir -p ~/.local/bin && install -m755 scadman ~/.local/bin/scadman   # or anywhere on your PATH
```

A browser marks what it downloads as quarantined, and macOS refuses to run an
unnotarized binary that carries the mark; `xattr` removes it. A download with `curl`
is not marked.

On Windows, in PowerShell:

```powershell
$zip = Get-Item scadman-*-x86_64-pc-windows-msvc.zip
$want = ((Select-String -Path checksums.txt -Pattern $zip.Name -SimpleMatch).Line -split '\s+')[0]
$have = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
if ($have -ne $want) { throw "checksum mismatch for $($zip.Name)" }
Unblock-File $zip
$dir = "$env:LOCALAPPDATA\Programs\scadman"
Expand-Archive $zip -DestinationPath $dir -Force
[Environment]::SetEnvironmentVariable("Path", [Environment]::GetEnvironmentVariable("Path", "User") + ";$dir", "User")
```

That checks the zip against `checksums.txt`, clears the mark Windows puts on a
downloaded file (the binary is not signed), and adds its folder to your user `PATH`;
open a new terminal to pick it up. scadman also needs
[Git for Windows](https://git-scm.com/download/win).

On another platform, build from source with a Rust toolchain (≥ 1.89):

```sh
cargo install --locked --git https://github.com/CameronBrooks11/scadman --tag v0.1.0-alpha.11 scadman-cli
```

Either way, `git` needs to be on `PATH`. scadman finds OpenSCAD through `$OPENSCAD` if you
set it to the program's path, then `PATH`, and on Windows the installer's folder under
`%ProgramFiles%`, which it does not add to `PATH`. `scadman doctor` reports what it
finds.

### In GitHub Actions

```yaml
- uses: CameronBrooks11/scadman/setup@v0.1.0-alpha.11
  with:
    sync: "true"
```

The action installs the release it is used at, checked against that release's
`checksums.txt`, on Linux, macOS or Windows runners, and puts `scadman` on `PATH`. With
`sync: "true"` it also syncs the project (`working-directory`, default `.`) and exports
`OPENSCADPATH` for the steps after it. Pass `version:` to install another release.
Add `locked: "true"` and the step fails when `scadman.lock` is missing or out of date,
in place of writing one; it needs 0.1.0-alpha.11 or newer.

GitHub lists a public project among the scadman repository's
[dependents](https://github.com/CameronBrooks11/scadman/network/dependents) when a file
in its `.github/workflows/` uses the action and the dependency graph is on in the
project's security settings. A project that installs scadman another way is not listed.

## A first project

In your project directory:

```sh
scadman init                                            # create scadman.toml
scadman add BOSL2 https://github.com/BelfrySCAD/BOSL2   # track its default branch
scadman lock                                            # pin it to an exact commit
scadman run -- model.scad -o out.stl                    # render with BOSL2 available
```

`model.scad` imports the library under the name you gave it:

```openscad
include <BOSL2/std.scad>
```

Commit `scadman.toml` and `scadman.lock`. The `.scadman/` directory is machine-local;
`init` adds it to `.gitignore`. Run scadman from the project root, where `scadman.toml`
lives.

## Commands

| Command | What it does |
|---|---|
| `scadman init [--name N]` | Create `scadman.toml` and ignore `.scadman/`. |
| `scadman add <name> <git-url>` | Declare a git dependency. `--tag`, `--branch` or `--rev` picks the ref; with none, it tracks the remote's default branch. A tag or branch is checked against the repository, and refused if it is not there. `--submodules` also fetches its git submodules. |
| `scadman add <name> --path <dir>` | Declare a local directory as a dependency. |
| `scadman remove <name>` | Drop a dependency from `scadman.toml`. |
| `scadman list` | Show declared dependencies and the commit each is locked to. |
| `scadman lock` | Bring `scadman.lock` into line with the manifest: resolve what was added or changed, drop what was removed, and keep every other commit where it is locked. |
| `scadman update [name…]` | Move branch and tag dependencies to their newest commit, all or just the named ones, and report what moved. |
| `scadman sync [--verify] [--locked]` | Build the project environment from the lockfile and warn about undeclared imports and unfetched submodules. `--verify` re-hashes every stored library first and restores any that was modified. |
| `scadman run [--locked] -- <openscad args>` | Sync, then run OpenSCAD with the environment on `OPENSCADPATH`. |
| `scadman env [--json \| --write-vscode] [--locked]` | Sync, then print the `OPENSCADPATH` for editors and tools. |
| `scadman graph [--json]` | Print the locked dependency graph. |
| `scadman doctor` | Report on OpenSCAD, the store, the manifest, the lockfile, whether the stored libraries are present and unmodified, and the environment. Exits 1 when the project needs fixing. |

`scadman run` exits with the code OpenSCAD exited with, so a script can treat it as
`openscad` itself. If scadman fails before OpenSCAD has started (a stale lockfile, a
dependency it cannot fetch, no OpenSCAD found) it prints the error and exits 125. Every
other command exits 0 on success and 1 on failure; a mistake in the command line itself
exits 2.

`sync`, `run` and `env` write `scadman.lock` when it is missing, and rewrite it when a
path dependency's directory has changed. With `--locked` they use the lockfile exactly as
it is, and fail without writing anything if it is missing or would change. That is the
form for CI, where the build should use what was committed.

`scadman doctor` always prints its whole report. It exits 1 when something in the
project needs fixing: `scadman.toml` is missing or cannot be read, the lockfile is
missing while dependencies are declared, unreadable or out of date, or a locked library's
copy in the store is missing or modified. OpenSCAD not being found or being older than
the project asks for, an environment that is not built yet, and a dependency that tracks
a branch or tag are reported without changing the exit code.

A branch or tag is pinned to an exact commit at lock time and stays there until you run
`scadman update`. A `--rev` pin never moves; give it the full 40-character commit hash,
since an abbreviated one is refused.

After editing `scadman.toml`, by hand or with `add` and `remove`, run `scadman lock`.
`sync` and `run` stop with an "out of date" error for some stale lockfiles, but they do
not catch every edit. The details are in [Manifest and lockfile](manifest.md).

## Local libraries and unusual layouts

- To develop a project alongside a library on your own disk, depend on it by path:
  `scadman add mylib --path ../mylib`. See [Path dependencies](path-dependencies.md).
- Some libraries, dotSCAD for example, keep their code under a subdirectory like `src/`
  and import relative to it. Add them with `--root src --on-path`. See
  [Library roots](library-roots.md).

## MCAD

OpenSCAD 2021.01 ships MCAD, so `use <MCAD/…>` works there with nothing declared. Builds
of OpenSCAD made after 2026-11-19 no longer include it. Declaring it gives the project
the same copy with either:

```sh
scadman add MCAD https://github.com/openscad/MCAD --tag 2026.11
scadman lock
```

## Undeclared imports

Many libraries pull in others without declaring it; `agentscad`, for example, uses
`scad-utils` and `list-comprehension-demos`. `sync` follows your project's imports into
each dependency and names any undeclared library it reaches:

```
warning: `agentscad` imports `scad-utils/` (in mesh.scad) but it is not in your dependencies
```

`scadman add` each named library, then `lock` and `sync` again. Imports of a bare
filename (`use <file.scad>`) can't be attributed to a library and aren't flagged.

## Git submodules

A dependency's git submodules are not fetched unless you ask, so by default a submodule
arrives as an empty folder. `lock`, `update` and `sync` name each one:

```
warning: `SBC_Case_Builder` has a git submodule at `SBC_Model_Framework/` (https://github.com/hominoids/SBC_Model_Framework.git) that was not fetched, so that folder is empty — to fetch it, set `submodules = true` on `SBC_Case_Builder` in scadman.toml
```

If the library reads from that folder, add `submodules = true` to its entry in
`scadman.toml` and run `scadman lock`:

```toml
SBC_Case_Builder = { git = "https://github.com/hominoids/SBC_Case_Builder", rev = "5979914bc23aa0d85da592be4c37b6ed97c20ddf", submodules = true }
```

For a library you have not added yet, `scadman add <name> <git-url> --submodules` writes
the key. Every submodule is then fetched at the commit the library records for it, nested ones
included. See [`submodules`](manifest.md#submodules) for what that changes in the
lockfile and when it fails.

A submodule the library's `.scad` files never read (a wiki, PCB footprints) can be left
unfetched. The warning also stops when the submodule's repository is a dependency of
its own, which is the other way to supply it: `scadman add` it under the folder's name,
pinned to the commit the library expects, and the library's imports of it can resolve
through `OPENSCADPATH`.

## Minimum OpenSCAD version

Declare `openscad = "2021.01"` under `[project]` (a `>=` prefix is accepted; ranges and
other operators aren't) and `doctor` and `run` warn when the installed OpenSCAD is
older. Advisory only, not a hard gate.
