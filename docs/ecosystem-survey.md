# Ecosystem survey — evidence for the design

> Design record — the evidence behind scadman's model, not user documentation.

scadman's model (see [DECISIONS.md](DECISIONS.md)) rests on a claim: the OpenSCAD
library ecosystem is a heterogeneous mix of single files, conventional libraries,
monorepos, and frameworks — not a uniform set of "packages" — and libraries carry
implicit assumptions that constrain how a manager may install them.

Two surveys tested that claim: **pass 1**, the 28 repositories on
[openscad.org/libraries.html](https://openscad.org/libraries.html) plus five archetype
fixtures; and **pass 2**, a wider sample of 200 repositories discovered by GitHub search
(`topic:openscad` ∪ `language:OpenSCAD`, forks/archived excluded), 168 of them
content-analyzed. Method and raw data live in the private `CameronBrooks11/scadman-survey`
repo (metadata via the GitHub API; structure via shallow clones + a `.scad`
include-graph / symbol scan). Findings are quoted pass 1 → pass 2 so the trend is
visible; unless noted, the wider sample confirms.

## Findings and what each locks in

### 1. Most libraries do not release. → exact-revision handling is mandatory

- **54% have no git tags at all** (pass 2: **63%**, and 65% among repos containing
  `.scad`); only 39% use version-like (`vX.Y`) tags (pass 2: 32%).

A tag-as-version model (as in olman) cannot address the majority of the ecosystem.
scadman must resolve a manifest reference to an **exact commit + content hash**, and
support unversioned sources as first-class, not an afterthought.

### 2. Libraries assume install-under-their-own-name. → the environment must honor it

- **18% reference themselves by their own root name** — e.g. files inside BOSL2 do
  `include <BOSL2/std.scad>`, and NopSCADlib, MCAD, and BOSL do the same.

A library that includes itself via `<Name/...>` breaks unless it is installed at a
path where `Name/` resolves. scadman's per-project environment must expose each
library **under its own canonical name** on the search path; renaming or flattening
the install directory is not safe.

Pass 2's aggregate self-rooted rate falls to 2% — but only because the wider sample is
mostly end-user *projects*, not installable libraries. The repos that are libraries
(threadlib, agentscad, …) still self-root, and pass-2 consumers overwhelmingly import
libraries by qualified root name (`MCAD/…`, `BOSL2/…`). Install-under-own-name holds.

### 3. The namespace is flat and already colliding. → one version per identity

- **412 module/function names are declared in ≥2 surveyed repos.** `reverse` appears
  in 7, `tube` in 6, and `arc`/`chamfer`/`flatten`/`ring`/`screw`/`unit` in 5 each.
  Some libraries even redeclare OpenSCAD built-ins (`rotate`, `scale`).

OpenSCAD has no module namespacing, and `include` is textual. Exposing two libraries
(or two versions of one) on the same path risks silent redefinition. This is direct
evidence for **one resolved version per package identity per environment** and against
a shared global library directory. At pass-2 scale the collision set grows to **2,705
names** (top name in 22 repos) — the risk scales with the ecosystem.

### 4. Imports use two resolution styles. → the environment must serve both

Of 4,461 import edges across the corpus (74% resolve within a repo, 26% do not), the
external edges split into:

- **qualified** (635) — name a library directory, e.g. `<BOSL2/std.scad>`;
- **bare** (514) — a plain filename resolved only via `OPENSCADPATH`, e.g. `<arc.scad>`.

`use` and `include` are used in near-equal measure (2,408 / 2,053). The environment
scadman constructs must satisfy **both** styles: named-library directories *and* a
configured `OPENSCADPATH` search root. Pass 2 sharpens this: **79% of external imports
are qualified** (name a library root), up from 55% — the named-directory case dominates.

### 5. Structure is genuinely heterogeneous. → artifact/package/project split holds

- Layouts: nested 71%, flat-multi 14%, single-file 11%, src-layout 4% (pass 2: nested
  58%, single-file **21%** — the small end gets heavier in the wild).
- `.scad` files per repo range **1 → 964** (median 16); pass 2 includes a generated
  corpus of 36k files.
- Only 21% ship `tests/`; 54% ship `examples/`.

No single "package" shape fits. This supports declared library-roots, optional
workspaces, and treating raw artifacts (single files, unversioned repos) as
first-class — the artifact/package/project distinction rather than a forced package
abstraction.

### 6. Much of the ecosystem is unlicensed. → license is optional metadata

- Pass 1's curated set was 93% licensed, but in the wild **41% of repos declare no
  license at all** (pass 2, n=200); of those that do, MIT 18% and GPL-3.0 16% lead.

scadman's package and registry model must treat a license as **optional/unknown**
rather than assumed, and surface "unlicensed" honestly rather than guessing one.

## Caveats

Findings come from two samples, not a census: pass 1 (28 curated repos, deliberately
eyeball-able) and pass 2 (200 search-discovered repos, 168 content-analyzed). Pass 2
confirmed the direction of every finding above; the one rate divergence (self-rooting)
is explained by pass 2 being weighted toward end-user projects rather than libraries.
Internal-edge counts are an upper bound (resolution is checked importer-relative *or*
repo-root, looser than OpenSCAD's own), and parsing is regex-based. One generated corpus
(36k `.scad` files) was excluded from edge aggregates — itself a signal that
registry-scale tooling must survive degenerate-scale repos.
