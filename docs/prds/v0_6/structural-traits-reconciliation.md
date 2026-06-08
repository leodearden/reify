# PRD: `std.structural` §4 trait-shape reconciliation

**Milestone:** v0.6
**Cluster:** gap-register `P10 structural-traits` (4 gaps, 2 high) — `docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md`
**Source doc:** `docs/reify-stdlib-reference.md` §4 (`std.structural`)
**Shipped source:** `crates/reify-compiler/stdlib/structural_physical.ri`
**Supersedes:** task **#3114** (deferred — "Tighten structural_physical.ri dimensioned params"; its stale `volume`/`centroid` clause is corrected here).

---

## 0. What this PRD is (and is not)

The structural-traits survey flagged §4 as four "missing feature" gaps depending on
fields-api + geometry mass-properties. **Investigation overturned that framing.** The
shipped trait shapes are *deliberate* (trait-breadth audit `findings/stdlib-trait-breadth.md`
M-003/M-004/M-012; Real-placeholder audit `docs/notes/stdlib-real-placeholder-audit.md`
task-D), and the two documented "rich" forms have **no consumer**:

- **Flexible.stiffness_model : `Field<Point3<Length>, Tensor<2,3,Pressure>>`** — parses
  *and* type-resolves (grammar-confirmed; `type_resolution.rs:1419`), but nothing samples
  a body-level spatially-varying stiffness tensor. FEA reads elastic moduli from the
  `material : MaterialSpec` slot (`youngs_modulus`/`poissons_ratio`), not a `Flexible`
  param. The shipped trait carries a lumped `stiffness : Stiffness` + `max_deflection`.
  → **G1 orphan.**
- **Rigid.moment_of_inertia** as an auto-derived `let moment_of_inertia = moment_of_inertia(geometry, material.density)`
  — the kernel builtin it would call is **unwired** (`dynamics_ops.rs:223,287`, returns
  `Undef`, task 3620 TODO) and returns a `Tensor<2,3,MomentOfInertia>`, not the
  doc-implied scalar. Auto-deriving it now would surface `Undef`. → **G6 field-population
  failure.**

So this is **not** a feature-build. It is a **reconcile + tighten** PRD:

1. **Tighten** the `Real` placeholders in `structural_physical.ri` to the dimensioned
   aliases the resolver already supports (the live half of #3114).
2. **Reconcile** `docs/reify-stdlib-reference.md` §4 to the shipped + tightened shapes.
3. **Defer** the two rich forms behind tracked, consumer/substrate-gated follow-ups
   (filed, parked; not built here).

The claimed `fields-api` and `geometry-mass-properties` dependencies **dissolve** under
this disposition (decided 2026-06-02).

---

## 1. Consumer + user-observable surface (G1)

The one mechanism this PRD introduces is **dimensioned typing of the §4 trait members**.
Named consumers:

- **The dimensional type-checker** (`reify check`). A structure conforming to a tightened
  trait with a correctly-dimensioned value type-checks; a wrong-dimension value is
  rejected with a dimensional-mismatch diagnostic. User-observable via the CLI.
- **A stdlib `.ri` conformance example that runs in CI** — `examples/structural_traits_dimensioned.ri`
  (new), exercising Rigid/Flexible/ElasticallyDeformable/Plastic/Sealed/ThermallyConductive
  with dimensioned literals.
- **The existing conformance suite** `crates/reify-compiler/tests/structural_physical_tests.rs`
  (updated to assign dimensioned values; stays green).

The doc-reconcile (β) introduces no runtime mechanism — it aligns documentation with the
shipped+tightened reality; its consumer is the reader and the in-GUI assistant reference chain.

---

## 2. Sketch of approach

### 2.1 Type-tightening (`structural_physical.ri`) — supersedes #3114

Five live `tightenable-now` members (the resolver already registers each alias —
`dimension.rs:362-393`, exercised today in `constitutive.ri`/`materials_fea.ri`):

| Trait | Member | `Real` → | Constraint RHS becomes |
|---|---|---|---|
| `Rigid` | `moment_of_inertia` | `MomentOfInertia` | `> 0.0 * 1kg * 1m * 1m` |
| `Flexible` | `max_deflection` | `Length` | `> 0.0 * 1m` |
| `Plastic` | `hardening_modulus` | `Pressure` | `> 0.0 * 1Pa` |
| `ThermallyConductive` | `max_service_temp` | `Temperature` | `> 0.0 * 1K` |
| `Sealed` | `seal_pressure_rating` | `Pressure` | `> 0.0 * 1Pa` |

- Each constraint RHS must be a **dimensioned literal** (bare `0` evaluates to
  `Indeterminate` at runtime — `eval_cmp` compares dimensions; esc-3115-112). Mirror the
  existing in-file pattern (`stiffness > 0.0 * 1N / 1m`). All five tightened bodies +
  dimensioned RHSs are **grammar-confirmed** (`/tmp/prd-gate-fixtures/structural-tighten.ri`,
  `tree-sitter parse --quiet` exit 0).
- **Unchanged:** `max_elastic_strain`, `plastic_strain` (genuine-dimensionless — stay
  `Real`); `stiffness`, `thermal_conductivity`, `electrical_conductivity`, `resistivity`
  (already tightened to named-dim aliases by #3115).
- **Dropped from #3114's scope:** `volume`/`centroid_x/y/z → Volume/Length`. Those flat
  params were removed by the geometry-handle-runtime rewrite (tasks 3603/3608); `Physical`
  now carries `geometry : Solid` + computed `let mass`/`let centroid`.
- **Cascade:** update conforming structures in `examples/` (e.g. `large_assembly.ri`) and
  the test fixtures to use dimensioned literals; `reify check` + `cargo test -p reify-compiler` green.

### 2.2 Doc-reconcile (`docs/reify-stdlib-reference.md` §4)

Rewrite the §4 code block to the shipped + tightened canonical shapes (see §4 table).
Closes, in one pass, all the §4 doc-drifts the survey surfaced:

- `Flexible : Physical` → standalone `Flexible` (the `: Physical` edge was deliberately
  removed, tasks 2410/2349 — gap-register Bucket-B, previously untracked).
- field-of-tensor `stiffness_model` → lumped `stiffness : Stiffness` + `max_deflection : Length`.
- auto-derived `let moment_of_inertia` → `param moment_of_inertia : MomentOfInertia`
  (with a note that geometry-derived MOI via the `moment_of_inertia(solid, density)`
  *query builtin* is a separate facility, deferred for auto-binding — §5).
- `Plastic.yield_point : Pressure` → removed; shipped `plastic_strain` + `hardening_modulus : Pressure`.
- `Sealed.seal_rating : Pressure` → `seal_pressure_rating : Pressure`.

---

## 3. Pre-conditions for activating

- **Substrate confirmed (G3 — all PASS, no prerequisite work):**
  - `MomentOfInertia`, `Pressure`, `Length`, `Temperature` resolve as `.ri` aliases
    (`dimension.rs:362-393`; live use in `constitutive.ri:92`, `materials_fea.ri:89`).
  - Dimensioned constraint-RHS forms parse (grammar gate, §2.1).
- **Supersedes #3114** — cancel #3114 as superseded by task α at decompose time.

No blocked-on-consumer condition for the active batch (α/β). The deferred follow-ups
(γ/δ) name their own gating substrate (§5).

---

## 4. Resolved design decisions

| # | Decision | Resolution (2026-06-02) |
|---|---|---|
| D1 | Disposition of the two never-shipped rich forms (Flexible field model, auto-derived MOI) | **Reconcile down + defer.** Document the shipped lumped/param shapes as canonical; file tracked, gated follow-ups for the rich forms; do not build them here. |
| D2 | `Plastic.yield_point` | **Reconcile away.** Yield strength is a material property (`materials_mechanical.Strong.yield_strength`, `Analysis.yield_strength`); no body-level consumer. Doc → shipped `plastic_strain` + `hardening_modulus`. |
| D3 | `Sealed` member name | Keep shipped `seal_pressure_rating` (more descriptive); reconcile the doc, not the code. |
| D4 | `Rigid.moment_of_inertia` form | Keep `param` (not auto-`let`); tighten `Real → MomentOfInertia`. Auto-derivation deferred (§5, δ). |
| D5 | Dimensionless members | `max_elastic_strain`, `plastic_strain` stay `Real` (genuine-dimensionless). |

### Canonical §4 shapes (target of both α and β)

```
trait Physical { param geometry : Solid; param material : Material
                 let mass = volume(geometry) * material.density
                 let centroid = centroid(geometry); constraint material.density > 0 }
trait Rigid : Physical { param moment_of_inertia : MomentOfInertia; constraint moment_of_inertia > 0… }
trait Flexible { param stiffness : Stiffness; param max_deflection : Length; constraint stiffness > 0…; constraint max_deflection > 0… }
trait ElasticallyDeformable : Flexible { param max_elastic_strain : Real; constraint max_elastic_strain > 0 }
trait Plastic : Flexible { param plastic_strain : Real; param hardening_modulus : Pressure; constraint hardening_modulus > 0…; constraint plastic_strain >= 0 }
trait ThermallyConductive : Physical { param thermal_conductivity : ThermalConductivity; param max_service_temp : Temperature; constraint thermal_conductivity > 0… }
trait ElectricallyConductive : Physical { param electrical_conductivity : ElectricalConductivity; param resistivity : ElectricResistivity; constraint electrical_conductivity > 0… }
trait Sealed { param seal_pressure_rating : Pressure; constraint seal_pressure_rating > 0… }
```
(`…` = dimensioned RHS per §2.1.)

---

## 5. Out of scope / deferred future work

Filed as **tracked, parked (deferred)** follow-up tasks (D1) — not built in this PRD:

- **γ — Flexible continuum stiffness-tensor field.** Add `Field<Point3<Length>, Tensor<2,3,Pressure>>`
  stiffness model. **Gated on a named FEA/field consumer** that samples a body-level
  spatially-varying stiffness tensor. Until such a consumer exists, this is a G1 orphan;
  do not add the param.
- **δ — Auto-derive Rigid.moment_of_inertia from geometry.** Convert the `param` to a
  geometry-derived `let`. **Gated on** (a) wiring the `moment_of_inertia(Solid, Density)`
  kernel seam (task 3620 / KGQ-λ, currently `Undef`) and (b) reconciling the
  scalar-vs-`Tensor<2,3,MomentOfInertia>` shape.

**Sibling clusters (NOT this PRD):** the other parked #3090 audit follow-ups —
#3111 (`materials_mechanical.ri`), #3112 (`materials_thermal.ri`), #3113
(`materials_optical.ri`) belong to **P12 materials-breadth**; #3116 (`tolerancing.ri`
Geometry/DatumRef) to **P13 tolerancing**. They share the tightenable-now pattern but are
owned by their own gap clusters/PRDs.

---

## 6. Cross-PRD relationship

| Other PRD / task | Direction | Seam | Owner | Status |
|---|---|---|---|---|
| `std-fields-api.md` (P16) | (was claimed) consumes | Flexible field model | — | **dissolved** — field model deferred (γ); no active dep |
| task 3620 / KGQ-λ MOI kernel seam | deferred-consumes | `moment_of_inertia(Solid,Density)` | task 3620 | referenced only by deferred δ |
| (future) FEA spatial-stiffness consumer | deferred-consumes | Flexible stiffness field | future PRD | referenced only by deferred γ |

No active cross-PRD seam; no reciprocal-ownership ambiguity.

---

## 7. Decomposition plan

**Approach: bare B** (G5). Blast radius 1 crate (`reify-compiler` stdlib + tests + examples);
not a load-bearing seam (not FEA/ComputeNode/parser/persistent-naming/multi-kernel);
2 active mechanisms. No B+H contract needed.

### Active batch (flip → pending)

- **α — Tighten `structural_physical.ri` §4 dimensioned params** (supersedes #3114).
  - *Signal:* `examples/structural_traits_dimensioned.ri` (new) conforms structures to
    the five tightened traits with dimensioned values and `reify check` passes (exit 0,
    no diagnostics); a wrong-dimension assignment (negative fixture) is rejected with a
    dimensional-mismatch diagnostic; `cargo test -p reify-compiler` green.
  - *Consumer:* dimensional type-checker / CI example.
  - *Leaf.*
- **β — Reconcile `docs/reify-stdlib-reference.md` §4 to the shipped trait shapes** (depends on α).
  - *Signal:* §4 code block matches `structural_physical.ri` member-for-member (names +
    dimensioned types); the stale `Flexible : Physical`, `stiffness_model` field,
    auto-derived-`let` MOI, `yield_point`, and `seal_rating` claims are all gone.
  - *Consumer:* doc reader / GUI assistant reference.
  - *Leaf (reconcile finalizer).*

### Deferred follow-ups (filed; left `deferred`, not flipped)

- **γ — Flexible continuum stiffness-tensor field** (gated on FEA consumer — §5).
- **δ — Auto-derive Rigid.moment_of_inertia** (gated on MOI kernel seam 3620 + shape reconcile — §5).

---

## 8. Open (tactical) questions

- Exact dimensioned-literal spelling for each constraint RHS (`0.0 * 1kg * 1m * 1m` vs a
  `kg*m^2`-style compound, etc.) — implementer's choice; all candidate forms parse.
- Whether to extend an existing conformance example (`large_assembly.ri`,
  `drivebelt_trait_bounds.ri`) or add a dedicated `structural_traits_dimensioned.ri`.
  Recommend the dedicated example for a focused CI signal.
- The exact dimensional-mismatch diagnostic code emitted on the negative fixture (confirm
  at implementation; the positive `reify check` pass is the load-bearing signal).
