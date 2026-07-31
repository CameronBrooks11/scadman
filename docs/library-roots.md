# Library roots and `on_path` (src-layout support)

Most OpenSCAD libraries put their code at the repo root and self-root their imports
(`include <BOSL2/std.scad>`), which fits scadman's default model: expose each package as
`env/<name>/` → its store content, with `OPENSCADPATH` pointing at `env`. Validated
end-to-end (BOSL2, NopSCADlib, MCAD, Round-Anything).

**src-layout libraries break that assumption.** dotSCAD keeps its code under `src/`, and
its files import from *that* root (`use <util/…>`, `use <__comm__/…>`, bare
`<polyline_join.scad>`) — which resolve only when `src/` itself is on `OPENSCADPATH`,
regardless of how deep the importing file is. Exposing the repo root under `dotSCAD/`
leaves those internal imports unresolvable (deep modules fail to render).

## Design — two per-dependency knobs

```toml
[dependencies]
BOSL2   = { git = "…", rev = "…" }                               # default
dotSCAD = { git = "…", rev = "…", root = "src", on_path = true }
```

- **`root`** (subdir, default `"."`) — expose `env/<name>` → `store/<hash>/<root>` instead
  of the repo root. Clean and collision-safe; handles libraries whose code is under a
  subdir. Validated as a safe relative path (no `..`/absolute) at the env chokepoint.
- **`on_path`** (bool, default `false`) — also add `env/<name>` to `OPENSCADPATH`, so the
  library's own root-relative imports resolve at any depth. `OPENSCADPATH` becomes
  `env : env/<name> : …` (env first, so `<name/…>` and named libraries resolve; the
  on-path dirs follow, for those libraries' internals).

Validated: with `root = "src", on_path = true`, dotSCAD renders end-to-end via `scadman
run`, including deep `src/_impl/…` modules (an `#[ignore]`d e2e test covers it).

## The tradeoff of `on_path`

`on_path` puts a library's root directly on the global search path, which **globalizes its
top-level filenames** — the flat-namespace risk scadman's named-dir model exists to avoid.
It is therefore **opt-in per dependency** and rare (only OPENSCADPATH-root libraries need
it). In practice the risk is contained: a consumer imports via `<name/…>`, so their own
imports don't clash; the collision is only *between two `on_path` libraries* that share a
top-level filename. scadman warns when that happens.

## Consuming a src-layout library

There are two ways to depend on a src-layout library like dotSCAD, chosen to match how your
own `.scad` files import it:

- **(a) Reach into `src/`.** Add it at the repo root (no knobs) and keep the subdir in the
  import path: `use <dotSCAD/src/shape_trapezium.scad>`. The module you name resolves under
  `env/dotSCAD/src/`, and the siblings it imports resolve relative to that file (OpenSCAD
  searches an included file's own directory). Simplest when you use a few specific files,
  and how existing code that already wrote `dotSCAD/src/…` keeps working. It does not put
  `src/` on the path, so files that import src-root-relative need approach (b).
- **(b) Expose `src/` as the root.** Add it with `root = "src", on_path = true` and drop the
  subdir: `use <dotSCAD/shape_trapezium.scad>`. This matches how the library documents its
  own imports and puts `src/` on `OPENSCADPATH`, so deep internal imports resolve at any
  depth. Prefer it for heavy use, or when the library's docs import as `<dotSCAD/…>`.

Both render correctly — pick the one that matches the import style already in your files.

## Discovery

A consumer must currently know a library needs `root`/`on_path` (documented per-library; a
direct mis-import in your own code shows up as an OpenSCAD render failure — the cue to set
them). The include-scan covers the *transitive* case: it follows the project's imports into
each dependency and names any undeclared library those dependency files reach, so a
manifest-less lib's own hidden dependency surfaces as a precise warning rather than a raw
failure. Because the scan is reachability-based, it reports only libraries the used files
actually reach — never a dependency's unused internals. A future registry/adapter layer can
declare a library's layout on its behalf, so consumers don't have to — but the manifest
knobs are the primitive that layer would populate.
