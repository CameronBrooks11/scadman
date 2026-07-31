# Fixtures

Real OpenSCAD libraries, pinned to exact revisions, chosen to represent the ecosystem
archetypes scadman must handle. They anchor design discussions and integration tests
(resolver, environment construction, layout detection).

These are **references, not vendored code** — [archetypes.toml](archetypes.toml) pins
each to a commit SHA so tests can fetch it reproducibly without bloating this repo or
mixing licenses. Measured properties come from the pass-1 ecosystem survey (see
[../docs/ecosystem-survey.md](../docs/ecosystem-survey.md)).

Each fixture earns its place by exercising a distinct behavior:

| fixture | archetype | why it's here |
|---------|-----------|---------------|
| BOSL2 | framework, self-rooted | large flat namespace (433 modules / 1369 functions); includes itself via `BOSL2/…` |
| NopSCADlib | framework + catalog, self-rooted, heavily released | 438 tags; component catalog; self-rooted includes |
| MCAD | conventional library, unlicensed, self-rooted | ships bundled with OpenSCAD; no license; only 2 tags |
| dotSCAD | src-layout, `OPENSCADPATH`-dependent | 695 files under `src/`; relies on bare (path-configured) imports |
| Round-Anything | focused utility, externally-dependent | small; nearly all edges reference an external library |
