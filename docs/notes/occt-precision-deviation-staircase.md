# The OCCT `#precision` → achieved-deviation staircase

How the facet-chord deviation OCCT actually achieves relates to the deflection a
module requests via `#precision`, why that relationship is a **staircase** rather
than a line, and what that forces on anyone choosing a `#precision` for a fixture
or a gate.

Extends — does not re-derive — §2 and §4.1 of
`docs/prds/v0_6/precision-nominal-representation-guarantee.md`, the adjudication of
record for gate 6060/esc-6060-1.

Every number below is either **measured** (quoted from a run recorded in one of the
cited sources) or **derived** (arithmetic on measured numbers). The distinction is
marked at each use.

---

## 0. Scope, audience, and what this note does NOT own

Two call sites point here, and this note exists so that the model lives in neither
of them:

- `crates/reify-cli/tests/fixtures/dfm_with_repr_within.ri` — the "#precision and
  the RepresentationWithin margin" header note.
- `crates/reify-eval/tests/representation_within_assertion.rs` — the
  `OCCT_SOURCE_FINE` doc comment.

Each of those owns the numbers it consumes directly. Neither owns the model. Until
this note existed the model lived inside the `.ri` fixture's comment header, so the
Rust engine test, the PRD and the calibration note all had to reach into one CLI
test fixture to find an OCCT-wide tessellation behaviour.

This note does **not** own, and deliberately does not restate:

| Owned elsewhere | Owner |
|---|---|
| Exact tooth periods, tooth width, duty cycle, the full regime walk, scale-invariance in `d/R`, per-class spot values, and the OCCT `.cxx` file:line citations | `docs/prds/v0_6/precision-nominal-representation-guarantee.md` §2, §4.1 |
| The dense ladder-bracketing sweep readings (per-`d` table, branch labels, the tooth sitting between two treads) | `docs/notes/precision-refine-envelope-calibration.md` §1.2 |
| The 8-point neighbourhood grid around 0.3 mm and the wall-clock rationale for choosing it | `OCCT_SOURCE_FINE`'s doc comment |
| `dfm_with_repr_within.ri`'s own measured point, its margin, its drift canary, and the by-hand one-point re-derivation recipe | that fixture's header note |

Consult those for the digits. Re-deriving them here would make three copies of the
adjudication where the point of this note is to leave one.

---

## 1. The shape: a staircase, not a sawtooth and not a line

`#precision` is threaded bit-exact into OCCT's `BRepMesh_IncrementalMesh`
linear-deflection argument. The achieved sampled facet-chord deviation is a
**staircase** in that request — measured, over a 388-point sweep (PRD §2):

- **Treads** follow an upper envelope near **~2.075×** the requested deflection.
- The treads are punctuated by narrow **periodic downward teeth** near **~0.758×**.
  The teeth are **real and recurrent, not isolated exceptions** — they recur across
  the whole range, at a period that tightens as the request gets finer (PRD §2 has
  the periods, widths and duty cycle).
- A **third branch near ~1.49×** appears below `d/R < 2.5e-4` — below ~0.25 mm on a
  1 m-radius sphere. It is **outside** the envelope/tooth characterization above:
  do not extrapolate that characterization that low, and do not read a reading
  taken down there as a tread.

The consequences of "staircase" that matter operationally: the **branch** a
candidate lands on, not the local slope, decides pass/fail; and two adjacent
requests can land on different branches, so nothing here is interpolatable. Measure,
do not interpolate.

All of the above is for the **sphere** class. Other classes have their own ratios
(PRD §2's per-class spot values) and the cylinder in particular behaves quite
differently — see §2.

---

## 2. The mechanism

### 2.1 It is NOT per-edge integer segment counts (the load-bearing falsification)

The obvious explanation for a staircase is integer quantisation of segment counts
along an edge. That explanation is **falsified**, and this falsification is
load-bearing for everything in §2.2:

A same-sweep control on `cylinder(1000mm, 2000mm)` — whose curved face **is**
edge-segment-quantised — measures ratio **0.489** (measured; PRD §2's cylinder spot
value). That is neither the sphere's ~2.075× envelope nor its ~0.758× tooth. If
per-edge integer quantisation were the mechanism, the class that actually has it
would show the sphere's branches. It does not.

### 2.2 It IS a two-ladder phase collision on the sphere face

Per PRD §4.1 (rejected alternative C), the mechanism for **which branch** — tread
or tooth — a candidate lands on is a phase collision between two independently
quantised ladders on the sphere face:

- **Interior surface nodes** are truncation-quantised at the literal constant
  `0.7 * ArcAngularStep` — `BRepMesh_SphereRangeSplitter`'s angular-step factor.
- The **seam meridian edge** is tessellated at **half** the deflection — the
  literal constant `0.5` in `BRepMesh_CurveTessellator`'s seam-edge handling.

Chord deflection scales as `theta^2` (deflection ≈ `r*theta^2/8`), so halving the
deflection does not halve the angular step: it yields an effective step of
`sqrt(0.5) = 0.70711 * ArcAngularStep` (derived). The resulting **~1 % gap between
0.70000 and 0.70711** — the splitter's literal factor versus the tessellator's
derived effective factor — is the phase-collision explanation for the branch.

**Honest status of this model, which must not be blurred:**

- For **branch prediction** it is **exact**: PRD §4.1 records that it predicts the
  branch correctly at **388/388 measured points**. It is rejected for *production*
  use only because it depends on OCCT internal, non-API constants that an upgrade
  could silently move — not because it is unverified.
- For the **periods** between teeth it is **not yet shown to predict them at all**.
  That half is a **hypothesis to re-verify, not a checked fact**. PRD §2 has the
  measured periods; nothing here derives them.

### 2.3 Why the constants are named, not cited by line

These numbers were measured against **OCCT 7.8** — the system OCCT that reify's
kernel links directly (see CLAUDE.md's native-deps note). OCCT is unpinned and
unvendored in-tree, and the hosts this was measured on carry **headers only**: there
are no `.cxx` sources present to check line numbers against. So the constants are
cited **by name** here. PRD §4.1 carries both the names and the `.cxx:line`
citations; go there if you need to point at a line.

---

## 3. What this means for choosing a `#precision`

### 3.1 Near the bound, the branch dominates everything else

PRD §2 records the sweep's finest-spaced adjacent pair. A **0.08 % change** in the
request swings the achieved deviation **2.69×** (measured):

| requested | achieved | branch | against a 1 mm bound |
|---|---|---|---|
| 0.5955 mm | 1.223e-3 m | ENVELOPE | **violates** |
| 0.5960 mm | 4.541e-4 m | TOOTH | clears |

So a value that clears its bound only because it landed on a tooth is **one retune
away** from the envelope — which, that close to the bound, violates.

**Never tune onto a tooth. Pick a candidate whose ENVELOPE value (~2.075× the
request) clears the bound.** The teeth deviate **downward only**, so the envelope is
the branch to size against; a tooth reading is free headroom you must not count on.

### 3.2 Farther below the bound, the envelope is an observation, not a bound

`~2.075×` is an **observed supremum over the regime walked** (`d/R ≳ 2.5e-4`), **not
a proven bound**. Three careful readings disagree on it, and the disagreement is not
noise:

| reading | regime | source |
|---|---|---|
| 2.079× | `d/R = 3.12e-4` | `docs/notes/precision-refine-envelope-calibration.md` §1.2 — a dense ladder-bracketing sweep, the tightest spacing of the three |
| 2.0835× | `d/R ∈ [4e-4, 4e-3]` | PRD §4.1, rejected alternative A — a wider sweep, **higher** despite less density around any one point |
| 2.106× | `d = 0.048 mm`, `d/R = 4.8e-5` | PRD §4.1, cited **cross-regime** — below the 2.5e-4 floor, so **not** a tread-branch reading |

The finer regime gave the **larger** value, which is exactly why PRD §4.1 rejects
sizing any constant from a finite sweep: such a constant fails **silently** when it
is slightly too small, the worst failure mode available for a safety property.

Practical reading for a fixture author:

- Size against the envelope, and take the largest of the readings above that you can
  defend for your regime as the anchor — including the cross-regime 2.106× if you
  want a conservative one.
- Treat any margin you compute from these ratios as an argument about the
  *envelope*, never as a prediction of the achieved value. The only way to learn the
  achieved value at a candidate is to measure it (recipe:
  `dfm_with_repr_within.ri`'s header note — one point only).
- Do not use a neighbour reading as evidence about your candidate. The tooth period
  near a fine request is far tighter than a coarse grid's spacing, so "the
  neighbours are fine" establishes nothing about the locality between them.
