# Path dependencies

To co-develop a project alongside a local library, depend on it by path instead of a git
source:

```sh
scadman add mylib --path ../mylib
```

or in `scadman.toml`:

```toml
[dependencies]
mylib = { path = "../mylib" }
```

## What tracking means

A path dependency tracks the directory's *current* content. `sync`, `run`, and `env`
re-read it — rewriting the lock if it changed — so edits to the sibling's code show up
immediately.

Git dependencies beside it stay pinned to their locked commits and are served from the
store, so those commands still work offline. If the sibling declares a new dependency of
its own, the next `sync` resolves it automatically (that first fetch needs the network).
Changing a git dependency's pin in your own manifest still needs an explicit
`scadman lock`; `sync` says so.

## Caveats

- The whole directory is copied into the store, minus `.git` and symlinks — keep build
  output and nested environments out of the library root.
- Only the root manifest may declare a path dependency; a dependency of your project
  cannot bring one in.
- A path dependency is a local-development convenience, not a reproducible pin: a
  lockfile that references one is not portable to another machine.
- `--root` and `--on-path` work as with a git source, for a src-layout sibling — see
  [library-roots.md](library-roots.md).
