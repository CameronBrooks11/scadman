# Roadmap

scadman reached an end-to-end v1 (`init → add → lock → sync → run`). This roadmap comes
from a codebase audit and an OpenSCAD-integration research pass; it prioritizes the work
to reach a *dependable* v1 and then extend outward. Sizes are rough (XS/S/M).

## Track A — dependability shore-ups (do first)

The audit found the architecture sound and the distance to dependable "small and concrete":
input validation at two trust boundaries, one real bug, and the integration test that
would have caught it. Do these before new surface area.

- **[A1] Validate package names as safe path segments** — [#15] · S · **security**.
  A transitive `scadman.toml` could key `../../foo` → an env symlink escapes `.scadman/env/`.
- **[A2] Detect stale/out-of-date lockfile in `sync`/`run`** — [#16] · S–M · **real bug**.
  `add` then `run` silently ignores the new dependency today.
- **[A3] Validate lockfile `version` on read** — [#17] · XS. Reject unknown/newer formats.
- **[A4] End-to-end integration test** (`lock → sync`, integrity-mismatch, stale-lock) — [#18] · M.
  The CLI seam currently has zero coverage.

## Track B — CLI rounding-out (quick, high user value)

- **[B1]** `.scadman/` gitignore scaffolding in `init` — XS. Users otherwise commit
  machine-local symlinks.
- **[B2]** `scadman remove` — S · **[B3]** `scadman list` — S. Table-stakes for a manager.

## Track C — OpenSCAD / editor integration

Research reordered this: the three JSON outputs the direction doc bundled have very
different value.

- **[C1] `scadman env` (+ `--json`)** — S · **highest-leverage next feature.** It's the
  *only* output today's ecosystem consumes for free: point openscad-LSP (`OPENSCADPATH`
  or `scad-lsp.searchPaths`) and the OpenSCAD binary/preview extension at scadman's env
  dir → goto-def / completion / preview resolve against *pinned* versions, zero code on
  either side. It's a thin serialization of the `Environment` + `Lockfile` structs.
- **[C2]** editor-wiring recipe (docs; optional `--write-vscode`) — XS docs / S writer.
- **Fold in** `graph --json` with C1 if wanted (near-zero cost; internal/CI value only).
- **Defer** `symbols --json` — no consumer exists and it needs a net-new definition scanner.
- **Blocked upstream** — GUI/native integration. OpenSCAD's library-path-injection patch
  is unmerged and stalled; `OPENSCADPATH` is the *only* hook, so scadman owns injection
  indefinitely. Do not scope anything that needs OpenSCAD to meet us halfway.

## Product decisions surfaced

- **`OPENSCADPATH` does not isolate.** It *adds* to the search path; OpenSCAD always also
  searches its built-in user/install dirs. The honest guarantee is "declared deps
  **shadow** globals" (the include-scan is the mitigation). The overclaiming comment in
  `scadman-cli/src/main.rs` is corrected as part of Track A.
- **Dead `project.openscad` field** — parsed but never used; it silently promises version
  checking scadman doesn't do. Decide: wire a minimal OpenSCAD-version check, or drop the
  field from the schema. Deferred, tracked here.

## Recommended sequence

**A → then (B + C1) together.** Harden the trust boundaries and kill the stale-lock bug
first (correctness/security), then land the quick CLI wins alongside `scadman env` — the
single highest-leverage feature, which unlocks editor integration essentially for free.
`symbols --json`, GUI, and native-core integration stay deferred.
