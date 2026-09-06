# Dimension-checked native readers: stdlib/solver reader chokepoints, load-extraction integrity

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-07-28 · **Approach: B + H**
(contract + two-way boundary tests; blast radius spans `reify-ir` / `reify-core` /
`reify-stdlib` / `reify-eval` / `reify-expr` / stdlib `.ri` / examples, and FEA is an
overlay-listed load-bearing seam).

**Program:** PRD 5 of five in the units-gating program. **Canonical evidence:**
`docs/notes/units-gating-gap-research-2026-07-28.md` — PHASE 2 sub-classes 2 (stdlib/native
readers) and 3 (solver inversions + dead wiring). Every anchor cited below was **re-verified
against `main` at `dc83d4fd60` (2026-07-28)**; drift and premise corrections are recorded in
§2.4. Ratified program decisions (Leo, 2026-07-28) — hard-REJECT bare numbers at dimensioned
positions, strict `DimensionVector` equality everywhere, eval = soundness / compile slots =
UX — are recorded, not re-opened.

**Normative substrate:** `docs/legibility/design-invariants.md`. This PRD is bound by
**INV-SF-1** (`undef-has-provenance`), **INV-SF-2** (`error-severity-exits-nonzero`),
**INV-SF-3** (`declared-intent-consumed-or-diagnosed`), **INV-SF-5**
(`placeholders-owned-and-loud`) and **INV-SF-6** (`diagnostics-carry-codes`).

---

## 1. Goal

Reify's dimension safety lives in **expressions** (arithmetic is strict) — not at
**value-construction** boundaries and not at **native-reader** boundaries. Roughly 50 reader
sites in `reify-stdlib` and `reify-eval` pull an `f64` out of a `Value` with no dimension
check at all, and the FEA solvers disagree with each other about what a load even is. After
this PRD lands:

1. `reify eval` on a design whose material carries `youngs_modulus: 200mm` **exits 1** with a
   coded diagnostic naming the builtin and the field — where today `prb_cantilever_beam`
   returns a spring rate that is **exactly 1e12× wrong**, with zero diagnostics
   (`flexures/common.rs:109` discards `dimension` via `Value::Scalar { si_value, .. }`).
2. `PointLoad(force: 5000N)` — the **units-correct** spelling — contributes the same 5000 N in
   `solve_elastic_static` as it already does in `solve_buckling`. Today it contributes
   **exactly zero** in the former (`elastic_static.rs:4029` matches `Some(Value::Real(f))`
   only) and 5000 N in the latter (`buckling.rs:726/:730` accepts both) — same source text,
   different physics per solver, no diagnostic on either side.
3. A `TractionLoad` or `BodyForce` in a `loads` list **exits 1** naming the unsupported load
   kind — where today `extract_loads` has **zero occurrences** of either type name and drops
   them silently (INV-SF-3: a declaration structurally incapable of being consumed must be
   diagnosed, never a silent no-op).
4. `ZVShaper(target_frequency: 50rad/s)` **exits 1** instead of silently applying a 6.28×
   error (the reader multiplies by 2π on the assumption the input is Hz).
5. `ElasticOptions(shell_voxel_size: 0.5mm)` actually changes the medial-axis tolerance —
   today the override is declared `Option<Length>` (`solver_elastic.ri:292`) and read as
   `Value::Real` (`shell_extract_compute.rs:550`), so it is **silently discarded**.
6. Every rejection is one `Severity::Error` `Diagnostic` carrying a `DiagnosticCode`, formatted
   from the **single** shared `ArgRejection::message` template already used by Contracts A/B/C
   — so `reify eval` exits nonzero (INV-SF-2) and the failure is filterable (INV-SF-6).

**Consumers (G1).** (a) `.ri` authors of flexure / joint / trajectory / FEA designs — they get
diagnostics and correct physics instead of plausible wrong numbers; (b) the solvers themselves
— `solve_elastic_static` and `solve_buckling` become one reader, so a load written once means
one thing; (c) **PRD 4** (`dimensioned-construction-strictness`) — the load-struct fields this
PRD retypes to `Force`/`Pressure` are exactly the ctor slots PRD 4's strict-equality promotion
enforces; (d) **PRD 1**'s closure-guard harness, extended here with its second probe universe.

---

## 2. Background (anchor-verified on `main` 2026-07-28)

### 2.1 The reader surface

Thirteen chokepoint helpers plus three direct-`as_f64` sites carry the defect. Full inventory,
per-site expected dimensions, and call-site tables are in the research note; the load-bearing
shape:

| Helper | Anchor | Dimension check | Worst consequence |
|---|---|---|---|
| `scalar_si` / `material_field_si` / `material_numeric_field` | `reify-stdlib/src/flexures/common.rs:109 / :126 / :146` | **none** (`Scalar { si_value, .. }`) | `youngs_modulus`/`yield_stress` read at **12 sites**; `200mm` as E ⇒ 1e12× |
| `length_si` | `flexures/common.rs:95` | LENGTH ✓, bare `Real`/`Int` accepted as metres | `prb_cantilever_beam(20, …)` ⇒ 20 m, 1e9× on spring rate |
| `length_input` | `reify-stdlib/src/joints.rs:1181` | LENGTH ✓, bare hole | screw `lead` `:587`, rack `pitch_radius` `:660`, couple `offset` `:539`, prismatic displacement `:1763` |
| `cell_f64` ×3 (byte-identical) | `reify-stdlib/src/dynamics/eval.rs:55`, `reify-eval/src/dynamics_ops.rs:49`, `reify-eval/src/dynamics_psd.rs:56` | none | `mass` read at `dynamics/eval.rs:354` through the blind copy **while `cell_mass_f64` at `:78` enforces MASS in the same file** |
| `read_scalar_si` / `field_f64` ×2 (byte-identical) + 1 variant | `trajectory/input_shape.rs:51/:62`, `trajectory/trampoline.rs:139/:831`, `reify-eval/src/modal_ops.rs:2553` | none; `field_f64` **substitutes a default**, `modal_ops` floors to `0.0` | `target_frequency` 6.28×; waypoint `t`, `velocity_limit`, `force_limit` all blind |
| `jointvalue_from_bound_value` | `reify-stdlib/src/loop_closure.rs:691` | none (`as_f64()` ×5) | `[angle, length]` swapped on a cylindrical joint is silently accepted here while `joints.rs:1224` rejects it |
| `envelope_critical_load` | `reify-stdlib/src/fea.rs:1455` | none; **propagates** `args[1]`'s dimension verbatim | a `reference_load` of `10mm` yields a "critical load" in metres; pinned as intended by `fea.rs:6502` |
| `field_scalar` / `opt_f64` | `reify-eval/src/compute_targets/as_printed_material.rs:237 / :317` | none | `layer_height`, `line_width`, `youngs_modulus`, `density`, `ex/ey/ez/gxy` |
| `safety_factor` | `reify-stdlib/src/analysis.rs:271` (+ the Field path at `reify-expr/src/lib.rs:394`) | none (`as_f64()`) | `safety_factor(σ_Pa, 250mm)` succeeds silently |

The **correct** reader already exists — `reify-stdlib/src/helpers.rs:229`
`validate_dimensioned_scalar` (Scalar + exact dimension + finite) — and is used at exactly
**6 production sites in 3 files** (`loads.rs:161`, `stackup.rs:32`, `tolerancing.rs:88/:89/:148/:160`).
`arg_acceptance::accept_arg` (`reify-eval/src/arg_acceptance.rs:117`) is the eval-side twin and
is imported by **zero** files under `compute_targets/**`.

### 2.2 The solver inversions

`extract_loads` (`reify-eval/src/compute_targets/elastic_static.rs:4013-4085`) dispatches
exactly three `type_name` arms and applies **four mutually inconsistent value-shape gates**:

| Site | Accepts `Real` | Accepts `Scalar` | Diagnostic on reject |
|---|---|---|---|
| `:4029` `PointLoad.force` | yes | **no** | **none** |
| `:4039` `PointLoad.direction` | yes | yes | none |
| `:4155` `PressureLoad.magnitude` | yes | yes | none (`return None`) |
| `:4059` `Gravity.magnitude` | yes | yes | none (`continue`) |
| `:3985` `extract_density` | **no** | yes | none (→ `0.0`, pinned by `:8240`) |
| `buckling.rs:726/:730` `force` | yes | yes, **and no `type_name` guard at all** | none (→ `1.0` N sentinel, `:741`) |

The only partial cover is the downstream `FeaFailure::NoLoads` Warning
(`elastic_static.rs:613`), which fires **only if the dropped load was the sole load** — a
mixed scene `[PointLoad(force: 5000N), Gravity()]` loses the 5000 N in complete silence.
`buckling.rs`'s sentinel comment defers its diagnostic to "task θ/3457" — **task 3457 is
`done`**, so that deferral is an orphaned pointer to a closed task.

### 2.3 The `.ri` side is inversely coupled

All six load structures live in `crates/reify-compiler/stdlib/fea_multi_case.ri`:
`PointLoad:315` (`force : Real = 0.0` at `:317`), `PressureLoad:418` (`magnitude : Real` at
`:419`), `TractionLoad:446` (`traction : Real` at `:448`), `BodyForce:476`
(`force_density : Real` at `:478`), `Gravity:515` (`magnitude : Acceleration` at `:516` — the
**only** dimensioned load, and correspondingly the only arm that accepts a `Scalar`),
`LoadCase:70`. `Real` resolves to `Type::dimensionless_scalar()`
(`type_resolution.rs:592`), so the retype and the reader fix are inversely coupled: retyping
alone makes every existing call site a ctor mismatch, and fixing the readers alone leaves the
declared type lying.

**Measured migration blast radius: 90 in-scope constructor sites** (78 `PointLoad` + 14
`PressureLoad` − 4 locally-shadowed `PressureLoad` defs in
`struct_ctor_field_conformance_tests.rs` + 2 `TractionLoad`), split 35 `.ri` / 77 `.rs`
fixtures; **zero** of them pass a dimensioned literal today. `Gravity`'s 16 sites are already
dimensioned. The precedent that the target type works end-to-end is `modal_analysis.ri:483`
`StepForce { param magnitude : Force }`, called as `magnitude: 10N` in
`examples/modal/transient_step_response.ri:105`.

### 2.4 Premise corrections to the research note (re-verified; the note is wrong on three counts)

1. **`cte` does not exist.** No `cte` identifier is declared anywhere. The real field is
   `thermal_expansion : ThermalExpansion` (`materials_thermal.ri:41`).
2. **`yield_stress` is NOT declared-only.** It has **7 production readers** in
   `reify-stdlib/src/flexures/` (`beam.rs:89`, `hinge.rs:98`/`:252`, `compound.rs:102`/`:281`,
   `notch.rs:135`, `prismatic.rs:110`), all above their files' `#[cfg(test)]` boundary. The
   note's "zero consumers" claim is true only when scoped to `compute_targets/**`.
   Genuinely declared-only (zero production readers repo-wide): **`shear_modulus`**
   (`materials_mechanical.ri:100`) and **`thermal_expansion`** (`materials_thermal.ri:41`).
   **`thermal_conductivity` is not one of them** (corrected by ο): the name is declared at two
   independent sites that differ. `ThermallyConductive.thermal_conductivity`
   (`structural_physical.ri:148`) has a **live DSL reader** — `structural_physical.ri:150`
   carries `constraint thermal_conductivity > 0W/(m*K)` — so for that site the zero-reader
   claim holds only when scoped to **Rust/host** readers.
   `ThermallyCharacterized.thermal_conductivity` (`materials_thermal.ri:39`) is a separate
   param that merely shares the name and has no reader of its own. The declared-only register
   in `docs/reify-stdlib-reference.md` §6.3 is the canonical record of both sites.
3. **The four fields are not on `Material`.** `Material` (`materials_mechanical.ri:82`) carries
   only `name` / `density : Density` / `youngs_modulus : Pressure` / `appearance`. The rest live
   on the `ElasticMaterial` / `Elastic` / `ThermallyCharacterized` / `ThermallyConductive`
   traits.

Two further corrections, both confirming the note: `stackup.rs:884` and `tolerancing.rs:229`
`scalar_si` are **test-only, strict, panicking** helpers — the *opposite* of the defect; they
are not among the 13. And `docs/legibility/design-invariants.md` runs **INV-SF-1..7**, not
1..6 (its own header at `:11` is stale).

**Zero line drift** was found in the reader or solver clusters: every anchor the research note
cited (`common.rs` 95/109/126/146, `joints.rs` 1181/1755/587/660/539/1763, `dynamics/eval.rs:55`,
`dynamics_ops.rs:49`, `dynamics_psd.rs:56`, `input_shape.rs:51/62`, `trampoline.rs:139/831`,
`modal_ops.rs:2553`, `loop_closure.rs:691`, `fea.rs:1455`, `as_printed_material.rs:237/317`,
`helpers.rs:229`, `elastic_static.rs:3884/3979/4013/4029`, `buckling.rs:714/730`,
`shell_extract_compute.rs:550`, `fea_multi_case.ri:315`) is exact at `dc83d4fd60`. Three
**in-source** doc citations are stale and get corrected where touched:
`flexures/common.rs:137` cites `modal_ops.rs:839` (real: `:2553`), `reify-stdlib/src/lib.rs`
cites `reify-expr/src/lib.rs:475` for `emit_dfm_diagnostics` (real: `:607`),
`loop_closure.rs:684` cites `:399` (real: `:416`).

---

## 3. Sketch of approach

Six legs. Legs A–D are the soundness work; E is the guard; F is the documented surface.

### Leg A — one shared acceptance family, reachable from both crates

`arg_acceptance` today lives at `reify-eval/src/arg_acceptance.rs` and is `pub(crate)`
(`reify-eval/src/lib.rs:93`). **`reify-stdlib` does not and cannot depend on `reify-eval`**
(the edge runs the other way — `reify-eval/Cargo.toml` normal-deps `reify-stdlib`), so the
flexure / joint / trajectory / loop-closure / fea readers cannot reach it. Symmetrically,
`reify-stdlib`'s `helpers.rs:229` `validate_dimensioned_scalar` is `pub(crate)` and unreachable
from `reify-eval`.

Resolution: **relocate the module verbatim to `reify-ir`** (`reify_ir::arg_acceptance` — the
lowest crate that owns `Value`, and it already depends on `reify-core` for `DimensionVector`),
and `pub use` it from `reify-eval` at its existing path so every current call site and both
sibling PRDs' additive constructors are untouched. `accept_arg` / `ArgSpec` / `Acceptance` /
`ArgRejection` semantics are **FROZEN** by the program's seam table and are not modified — this
is a move plus additive constructors, not a redesign. `helpers.rs`'s
`validate_dimensioned_scalar` becomes a thin adapter over `accept_arg`, so the repo has **one**
dimension-acceptance rule and **one** rejection wording.

### Leg B — reader chokepoint adoption, with per-argument triage

Every chokepoint in §2.1 routes through the shared family with an explicit per-position
`ArgSpec`. The triage is per-site, not per-helper (§6 decision 4 records the rule; the full
positive/negative lists are in the leaf bodies):

- **Gated** (dimensioned): `youngs_modulus`/`yield_stress`/`shear_modulus`/`ex,ey,ez,gxy`/
  `yield_val` → PRESSURE · `density` → MASS_DENSITY · flexure & joint lengths, `com`,
  `layer_height`, `line_width`, `shell_voxel_size` → LENGTH · `mass` → MASS · inertia cells →
  MOMENT_OF_INERTIA · `spring_rate` → STIFFNESS **or** ROTATIONAL_STIFFNESS by joint kind
  (distinct vectors) · `damping` → TRANSLATIONAL_DAMPING / ROTATIONAL_DAMPING ·
  `target_frequency` → FREQUENCY · waypoint `t` → TIME · `velocity_limit` → VELOCITY ·
  `acceleration_limit`/`max_accel` → ACCELERATION · `force_limit`, `reference_load`,
  `PointLoad.force` → FORCE · `PressureLoad.magnitude`, `TractionLoad.traction` → PRESSURE ·
  `BodyForce.force_density` → FORCE_DENSITY.
- **Deliberately bare** (stay dimensionless-accepting, gated to `dimensionless_spec` so a
  *dimensioned* Scalar is still rejected): `poisson_ratio`, `damping_ratio`,
  `vibration_tolerance`, `tol`, `max_iters`, `Gravity.direction` / `PointLoad.direction`
  (`List<Real>` unit vectors, deliberate per task 4439), `vec3` axis components (already
  strict via `validate_dimensionless_unit_axis_vec3`), joint `ratio` (already
  DIMENSIONLESS-checked by `ratio_input`), the buckling eigenvalue λ,
  `infill_gibson_ashby_c/n`, `read_location_index`, and tensegrity's bare `List<Real>`
  force/ratio inputs — `form_find`'s `force_densities` parameter, `form_find_free`'s
  `seed_ratios` parameter, and both functions' `surface_stresses` parameter
  (illustrative, not an exhaustive list of every such site) — nullity-invariant
  relative ratios, documented as such in the "Dimensional bridge" paragraph of
  `tensegrity.ri`'s `FormFindResult` doc block, which also covers `FormFindResult`'s
  own `force_densities` field (a solver-constructed *output* echo, not a reader
  input); genuinely dimensionless, not a gap.
- **Angle-semantic positions** (`revolute` binds, planar/cylindrical θ, `ramp_profile`
  from/to on a revolute) route through `reify-stdlib`'s existing ANGLE-checked `trig_input`.
  This PRD changes **no angle policy** — it replaces ad-hoc `as_f64()` with the helper that
  already encodes today's convention. Retiring `trig_input`'s bare-`Real`-radians arm is
  **PRD 3's**, and lands in one place once this leg has funnelled the sites into it.

The three `cell_f64` copies and the three `read_scalar_si`/`field_f64` copies are
**consolidated to one each while touched** (the project's ≥2-duplicate hoist norm; two of each
trio are byte-identical, verified by `diff`).

### Leg C — solver load integrity, landed in three green steps

The corpus-first landing shape (`real-dimensionless-unification.md:64` template) keeps the
workspace green at every step:

1. **Widen** `extract_loads`/`extract_total_load` to accept correctly-dimensioned Scalars in
   addition to bare `Real`, add buckling's missing `type_name == "PointLoad"` guard, and make
   both solvers read through the same helper. The inversion closes here.
2. **Retype + migrate**: `PointLoad.force → Force`, `PressureLoad.magnitude → Pressure`,
   `TractionLoad.traction → Pressure`, `BodyForce.force_density → ForceDensity`, and migrate
   all 90 in-scope call sites.
3. **Narrow**: bare `Real` at a dimensioned load slot becomes a coded `Severity::Error`
   rejection; `extract_density`'s bare→`0.0` default, buckling's `1.0` N sentinel, and
   `extract_loads`' silent-skip of unrecognised `type_name` all become named diagnostics.
   `TractionLoad`/`BodyForce` get an explicit **`E_FeaLoadKindUnsupported`** rejection rather
   than a wire-up (§6 decision 6).

`shell_voxel_size` is a double miss — the read fails on **both** the `Option` wrapper and the
`Scalar` variant, and there is **not one `shell_voxel_size:` override anywhere in the repo**,
so no test could have caught it. The correct idiom already exists two files over
(`elastic_static.rs:4441`, `threads : Option<Int>`).

### Leg D — language-surface reads

`min`/`max` (`reify-stdlib/src/numeric.rs:46-87`) fall through to blind `as_f64()` on any
dimension mismatch, so `min(10mm, 5kg)` yields `Real(0.005)`. Inverse trig
(`trig.rs:20-40`: `asin`/`acos`/`atan`/`atan2`/`sinh`/`cosh`/`tanh`) takes any-dimension
Scalar as the ratio via `unary_f64`/`binary_f64`. Both become rejections keyed on strict
equality, treating `Real`/`Int` as DIMENSIONLESS so `min(3, 7.0)` is unaffected. Measured
corpus exposure: **10 non-comment `min`/`max` uses in `.ri`**, all unary field reductions or
same-dimension; inverse trig is used only on genuinely dimensionless arguments
(`examples/math_linalg.ri:49`, `examples/kernel_queries/angle_smoke.ri:7`).

### Leg E — closure guard, second universe

PRD 1 builds the source-text-driven behavioural probe over the compiler's
`GEOMETRY_FUNCTION_NAMES`. That universe **cannot see this PRD's surface**: `plane_xy`/
`plane_yz`/`axis_*`/`point3` and every `prb_*` / joint / FEA / trajectory builtin are pure
`reify-stdlib` `eval_builtin` names with no compiler signature and no `.ri` declaration
(`reify-stdlib/src/geometry.rs:820` is the whole of `plane_yz`). This PRD adds the **second
probe universe**, additively — no harness rewrite.

### Leg F — the documented surface

Four docs-truth leaves (the overlay's gate), plus the declared-only register. The FEA/flexure
surface has **zero** presence in `crates/reify-mcp/src/tools/chunks/*.md` today (no chunk
mentions `prb_cantilever_beam`, `PointLoad` or `solve_elastic_static`), and
`docs/reify-stdlib-reference.md:979-1047` still documents `Material.density`/`youngs_modulus`
as `Real` "pending #3111" with nine per-line `// shipped: Real (aspiration pending #3111)`
annotations — **#3111 is `done` and the live declarations are `Density`/`Pressure`**
(~12 mismatches, listed in the leaf).

---

## 4. Pre-conditions (G3 — verified on `main` 2026-07-28)

**No novel grammar.** Every dimension name this PRD uses as a `.ri` param type already
resolves: `Pressure`, `Force`, `Mass`, `Frequency`, `Time`, `Velocity`, `Acceleration`,
`Stiffness`, `RotationalStiffness`, `Density`, `MomentOfInertia`, `ForceDensity`,
`ThermalConductivity`, `Energy`, `Area`, `Volume`, `Temperature` are all in `NAMED_DIMENSIONS`
with `DimensionVector` consts at `reify-core/src/dimension.rs:120-293`. (`Torque` is absent —
that is **PRD 3's** deliverable and this PRD does not use it.) `StepForce`'s
`param magnitude : Force` + `magnitude: 10N` proves the end-to-end path today.

**The diagnostic substrate exists and is wired.** A `reify-stdlib` builtin cannot emit
directly (`eval_builtin(name, args) -> Value`, `lib.rs:225`, no sink; there is no thread-local
anywhere in `reify-stdlib`/`reify-expr`). The house pattern is a **pure post-hoc classifier**
`diagnose(name, args[, result]) -> Option<Diagnostic> | Vec<Diagnostic>`, dispatched from
`reify-expr`'s `FunctionCall` arm into `EvalContext::diagnostics`
(`reify-expr/src/lib.rs:68` sink; `:589` `emit_undef_builtin_diagnostics` →
`:1965`; both-path hooks at `:599`/`:607`/`:613`). **Nine classifiers are registered today.**
The exact end-to-end exemplar is already a dimension rejection:
`stackup.rs:31 len_scalar` → `StackupError::DimMismatch` (`:194`) →
`Diagnostic::error("E_StackupDimMismatch: …").with_code(DiagnosticCode::StackupDimMismatch)`
(`:246`) → `reify-expr/src/lib.rs:1973`. `reify-eval` compute targets use the parallel
`ComputeOutcome::Failed { diagnostics, structured_detail }` channel
(`reify-compute-contract/src/dispatch.rs:64`), already the house severity policy per esc-2929-40
("Error → `ComputeOutcome::Failed`").

**Two live constraints this imposes** (design content, not blockers):
`emit_undef_builtin_diagnostics` is gated on `result == Value::Undef`, so any reader that
*coerces* rather than rejects (`field_f64`'s `unwrap_or(default)`, `modal_ops`' `0.0` floor)
is **unreachable** by that hook and must be changed to return `Undef`; and the classifier
re-derives the failure from `(name, args)` after the fact, so the dimension expectation must
live in **one** shared table consulted by both the reader and the classifier (§7).

**`reify eval` exit gate exists**: `cmd_eval` (`reify-cli/src/main.rs:1482`) fails on any
`Severity::Error` diagnostic (`:1545`). Every leaf signal here is phrased against
`reify eval` — **PRD 2 owns `reify check` semantics**.

**Upstream tasks:** PRD 1's closure-guard harness leaf (Leg E depends on it);
`arg_acceptance`'s relocation must land before the sibling PRDs' additive constructors touch
the same file (ordering only — no semantic dependency).

---

## 5. Cross-PRD relationship (G4)

| Other work | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| PRD 1 `units-length-gate-completion` | consumes | `arg_acceptance` core (FROZEN) + its length-side constructors; the closure-guard harness | **PRD 1** owns the harness and geometry chokepoints; **this PRD** owns the `reify-ir` relocation and the second probe universe | relocation is verbatim + `pub use`; PRD 1's additive constructors are unaffected by order |
| PRD 3 `angle-units-surface-convergence` | consumes | `angle_spec`; `trig_input`'s bare-radians arm; all ANGLE policy | **PRD 3** | this PRD funnels angle-semantic reader positions into `trig_input` and changes **no** angle policy; PRD 3 retires the bare arm in one place afterwards |
| PRD 4 `dimensioned-construction-strictness` | **bidirectional** | the four retyped load-ctor slots (`PointLoad.force`, `PressureLoad.magnitude`, `TractionLoad.traction`, `BodyForce.force_density`) become dimensioned ctor slots under PRD 4's strict-equality promotion; ctor conformance is Warning-only today (`conformance/mod.rs:32`, δ flip pending) | **PRD 4** owns `conformance/mod.rs` / `type_compat.rs` / the δ severity flip; **this PRD** owns the `.ri` retype + the 90-site migration | the migration must cover the bare `PointLoad(force: 1000.0)` corpus **regardless** of PRD 4's landing order; the coordinator wires the definitive cross-batch edge once both batches exist |
| PRD 2 `check-diagnostic-truthfulness` | produces-for | every new `Severity::Error` code here becomes check-visible once PRD 2 lands | **PRD 2** | all signals here are phrased against `reify eval`; the new `FeaLoadKindUnsupported` and the reused `DimensionedArgRejected` are **genuine failures** and must never enter PRD 2's exit-code allowlist |
| `eradicate-silent-undef.md` (ε #5403) | sibling / consumes | the check Error-severity exit gate + `UndefCause` never-overwrite half of INV-SF-1 | **that PRD** | this PRD's readers push their **specific** code (`DimensionedArgRejected`, per §6 decision 1's reconciliation) through `push_op_contract_failure` (`reify-expr/src/lib.rs:3262` takes the code as a parameter; call sites `:646`/`:4282`) rather than the generic `OpContractViolation`. The exercised push is leaf β's, not α's — task 5791 amendment A6 |
| `placeholder-type-eradication-ratchet.md` (η, **task 5412** `pending`) | **defers to** | `pub type JointValue = Real` (`trajectory.ri:77`) and its four uncited `TODO(joint-value-type)` blanket escapes (`trajectory.ri:75/:184/:222`, `dynamics.ri:189`) | **that PRD / task 5412** | this PRD does **not** retype `JointValue`; it fixes `jointvalue_from_bound_value`'s *reads* per joint kind, which is independent of the alias. Recorded so the seam is not double-owned |
| `materials-parameter-surface-completion.md` | adjacent | `std.materials` param surface | that PRD | its "#3111-family deferred" out-of-scope note is **stale** (#3111/#3112/#3113 all `done`); this PRD's docs leaf corrects `docs/reify-stdlib-reference.md`, not the `.ri` |
| `compute-fea-hardening.md` (INV-FEA-2, task 5083 `done`) | builds on | `FeaValueShapeError` + the Result-ified `extract_loads` | that PRD | this PRD extends the same error enum rather than adding a parallel one |
| naming-convergence program | flags only | `trait Analysis.yield_strength : Real` (`analysis.ri:46`) vs `trait Strong.yield_strength : Pressure` (`materials_mechanical.ri:112`) — same name, inconsistent dimension | **not this PRD** | this PRD retypes `analysis.ri`'s to `Pressure` (zero conformers ⇒ zero migration) and records the collision |

No contested-ownership pair from the audit catalogue is touched. No new in-engine seam: the
compute-target work stays inside the existing `@optimized` ComputeNode trampolines
(engine-integration-norm §3.1/§3.4), and the stdlib work stays on the existing `FunctionCall`
→ `eval_builtin` → classifier path.

---

## 6. Resolved design decisions

1. **Rejections are `Severity::Error` with a `DiagnosticCode`, not Warnings.** The shipped
   Contract A/B/C geometry rejections are Warnings because the op degrades to `Undef` within
   one primitive; here the consequence is a *plausible wrong number* propagating through a
   solve (1e12× spring rate, a load that vanishes). INV-SF-2's corollary applies in the
   affirmative: this is not a diagnostic a healthy path can hit. The house precedent is
   already `Diagnostic::error("E_StackupDimMismatch: …")` for exactly this failure class.
   **ONE** new code only — `FeaLoadKindUnsupported` — kept distinct from the arithmetic
   `DiagnosticCode::DimensionMismatch` (`diagnostics.rs:497`), whose semantics are the
   Add/Sub operator-level invariant. Reader and field DIMENSION rejections REUSE the
   existing `reify_core::DiagnosticCode::DimensionedArgRejected`
   (`crates/reify-core/src/diagnostics.rs:4045`); the message still names builtin +
   arg/field and still carries the migration hint, because both surfaces format through
   the one `ArgRejection::message`.

   > **RECONCILIATION — RULED by Leo 2026-08-30 (esc-5791-3). Supersedes this decision's
   > original wording, "Two new codes only — `ArgDimensionMismatch` and
   > `FeaLoadKindUnsupported` … so they can be counted and gated separately."**
   > The governing rule is ONE CODE PER REJECTION REASON, not one per surface, so
   > `ArgDimensionMismatch` is **not** minted. As originally written this decision
   > contradicted the clause at `crates/reify-core/src/diagnostics.rs:4035-4037` (landed by
   > PRD 1's task 5743), which charters PRD 5 to reuse `DimensionedArgRejected`. The two
   > were authored **four minutes apart in the same 2026-07-28 fan-out session** (PRD 1
   > `54afdee50b` 22:14:36, this PRD `efba5a8036` 22:18:14), so neither superseded the
   > other and no reconciliation record existed until now.
   >
   > *Why reuse won.* PRD 1's D9 exists to give PRDs 1/3/5 one wording template and ONE
   > code; after task α relocates `arg_acceptance` the dimension predicate is literally one
   > function (`accept_arg`), so two codes over one predicate is incoherent. PRD 3 already
   > complied and mints none (`angle-units-surface-convergence.md:336-341`, `:542`, `:555`).
   > And `DimensionedArgRejected` is **already** reused at a stdlib surface — the `bbox` arm
   > at `crates/reify-stdlib/src/geometry.rs:1670-1681` (task 6081) — so "eval-layer
   > `geometry_ops` only" was never the real boundary; task α widens that variant's Origin
   > block to name this reader/field surface instead.
   >
   > *The original justification was measured false.* "So they can be counted and gated
   > separately" presupposed a PRD 2 ratchet that counts by code. PRD 2 **forbids** a
   > per-code allowlist (`check-diagnostic-truthfulness.md:252-256`); the real ratchet
   > (`CHECK_ERROR_EXIT_ALLOWLIST`, `eradicate-silent-undef.md:121-144`, task 5403) is an
   > exemption list burning down to zero, not a census, and already admits message-prefix
   > matchers; this PRD's own capability manifest requires the codes never enter it; and
   > 5403 has not landed, so no consumer exists. Note also that this decision's stated
   > comparand was the *arithmetic* `DimensionMismatch`, never `DimensionedArgRejected`,
   > which did not exist when this PRD was written.
   >
   > Binding record: task 5791 amendment **A7**. Revisit only on a DEMONSTRATED consumer
   > that must distinguish the reader surface from the geometry-builtin surface by code
   > identity — not a speculative one.
2. **`Undef` in ⇒ `Undef` out, quietly.** `Acceptance::Undefined` keeps its existing quiet
   degradation. Undef inputs are expected transient state during solver iteration; only
   *defined-but-wrong* values are rejected. This is unchanged `accept_arg` semantics.
3. **Absent ≠ wrong.** `field_f64`'s current behaviour conflates "field absent" with "field
   present but unreadable" and substitutes a default for both. An absent **optional** field
   keeps its declared default; a **present but wrong-dimension** field is a rejection; an
   absent **required** field gets its own distinct wording. No coercing reader survives —
   every reader returns `Undef` on rejection so the classifier hook can reach it.
4. **Per-argument triage is per-site and explicit** (§3 Leg B). A "dimensionless" position is
   gated to `dimensionless_spec` (accepts `Real`/`Int`/`Scalar{DIMENSIONLESS}`, rejects any
   dimensioned Scalar) — it is *not* left ungated. Silence about a position is not permission;
   the Leg E guard's allowlist is keyed by expected `DimensionVector` **per position**, so
   every deliberately-bare position is an individually justified allowlist entry.
5. **`spring_rate` and `damping` are joint-kind-polymorphic.** Translational and rotational
   stiffness are *different* `DimensionVector`s (`STIFFNESS` vs `ROTATIONAL_STIFFNESS`,
   `dimension.rs:245`/`:275`). The spec is selected by the joint's `kind` string, exactly as
   `couple`'s offset already selects `length_input` vs `trig_input` (`joints.rs:539`).
6. **`TractionLoad`/`BodyForce` are rejected, not wired.** Both self-identify as placeholders
   for `Vector3<Pressure>` / `Vector3<ForceDensity>` (`fea_multi_case.ri:437`/`:467`) and
   neither carries a `direction` field, so a physically meaningful wire-up needs a type-surface
   extension this PRD does not own. INV-SF-3 forbids the silent no-op, so they get an explicit
   `E_FeaLoadKindUnsupported` Error naming the kind, plus a filed follow-up task owning the
   bridge. Constructing the values stays legal; only passing them to a solver errors.
7. **Landing shape is migrate-then-error, in three green steps** (§3 Leg C). No step leaves the
   workspace red; step 1 alone already closes the inversion, which is the highest-value change.
8. **`arg_acceptance` moves down, it does not fork.** A second copy in `reify-stdlib` would be
   lock-step duplication of the exact table whose single-sourcing is the point. The move is
   verbatim + `pub use`; `validate_dimensioned_scalar` becomes an adapter, not a rival.
9. **`analysis.ri`'s `yield_strength : Real` retypes to `Pressure`.** `trait Analysis` and
   `trait AnalysisResult` have **zero conformers** repo-wide, so the migration cost is zero and
   the `// (Pa, using Real)` comment at `analysis.ri:44` stops lying. ~~`AnalysisResult`'s six
   `Real` params stay — their doc-comment explicitly declares them a dimension-agnostic
   structural contract, which is a stated intent, not a silent placeholder.~~ **SUPERSEDED
   2026-08-10 (Leo, ruling task 6165):** the five stress params retype to `Stress` too
   (`safety_factor_value` stays `Real`, genuinely dimensionless). The stated-intent defence was
   scoped to this PRD's reader seam; as a *signature* posture it rides an erasing conformance
   check and dies under strict dimension equality (D11 direction) once `Real` ≡
   `Scalar{DIMENSIONLESS}`. Cross-domain reuse, if ever wanted, goes via per-domain or
   quantity-parameterized traits (posture-3 breadcrumb in 6165), never by re-weakening to Real.
10. **The guard's universe is derived, never hand-listed.** Second-universe names come from a
    token scan of `reify-stdlib`'s `eval_builtin` sub-dispatcher match-arm string literals,
    unioned with the LSP's independently-maintained `BUILTIN_FUNCTIONS`
    (`reify-lsp/src/completion.rs:314`, 95 entries — which covers `plane_*`/`axis_*`/`point3`
    but **not** `prb_*`/joints/FEA, hence the union). A hand list would reproduce the
    `arg_slot_keys_are_registered_builtin_names` vacuity lesson: the probe universe must be
    independent of the assertion target.

---

## 7. Contract (H) — the validated-read helper family

**One family. No new ad-hoc coercions anywhere in the touched files.**

```rust
// crates/reify-ir/src/arg_acceptance.rs  (relocated verbatim from reify-eval; FROZEN core)
pub struct ArgSpec { pub type_name: &'static str,
                     pub dimension: reify_core::DimensionVector,
                     pub migration_hint: Option<&'static str> }
pub enum Acceptance { Accepted(f64), Undefined, Rejected(ArgRejection) }
pub fn accept_arg(value: &Value, spec: &ArgSpec) -> Acceptance;   // unchanged semantics

// ADDITIVE — this PRD (PRD 1 owns the length-side additions, PRD 3 owns angle_spec):
pub fn pressure_spec() -> ArgSpec;        pub fn force_spec() -> ArgSpec;
pub fn mass_spec() -> ArgSpec;            pub fn frequency_spec() -> ArgSpec;
pub fn time_spec() -> ArgSpec;            pub fn velocity_spec() -> ArgSpec;
pub fn acceleration_spec() -> ArgSpec;    pub fn force_density_spec() -> ArgSpec;
pub fn moment_of_inertia_spec() -> ArgSpec;
pub fn translational_stiffness_spec() -> ArgSpec;
pub fn rotational_stiffness_spec() -> ArgSpec;
pub fn dimensionless_spec() -> ArgSpec;   // Real | Int | Scalar{DIMENSIONLESS}

// ADDITIVE — the struct-field read, which is the dominant shape in this PRD:
pub fn accept_field(data: &StructureInstanceData, key: &str, spec: &ArgSpec) -> FieldAcceptance;
pub enum FieldAcceptance { Accepted(f64), Absent, Undefined, Rejected(ArgRejection) }
```

**Invariants.**

- **I1 — single expectation.** Each reader position's `ArgSpec` is named in exactly one place,
  consulted by both the reader and the post-hoc classifier. A classifier that re-derives its
  own dimension expectation is a defect.
- **I2 — no coercion.** A reader returns `Accepted` or `Undef`. It never substitutes a
  default, a `0.0` floor, or a `1.0` sentinel for a *present* value. (`Absent` + a declared
  optional default remains legal — decision 3.)
- **I3 — strict equality.** `dimension == spec.dimension`. Bare `Real`/`Int` at a dimensioned
  position is `Rejected`; a dimensioned Scalar at a dimensionless position is `Rejected`;
  `DIMENSIONLESS` and bare are interchangeable only at a `dimensionless_spec` position.
- **I4 — one wording.** Every rejection message comes from `ArgRejection::message(builtin,
  arg_name)`; every diagnostic carries a `DiagnosticCode` (INV-SF-6); every non-finite value is
  rejected with its own distinct wording rather than silently dropped.
- **I5 — specific cause.** When a reader rejection makes a builtin return `Undef`, the
  `UndefCause` recorded is the specific `DimensionedArgRejected`, not the generic
  `OpContractViolation` (INV-SF-1's never-overwrite half; `push_op_contract_failure`
  already takes the code as a parameter — it is at `crates/reify-expr/src/lib.rs:3262`,
  and its two call sites `:646` and `:4282` both pass the generic code today).
  The invariant demands a cause that is SPECIFIC rather than `OpContractViolation`, which
  `DimensionedArgRejected` satisfies (§6 decision 1 reconciliation). **The exercised push
  and its `reify-expr` edit belong to leaf β, not α** — they are unreachable from α's
  declared module set, and `record_op_contract_failures`
  (`crates/reify-eval/src/engine_eval.rs:4621`) implements the first-push-wins half. α owns
  only a black-box ordering floor test. See task 5791 amendment A6.
- **I6 — both emission channels.** `reify-stdlib` readers surface via the `diagnose` classifier
  → `EvalContext::diagnostics`; `reify-eval` compute targets surface via
  `ComputeOutcome::Failed { diagnostics, .. }`. The *wording and code* are identical across
  both; only the transport differs.
- **I7 — one reader per quantity.** `solve_elastic_static` and `solve_buckling` read a load
  through the **same** function. A second spelling of "read a force off a load struct" is a
  defect by construction.

## 8. Boundary-test sketch (both faces of each seam)

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| B1 | solver ↔ stdlib, **positive** | one body, one support, `PointLoad(force: 5000N)` | `solve_elastic_static` and `solve_buckling` report the **same** applied force; the elastic reaction is non-zero and matches the closed-form tip deflection band. *Negative-assertion mandate: asserts the force is APPLIED, not merely that a warning appeared* |
| B2 | solver ↔ stdlib, **negative** | same scene, `PointLoad(force: 5000.0)` (bare) | `reify eval` exits 1; one `DimensionedArgRejected` Error naming `PointLoad`/`force`; the solve does **not** silently proceed with 0 N |
| B3 | dead-wired load | `loads: [TractionLoad(traction: 5.0e6Pa)]` | `reify eval` exits 1 with `FeaLoadKindUnsupported` naming `TractionLoad`; **no** result value is produced |
| B4 | stdlib ↔ eval, material | `prb_cantilever_beam(20mm, 5mm, 0.5mm, Material(youngs_modulus: 200mm, …), …)` | exits 1, `DimensionedArgRejected` naming `prb_cantilever_beam` / `youngs_modulus`; the `200GPa` twin yields the reference spring rate within band |
| B5 | absent-vs-wrong | material with `yield_stress` **absent** vs `yield_stress: 310mm` | absent ⇒ the documented PRB small-deflection fallback, no diagnostic; wrong-dimension ⇒ exit 1 (decision 3) |
| B6 | Undef transience | a load whose `force` cell is `Undef` mid-solve | quiet degradation, **no** diagnostic, exit 0 (decision 2) |
| B7 | joint-kind polymorphism | `bind` an ANGLE to a prismatic joint and a LENGTH to a revolute, through the loop-closure path | both exit 1 with the position named — matching `joints.rs:1224`'s existing behaviour, which `loop_closure.rs:691` contradicts today |
| B8 | override actually applied | `ElasticOptions(shell_voxel_size: 0.5mm)` vs default | the medial-axis tolerance observably differs (today byte-identical — the override is discarded) |
| B9 | closure guard, anti-vacuity | shrink the second-universe allowlist by one entry | the guard **fails**; restoring it passes (mirrors `version_id_discipline_gate.rs`'s seeded self-tests) |
| B10 | closure guard, universe independence | add a new `eval_builtin` match arm with a bare-accepting numeric position | the guard fails **without** the guard file being edited (the universe is derived, decision 10) |

---

## 9. Decomposition plan

Signals are `reify eval`-phrased (PRD 2 owns check). Intra-batch prereqs by Greek label.
G7 walk: no invariant hit is unresolved — the batch **implements** INV-SF-1/2/3/6 and defers
INV-SF-5's `JointValue` retarget to its named owner (task 5412), which is a resolution, not a
waiver. No G7 waivers are required.

**Phase 1 — foundation**

- **α — relocate `arg_acceptance` to `reify-ir`; add the PRD-5 spec constructors,
  `accept_field`, and the two `DiagnosticCode` variants.** *(reify-ir, reify-core, reify-eval,
  reify-stdlib.)* INTERMEDIATE — unlocks β…ι. Downstream consumers: every leg-B/C/D leaf.
  Includes making `helpers.rs:229` an adapter so there is one rule.

**Phase 2 — vertical slice (the headline defect)**

- **β — flexure reader adoption + dimension-rejection classifier arm.** Prereq α. *Signal:*
  `reify eval` on the B4 fixture exits 1 with `DimensionedArgRejected` naming
  `prb_cantilever_beam`/`youngs_modulus`; the `200GPa` twin reproduces the reference spring
  rate. Consolidates `scalar_si`/`material_field_si`/`material_numeric_field`/`length_si`;
  fixes the stale `modal_ops.rs:839` citation at `common.rs:137`.

**Phase 3 — solver load integrity (three green steps; γ2 and γ3 carry hard edges)**

- **γ1 — widen both solvers' load readers; one shared reader; buckling `type_name` guard.**
  Prereq α. *Signal:* B1 — `PointLoad(force: 5000N)` produces the same applied force in
  `solve_elastic_static` and `solve_buckling`, and a non-zero elastic reaction.
- **γ2 — retype the four load fields in `fea_multi_case.ri`; migrate all 90 in-scope call
  sites.** Prereq γ1 (hard edge). *Signal:* `reify eval examples/fea_multi_case_bracket.ri`
  and `examples/multi_load_bracket.ri` produce byte-identical results with `force: 1000N`.
- **γ3 — narrow: reject bare at load slots; delete the `0.0` density default, the `1.0` N
  sentinel and the silent unknown-`type_name` skip; `E_FeaLoadKindUnsupported` for
  `TractionLoad`/`BodyForce`.** Prereq γ2 (hard edge). *Signal:* B2 + B3. Retargets the two
  tests that pin the defect (`elastic_static.rs:8240`, `:8303`) and corrects `buckling.rs:741`'s
  orphaned "task θ/3457" deferral (3457 is `done`). Files the `TractionLoad`/`BodyForce`
  wire-up follow-up task cited from the diagnostic.

**Phase 4 — remaining reader chokepoints (parallel; all prereq α)**

- **δ — joints + loop closure.** *Signal:* B7. Closes `length_input`'s bare hole at
  `joints.rs:539/:587/:660/:1763`; keys `jointvalue_from_bound_value` per joint kind through
  `length_input`/`trig_input`. Does **not** touch `pub type JointValue`.
- **ε — dynamics `cell_f64`: consolidate three copies to one; gate mass/com/inertia/
  spring_rate/damping.** *Signal:* `mass_properties(mass: 2m, …)` exits 1 with the field named;
  today it silently reads 2.0 kg. Routes `dynamics/eval.rs:354` through the existing
  `cell_mass_f64` (`:78`), closing an inconsistency inside one file.
- **ζ — trajectory `read_scalar_si`/`field_f64`: consolidate three near-duplicates to one; gate
  `target_frequency`/`t`/limits; stop default-substitution for present-but-wrong fields.**
  *Signal:* `ZVShaper(target_frequency: 50rad/s)` exits 1 instead of applying a silent 6.28×
  error; `50Hz` is unchanged.
- **η — FDM / modal / as-printed readers.** *Signal:* `AsPrintedOptions(line_width: 0.4)` and
  `FDMCouponOverride(ex: 2mm)` each exit 1 naming the field; the dimensioned spellings are
  unchanged.
- **θ — `safety_factor` at BOTH entry points + `analysis.ri` retype.** *Signal:*
  `safety_factor(σ, 250.0)` exits 1; `safety_factor(σ, 250MPa)` evaluates to the same value it
  does today. Covers `reify-stdlib/src/analysis.rs:271` **and** the Field interception at
  `reify-expr/src/lib.rs:394`; retypes `analysis.ri:46` and migrates
  `examples/fields_analysis.ri:46` (+ its comments) and the ~4 Rust fixtures.
- **ι — `envelope_critical_load` reference_load → FORCE; `shell_voxel_size` `Option<Length>`
  read.** *Signal:* B8, plus `critical_load(result, 1mm)` exits 1 while
  `critical_load(result, 1kN)` is unchanged. Retargets the test that pins the propagation
  (`fea.rs:6502`).

**Phase 5 — language-surface reads**

- **κ — `min`/`max` cross-dimension rejection + inverse-trig dimensionless discipline.**
  Prereq α. *Signal:* `min(10mm, 5kg)` and `asin(3mm)` each exit 1 with a coded diagnostic;
  `min(10mm, 20mm)`, `min(3, 7.0)` and `asin(0.5)` are unchanged. Measured `.ri` exposure ≈ 0.

**Phase 6 — guard, docs, integration gate**

- **λ — closure-guard second universe.** Prereq: **PRD 1's harness leaf (hard cross-PRD
  edge)** + β…ι. *Signal:* B9 + B10. Additive universe + `DimensionVector`-keyed allowlist +
  seeded anti-vacuity self-tests. **Drift-guard registrations ship in this same diff** —
  `tests/infra/run-all-classification.manifest` row if it lands a shell test, `.config/nextest.toml`
  partition entry, and no new wall-clock bound (esc-4914-162).
- **μ — doc chunk.** *Signal:* the `units.md` (or a new stdlib-analysis) chunk documents the
  dimensioned reader surface and the FEA load signatures; every documented signature compiles
  as written in a smoke `.ri`; verified against `builtin_signatures.rs` / the `units.rs` name
  registries.
- **ν — exemplar corpus.** *Signal:* `examples/best_practices/dimensioned_analysis_inputs.ri`
  compiles under `examples_smoke.rs` and demonstrates the correct spelling against the
  anti-pattern; its `INDEX.md` row lands in the same diff.
- **ξ — reify-design cheatsheet index line + discoverability acceptance.** *Signal:* an author
  who knows the goal ("make sure my FEA loads actually get applied", "check my material's
  units") reaches the mechanism from the chunk topic or the corpus index without knowing a
  function name.
- **ο — stale-doc correction + declared-only register.** *Signal:*
  `docs/reify-stdlib-reference.md:979-1047`'s ~12 `Real`/"pending #3111" mismatches are
  corrected against the live `Density`/`Pressure`/`Energy` declarations; `shear_modulus` and
  `thermal_expansion` are registered as **declared-only with zero production readers** under a
  named tracking task (**#5801**); `thermal_conductivity` is **not** registered — it is recorded
  with its corrected split status instead (§2.4 correction 2); the research note's `cte` and
  `yield_stress` premises are corrected in place.
- **π — integration gate.** Prereq β, γ3, δ, ε, ζ, η, θ, ι, λ. *Signal:* the full §8
  boundary-test table runs green as a gate-resident suite, with B1/B2 asserting **application**
  of the load rather than the presence of a warning.

---

## 10. Out of scope

- **`reify check` visibility** — PRD 2. Every signal here is `reify eval`-phrased.
- **Ctor conformance, `type_compat.rs`, the δ severity flip, task 5627's ruling** — PRD 4.
  This PRD retypes `.ri` load fields; it does not touch the conformance machinery that
  enforces them.
- **Geometry eval chokepoints (R7/R3/R8/R11/R12), the kernel tripwire, compile-layer
  `builtin_signatures.rs` slots, the GUI param editor, compiler-desugaring literal
  dimensioning** — PRD 1.
- **All ANGLE policy** — bare-radians retirement, `resolve_bare_angle`, `Nm`/Torque,
  middle-dot round-trip — PRD 3.
- **`pub type JointValue = Real` retarget** and the four `TODO(joint-value-type)` blanket
  escapes — `placeholder-type-eradication-ratchet.md` / **task 5412**.
- **Wiring `TractionLoad`/`BodyForce` into the solver** — needs the `Vector3<Pressure>` /
  `Vector3<ForceDensity>` surface plus a direction convention; rejected loudly here, filed as a
  follow-up (decision 6).
- ~~**`AnalysisResult`'s six `Real` params** — a declared dimension-agnostic structural contract
  (`analysis.ri:23-28`), not a silent placeholder.~~ **SUPERSEDED 2026-08-10** — ruled in scope
  of the type-decision programme instead: five stress params → `Stress`, `safety_factor_value`
  stays `Real` (ruling task 6165; decision 9 amendment above has the rationale).
- **Consumers for the declared-only `shear_modulus` / `thermal_expansion`** — registered and
  tracked (ο), not built here; there is no thermal or orthotropic-shear solver to consume them.
- **A Rust/host consumer for `thermal_conductivity`** — not built here for the same reason (no
  thermal solver), but it is *not* declared-only and so is deliberately absent from the ο
  register: `ThermallyConductive.thermal_conductivity` already has a live DSL reader at
  `structural_physical.ri:150` (§2.4 correction 2).
- **The `trait Analysis.yield_strength` vs `trait Strong.yield_strength` name collision** —
  flagged for the naming-convergence program.

---

## 11. Open questions (tactical)

1. **Module name at its new home.** `reify_ir::arg_acceptance` (preserve) vs
   `reify_ir::dimension_contract` (clearer). **Suggested:** preserve the name so the `pub use`
   is a pure alias and no sibling PRD's diff churns. Decide in α.
2. **CLOSED AS MOOT — ruled by Leo 2026-08-30 (esc-5791-3).** This asked "one
   `ArgDimensionMismatch` code vs a per-family set (`FlexureArgDimension`, `FeaLoadDimension`,
   …)", on the premise that a finer set "would let PRD 2's allowlist ratchet reason more
   finely". Both halves fell. No `ArgDimensionMismatch` is minted at all — reader rejections
   reuse `DimensionedArgRejected` (§6 decision 1 reconciliation) — and PRD 2 has no such
   ratchet: it forbids per-code allowlists outright
   (`check-diagnostic-truthfulness.md:252-256`). Nothing left to decide in α.
3. **Rejection wording for a wrong-dimension *field* vs a wrong-dimension *positional arg*.**
   `ArgRejection::message` takes `(builtin, arg_name)`; a field read wants
   `(builtin, "material.youngs_modulus")`. **Suggested:** dotted path in `arg_name`, no new
   formatter. Decide in α.
4. **γ2's 77 Rust fixture sites: codemod or hand-edit.** The `.ri` 35 are trivially mechanical;
   the Rust ones are inside `r#"…"#` literals. **Suggested:** a scripted textual pass with a
   reviewed diff, mirroring PRD 1's append-unit codemod judgement. Decide in γ2.
5. **Whether `field_f64`'s consolidation lands in `reify-stdlib` or moves up beside
   `accept_field`.** **Suggested:** `accept_field` in `reify-ir` is the one helper; the
   trajectory-local wrappers disappear entirely. Decide in ζ.
6. **λ's probe cost.** The stdlib universe is larger than `GEOMETRY_FUNCTION_NAMES` and some
   builtins need structured args (a joint `Map`, a `StructureInstance`) that a bare-numeric
   probe cannot synthesize. **Suggested:** probe only positions reachable with scalar
   arguments; record unreachable positions as explicit allowlist entries with a reason, so
   coverage is visible rather than assumed. Decide in λ.
