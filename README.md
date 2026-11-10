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

Grab the prebuilt binary and `checksums.txt` from the
[latest release](https://github.com/CameronBrooks11/scadman/releases/latest) (Linux x86_64
only). In the directory you downloaded both into:

```sh
sha256sum --ignore-missing -c checksums.txt
tar -xzf scadman-*-x86_64-unknown-linux-gnu.tar.gz
install -Dm755 scadman ~/.local/bin/scadman   # or anywhere on your PATH
```

On macOS, or another Linux architecture, build from source with a Rust toolchain
(≥ 1.88):

```sh
cargo install --locked --git https://github.com/CameronBrooks11/scadman --tag v0.1.0-alpha.3 scadman-cli
```

Either way, `git` and `openscad` need to be on `PATH`.

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
breaking changes to come. Git and local path sources only; Linux and macOS. The
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
