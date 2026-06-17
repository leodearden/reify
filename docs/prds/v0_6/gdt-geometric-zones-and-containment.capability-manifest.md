# Capability manifest — `gdt-geometric-zones-and-containment.md`

Mechanizes `/prd` gates **G3** (substrate exists / wired) + **G6** (premise valid) per leaf, per
`.claude/skills/prd/project.md` → *Capability Manifest*. Each row binds a leaf's asserted capability
to **evidence** (file:line on main, or a queued/upstream producer) + a **verdict**. Any **FAIL**
binding blocks queueing until resolved. Authored 2026-06-10 against main `28157c5753` (all anchors
re-verified at decompose time).

Evidence forms: **wired-on-main** (anti-orphan — symbol reached from a production path, not
test-only), **grammar-fixture** (G3 — novel syntax parses, or N/A), **numeric-floor** (G6 — `bound >
floor`, or "no fabricated bound"), **field-population** (producer writes a real non-`Undef` value).
Verdicts: **PASS** / **PASS (producer-self)** (the leaf *is* the producer of a not-yet-wired
capability — wiring is the leaf's own scope) / **PASS (upstream)** (delivered by a wired or
dep-edge-gated producer) / **FAIL**.

> **G3 stance.** No novel grammar anywhere in the batch: both candidate fragments re-parsed at
> decompose time (`tree-sitter parse --quiet` exit 0, 2026-06-10): (1) constraint param with
> call-expression default (`param actual : Geometry = nominal()` — the C3 shape, mirrors
> `Conforms`'s existing param-default forms); (2) trait/structure/enum additions for α (`enum
> ZoneShape`, new structures with `zone_shape` params — mirror `enum MaterialCondition`
> `tolerancing.ri:13` and the 18 shipped callout structures). `grammar_confirmed=true` for every
> task. Rung-4 leaves (ι, κ) are additionally gated on out-of-batch producers **4382/4385/4388**
> (pending, real dep edges) — their bindings are PASS (upstream) by dep edge, re-checked at their
> dispatch.

---

## Leaf α — tolerancing.ri legality-bearing restructure (additive)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| GeometricTolerance hierarchy + `nominal_zone` scalar exist (additive base) | wired-on-main | `crates/reify-compiler/stdlib/tolerancing.ri:46-51` (trait + `nominal_zone` let); 18 callout structures below; shipped by 4265–4268 | PASS |
| Enum + trait/structure grammar for `ZoneShape`, `StraightnessOfAxis`, `…Related` variants, required `datum_refs` | grammar-fixture | `/tmp/prd-gate-fixtures/gdt-zones-2.ri` parses (exit 0, 2026-06-10); mirrors `enum MaterialCondition { MMC, LMC, RFS }` (`tolerancing.ri:13`) | PASS |
| Structures may re-declare inherited trait params (the α mechanism for per-callout `zone_shape`/`datum_refs`) | wired-on-main | `trait_requirements.rs` re-declaration semantics (substrate correction, PRD §3); the 18 shipped structures already re-declare `tolerance_value`/`feature`/`material_condition` | PASS |
| B4 regression target exists (Test A/B) | wired-on-main | `examples/tolerancing/std_tolerancing_surface.ri` + `crates/reify-cli/tests/cli_tolerancing_eval.rs:22` (CI-gated) | PASS |
| Template-count assertions updatable | wired-on-main | same CI test asserts template counts today; α updates them — its own scope | PASS (producer-self) |

## Leaf β — check-time GD&T legality diagnostics (Rust)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Callout-instance enumeration substrate (C1): transitive trait-conformance walk | wired-on-main | `satisfies_trait_bound` (`crates/reify-compiler/src/entity.rs:3732`), the refinement-chain walk per 4081/4232 | PASS |
| Diagnostics registry accepts new `E_*`/`W_*` codes with spans | wired-on-main | `crates/reify-core/src/diagnostics.rs` (registry + span machinery; naming finalized in β per PRD §11 Q3) | PASS |
| Kernel-less always-runs placement (legality needs only param values) | wired-on-main | `reify check` P1 path runs constraint/template analysis without a kernel — same placement family as `check_constraints_against_templates`; β's lint reads compiled param values only | PASS |
| Legality must be Rust-side, not `.ri` guards (premise) | wired-on-main | substrate correction PRD §3: template-declared ctor constraints empirically not enforced per instance; `.ri`-guard route structurally unavailable — diagnostic route is the sanctioned one | PASS |
| `Flatness(material_condition: MMC)` fixture is legal *syntax* (the lint, not the parser, rejects it) | grammar-fixture | existing callout-ctor grammar (shipped Test A/B exercises ctor-lets with `material_condition:` args) — no novel syntax | PASS |
| Enumerator has a second consumer (anti-orphan) | wired-on-main (producer-self) | C1 contract: η reuses the β enumerator verbatim; dep edge η←β wired at decompose | PASS (producer-self) |

## Leaf γ — prismatic zone constructors (`zone_cylinder`, `zone_annulus`, `zone_profile` — composition-only)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Registered-geometry-fn recipe (registry lists + lowering + eval dispatch) | wired-on-main | `crates/reify-compiler/src/geometry.rs:1730-1790` (`GEOM_ARG_FUNCTIONS` + `compile_geometry_call`); `crates/reify-eval/src/geometry_ops.rs` dispatch; nested composition proven by `examples/m5_geometry.ri` / `m5_geometry_flange.ri` | PASS |
| Existing ops sufficient for composition (no new kernel op in γ) | wired-on-main | `crates/reify-ir/src/geometry.rs:529-839`: `Cylinder` , `Tube`, sweep/pipe, `Difference` (`:575`), `Thicken` (`:793`) — `zone_profile = Difference(Thicken(+w/2), Thicken(−w/2))` composes shipped ops only | PASS |
| Widths passed as plain `Length` args (struct-arg lowering NOT assumed) | wired-on-main | C6 contract: constructors take `nominal_zone`-derived widths as scalar args; struct-arg lowering into geometry fns is explicitly **not** assumed (unverified substrate, PRD §8 C6 / §11 Q6) | PASS |
| **G6 — B6 cylinder volume oracle `V=π/4·d²·L` at 1e-9 rel** | numeric-floor | GProp mass-properties on an **analytic B-rep** cylinder (no tessellation in the loop) — closed-form identity, machine-precision floor ≪ 1e-9 rel; volume query wired (`geometry_ops.rs:1846` `"volume"` scalar-query dispatch) | PASS |

## Leaf δ — face-offset-slab kernel op + `zone_slab`

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| `GeometryOp` enum + OCCT FFI extension recipe (the ONE new kernel op) | wired-on-main | `crates/reify-ir/src/geometry.rs` op enum (offset-family precedent `Thicken` `:793` / `ModifyThicken` `:294`); `reify-kernel-occt` cpp+rs FFI pattern (`measure_mesh_deviation` `ffi.rs:1068` is the most recent worked addition); exact OCCT recipe = PRD §11 Q1 (tactical, in-δ) | PASS (producer-self) |
| Face handles reachable as op inputs | wired-on-main | face selectors ship (`faces_by_normal` family; topology-selector dispatch `geometry_ops.rs:2675`) | PASS |
| **G6 — B6 planar-slab volume identity `V=w·A`** | numeric-floor | exact-planar identity: offsetting a **planar** face by ±w/2 and capping yields a prism — closed-form, no mesh in the loop. Curved-face case deliberately held to a smoke bar (non-failure + volume>0), NO fabricated curved-face bound | PASS |

## Leaf ε — VC/RC scalars + VC boundary solid + clearance e2e

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| `min_clearance` wired on main | wired-on-main | `crates/reify-eval/src/geometry_ops.rs:2109` (`KinematicHelper::MinClearance`, production dispatch), task 2530 done | PASS |
| `DimensionalTolerance` + size-callout structs exist for VC arithmetic inputs | wired-on-main | `tolerancing.ri` (shipped 4266/4267: `DimensionalTolerance`, `symmetric_tolerance` returning it) | PASS |
| `zone_cylinder` available at VC Ø | producer:γ upstream | intra-batch dep edge ε←γ | PASS (upstream) |
| **G6 — VC/RC scalar exactness** | numeric-floor | VC/RC = exact `Length` arithmetic (MMC size ± geometric tol — additions only, no solver, no sampling); exactness claim is closed-form ⇒ no floor hazard. B8 asserts *verdict flips* (Satisfied/Violated), not float equality | PASS |

## Leaf ζ — `MaxDeviation` GeometryQuery (promotes `measure_mesh_deviation`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Sampled max-deviation primitive exists | wired-on-main | `measure_mesh_deviation(shape, mesh) -> f64` (`crates/reify-kernel-occt/src/ffi.rs:1068`), built+wired by done 4198 (tess-QA path `engine_build.rs` tessellate site) — ζ promotes it to a query, does not build it | PASS |
| `GeometryQuery` enum + `QueryCapability` repr-gating mechanism | wired-on-main | `crates/reify-ir/src/geometry.rs:942` (`enum GeometryQuery`) + `:1379` (`enum QueryCapability`, BRep-only routing note `:1407`) | PASS |
| Units signature table accepts a `Length`-returning query | wired-on-main | `"volume"`/`"area"` scalar-query precedent (`geometry_ops.rs:1802,1846`) — same dispatch + dimension-tagging shape | PASS |
| **G6 — deviation floor: `0.5mm` bound ≫ `±(h+chord_tol)` floor** | numeric-floor | floor = sample spacing `h` + tessellation `chord_tol` (µm-scale at engine repr tolerance) vs a 0.5mm imposed translation — ≥2 orders of magnitude headroom; test asserts an **inequality against the stated floor, documented in the test**, never exactness (esc-3453/esc-3770 class structurally avoided) | PASS |

## Leaf η — `measure_gdt_conformance` pass + `Conforms.actual` + `nominal()` marker

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Engine measured-constraint pass pattern + C1 invariant precedent | wired-on-main | `RepresentationWithin` interception (`crates/reify-eval/src/engine_constraints.rs:42-125`): kernel-less → Indeterminate-never-false-Violated, the exact invariant C3/C5 mirror | PASS |
| Caller-order weave contract | wired-on-main | `dispatch_constraints` (`engine_constraints.rs:42`) weaves `ConstraintResult`s in caller order — C5 binds to it; B9 is the two-way boundary test | PASS |
| Constraint param with call-expression default parses (C3 surface) | grammar-fixture | `/tmp/prd-gate-fixtures/gdt-zones-1.ri` parses (exit 0, 2026-06-10) | PASS |
| Static-binding detection is the only sound trigger (premise) | wired-on-main | neutral-scope param-default compilation (`crates/reify-compiler/src/functions.rs:95-119`): defaults can't reference siblings ⇒ the evaluated default is useless as a sentinel ⇒ C3 keys on the statically-present explicit `actual` binding — design already encodes the substrate constraint | PASS |
| `nominal()` no-arg registered builtin (new) | wired-on-main (producer-self) | η adds it via the shipped stdlib-builtin recipe (`crates/reify-stdlib/src/tolerancing.rs:18` dispatch precedent — `effective_tolerance_zone`); consumer = C3 default surface; name = PRD §11 Q2 (tactical) | PASS (producer-self) |
| `MaxDeviation` query available | producer:ζ upstream | intra-batch dep edge η←ζ | PASS (upstream) |
| Scalar predicate to feed measured value into | wired-on-main | shipped `Conforms` body (`tolerancing.ri:227-232`) — `effective_tolerance_zone(...) >= measured_deviation`; η substitutes the measured value, body unchanged | PASS |
| Violated carries a real magnitude (not `Undef`) | field-population | C5: diagnostic message carries measured magnitude + zone width — produced by ζ's query (a real `f64` in SI metres), substituted before the shipped predicate evaluates; kernel-less path short-circuits to Indeterminate *before* any value fabrication | PASS |

## Leaf θ — integration gate + engine-integration-norm §3 entry (critical leaf)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| All §9 matrix inputs delivered by deps | producer:γ,δ,ε,η upstream | intra-batch dep edges θ←γ,δ,ε,η (α,β,ζ transitively) | PASS (upstream) |
| B4 regression target byte-stable | wired-on-main | `examples/tolerancing/std_tolerancing_surface.ri` + `cli_tolerancing_eval.rs` Test A/B (CI today); B4 asserts unmodified semantics both ways | PASS |
| B5 boolean cross-oracle composable from shipped ops | wired-on-main | `Difference` (`reify-ir geometry.rs:575`) + `"volume"` query (`geometry_ops.rs:1846`) — oracle-only usage on clearly-inside/clearly-outside fixtures (the near-coincident fragile case deliberately excluded, PRD §4) | PASS |
| Norm doc exists to receive the §3 entry | wired-on-main | `docs/prds/v0_3/engine-integration-norm.md` §3 (seam catalogue); sibling-entry precedent = 4408's DFM pass entry (cite if landed, else standalone — PRD §7) | PASS |
| **G6 — matrix asserts verdicts + floor-bounded inequalities only** | numeric-floor | B1 magnitude within ±(h+chord_tol) of 0.5mm (ζ's floor); B6 identities per γ/δ rows; B8 verdict flips; no exact-float assertions anywhere in §9 | PASS |

## Leaf ι — datum-anchored zones (rung 4)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Typed datums (`Direction` etc.) | producer:task-4382 upstream | out-of-batch dep edge ι←4382 (pending, `geometric-relations.md` α) — real `add_dependency` edge; scheduler holds ι | PASS (upstream) |
| Feature→datum projections (planar face→Plane etc.) | producer:task-4385 upstream | out-of-batch dep edge ι←4385 (pending, `geometric-relations.md` ε) | PASS (upstream) |
| Zone constructors + conformance pass to anchor | producer:γ,η upstream | intra-batch dep edges ι←γ,η | PASS (upstream) |
| `datum_refs` slot exists to retarget | producer:α upstream | α makes runout `datum_refs` required; shipped `datum_refs : Geometry` slots (4 sites, `tolerancing.ri`) — transitively via γ←α | PASS (upstream) |

## Leaf κ — DRF ordering + DOF-arrest diagnostics (rung 4)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| DOF ledger to consume | producer:task-4388 upstream | out-of-batch dep edge κ←4388 (pending, `geometric-relations.md` θ) — real edge; re-verify ledger API shape at κ dispatch (rung-4 caveat, PRD §3) | PASS (upstream) |
| Datum-anchored zones to order | producer:ι upstream | intra-batch dep edge κ←ι | PASS (upstream) |
| Diagnostic surface for DRF-incompleteness | wired-on-main | `reify-core/src/diagnostics.rs` registry (same mechanism as β; β's codes land first by transitivity κ←ι←η←β) | PASS |

## BM — bookmark: measured-feature import (filed deferred)

No capability bindings — genuine forward-stub gate, no implementation. Points at
`docs/prds/v0_6/gdt-measured-feature-import.md` (committed `28157c5753`); names η as its consumer
seam and #4290 (PointCloud, deferred) as substrate. Dep edges BM←η (intra) + BM←4290 record the
gating; the task stays `deferred` per the bookmark pattern (promotion = a fresh `/prd` pass on the
stub).

---

## Cross-batch notes

- **#4269 closure**: this batch's γ+δ supersede the deferred zone-REGION task; decompose cancels
  4269 `cancelled-superseded` citing γ/δ IDs (PRD §7 row 1).
- **No FAIL bindings.** Nothing re-scoped, re-homed, or bound-relaxed at decompose time; all PRD §10
  manifest-binding seeds re-verified against main `28157c5753` unchanged.
