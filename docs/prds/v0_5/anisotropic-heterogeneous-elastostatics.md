# PRD: Anisotropic & Spatially-Varying Elastostatics (shared constitutive foundation)

Status: **active foundation** — v0.5. Authored 2026-05-26 via `/prd` (G1–G5+META). Decompose-ready (B+H).

Shared upstream foundation for two consumers: `fdm-as-printed-fea.md` (this milestone) and `composite-laminated-shells.md` (this milestone). Activates the constitutive-law portion of **GR-041** ahead of the rest of that cluster — see "Relationship to GR-041" and the companion edit in `docs/architecture-audit/gap-register.md`.

## Goal

Extend the shipped linear-elastostatic FEA solver from **isotropic, single-material** to **anisotropic and spatially-varying**, without changing the analysis kind (still linear-elastostatic) and without breaking any existing `solve_elastic_static` caller. After this lands, a user can solve a body whose stiffness is (a) anisotropic (orthotropic / transverse-isotropic, expressed in a per-point material frame) and (b) different at every point (a `Field` of material values), and obtain the same `ElasticResult` shape as today.

Concretely, the solver's `material` argument generalises from a single `ElasticMaterial` to *either* a single constitutive law *or* a `Field<Point3<Length>, AnisotropicMaterial>`. The isotropic single-material path is the constant-field special case and remains the default.

User-observable signal (this PRD's own leaf): an `examples/anisotropic_bar.ri` that solves the same bar twice — once isotropic, once transverse-isotropic with a weak build-axis — and prints materially different tip deflection / stress. CI golden output committed.

## Background

`crates/reify-solver-elastic/src/constitutive.rs` ships exactly one constitutive law: `IsotropicElastic { youngs_modulus, poisson_ratio } → d_matrix() -> [[f64;6];6]` (engineering-strain Voigt order `[εxx,εyy,εzz,γxy,γyz,γxz]`). Element-stiffness assembly (`K_e = ∫ Bᵀ D B dV`, tasks #8/#2915) calls `IsotropicElastic::d_matrix()` once per material — there is one `D` for the whole body, in the global frame.

The 2026-05-12 architecture audit recorded the gap directly: *"The ElasticMaterial trait `materials_fea.ri` is isotropic-only and does not include the Orthotropic constitutive law trait surface"*; *"There is no MaterialConstitutiveLaw abstraction"*; *"The Orthotropic constitutive law trait surface blocks all downstream composite mechanisms."* This was parked under **GR-041** as deferred v0.5+ work with no task. Two independent consumers now need it, so it is promoted to an owned foundation.

The two foundation prerequisites are already shipped (verified 2026-05-26 against task state, correcting stale gap-register State columns):
- **GR-006 / `Field<X,Y>` in `param` position — DONE.** Task 3088 added the `Field<D,C>` arm to `resolve_parameterized_builtin_type` (+ `_with_subst` mirror); task 3117 confirmed it works in param positions and tightened `ElasticResult.displacement`/`.stress`. So `param material : Field<Point3<Length>, AnisotropicMaterial>` resolves today.
- **GR-001 / struct-constructor runtime — DONE.** `Value::StructureInstance` shipped (SIR-α task 3540, SIR-β-mat 3542). `OrthotropicMaterial(...)` etc. evaluate at runtime to inspectable typed values.

## Sketch of approach

**Constitutive trait surface.** Introduce the `MaterialConstitutiveLaw` abstraction the audit named (trait `ConstitutiveLaw`): the contract is "produce a 6×6 stiffness `D` in the material's local frame." Conformers:
- existing isotropic materials (`ElasticMaterial` becomes a `ConstitutiveLaw` conformer via the isotropic `D`);
- `OrthotropicMaterial` — 9 constants `(E1,E2,E3, G12,G13,G23, ν12,ν13,ν23)`;
- `TransverseIsotropicMaterial` — 5 constants `(E_p, E_z, ν_p, ν_pz, G_pz)` (in-plane isotropic, distinct build/`z` axis). This is the literature-standard FDM simplification and the expected common case.

```
trait ConstitutiveLaw {
    // local-frame 6x6 stiffness in engineering-strain Voigt order
}

structure def OrthotropicMaterial : ConstitutiveLaw {
    param e1 : Pressure
    param e2 : Pressure
    param e3 : Pressure
    param g12 : Pressure
    param g13 : Pressure
    param g23 : Pressure
    param nu12 : Real
    param nu13 : Real
    param nu23 : Real
    param density : Density
}

structure def TransverseIsotropicMaterial : ConstitutiveLaw {
    param e_in_plane : Pressure
    param e_axial : Pressure
    param nu_in_plane : Real
    param nu_axial : Real
    param g_axial : Pressure
    param density : Density
}
```

**Material frame.** Anisotropy is meaningless without an orientation. Every anisotropic constitutive value carries a `Frame` (local material axes); the solver rotates the local-frame `D` into the global frame per element via the standard 6×6 Voigt rotation `D_global = Tᵀ D_local T`. Isotropic materials ignore the frame.

**Spatially-varying value: `AnisotropicMaterial`.** The field codomain is a concrete *evaluated* value carrying a resolved stiffness plus its frame:

```
structure def AnisotropicMaterial {
    param law : ConstitutiveLaw
    param frame : Frame
}
```

**Generalised solver entry.** `solve_elastic_static`'s `material` parameter accepts either:
- a single `ConstitutiveLaw` (homogeneous body — isotropic, orthotropic, or transverse-isotropic), or
- a `Field<Point3<Length>, AnisotropicMaterial>` (heterogeneous body).

A bare `ConstitutiveLaw` auto-lifts to a constant field, so a single code path handles both. Assembly samples the material **per element** (element-centroid or per-Gauss-point), builds the rotated `D`, and assembles `K_e` as today. The CG solve, BC application, and `ElasticResult` shape are unchanged.

**Why per-element field sampling (not a baked realization).** The consuming PRD (`fdm-as-printed-fea.md`) refines the material field progressively (R0→R1→R2…). Keeping the FEA mesh fixed and sampling a *refining field* at stable Gauss points means each fidelity bump only improves `D` values — the mesh/DOF structure is unchanged — so the solver's **warm-start (prior displacement iterate) carries across every fidelity bump**. Baking properties into a realization-kind would couple fidelity to mesh identity and break warm-start on re-discretisation. This is the load-bearing reason the material is a `Field` argument, not a realization. (Design decision confirmed 2026-05-26.)

## Pre-conditions for activating

- v0.3 FEA stack shipped through ComputeNode integration (`solve_elastic_static` `@optimized` entry, task 3426 lineage). **Met.**
- GR-006 `Field<X,Y>` in param position. **Met** (tasks 3088/3117).
- GR-001 struct-constructor runtime. **Met** (SIR-α 3540, SIR-β-mat 3542).
- `Frame` value type available in stdlib (already used by shells mid-surface frames; confirm at task α).

## Resolved design decisions (2026-05-26)

1. **Generalise the material argument; do not add a sibling solver.** The v0.3 FEA PRD reserves sibling functions for analysis *kinds* (modal/thermal) and keeps materials as plain values. Anisotropic + heterogeneous is the same elastostatic analysis with a richer material, so it enriches the `material` argument. No `solve_elastic_static_anisotropic`.
2. **Field-typed material argument, with scalar auto-lift.** Per-element sampling of a (possibly refining) field; constant materials lift to constant fields. Chosen over a realization-kind because it preserves warm-start across progressive fidelity refinement (see "Sketch").
3. **Transverse isotropy is a first-class conformer, not just orthotropy.** Literature treats FDM as transversely isotropic (5 constants) as the standard simplification; orthotropy (9) is the escalation. Both ship.
4. **Material frame folds into the constitutive value, not a separate solver argument.** Keeps the field codomain self-contained (`{law, frame}`) and avoids a parallel orientation-field argument.
5. **Discontinuity handling = per-element-constant `D` + zone-conforming refinement.** Sharp wall↔infill jumps are represented by element boundaries; assembly uses one `D` per element (sampled at centroid for P1, per-Gauss-point optional for P2). Documented as the standard heterogeneous-FE treatment; mesh refinement near zone boundaries is the accuracy knob.

## Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_5/fdm-as-printed-fea.md` | produces (consumed by FDM) | `Field<Point3<Length>, AnisotropicMaterial>` arg to `solve_elastic_static`; `ConstitutiveLaw` / `AnisotropicMaterial` value types | **this PRD** | queued |
| `v0_5/composite-laminated-shells.md` | produces (consumed by composites) | `OrthotropicMaterial` + 3D `D` rotation; `ConstitutiveLaw` trait | **this PRD owns the 3D solid constitutive core**; composites owns the *plane-stress reduction* + ply-stack/through-thickness integration | composites edited to depend on this (companion task) |
| `v0_3/structural-analysis-fea.md` | extends | generalises `solve_elastic_static.material` (non-breaking; scalar path preserved) | this PRD | queued |
| `engine-integration-norm.md` §3.4 | consumes seam | `solve_elastic_static` is an existing §3.4 ComputeNode consumer; no new seam | n/a | wired |
| `gap-register.md` GR-041 | corrects | reassign constitutive-law portion to this PRD | this PRD (companion edit) | done in authoring commit |

**G4 note — the composite-shells seam.** `composite-laminated-shells.md` previously implied it would build orthotropic itself. Ownership is now split cleanly: **this PRD owns the 3D-solid orthotropic/transverse-iso `ConstitutiveLaw` + the 6×6 frame rotation**; composites owns its *shell plane-stress* reduction of that law plus the ply-stack through-thickness integration. The companion correction task edits the composite-shells PRD's "Sketch of approach" to consume `ConstitutiveLaw` rather than re-derive it. No reciprocal ambiguity remains.

## Contract (the seam — approach H)

**C1 — `ConstitutiveLaw::d_matrix_local() -> [[f64;6];6]`.** Engineering-strain Voigt order `[εxx,εyy,εzz,γxy,γyz,γxz]`, identical convention to `IsotropicElastic` (`constitutive.rs`). Invariants: symmetric; positive-definite over the conformer's declared validity range. Orthotropic PD requires the standard constraints (e.g. `1 − ν12 ν21 − ν23 ν32 − ν31 ν13 − 2 ν21 ν32 ν13 > 0` with `νji = νij Ej/Ei`); debug-asserted, mirroring `IsotropicElastic::debug_assert_valid`.

**C2 — frame rotation `rotate_voigt(D_local, frame) -> D_global`.** The 6×6 Voigt transform `Tᵀ D T` for the rotation taking material axes → global axes. Invariants: orthonormal `frame` ⇒ symmetry and PD preserved; identity frame ⇒ `D_global == D_local`; round-trip `rotate(rotate(D, R), R⁻¹) == D` within tol.

**C3 — material sampling `material_at(field, point) -> AnisotropicMaterial`.** For a constant-lifted field returns the single material everywhere. For a discrete field, returns the value of the element/cell containing `point`. Contract: total over the meshed domain; deterministic given the field value-hash.

**C4 — assembly hook.** Element assembly obtains `D_global` per element via C1+C2+C3 and assembles `K_e = ∫ Bᵀ D_global B dV` unchanged otherwise. Contract: when the field is a constant isotropic lift, the assembled `K` is **bit-identical** to today's isotropic path (regression anchor).

**C5 — cache-key.** The material field is a graph-participant `Value`; the FEA ComputeNode keys on its value-hash exactly as it keys on the scalar material value-hash today. No new cache-key machinery; thread count still excluded.

## Boundary-test sketch (approach H)

Producer-side (`reify-solver-elastic`):

| Scenario | Precondition | Postcondition |
|---|---|---|
| Orthotropic `D` vs analytical | `OrthotropicMaterial` with known constants | `d_matrix_local` matches closed-form 6×6 within 1e-9 |
| Transverse-iso ⊂ orthotropic | `TransverseIsotropicMaterial` | equals the orthotropic `D` with `E1=E2`, `ν13=ν23`, `G13=G23`, `G12=E1/2(1+ν12)` |
| Frame rotation preserves SPD | non-axis-aligned `frame` | `D_global` symmetric + PD; eigenvalues match `D_local` |
| Identity-frame no-op | identity `frame` | `D_global == D_local` bitwise |
| Isotropic lift == legacy | isotropic material as constant field | assembled `K` bit-identical to v0.3 isotropic assembly |

Consumer-side (`reify-eval` / `solve_elastic_static`):

| Scenario | Precondition | Postcondition |
|---|---|---|
| Homogeneous orthotropic solve | single `OrthotropicMaterial`, cantilever | tip deflection matches anisotropic-beam reference within band |
| Heterogeneous solve | two-zone `Field` (stiff skin / soft core) | deflection between the two homogeneous bounds; stress concentrates in stiff zone |
| Warm-start across field refinement | solve with field v1, then refined field v2 (same mesh) | second solve warm-starts from v1 iterate; CG iterations drop; result within tol of cold solve |
| Constant-field equivalence | isotropic material vs its constant-field lift | identical `ElasticResult` (tolerance-equivalent) |
| Cancellation | rapid material-field retick mid-solve | no orphaned solver threads; prior cache entry intact |

The heterogeneous + warm-start rows are the integration-gate signal closing G2 for the consuming PRD.

## Decomposition plan

B+H vertical slice. Greek labels; task IDs assigned at decompose.

- **α — `ConstitutiveLaw` trait + 6×6 rotation in `reify-solver-elastic`.** `OrthotropicMaterial`/`TransverseIsotropicMaterial` `d_matrix_local`; `rotate_voigt`; PD debug-asserts. *Crates:* reify-solver-elastic. *Signal (intermediate):* unlocks β, δ; producer boundary-tests (table above) green.
- **β — Per-element material sampling + assembly hook.** Generalise `K_e` assembly to obtain `D_global` per element; isotropic-lift bit-identity regression. *Crates:* reify-solver-elastic. *Signal (intermediate):* unlocks ε; isotropic-equivalence test green.
- **γ — Stdlib `ConstitutiveLaw` trait + `OrthotropicMaterial`/`TransverseIsotropicMaterial`/`AnisotropicMaterial` structures.** With per-property provenance metadata where constants are physical. *Crates:* reify-compiler (stdlib `.ri`). *Signal (intermediate):* `OrthotropicMaterial(...)` evaluates to non-`Undef` `StructureInstance`; field-codomain type resolves.
- **δ — Generalise `solve_elastic_static.material` to `ConstitutiveLaw | Field<Point3, AnisotropicMaterial>` with scalar auto-lift.** *Crates:* reify-compiler stdlib + reify-eval (trampoline arm). *Signal (intermediate):* homogeneous orthotropic solve returns a non-trivial `ElasticResult`.
- **ε — Integration gate: heterogeneous solve + warm-start-across-refinement boundary tests.** The consumer-side table above. *Crates:* reify-eval, reify-solver-elastic (tests). *Signal (leaf):* the two-zone heterogeneous solve test + warm-start test green in CI.
- **ζ — `examples/anisotropic_bar.ri` + golden output.** Isotropic vs transverse-isotropic same bar; printed deflection differs materially. *Crates:* examples, reify-compiler tests. *Signal (leaf, user-observable):* `reify eval examples/anisotropic_bar.ri` prints two distinct deflections; golden committed.
- **η — Companion edits.** Edit `composite-laminated-shells.md` "Sketch of approach" to consume `ConstitutiveLaw`; the GR-041 gap-register edit is in this authoring commit (η covers only the composite-PRD prose + a real cross-PRD dep edge). *Crates:* docs. *Signal (leaf):* composite-shells PRD references this PRD as constitutive owner; dep edge wired.

Dependencies: α→β→ε; α→δ→ε; γ→δ; ε→ζ; η independent (docs).

## Out of scope for this PRD

- The FDM print-structure model, slicing, infill, bond strength — `fdm-as-printed-fea.md` and its R1–R3 stubs.
- Ply-stack through-thickness integration + composite failure criteria (Tsai-Wu/Hashin) — `composite-laminated-shells.md`.
- Material **non-linearity** (plasticity, hyperelasticity) — separate PRD; this is linear anisotropic only.
- Anisotropic *failure/strength* criteria — this PRD ships anisotropic *stiffness* only; strength is downstream.
- Auxetic / fully-anisotropic (21-constant) materials — orthotropic (9) is the ceiling here.

## Open questions (tactical — decide at impl time)

1. **Sampling point.** Element-centroid `D` (one per element) vs per-Gauss-point `D`. **Suggested:** centroid for P1, per-Gauss-point for P2. Decide at task β.
2. **Field spatial-index backing.** A discrete `AnisotropicMaterial` field needs point-in-cell lookup; backing structure (BVH vs uniform grid) is an impl detail. Decide at task β alongside the FDM field producer.
3. **Scalar-lift surface.** Whether the dual-type `material` param is a union, an overload, or a coercion in the stdlib signature. **Suggested:** coercion (bare `ConstitutiveLaw` lifts to constant field at lowering). Decide at task δ.
4. **PD validity messaging.** Orthotropic PD-constraint violation → `E_*` diagnostic vs debug-assert only. **Suggested:** debug-assert now, `E_CONSTITUTIVE_NOT_PD` diagnostic as a follow-up (mirrors `IsotropicElastic` hardening note). Decide at task α.
