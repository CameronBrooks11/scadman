# Contributing

Bug reports, feedback and patches are welcome. For anything larger than a small fix,
open an [issue](https://github.com/CameronBrooks11/scadman/issues) first so the approach
can be agreed before the work is done.

## Setup

You need a Rust toolchain (1.88 or newer, via [rustup](https://rustup.rs)),
[`just`](https://github.com/casey/just), [`pre-commit`](https://pre-commit.com) and
`git`. `openscad` is needed only for the real-library tests below.

```sh
just setup    # rustfmt, clippy, and the pre-commit git hooks
```

## Build, check, test

```sh
just fmt      # format
just check    # fmt-check + clippy with warnings as errors
just test     # unit and end-to-end tests
just run doctor   # run the CLI from source; arguments pass through
```

`just check && just test` must pass before a commit. The tests build throwaway git
repositories under a temporary directory and use a temporary store, so they need `git`
but no network and do not touch your own scadman store.

Two tests fetch real libraries from GitHub and render them with OpenSCAD. They are
ignored by default:

```sh
cargo test -- --ignored
```

## Layout

```
crates/scadman-core/   data model and logic: manifest, lockfile, resolver, store, environment, include-scan
crates/scadman-cli/    the `scadman` binary and its end-to-end tests
docs/                  user guides and design records (see docs/README.md)
fixtures/              real libraries pinned for tests, by reference
```

Logic belongs in `scadman-core`; the CLI stays a thin shell over it.

[AGENTS.md](AGENTS.md) holds the short form of the conventions and the design
constraints. It is written for AI coding agents and applies equally to people. The
reasoning behind the constraints is in [docs/DECISIONS.md](docs/DECISIONS.md).

## Changes

- Branch from `main` and open a pull request against it.
- [Conventional Commits](https://www.conventionalcommits.org): `type(scope): subject`,
  imperative and lowercase. One logical change per commit.
- A behaviour change comes with a test. A bug fix comes with a test that fails without
  the fix.
- Add a line under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for anything a
  user would notice.
- Update the docs in the same change when behaviour they describe moves:
  [README.md](README.md) and [docs/](docs/).
- Never bypass the pre-commit hooks.

CI runs on every pull request: formatting, a type-check, a check that `Cargo.lock`
matches `Cargo.toml`, the test suite, and a type-check on the minimum supported Rust
version. Clippy also runs, as an advisory job that does not block.

## Releasing

1. Set the new version in the root `Cargo.toml` and refresh `Cargo.lock`
   (`cargo check`).
2. Rename `## [Unreleased]` in `CHANGELOG.md` to the version and date, add a fresh
   empty `## [Unreleased]` above it, and add the link at the bottom of the file.
3. Update the version in the README's `cargo install` line.
4. Merge, then tag the merge commit `v<version>` and push the tag.

The tag triggers the release workflow, which builds the Linux binary from the committed
`Cargo.lock`, writes `checksums.txt`, and publishes a GitHub release whose notes are
that version's CHANGELOG section. It fails if the section is missing.
