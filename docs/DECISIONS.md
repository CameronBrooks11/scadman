# scadman — Foundational Decisions

> **Purpose:** lock the load-bearing choices *before* writing product code, so the
> repo stops silently hedging between "new Rust product" and "evolve olman in Python."
>
> **How to use:** read each decision, edit the `**Decision:**` line with your call
> (and any reasoning in **Notes**). Leave `PENDING` on anything you're not ready to
> settle. Recommendations are mine, argued from `working/ROUGH_CHAT_DIRECTION.md`
> and the current repo state — push back freely.
>
> Status legend: `PENDING` · `DECIDED` · `DEFERRED` (revisit at a named milestone)

---

## Resolved direction (TL;DR)

**scadman is a greenfield, Rust, project-local OpenSCAD dependency & environment
manager. Apache-2.0. olman is reference-only (no code/metadata inheritance). First
real work is a Python ecosystem-survey spike, done properly in a separate private repo
(`CameronBrooks11/scadman-survey`).**

All D1–D8 confirmed. Survey repo created. Housekeeping applied (Apache-2.0 license,
Rust gitignore, README thesis).

---

## D1. Product strategy — greenfield vs. evolve olman

**The fork in the road. Everything else hangs off this.**

Right now the repo hedges: the strategy doc argues for a *new* project-local product,
but your `working/…-camfork` has already started hardening olman's Python client
(added `docs/`, a roadmap about "stabilize `olman-client`", Poetry).

**Options**

- **A — Greenfield `scadman` (recommended).** New product model (project-local
  reproducibility first, artifact/package/project distinction, content-addressed
  store). olman becomes a *reference + metadata source*, not a codebase to evolve.
  The doc's own §2/§20 argue against preserving olman's client architecture.
- **B — Fork-and-evolve olman.** Keep the Python monorepo, fix it incrementally
  (removal safety, lockfile, resolver). Faster to *something usable*, but inherits
  the global-install / one-version model the doc calls the core limitation. Also
  makes *this* repo largely redundant with `-camfork`.
- **C — Hybrid.** Evolve olman short-term for a usable tool now; greenfield in
  parallel as the real bet. Risk: splits limited attention two ways.

**Recommendation:** A. The value you're adding over olman/OPM is the *product model*,
not a cleaner install loop — and B can't get there without becoming A anyway.

**Decision:** A

**Notes:**

---

## D2. Implementation language

Coupled to D1 but worth an explicit, separate call — the doc treats Rust as decided,
yet its own honest caveat is that Rust may throttle contributor throughput on a
low-adoption (~13-star) ecosystem.

**Options**

- **A — Rust (doc's pick).** Best fit for immutable models, lockfiles, transactional
  FS state; single-binary distribution; realistic long-term C++ FFI into OpenSCAD.
  Cost: slower v1, steeper contributor onboarding.
- **B — Go.** Doc's stated "reliable v1 quickly" choice. Simpler onboarding, great
  stdlib for HTTP/archives/paths, trivial cross-compile. Weaker for solver-state
  modeling; awkward to embed in OpenSCAD's C++ later.
- **C — Stay Python.** Only sensible under D1=B. Distribution is the product's weak
  point (the existing Snap + `libversion` native dep prove it).

**Recommendation:** Rust *if* this is a multi-year infrastructure bet you'll carry;
Go *if* "dependable tool in months, small contributor base" matters more than
eventual native OpenSCAD integration. Genuinely worth a second look rather than
inheriting the doc's default — this is the highest-regret choice to get wrong.

**Decision:** A — Rust (confirmed)

**Notes:** A

---

## D3. Name — product and binary

Three names are currently in play: repo/product `scadman`, the doc's CLI `scadpkg`,
and legacy `olman`. Manifest examples, env dirs, and docs all reference the binary
name, so pin it now.

**Options**

- **A — Product `scadman`, binary `scadman`.** One name, matches the repo. Simplest.
- **B — Product `scadman`, binary `scadpkg`.** Keeps the doc's examples; but two
  names to explain forever.
- **C — Something else entirely.** (`scad-pm`, `scfrom`, …)

**Recommendation:** A — collapse to one name. Rename `scadpkg` → `scadman` across
the strategy doc when it's promoted to real docs.

**Decision:** A — product and binary both `scadman` (confirmed)

**Notes:**A

---

## D4. License

Top-level is currently **MPL-2.0**; upstream olman is **MIT**.

**Context:** MIT→(MPL or anything) is fine for a derivative or clean reimplementation.
If you lift olman's accepted-repositories catalog or manifest schema verbatim, keep
an attribution notice. Choice mostly signals intent: MPL = file-level copyleft
(changes to *these* files stay open, but it links freely into other licenses);
MIT/Apache-2.0 = maximal permissive, easiest to bundle into OpenSCAD itself later.

**Options**

- **A — MPL-2.0 (current).** Weak copyleft; reasonable default for infra.
- **B — Apache-2.0.** Permissive + explicit patent grant; easiest path to eventual
  upstream bundling into OpenSCAD; conventional for Rust/Go tooling.
- **C — MIT.** Match olman; minimal friction.

**Recommendation:** If native OpenSCAD integration is a real goal (it is, per doc §16),
lean **Apache-2.0** — permissive licensing removes a bundling obstacle later. Keep MPL
only if you specifically want the core files to stay copyleft.

**Decision:** Apache-2.0

**Notes:**

---

## D5. Relationship to olman

**Options (not mutually exclusive):**

- Reference only — read it for design/behavior, share no code.
- Import metadata — consume its `accepted_repositories` / `remote_index.json` as an
  initial registry seed (doc §20 "potential compatibility", `scadman migrate olman`).
- Upstream collaboration — coordinate with OpenSCAD maintainers (Torsten Paul) before
  investing heavily, to avoid a competing-fork dynamic.

**Recommendation:** Reference + import metadata now; open a lightweight upstream
conversation *after* the Stage 0 survey gives you data to point at (avoids "unfinished
package manager, please adopt").

**Decision:** reference only, we are diverging quite a bit from olman at this point

**Notes:** Implication for the survey: we do **not** seed the registry from olman's
`accepted_repositories` / `remote_index.json`. The Stage 0 crawl builds its own repo
list from primary sources (openscad.org/libraries.html, GitHub `openscad` topic
search). olman's list can be read as one cross-check input, nothing more. No
`migrate olman` command in scope.

---

## D6. First real work — Stage 0 survey before any product code

The doc's Stage 0 (crawl the official library list + top repos; measure sizes, `.scad`
counts, releases, licenses, include graphs, layouts) is doing more load-bearing work
than it looks: the entire "artifacts aren't packages / progressive adoption levels"
thesis is a *bet about what the ecosystem actually looks like*.

**Options**

- **A — Survey first (recommended).** Cheap Python crawler, output checked in as
  fixtures + a findings report. Either validates or cheaply kills downstream design
  (workspaces, adapters, `kind` taxonomy) before you write a line of Rust/Go.
- **B — Vertical slice first.** Start building `init/add/sync/run`; survey later.
  Risk: bake in ecosystem assumptions that the data would have corrected.

**Recommendation:** A. This is also the one place Python stays regardless of D2.

**Decision:** A, lets do this survery spike and do it properly

**Notes:**

---

## D7. This repo's shape

**Options**

- **A — This repo *is* the product** (Rust/Go workspace or Python monorepo per D2).
  `working/` stays as gitignored scratch. `-camfork` is demoted to reference.
- **B — This repo is a meta/planning repo**, product lives elsewhere (e.g. keep
  evolving `-camfork`). Then `scadman` is docs + registry only.

**Recommendation:** A, assuming D1=greenfield — one repo, product + registry +
fixtures + docs, split out later only if governance demands it (doc §17).

**Decision:** A, yes this is the repo. If u think we should create a second repo for the python crawler and research spike we can do so and create a new repo on my personal account CameronBrooks11 so we can track it and write stuff etc as a private spike/working repo without polluting the scadman which is supposed to be clean and no slop and proper

**Notes:** Agreed, and recommended. Keep scadman as the clean Rust product; the survey
is throwaway-ish Python at a different quality bar and shouldn't live here. Proposal:
a separate **private** repo `CameronBrooks11/scadman-survey`. Its *outputs* — the
findings report + a curated set of fixture repos — get promoted into scadman
(`docs/` + `fixtures/`) once stable; the crawler itself stays behind. Needs your ✅ on
(a) do it as a separate repo, (b) the name `scadman-survey` (vs `-spike` / `-research`).

**CONFIRMED:** private repo `CameronBrooks11/scadman-survey` created.

---

## D8. Housekeeping (decide once D1/D2 land — low stakes)

- **`.gitignore`:** currently the full Python template. Swap to a Rust/Go
  (`target/`, `dist/`) template if the language changes. → PENDING
  CAM: YES SWAP
- **README:** replace pitch+joke stub with the doc's actual thesis (project-local
  reproducibility; artifact/package/project) once direction is set. → PENDING
  CAM: sgtm (keep the joke its whatever for now)
- **Registry topology:** doc §12–13 recommends static curated index + direct git +
  federation-ready. Real, but downstream of a working client — explicitly DEFERRED
  to post-Stage-1. → DEFERRED
  CAM: yes we can defer this, but i def plan for it one day - i already reserved openscad-libraries and some other org name patterns on github etc so its ready but lets not get ahead.

---

## D9. Dependency resolver (v1)

Settled after a research pass over the ecosystem survey + solver prior art. Full rationale
in [resolver-direction.md](resolver-direction.md).

The data shows there is nothing to *solve* (81% of wild repos have zero deps, max depth 2,
zero version conflicts in 196 repos, zero formal manifests). So v1 is **"collect-and-unify"**,
not a constraint solver: deterministic DFS, resolve each ref → exact SHA, one identity →
one rev, hard error on conflict. Formalize the data model (`Requirement`/`Conflict`) behind
a narrow `manifest + fetcher → ResolvedSet` interface so a later **pubgrub-rs** swap is
contained.

Cut from v1 (0 triggers in the corpus): pseudo-versions, a reserved semver-range variant,
the polished conflict renderer. **Added** to v1: a post-install include-scan flagging
unmet imports (the only feature the data shows firing today; manifest-less transitive deps
are the real scenario). Key settled choices: identity = name-bound-to-canonical-URL (URL
authoritative); manifest-less transitive deps → user declares + include-scan warns;
override syntax reserved, impl deferred; branch refs allowed but SHA-locked; bundled MCAD
is fetchable (shadowable via `OPENSCADPATH`).

**Decision:** collect-and-unify for v1; adopt pubgrub-rs only when a registry brings semver
ranges or real library-on-library depth appears.

---

## Decision summary (fill in as you go)

| # | Decision | Call | Status |
|---|----------|------|--------|
| D1 | Product strategy | Greenfield `scadman` | DECIDED |
| D2 | Language | Rust | DECIDED |
| D3 | Name | `scadman` (product + binary) | DECIDED |
| D4 | License | Apache-2.0 | DECIDED |
| D5 | olman relationship | Reference only | DECIDED |
| D6 | Stage 0 first? | Yes — survey spike, done properly | DECIDED |
| D7 | Repo shape | scadman = product; survey = `CameronBrooks11/scadman-survey` (private) | DECIDED |
| D8 | Housekeeping | gitignore→Rust ✓ · README thesis ✓ · Apache-2.0 ✓ · registry DEFERRED | DECIDED |
| D9 | Resolver (v1) | Collect-and-unify (not a solver); include-scan; pubgrub-rs later | DECIDED |
