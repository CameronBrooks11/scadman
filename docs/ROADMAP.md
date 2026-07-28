# Roadmap

scadman is usable end-to-end (`init · add · remove · list · lock · sync · run · env`),
hardened, and CI-green. This roadmap reflects that and prioritizes what makes it
*trustworthy and shareable* next. Sizes are rough (XS/S/M).

## Done

- **v1 core** — manifest, lockfile, content-addressed store, git acquisition,
  collect-and-unify resolver, post-install include-scan, environment builder, and the
  CLI. Design grounded in the ecosystem survey ([ecosystem-survey.md](ecosystem-survey.md),
  [resolver-direction.md](resolver-direction.md)); decisions in [DECISIONS.md](DECISIONS.md).
- **Track A — dependability shore-ups** (#15–#18): package-name validation at every
  filesystem chokepoint (path-traversal, PoC-verified), stale-lock detection, lockfile
  version check, and end-to-end integration tests.
- **Track B — CLI rounding-out**: `remove`, `list`, and a `.scadman/` gitignore in `init`.
- **Track C1 — `scadman env` (+ `--json`)**: the OPENSCADPATH / machine-readable report
  that openscad-LSP and preview extensions consume to resolve against pinned versions.

## Next (near-term)

1. **Validate against the real ecosystem** — [#21] · S–M · **top priority**. scadman has
   only run against synthetic `file://` repos; prove it against the five archetype fixtures
   (BOSL2 self-rooted, dotSCAD bare-import/src-layout, NopSCADlib scale, MCAD bundled,
   Round-Anything) plus a real diamond/conflict. De-risks the whole design; leaves a
   repeatable validation behind.
2. **First alpha release** — [#22] · S. Tag `v0.1.0-alpha`, document install, optionally a
   CI release binary. Ship something *proven* (do after #21).

## Open decisions

- **Dead `project.openscad` field** — [#23]. Wire a minimal OpenSCAD-version check or drop
  it from the schema so it stops promising nothing.

## Deferred / blocked

- **`graph --json`** — near-zero cost to fold into `env`, but internal/CI value only; do
  when a consumer appears.
- **`symbols --json`** — no consumer exists and it needs a net-new definition scanner.
- **Editor-wiring writer** (`--write-vscode`) / a `doctor` command — UX polish.
- **GUI / native-core integration** — upstream-blocked: OpenSCAD's library-path-injection
  patch is unmerged and stalled, `OPENSCADPATH` is the only hook, so scadman owns injection
  indefinitely. Do not scope anything that needs OpenSCAD to meet us halfway.
- **Registry / hosted index** — after v1 proves out. Org names are reserved; don't get ahead.

## Corrections carried in the code

- `OPENSCADPATH` *shadows* globally-installed libraries, it does not hide them (OpenSCAD
  always also searches its built-in user/install dirs; the include-scan is the mitigation).
