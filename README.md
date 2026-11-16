# scadman

[![CI](https://github.com/CameronBrooks11/scadman/actions/workflows/ci.yml/badge.svg)](https://github.com/CameronBrooks11/scadman/actions/workflows/ci.yml)
[![Docs](https://github.com/CameronBrooks11/scadman/actions/workflows/docs.yml/badge.svg)](https://cameronbrooks11.github.io/scadman/)
[![Release](https://img.shields.io/github/v/release/CameronBrooks11/scadman?include_prereleases)](https://github.com/CameronBrooks11/scadman/releases)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A project-oriented package manager for OpenSCAD libraries and scripts.

Instead of copying libraries into OpenSCAD's global `libraries/` folder and hoping every
project still works when one of them changes, scadman gives each project a manifest
(`scadman.toml`) and a lockfile (`scadman.lock`) that pin exact dependency versions. Most
OpenSCAD libraries are git repositories that never publish releases, so scadman is
GitHub-first: add a library by URL, track its branch, and pin it to an exact commit at
lock time.

**Documentation: <https://cameronbrooks11.github.io/scadman/>**

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

On another platform, build from source with a Rust toolchain (≥ 1.88):

```sh
cargo install --locked --git https://github.com/CameronBrooks11/scadman --tag v0.1.0-alpha.8 scadman-cli
```

Either way, `git` needs to be on `PATH`. scadman finds OpenSCAD through `$OPENSCAD` if you
set it to the program's path, then `PATH`, and on Windows the installer's folder under
`%ProgramFiles%`, which it does not add to `PATH`.

### In GitHub Actions

```yaml
- uses: CameronBrooks11/scadman/setup@v0.1.0-alpha.8
  with:
    sync: "true"
```

The action installs the release it is used at, checked against that release's
`checksums.txt`, on Linux, macOS or Windows runners, and puts `scadman` on `PATH`. With
`sync: "true"` it also syncs the project (`working-directory`, default `.`) and exports
`OPENSCADPATH` for the steps after it. Pass `version:` to install another release.

## Use

```sh
scadman init                                            # create scadman.toml
scadman add BOSL2 https://github.com/BelfrySCAD/BOSL2   # track its default branch
scadman lock                                            # pin it to an exact commit
scadman run -- model.scad -o out.stl                    # render with BOSL2 available
```

The rest is in the documentation:

- [Getting started](https://cameronbrooks11.github.io/scadman/getting-started.html) — a first project and every command
- [Manifest and lockfile](https://cameronbrooks11.github.io/scadman/manifest.html) — the file formats and where scadman keeps things
- [Path dependencies](https://cameronbrooks11.github.io/scadman/path-dependencies.html) and [library roots](https://cameronbrooks11.github.io/scadman/library-roots.html)
- [Editor and GUI integration](https://cameronbrooks11.github.io/scadman/editor-integration.html)

The same pages are in [docs/](docs/).

## Status

An alpha: usable end-to-end and validated against real libraries, with rough edges and
breaking changes to come. Git and local path sources only; Linux, macOS and Windows. The
[introduction](https://cameronbrooks11.github.io/scadman/) lists the current limits, and
planned work is in the
[post-alpha roadmap](https://github.com/CameronBrooks11/scadman/issues/44).

## Contributing

Bug reports and feedback are welcome on the
[issue tracker](https://github.com/CameronBrooks11/scadman/issues). To build, test and
send a change, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache-2.0.

> I'm a scad mannnnnnnnnnnnnnn ski-ba-bop-ba-dop-bop
