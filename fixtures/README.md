# Fixtures

Real OpenSCAD libraries, pinned to exact revisions, chosen to represent the ecosystem
archetypes scadman must handle. They anchor design discussions and the real-library
tests.

These are **references, not vendored code** — [archetypes.toml](archetypes.toml) pins
each to a commit SHA, so nothing is copied into this repo and no licenses are mixed.
Measured properties come from the pass-1 ecosystem survey (see
[../docs/ecosystem-survey.md](../docs/ecosystem-survey.md)).

Each fixture earns its place by exercising a distinct behavior:

| fixture | archetype | why it's here |
|---------|-----------|---------------|
| BOSL2 | framework, self-rooted | large flat namespace (433 modules / 1369 functions); includes itself via `BOSL2/…` |
| NopSCADlib | framework + catalog, self-rooted, heavily released | 438 tags; component catalog; self-rooted includes |
| MCAD | conventional library, unlicensed, self-rooted | ships bundled with OpenSCAD; no license; only 2 tags |
| dotSCAD | src-layout, `OPENSCADPATH`-dependent | 695 files under `src/`; relies on bare (path-configured) imports |
| Round-Anything | focused utility, externally-dependent | small; nearly all edges reference an external library |

## Use in tests

Two end-to-end tests fetch a fixture at its pinned revision and render a model with it:
`bosl2_renders_end_to_end` and `dotscad_renders_with_root_and_on_path` in
`crates/scadman-cli/tests/e2e.rs`. They need network access and `openscad`, so they are
`#[ignore]`d; run them with `cargo test -- --ignored`.

The tests carry their own copy of the two revisions rather than reading
`archetypes.toml` ([#59](https://github.com/CameronBrooks11/scadman/issues/59)). When a
pin changes here, change it there too.
