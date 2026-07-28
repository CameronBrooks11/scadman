# Ecosystem survey — evidence for the design

scadman's model (see [DECISIONS.md](DECISIONS.md)) rests on a claim: the OpenSCAD
library ecosystem is a heterogeneous mix of single files, conventional libraries,
monorepos, and frameworks — not a uniform set of "packages" — and libraries carry
implicit assumptions that constrain how a manager may install them.

A pass-1 survey of the **28 repositories** on
[openscad.org/libraries.html](https://openscad.org/libraries.html) plus five
archetype fixtures tested that claim. Method and raw data live in the private
`CameronBrooks11/scadman-survey` repo (metadata via the GitHub API; structure via
shallow clones + a `.scad` include-graph / symbol scan). Percentages below are of the
28 surveyed repos.

## Findings and what each locks in

### 1. Most libraries do not release. → exact-revision handling is mandatory

- **54% have no git tags at all**; only 39% use version-like (`vX.Y`) tags.

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

### 3. The namespace is flat and already colliding. → one version per identity

- **412 module/function names are declared in ≥2 surveyed repos.** `reverse` appears
  in 7, `tube` in 6, and `arc`/`chamfer`/`flatten`/`ring`/`screw`/`unit` in 5 each.
  Some libraries even redeclare OpenSCAD built-ins (`rotate`, `scale`).

OpenSCAD has no module namespacing, and `include` is textual. Exposing two libraries
(or two versions of one) on the same path risks silent redefinition. This is direct
evidence for **one resolved version per package identity per environment** and against
a shared global library directory.

### 4. Imports use two resolution styles. → the environment must serve both

Of 4,461 import edges across the corpus (74% resolve within a repo, 26% do not), the
external edges split into:

- **qualified** (635) — name a library directory, e.g. `<BOSL2/std.scad>`;
- **bare** (514) — a plain filename resolved only via `OPENSCADPATH`, e.g. `<arc.scad>`.

`use` and `include` are used in near-equal measure (2,408 / 2,053). The environment
scadman constructs must satisfy **both** styles: named-library directories *and* a
configured `OPENSCADPATH` search root.

### 5. Structure is genuinely heterogeneous. → artifact/package/project split holds

- Layouts: nested 71%, flat-multi 14%, single-file 11%, src-layout 4%.
- `.scad` files per repo range **1 → 964** (median 16).
- Only 21% ship `tests/`; 54% ship `examples/`.
- Licenses spread wide: MIT 36%, CC0 25%, GPL 18%, plus BSD/LGPL/Apache; two have none.

No single "package" shape fits. This supports declared library-roots, optional
workspaces, and treating raw artifacts (single files, unversioned repos) as
first-class — the artifact/package/project distinction rather than a forced package
abstraction.

## Caveats

Pass 1 is 28 curated repos — a deliberate, eyeball-able first pass, not a random
sample; a wider pass-2 (100–300 repos via GitHub search) is planned to confirm the
distributions. Internal-edge counts are an upper bound (resolution is checked
importer-relative *or* repo-root, looser than OpenSCAD's own), and parsing is
regex-based. None of these caveats affect the direction of findings 1–5.
