# scadman — Foundational decisions

> Design record — the load-bearing choices made before and during early development,
> kept as history. Not user documentation; to use scadman, start at the
> [README](../README.md).

scadman is a greenfield, Rust, project-local OpenSCAD dependency and environment
manager, licensed Apache-2.0. The predecessor tool olman is reference-only — no code
or metadata inheritance. The first real work was an ecosystem survey (a Python spike
in a separate private repo, `CameronBrooks11/scadman-survey`) whose outputs — the
findings in [ecosystem-survey.md](ecosystem-survey.md) and the fixture set in
`fixtures/` — were promoted here once stable.

## The product model

scadman models three distinct things instead of forcing everything into one
"package" shape:

- **Artifact** — any reusable OpenSCAD content (a single `.scad`, a repo, a tagged
  release), with or without metadata.
- **Package** — an artifact with an explicit contract: identity, version, license,
  declared roots, dependencies, integrity.
- **Project** — a reproducible environment that consumes packages and raw artifacts
  via a manifest and an exact lockfile.

This distinction is the bet the ecosystem survey was designed to test: most OpenSCAD
content in the wild is an artifact, not a package, so a tool that demands full
package metadata up front would exclude nearly everything worth depending on.

## D1. Product strategy — greenfield, not evolve olman

**Decision:** build a new product.

Options were a greenfield tool, fork-and-evolve olman's Python client, or both in
parallel. Evolving olman would have been faster to something usable but inherits the
global-install, one-version model that is the core limitation being fixed; the value
added over olman/OPM is the product model (project-local reproducibility first), not
a cleaner install loop. A fork could not get there without becoming a rewrite anyway.

## D2. Implementation language — Rust

**Decision:** Rust.

Considered against Go (faster to a reliable v1, simpler contributor onboarding) and
staying in Python (only sensible when evolving olman). Rust fits immutable models,
lockfiles, and transactional filesystem state; ships as a single binary; and keeps a
realistic long-term path to native integration with OpenSCAD's C++. The accepted
cost is a slower v1 and steeper onboarding for casual contributors.

## D3. Name — product and binary both `scadman`

**Decision:** one name, `scadman`, for the product, the repo, and the binary.

An earlier draft used `scadpkg` for the CLI; two names would have to be explained
forever, so they were collapsed.

## D4. License — Apache-2.0

**Decision:** Apache-2.0 (relicensed from an initial MPL-2.0).

Native OpenSCAD integration is a real goal, and permissive licensing with an
explicit patent grant removes a bundling obstacle later; it is also the convention
for Rust tooling. MPL's file-level copyleft bought nothing scadman needs.

## D5. Relationship to olman — reference only

**Decision:** read olman for design and behavior; share no code and no metadata.

In particular, the registry is **not** seeded from olman's `accepted_repositories` /
`remote_index.json`: the survey built its own repository list from primary sources
(openscad.org/libraries.html, GitHub topic search), using olman's list only as a
cross-check. No `migrate olman` command is in scope.

## D6. First real work — survey before product code

**Decision:** run the ecosystem survey first; write no product code until it lands.

The artifacts-aren't-packages thesis (see *The product model*) is a bet about what
the ecosystem actually looks like. A cheap crawler either validates or cheaply kills
downstream design (workspaces, adapters, a `kind` taxonomy) before any Rust exists.
The alternative — build the vertical slice first — risks baking in assumptions the
data would have corrected. The survey's results are in
[ecosystem-survey.md](ecosystem-survey.md).

## D7. Repo shape — this repo is the product

**Decision:** this repo is the product (Rust workspace, docs, fixtures). The survey
lives in a separate private repo, `CameronBrooks11/scadman-survey`.

The survey is throwaway-ish Python at a different quality bar and would pollute a
repo meant to stay clean; only its stable outputs (findings report, curated fixture
list) are promoted here. Splitting product and registry into separate repos is
deferred until governance demands it.

## D8. Housekeeping

**Decision:** Rust `.gitignore` template; README carries the product thesis;
registry topology (static curated index + direct git, federation-ready) is real but
**deferred** until well after a working client — GitHub names for a future registry
org are already reserved so nothing blocks it later.

## D9. Dependency resolver (v1) — collect-and-unify, not a solver

**Decision:** v1 resolves by deterministic DFS: each ref resolves to an exact SHA,
one identity gets one rev, and a genuine conflict is a hard error. Adopt a real
solver (pubgrub-rs) only when a registry brings semver ranges or real
library-on-library depth appears.

The survey shows there is nothing to *solve* — 81% of wild repos have zero
dependencies, max depth 2, zero version conflicts in 196 repos, zero formal
manifests. Cut from v1 (zero triggers in the corpus): pseudo-versions, a reserved
semver-range variant, a polished conflict renderer. Added to v1: the include-scan
flagging unmet imports, the one failure mode the data shows firing today. Other
settled choices: identity is a name bound to a canonical URL (URL authoritative);
manifest-less transitive dependencies are declared by the user, with the
include-scan warning; overrides deferred entirely (unknown manifest keys are
rejected, so adding the syntax later is non-breaking); branch refs allowed but
SHA-locked; OpenSCAD's bundled MCAD is fetchable and shadowable via `OPENSCADPATH`.
Full rationale in [resolver-direction.md](resolver-direction.md).

## Summary

| # | Decision | Call |
|---|----------|------|
| D1 | Product strategy | Greenfield `scadman` |
| D2 | Language | Rust |
| D3 | Name | `scadman` (product + binary) |
| D4 | License | Apache-2.0 |
| D5 | olman relationship | Reference only |
| D6 | First work | Ecosystem survey, before product code |
| D7 | Repo shape | scadman = product; survey in a separate private repo |
| D8 | Housekeeping | Rust gitignore · README thesis · registry deferred |
| D9 | Resolver (v1) | Collect-and-unify; include-scan; pubgrub-rs later |
