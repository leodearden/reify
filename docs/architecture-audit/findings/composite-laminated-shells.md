# Audit: Composite / Laminated Shell Elements

**PRD path:** `docs/prds/v0_5/composite-laminated-shells.md`
**Auditor:** audit-composite-laminated-shells
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 14

> **Overlay notice (2026-09-03):** this is a **dated 2026-05-12 snapshot** that now carries dated
> `CORRECTION` overlays below (both under M-001; M-002 and M-003 carry in-section pointers to
> them). Its named-symbol claims are **partly superseded**:
> `OrthotropicMaterial` and `TransverseIsotropicMaterial` shipped on **2026-05-26** in
> `crates/reify-compiler/stdlib/constitutive.ri` (task 3779 γ, commit `6d77ce0c0a`) — two weeks
> *after* this audit ran — so the Top-concerns bullet "Every named runtime entity in the PRD is
> fiction" no longer holds in full. That bullet is deliberately left as written (it was true on
> 2026-05-12); read it together with the M-001 overlay. `Laminate`, `Ply`, `tsai_wu` and `hashin`
> do remain absent; `max_strain` was not re-checked and no claim is made about it here.

## Top concerns

- **Every named runtime entity in the PRD is fiction.** No `OrthotropicMaterial`, `Laminate`, `Ply`, `tsai_wu`, `hashin`, or `max_strain` symbol exists anywhere in the codebase (`crates/`, `stdlib/*.ri`, or PRDs). The PRD is a green-field design, with no scaffolding yet — but it lands on top of the already-broken structure-constructor evaluation (GR-001), the unresolved `Field<X,Y>` param-position issue (TODO #3117), and a not-yet-shipped parent shells PRD.
- **Foundation is explicitly absent.** Parent v0.4 `structural-analysis-shells.md` is "design resolved + decomposed (2026-05-05), deferred"; downstream tasks (Shells T5/T6 in fused-memory) are partially done but the kernel is **constant-thickness, isotropic D-matrix only** (`shell_assembly.rs:10-11`, `:201`, task 3014 observation memory). Composite swap-in requires re-architecting the through-thickness integration loop, the D-matrix construction, and `ElasticResult.stress`'s `top/mid/bottom` shape (which currently models surface fibre only, not per-ply).
- **No decomposition tasks exist.** Unlike sibling v0.5 stubs that have decomposition tasks queued under their PRDs, this PRD is purely a stub (`Status: stub — deferred, candidate v0.5+`). No tasks own any of the proposed mechanisms, which is appropriate for a stub but means everything is `FICTION` rather than `TODO`.
- **Layup syntax open question collides with `List<Struct>` call-site conformance.** The proposed `Laminate { plies : List<Ply> }` shape needs either (a) struct-constructor runtime eval (GR-001) plus `List<Ply>` flowing through `Value::List` of struct-instance Maps, or (b) an alternate constructor design. The PRD names it as open, and task 2227 (`List<TraitObject>` wiring) is done — but `List<Struct>` of a concrete (non-trait) struct in param position has not been confirmed wired in any audit-relevant memory.

## Mechanisms

### M-001: `OrthotropicMaterial` stdlib structure with `E1, E2, G12, ν12, density, X_T, X_C, Y_T, Y_C, S` cells

- **State:** FICTION
- **Failure mode:** F1 (compile-time contract; no code)
- **Evidence:** No grep hit for `Orthotropic` or `OrthotropicMaterial` in `crates/`, `stdlib/*.ri`, or `docs/prds/` (except this PRD). Existing materials stack has `Material` (`materials_mechanical.ri:63`), `ElasticMaterial` trait (`materials_fea.ri:88`), and four isotropic concrete structures (`Steel_AISI_1045`, `Aluminium_6061_T6`, `Titanium_Ti6Al4V`, `ABS_Plastic` in `materials_fea.ri:132-249`). All four are isotropic-only (carry `youngs_modulus`, `poisson_ratio`, `density`, `yield_stress` — no directional moduli or ply allowables).
- **Blocks:** Tasks gated on this PRD activation (none currently queued).
- **Note:** Type would be a new structure-def with 10 cells; co-blocks with GR-001 (struct-constructor eval) and the open question of whether `ElasticMaterial` trait covers orthotropic or a new `OrthotropicElasticMaterial` trait is needed (the trait's surface in `materials_fea.ri:88` was designed for isotropic).

> **CORRECTION 2026-09-03 (#6877) — the materials_fea.ri anchors and field list above have
> drifted; M-002's *first* evidence sentence still holds.** This is a dated audit snapshot
> (**Date:** 2026-05-12), so the Evidence bullets are preserved as the record of what was measured
> then. What #6877 changed:
>
> - The four presets now declare `: DampedMaterial + Visual` and carry **five** elastic/damping
>   properties — `youngs_modulus`, `poisson_ratio`, `density`, `yield_stress`, `loss_factor` — plus a
>   `loss_factor_provenance : MaterialPropertyProvenance` member (`materials_fea.ri:303`, `:360`,
>   `:417`, `:477`). #6877 introduced the `Damped` mixin trait (`:194-202`) and the named intersection
>   `trait DampedMaterial : ElasticMaterial + Damped {}` (`:224`).
> - `loss_factor` (η) is a *scalar* hysteretic damping ratio, not a directional modulus and not a ply
>   allowable — so #6877 did **not** make these four presets orthotropic; they remain isotropic-only
>   exactly as this audit said. #6877 is therefore *not* the delta that moves M-001; for that, see the
>   second correction immediately below, which is a different and earlier change.
> - **M-002's first evidence sentence is STILL TRUE of the trait.** `ElasticMaterial` requires
>   `youngs_modulus, poisson_ratio, density, yield_stress` only: #6877 put `loss_factor` on the
>   separate `Damped` mixin, **not** on `ElasticMaterial`, which is unchanged by #6877.
> - **Anchor drift:** `ElasticMaterial` trait `materials_fea.ri:88` → **`:130-146`**; the preset span
>   `materials_fea.ri:132-249` → **`:272-492`** (Steel `:272`, Al `:329`, Ti `:386`, ABS `:446`).
>   Grep the named symbol rather than trusting these numbers.

> **CORRECTION 2026-09-03 (task 3779 γ, PRD `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md`,
> landed 2026-05-26) — M-001 is now PARTIAL, not FICTION; M-002's *second* evidence sentence is
> SUPERSEDED.** This is a **separate and earlier** delta from the #6877 one above and must not be
> conflated with it: it landed two weeks *after* this audit's **Date:** 2026-05-12, in commits
> `6d77ce0c0a` (`reify-compiler/stdlib`) and `7abf09ed11` (`reify-solver-elastic`).
>
> - **`OrthotropicMaterial` exists.** `structure def OrthotropicMaterial : ConstitutiveLaw` at
>   `crates/reify-compiler/stdlib/constitutive.ri:88` — the 9-constant orthotropic conformer
>   (`e1`/`e2`/`e3`, `g12`/`g13`/`g23`, `nu12`/`nu13`/`nu23`, `density`), each physical param paired
>   with a `..._provenance : MaterialPropertyProvenance` slot. `structure def
>   TransverseIsotropicMaterial : ConstitutiveLaw` (5-constant) is at `constitutive.ri:125`. The
>   module is loaded as `std.constitutive` (`crates/reify-compiler/src/stdlib_loader.rs:106`).
> - **M-001 → PARTIAL.** The named symbol exists and supplies directional moduli — a *superset* of the
>   `E1, E2, G12, ν12, density` cells this mechanism asked for. Still absent: the five ply allowables
>   `X_T`, `X_C`, `Y_T`, `Y_C`, `S`, which no material structure in `stdlib/*.ri` declares.
> - **M-002's second evidence sentence is SUPERSEDED.** "no `MaterialConstitutiveLaw` trait abstracts
>   over isotropic vs orthotropic" no longer holds. Note the shipped abstraction is spelled
>   **`ConstitutiveLaw`**, *not* the audit's hypothesised `MaterialConstitutiveLaw` — grep the shipped
>   name. It exists on both sides of the seam: the DSL marker trait `trait ConstitutiveLaw { }`
>   (`materials_fea.ri:105`, with `trait ElasticMaterial : ConstitutiveLaw` at `:130`), and the Rust
>   `pub trait ConstitutiveLaw` (`crates/reify-solver-elastic/src/constitutive.rs:35`) with
>   `fn d_matrix_local(&self) -> [[f64; 6]; 6]` at `:39`, implemented for `pub struct
>   OrthotropicMaterial` (`:206`, impl at `:395`) alongside `IsotropicElastic` (impl at `:177`).
>   The design fork M-002 named was thus resolved *away from* extending `ElasticMaterial`: orthotropic
>   shipped as a sibling structure under a shared `ConstitutiveLaw`, not as an
>   `OrthotropicElasticMaterial` trait.
> - **`Laminate`, `Ply`, `tsai_wu` and `hashin` remain FICTION** — zero definitions anywhere in
>   `crates/` (including `stdlib/*.ri`) as of 2026-09-03. The laminate/ply half of this PRD is still
>   green-field; only the orthotropic-material half is not.
>
> **NON-EXHAUSTIVE — this overlay re-verified M-001 and M-002 ONLY.** M-003…M-014 were *not*
> re-checked against the landed anisotropic work and must be re-verified rather than trusted. M-003 in
> particular is known to be at least partly stale — its "`IsotropicElastic::d_matrix()` … is the only
> D-matrix builder" no longer holds now that `OrthotropicMaterial::d_matrix_local`
> (`reify-solver-elastic/src/constitutive.rs:356`, trait impl `:402`) and
> `TransverseIsotropicMaterial::d_matrix_local` (`:496`, impl `:514`) exist. Flagged here, **not**
> adjudicated; re-audit M-003…M-014 before relying on them.

### M-002: Orthotropic constitutive law trait surface (per-direction moduli, ply allowables)

> **Partly superseded — see the `CORRECTION 2026-09-03` overlays under M-001 above.** The *first*
> evidence sentence below still holds (`ElasticMaterial` requires those four params and is unchanged
> by #6877), but its anchors have drifted: `materials_fea.ri:88` → **`:130-146`**, and
> `pub struct IsotropicElastic` is at `reify-solver-elastic/src/constitutive.rs:102` with its
> inherent `impl` at `:109` and `d_matrix()` at `:151`, not `:9-93`. The *second* sentence — "no
> `MaterialConstitutiveLaw` trait abstracts over isotropic vs orthotropic" — no longer holds: a
> `ConstitutiveLaw` abstraction shipped 2026-05-26 (spelled `ConstitutiveLaw`, *not*
> `MaterialConstitutiveLaw`), and the open design fork this row names was resolved there. The
> **State** line below was not re-measured.

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `ElasticMaterial` trait (`materials_fea.ri:88`) requires `youngs_modulus, poisson_ratio, density, yield_stress` only — fundamentally isotropic. `IsotropicElastic` Rust struct in `crates/reify-solver-elastic/src/constitutive.rs:9-93` builds the 6×6 D matrix from scalar `E, ν`; no `MaterialConstitutiveLaw` trait abstracts over isotropic vs orthotropic.
- **Blocks:** All downstream composite mechanisms.
- **Note:** This is the conceptual fork that determines whether orthotropic is a sibling structure to `Steel_AISI_1045` (separate trait) or a parameterised member of a polymorphic constitutive-law surface. Open design.

### M-003: Per-ply orthotropic D-matrix construction (6×6 in material frame, rotated to laminate frame)

> **At least partly stale, and deliberately *not* re-adjudicated — see the `CORRECTION 2026-09-03`
> overlays under M-001 above.** The evidence's "only D-matrix builder" claim no longer holds:
> `OrthotropicMaterial::d_matrix_local` (`reify-solver-elastic/src/constitutive.rs:356`, trait impl
> `:402`) and `TransverseIsotropicMaterial::d_matrix_local` (`:496`, impl `:514`) shipped 2026-05-26.
> The cited anchor has also drifted — `IsotropicElastic::d_matrix()` is at `constitutive.rs:151`, not
> `:88`. Re-verify this row before relying on it; the **State** line below was not re-measured.

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `IsotropicElastic::d_matrix() -> [[f64; 6]; 6]` (`constitutive.rs:88`) is the only D-matrix builder. No rotation by fibre orientation; no orthotropic 6×6 stiffness routine.
- **Blocks:** M-004, M-005 (through-thickness sum needs per-ply D).
- **Note:** Classical lamination theory; well-known maths but a new code path.

### M-004: `Laminate` stdlib structure with `plies : List<Ply>` ordered stack

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `Laminate`, `Ply`, or stdlib `List<<StructureName>>` of concrete (non-trait) structs in `materials_fea.ri` or `solver_elastic.ri`. Closest precedent: `fea_multi_case.ri:50` uses `List<LoadCase>` as a list of structures, but typed as `List<Real>` placeholder per the `Field<X,Y>`-in-param TODO (#3117); kind-match silently accepts the runtime list. Whether the same placeholder-list mechanism transfers to `List<Ply>` is unverified by any audit memory.
- **Blocks:** M-005 (kernel iterates the ply list), M-008 (helper functions).
- **Note:** Coupled to GR-001 (struct-ctor eval) and the open design question of constructor surface (list-literal vs dedicated ctor vs external file).

### M-005: Through-thickness sum-over-plies integration in shell element kernel

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `crates/reify-solver-elastic/src/shell_assembly.rs:10-11` explicitly: "Reissner-Mindlin shell element under a **constant-thickness isotropic** linear-elastic constitutive law. Through-thickness integration is..." (analytical, single material). `:118` describes it as "Baked in as a private constant — it is a property of the through-thickness". Task 3014 ("Shells T6: shell stiffness assembly under isotropic linear-elastic constitutive law") confirms "Constant-thickness, isotropic D matrix. Through-thickness integration analytical (closed form for membrane + bending + transverse shear contributions)."
- **Blocks:** M-006, M-007.
- **Note:** The PRD says "the through-thickness integration becomes a sum over plies with discontinuous derivatives at ply boundaries" — this is a structural rewrite of the shell stiffness assembly path, not an additive extension.

### M-006: Per-Gauss-point layered constitutive evaluation

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** Shell kinematics module (`shell_kinematics.rs:44`) returns kinematic primitives only — no per-Gauss-point material evaluation hook; D matrix is computed once at element scope from the single material. No infrastructure for "compute D per Gauss point as a layered stack rather than a single isotropic relation."
- **Blocks:** M-005.
- **Note:** New code path; would need either a per-Gauss-point material callback or an unrolled per-ply integration scheme.

### M-007: Per-ply stress and strain result fields in `ElasticResult`

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `ElasticResult` in `stdlib/solver_elastic.ri:295-316` has `displacement, stress, frame, max_von_mises, converged, iterations` only. `ShellStress` (`:352-356`) has `top, mid, bottom` — a 3-channel through-thickness shape designed for **single-material** outer/neutral/inner fibres, NOT per-ply (the comment at `:343-345` is explicit: "preserves the invariant that ShellStress always has all three channels populated even for solid-element results"). No precedent for `List<Field<...>>` or per-ply indexed field collections.
- **Blocks:** All composite-result consumers (GUI, multi-load-case envelopes).
- **Note:** PRD says "top, mid, bottom of each ply" — a 3 × N_plies result tensor, which has no analogue in the current result-data shape. Coupled to the `Field<X,Y>` in param position TODO (#3117) — every existing field-typed slot in `ElasticResult/ShellStress` is `Real` placeholder.

### M-008: `tsai_wu(...)` stdlib failure-criterion function

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `tsai_wu` grep hit anywhere in repo. Closest precedent: `von_mises_stress` field on `AnalysisResult` in `stdlib/analysis.ri:30,36` (a scalar field, not a function). No stdlib function precedent for "stress × allowables → failure index field" mapping.
- **Blocks:** M-011 (failure-index result field).
- **Note:** Requires both M-001 (allowables in `OrthotropicMaterial`) and M-007 (per-ply stress fields) to be wired before this function has well-defined inputs.

### M-009: `hashin(...)` stdlib failure-criterion function

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `hashin` grep hit. Same shape as M-008.
- **Blocks:** M-011.
- **Note:** Hashin distinguishes fibre-tension/fibre-compression/matrix-tension/matrix-compression modes — output cardinality higher than scalar Tsai-Wu.

### M-010: `max_strain(...)` stdlib failure-criterion function

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `max_strain` grep hit. Same shape as M-008.
- **Blocks:** M-011.

### M-011: Per-failure-criterion failure-index field in `ElasticResult`

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `failure_index` grep hit. PRD says "plus failure-index field per failure criterion." `ElasticResult` (`solver_elastic.ri:295`) does not declare any failure-index cell; `Field<X,Y>` in param position TODO (#3117) still gates field-typed result cells.
- **Blocks:** GUI composite-result rendering (not yet PRD'd).
- **Note:** Cardinality grows with criterion count × ply count — UX/data-shape open question.

### M-012: Inter-laminar shear stress recovery (equilibrium post-processing)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No post-processing equilibrium-recovery pass in `crates/reify-solver-elastic/` (only `error_estimator.rs` and direct stress evaluation `shell_result.rs`). PRD acknowledges "Standard but not free in implementation."
- **Blocks:** Practical composite analysis (delamination is the dominant failure mode per PRD).
- **Note:** PRD-flagged open issue; mentioned but neither task nor code stub exists.

### M-013: Layup helpers (symmetric, balanced, quasi-isotropic constructors)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No grep hit. PRD: "Helpers for symmetric, balanced, and quasi-isotropic layups."
- **Blocks:** Convenience layer; not load-bearing.
- **Note:** Sugar around M-004; whether stdlib fn or constructor variants is open.

### M-014: Tabular layup import helper (external JSON/TOML/spreadsheet)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `ImportHelper`, `read_toml`, `import_csv`, `json_load` grep hit in `crates/reify-compiler/stdlib/` or `crates/reify-eval/`. Adjacent infrastructure: `field_import_provenance.rs` for VDB/CSV ingestion, but that targets `Field<X,Y>` not structure-of-structs literal data. PRD calls this an open design question ("lean: import helper for tabular cases").
- **Blocks:** Not load-bearing; deferred-of-deferred.
- **Note:** Cross-cuts a broader open question about whether Reify gains a generic stdlib-data-from-file mechanism.

## Cross-PRD breadcrumbs

- **`structural-analysis-shells.md` (v0.4)** — this PRD's hard prerequisite. Mid-surface extraction, MITC3+ kinematics, `ShellStress` shape, `@shell` annotation all live there. Status per parent PRD: "design resolved + decomposed (2026-05-05) — deferred."
- **`structural-analysis-fea.md` (v0.3)** — gates the entire FEA stack including `ElasticResult`, `ElasticOptions`, solver loop. Composite extends `ElasticResult` shape.
- **`multi-load-case-fea.md` (v0.3.x)** — PRD says "composes with multi-load-case" for per-load-case envelopes. Envelope helpers (`envelope_von_mises`, `linear_combine`) in `fea_multi_case.ri` are scalar/single-stress-field — extending to per-ply, per-criterion envelopes is an additional cross-cut.
- **`fea-gui-rendering-shells.md` (v0.4)** — PRD says "composes with" for per-ply visualisation; sibling PRD is itself deferred.
- **`structural-analysis-progressive-damage.md`** — PRD seeds this hypothetical follow-on; not filed.
- **GR-001 (structure-constructor runtime eval)** — every proposed stdlib structure (`OrthotropicMaterial`, `Laminate`, `Ply`, instances of starter library like `T300_5208`) hits this gap. No mechanism in this PRD is unblocked by GR-001's resolution alone, but every mechanism is blocked by it.
- **TODO(field-in-param, task #3117)** — per-ply stress/strain/failure-index result fields all need `Field<X,Y>` in param position, same as existing `ElasticResult.stress/frame/displacement`.
- **Task 2227 (`List<TraitObject>` call-site conformance)** — done; partially relevant if `List<Ply>` is typed as `List<TraitObject>`-of-`Ply`. If `Ply` is a concrete struct (not a trait object), the call-site conformance check for `List<<ConcreteStruct>>` in param position is not confirmed wired by audit memory.
