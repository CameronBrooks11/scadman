# Editor and GUI integration

`scadman env` prints the project's `OPENSCADPATH`, and `scadman env --json` emits a
machine-readable report of the resolved libraries. Point an OpenSCAD language server or
preview extension at that path and editor features resolve against the project's pinned
dependency versions:

```sh
export OPENSCADPATH="$(scadman env)"
```

`env` syncs first, so the path it prints always exists and matches the lockfile.

## VS Code

`scadman env --write-vscode` writes the path into `.vscode/settings.json` as
`openscad.search_paths` (for [openscad-LSP](https://github.com/Leathong/openscad-LSP)),
keeping the other settings. Paths are written relative to `${workspaceFolder}`, so the
file is safe to commit.

An existing `settings.json` must be plain JSON: one with comments is refused, and you
add the setting by hand.

## The OpenSCAD GUI

`scadman run -- model.scad` opens the model with `OPENSCADPATH` set. Or export the
variable, as above, in the shell you launch OpenSCAD from.

## For tool authors

`scadman env --json` reports the search path and, for each library, its name, source,
exact revision, content hash and the directory its content is stored in.
`scadman graph --json` reports the project name, its direct dependencies and every
locked library with the libraries it declares.
