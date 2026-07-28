# Resolver direction (v1)

Grounded in a multi-agent research pass over the ecosystem survey (`ecosystem-survey.md`,
28 curated + 200 wild repos) and solver prior art (PubGrub, Go MVS, backtracking, OPM,
olman). This records the decision so it isn't relitigated.

## Finding: there is nothing to *solve* yet

- **81% of wild repos declare zero external dependencies.** Of the 19% that do, 75% have
  exactly one; the corpus max is **5** — all direct, all leaves.
- **Max dependency depth is 2**, converging on a single version. **Zero version conflicts
  and zero divergent diamonds in 196 repos.**
- **Zero formal dependency manifests exist in the wild.** The only real declaration
  mechanism is git submodules (8 repos) — exact-rev pins, i.e. scadman's model already.
- **54–63% of libraries have no tags**, so an exact git SHA is the only *expressible*
  constraint → at most one candidate per package. The search PubGrub/backtracking/MVS
  exist to perform structurally cannot occur.

So v1 resolution is **collect + unify + detect-conflict**, not constraint solving.

## Decision: "collect-and-unify"

Deterministic DFS from the root `scadman.toml`. For each dependency: resolve its ref to
an exact commit SHA via the existing git-acquisition layer, fetch it into the store,
recurse into its `scadman.toml` if it ships one (cycle guard + depth cap), unify by
identity. **Two different revs for one identity is a hard error — no automatic join.**
~100–300 lines, zero new dependencies.

Formalize the *data model* now even though the algorithm is trivial, so evolution is
contained:

- `Requirement { identity, constraint, required_by }` — keep provenance (olman's fatal
  bug was dropping it and ending at `Exception("Can't construct graph")`).
- A structured `Conflict` error type.
- A narrow interface: `manifest + fetcher → ResolvedSet | Conflict`. The lockfile writer,
  store, and env builder consume `ResolvedSet` and never see the algorithm — so a later
  swap to **pubgrub-rs** is a contained change.

### Cut from v1 (trigger conditions occur 0× in 196 repos)

- Pseudo-version strings — plain SHAs only (a sortable-looking string invites bad sorts).
- A reserved `Range(semver)` enum variant — adding it later is a contained change.
- The *polished* two-chain conflict renderer — keep `required_by` + a plain
  duplicate-identity error; build the pretty renderer when depth-2 manifests actually exist.

### Added to v1 (the one thing the data shows firing today)

- **Post-install include-scan.** Both real transitive chains (`agentscad`,
  `openscad_annotations`) are *manifest-less*, so a user adding `agentscad` otherwise gets
  a raw OpenSCAD `can't open include file "scad-utils/…"`. The survey's `external_libs`
  extraction *is* this scan (~50 lines): diff installed deps' `use`/`include` targets
  against the resolved set → "`agentscad` imports `scad-utils/`, not in your manifest — add
  it." It is also the honest replacement for a hermeticity guarantee `OPENSCADPATH` cannot
  deliver (see corrections).

## Alternatives and why they lost

| Option | Verdict |
|---|---|
| **PubGrub (pubgrub-rs)** | CDCL never fires with ≤1 candidate; git SHAs have no ordering (uv itself pins git deps *before* PubGrub). Right aspiration (its error prose — copied above), wrong machinery. Clean swap-in later. |
| **Go MVS** | Its `max()` join needs a semver-compatibility contract this ecosystem lacks (63% untagged); under exact pins it degenerates to collect-and-unify. Adopt its *properties* (determinism, integrity records), not its algorithm. |
| **Backtracking (olman/resolvelib)** | Only matters with multiple candidates, which don't exist under exact pins → untestable dead branches now. If search becomes real, go straight to pubgrub-rs. |
| **npm nesting / OPM source-rewriting** | Mechanically impossible under a flat namespace; rewriting breaks content-addressing and verification. Already a recorded decision. |

## v1 scope

**Does:** recursively collect deps (root + any dep shipping a `scadman.toml`); resolve
every ref → exact SHA at lock time; write lockfile entries `{identity, source, commit,
content-hash}`; enforce one identity → one rev; detect identity collisions (same name,
different canonical URL) and cycles; deterministic output. Plus the post-install
include-scan.

**Does not:** solve/compare/order versions; backtrack or learn clauses; auto-resolve
conflicts; rewrite source; nest versions; touch a registry; manage global installs.

## Settled decisions

1. **Identity = name bound to canonical source URL, URL authoritative**, with a documented
   normalization rule (https/ssh, `.git` suffix, case; watch GitHub renames/transfers that
   redirect). This is the one choice that becomes a lockfile migration if wrong — fixed now.
2. **Manifest-less transitive deps:** the user declares them directly; the include-scan
   guides them. No curated metadata overlay in v1.
3. **Overrides:** reserve the syntax in the manifest schema now; defer implementation. The
   hard identity-conflict error must not promise a remedy that doesn't exist yet.
4. **Branch refs:** allowed, but always locked to an exact SHA in the lockfile.
5. **Bundled libs (MCAD):** fetchable in v1, no special "provided" casing. It is
   shadowable via `OPENSCADPATH`, so a pinned copy is reproducible.

## Corrections carried forward

- MCAD **is** shadowable via `OPENSCADPATH` — scadman can manage the most-depended-on library.
- Replacing `OPENSCADPATH` does **not** stop OpenSCAD searching the user and installation
  library folders. The honest guarantee is "declared deps shadow globals," not "undeclared
  deps fail" — hence the include-scan.
- The **lock** step (manifest → lockfile) is *not* pure: a branch/moving-tag resolves to
  different SHAs on different days. Determinism holds for **lockfile → environment**.

## Upgrade trigger

Adopt **pubgrub-rs** (not a hand-rolled backtracker) when either: a registry introduces
real semver ranges with multiple candidates per identity, or library-on-library depth
materializes with multi-step conflict chains (currently ~1% of repos). Until then a solver
is dead machinery.
