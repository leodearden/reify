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

> **Re-audit notice (2026-09-30, task #7237, re-verified against main `28e442d0ac`):** M-003…M-014 were
> each re-verified; every one carries its own dated `CORRECTION` blockquote, and the evidence lives
> there. This supersedes the 2026-09-03 "NON-EXHAUSTIVE" clause under M-001 and the "`max_strain` was not
> re-checked" caveat above (checked: one test-local variable, see M-010). M-001 and M-002 keep their
> 2026-09-03 overlays; M-002's State line was not re-measured then and is not re-measured here.
>
> - **Tally.** M-003 FICTION → PARTIAL; M-004…M-014 State FICTION unchanged. Evidence partly superseded
>   or incomplete: M-004, M-005, M-007, M-008, M-011, M-012; stands in substance: M-006, M-009, M-010,
>   M-013, M-014.
> - **Top-concerns bullet 1, clause by clause.** `OrthotropicMaterial` exists (M-001 overlay).
>   `Laminate`, `Ply`, `tsai_wu`, `hashin` and `max_strain` are still absent as symbols under `crates/`
>   (the "or PRDs" scope is not re-asserted: the composite PRD names them). "Already-broken
>   structure-constructor evaluation (GR-001)" and "unresolved `Field<X,Y>` param-position issue (TODO
>   #3117)" are both SUPERSEDED: GR-001 State is DONE (see M-004) and #3117 is done (see M-007).
>   "Not-yet-shipped parent shells PRD" has two measured facts, neither adjudicated here: the parent PRD's
>   `Status:` line (`docs/prds/v0_4/structural-analysis-shells.md`) still reads "design resolved +
>   decomposed (2026-05-05) — deferred", and its shell route is live in `solve_elastic_static` (task 3594
>   done), isotropic-only (see M-005).
> - **Bullets 2 and 4** are covered by the M-003, M-005, M-006, M-007 and M-004 overlays. **Bullet 3
>   re-measured TRUE:** `search_tasks` ("composite laminated shell ply laminate Tsai-Wu Hashin layup")
>   finds nothing at score threshold 0.45; at 0.3 the nearest hit is task 3014 (isotropic shell
>   stiffness) and none of the hits owns a ply, laminate or failure-criterion mechanism. The composite
>   PRD's `Status:` line still reads "stub — deferred, candidate v0.5+".

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

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-003 is PARTIAL, not
> FICTION: the per-material 6×6 and its frame rotation shipped; the shell/ply consumer did not.** This is
> a dated audit snapshot (**Date:** 2026-05-12), so the bullets above are preserved as the record of what
> was measured then. The 2026-09-03 pointer above this row flagged it as stale without adjudicating it;
> this overlay does the adjudication.
>
> - **Shipped: both primitives the Evidence says are absent.** `OrthotropicMaterial::d_matrix_local` and
>   `TransverseIsotropicMaterial::d_matrix_local` (`crates/reify-solver-elastic/src/constitutive.rs`; line
>   numbers are in the M-001 overlay) build the 6×6 in the material frame. `rotate_voigt(d_local,
>   rotation)` (same file, `:615`) is the general local→global Bond rotation, `D_global = T·D_local·Tᵀ`;
>   rotation about the shell normal by a fibre angle is the special case pinned by test
>   `rotate_voigt_30deg_about_z_matches_lamina_transformation_with_correct_sign`
>   (`crates/reify-solver-elastic/tests/constitutive_laws.rs`). All three first landed 2026-05-26, commit
>   `7abf09ed11` (`git log -S`), two weeks after this snapshot. So both Evidence sentences — "the only
>   D-matrix builder" and "No rotation by fibre orientation; no orthotropic 6×6 stiffness routine" — are
>   SUPERSEDED. (The drift of the `constitutive.rs:88` anchor is recorded in the 2026-09-03 pointer above.)
> - **Consumed by SOLID assembly only.** `AnisotropicMaterial::from_law` (`material_field.rs`) wraps a
>   `ConstitutiveLaw` and a frame into the value that `MaterialField::material_at` returns, and its
>   `d_matrix_global()` applies `rotate_voigt`. `MaterialField` feeds four solid element entry points:
>   `element_stiffness_p1_with_field` and `element_stiffness_p2_with_field` (`assembly/tet.rs`),
>   `element_stiffness_hex_p1_with_field` (`assembly/hex.rs`) and
>   `element_stiffness_wedge_p1_with_field` (`assembly/wedge.rs`). A `git grep -E
>   'rotate_voigt|MaterialField|ConstitutiveLaw|AnisotropicMaterial|OrthotropicMaterial'` over
>   `shell_assembly.rs`, `shell_solve.rs`, `shell_kinematics.rs`, `shell_result.rs`, `shell_boundary.rs`,
>   everything under `elements/` (including `degenerate_shell.rs` and `mitc3_plus.rs`) and the engine's
>   `reify-eval` `compute_targets/shell_solve.rs` returns no hit.
> - **Still absent, which is why this is PARTIAL and not DONE.** (i) The shell plane-stress reduction of a
>   6×6 orthotropic D: the shell kernels' only plane-stress builder is `plane_stress_d(material:
>   &IsotropicElastic) -> [[f64; 3]; 3]` (`shell_assembly.rs:182`), which is 3×3 and isotropic. (ii) Any
>   per-ply construction loop. The composite PRD's own 2026-05-26 companion edit (commit `25b5c374ed`)
>   assigns that reduction to *this* PRD and not to the anisotropic foundation: "Composites owns only the
>   *shell plane-stress reduction* of that law plus the ply-stack through-thickness integration and
>   failure criteria" (`docs/prds/v0_5/composite-laminated-shells.md`).
> - **Blocks line unchanged.** "M-004, M-005" still holds for the missing half: both rows are still
>   FICTION (see their overlays below).

### M-004: `Laminate` stdlib structure with `plies : List<Ply>` ordered stack

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `Laminate`, `Ply`, or stdlib `List<<StructureName>>` of concrete (non-trait) structs in `materials_fea.ri` or `solver_elastic.ri`. Closest precedent: `fea_multi_case.ri:50` uses `List<LoadCase>` as a list of structures, but typed as `List<Real>` placeholder per the `Field<X,Y>`-in-param TODO (#3117); kind-match silently accepts the runtime list. Whether the same placeholder-list mechanism transfers to `List<Ply>` is unverified by any audit memory.
- **Blocks:** M-005 (kernel iterates the ply list), M-008 (helper functions).
- **Note:** Coupled to GR-001 (struct-ctor eval) and the open design question of constructor surface (list-literal vs dedicated ctor vs external file).

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-004's State (FICTION)
> holds; its `List<Real>`-placeholder precedent is superseded.** This is a dated audit snapshot
> (**Date:** 2026-05-12), so the bullets above are preserved as the record of what was measured then.
>
> - **State holds.** `Laminate`, `Ply` and `plies` have no definition under `crates/`:
>   `git grep -n -P '\bLaminate\b|\bPly\b|\bplies\b' -- crates` returns no hit.
> - **The Evidence's closest precedent is gone.** It cited `fea_multi_case.ri:50` as `List<LoadCase>`
>   "typed as `List<Real>` placeholder"; line 50 of that file is now a comment in the header block above
>   `structure def LoadCase`, and the placeholder is retired. `LoadCase.loads : List<Load>` and
>   `LoadCase.supports : List<Support>` (`fea_multi_case.ri:82`, `:88`; task ζ/4444, done) are lists of
>   TRAIT objects (`Load` and `Support` are traits, declared in `fea_types.ri`) whose elements are
>   conformance-checked at compile time (`TypeNotConformingToTrait`); a comment in `solver_elastic.ri`
>   records their tightening from `List<Real>`. `solve_load_cases(... cases : List<LoadCase> ...)`
>   (`fea_multi_case.ri:659`, plus the body-arg overload at `:706`) takes a list of a CONCRETE structure
>   in fn-param position; `git log -S'fn solve_load_cases'` dates its first appearance to commit
>   `dbae0d1779` (2026-05-30), after this snapshot.
> - **"No stdlib `List<<StructureName>>` of concrete structs" is true only as literally scoped.** Over
>   `materials_fea.ri` and `solver_elastic.ri` the only declared list-of-name types are the trait lists
>   `List<Load>` and `List<Support>`. Stdlib-wide the sentence is false: `BucklingResult.modes :
>   List<BucklingMode>` (`solver_buckling.ri:222`), `PiecewisePolynomialProfile.waypoints :
>   List<Waypoint>` (`trajectory.ri:290`) and `Toolpath.beads : List<Bead>` / `Toolpath.layers :
>   List<Layer>` (`fdm_slice.ri:98`, `:100`) are concrete-structure list params.
> - **Call-site rejection of a wrong-typed element in a `List<ConcreteStruct>` param: not re-measured by
>   this overlay.** The closest pinned behaviour is a different site — element conformance of the trait
>   lists on `LoadCase(...)` constructor arguments — pinned by
>   `loadcase_bare_numeric_in_loads_emits_type_not_conforming`,
>   `loadcase_bare_numeric_in_supports_emits_type_not_conforming` and
>   `loadcase_cross_trait_in_loads_emits_type_not_conforming`
>   (`crates/reify-compiler/tests/harness_diagnostics_robustness/multi_load_case_stdlib_tests.rs`). No test
>   was found that passes a wrong-typed element to a concrete-structure list fn param such as
>   `solve_load_cases(cases: ...)`. The stdlib source documents only a run-time outcome for that case (the
>   contract comment above `solve_load_cases` lists "any element of `cases` is not a `LoadCase`
>   StructureInstance" among its silent-Undef failure modes), and no test pinning that was found either, so
>   this overlay asserts no enforcement. This is the open half of Top-concerns bullet 4.
> - **The Note's coupling to GR-001 is superseded.** `gap-register.md` GR-001 State is DONE (2026-05-26:
>   SIR-α task 3540 + SIR-β-mat task 3542). The Note's other clause, the constructor surface (list-literal
>   vs dedicated constructor vs external file), is the stub PRD's own open question ("**Layup syntax**")
>   and is not something GR-001 resolves.

### M-005: Through-thickness sum-over-plies integration in shell element kernel

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `crates/reify-solver-elastic/src/shell_assembly.rs:10-11` explicitly: "Reissner-Mindlin shell element under a **constant-thickness isotropic** linear-elastic constitutive law. Through-thickness integration is..." (analytical, single material). `:118` describes it as "Baked in as a private constant — it is a property of the through-thickness". Task 3014 ("Shells T6: shell stiffness assembly under isotropic linear-elastic constitutive law") confirms "Constant-thickness, isotropic D matrix. Through-thickness integration analytical (closed form for membrane + bending + transverse shear contributions)."
- **Blocks:** M-006, M-007.
- **Note:** The PRD says "the through-thickness integration becomes a sum over plies with discontinuous derivatives at ply boundaries" — this is a structural rewrite of the shell stiffness assembly path, not an additive extension.

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-005's State (FICTION)
> holds: no sum-over-plies exists; "constant-thickness" and "analytical" are no longer universal.** This is
> a dated audit snapshot (**Date:** 2026-05-12), so the bullets above are preserved as the record of what
> was measured then.
>
> - **The quoted kernel is unchanged, with one anchor drift.** The `shell_assembly.rs:10-11` quote
>   ("Reissner-Mindlin shell element under a constant-thickness isotropic linear-elastic constitutive law.
>   Through-thickness integration is…", continuing "closed-form" on `:12`) is still verbatim and describes
>   the flat MITC3 kernel. The `:118` "Baked in as a private constant" line is now at `:163`, in the doc
>   comment of `const KAPPA` (the 5/6 shear-correction factor, declared at `:165`).
> - **New since this snapshot: a degenerated continuum-shell kernel, still single-material.**
>   `degenerate_stiffness_core` (private) and its public wrappers `shell_element_stiffness_degenerate`,
>   `shell_element_stiffness_degenerate_ans` and `shell_element_stiffness_degenerate_ans_bubble`
>   (`shell_assembly.rs`; tasks 4068, 4069, 4065; the first wrapper landed 2026-05-31, commit
>   `936190b553`). It integrates through the thickness NUMERICALLY — 2-point Gauss in ζ — with per-node
>   `thicknesses: &[f64; 3]`, so "analytical, single material" is no longer true of every shell kernel. But
>   it still takes one `&IsotropicElastic` and evaluates `plane_stress_d` once (`:942`), before its
>   quadrature loops (`:976`): ζ has no ply partition. Outside `shell_assembly.rs` and the crate's
>   `tests/`, `git grep -n shell_element_stiffness_degenerate -- crates
>   ':!crates/reify-solver-elastic/src/shell_assembly.rs' ':!crates/reify-solver-elastic/tests'` finds the
>   `lib.rs` re-export and crate-doc example, `// G-allow:` comment lines in
>   `elements/degenerate_shell.rs`, and mentions in a `reify-audit` test that pins those markers — no call
>   expression. (Those G-allow comments describe the wrappers as reached on the shell-routing compute path;
>   this overlay found no by-name call site for that, and the next bullet shows the engine route calling
>   the MITC3+ kernel.)
> - **The engine shell route is isotropic-only.** `solve_flat_plate_shell`
>   (`crates/reify-solver-elastic/src/shell_solve.rs:102`) calls `shell_element_stiffness_mitc3_plus`
>   (`:151`) with a `&IsotropicElastic`. The `solve_elastic_static` trampoline
>   (`crates/reify-eval/src/compute_targets/elastic_static.rs`, guard at `:715`) refuses a non-isotropic
>   material on the Shell route: under `ShellForce::On` it aborts with no tet fallback, and under
>   `ShellForce::Auto` it warns and falls back to the tet/solid path (`ShellForce::Off` already routes
>   Tet). That policy first landed 2026-06-01, commit `9f12a281d6`. An `OrthotropicMaterial` reaches the
>   guard as `MaterialModel::Anisotropic`, so it cannot reach any shell kernel today.
> - **The task-3014 record stays true of the flat kernel.** Task 3014 (done; last updated 2026-05-08) is
>   the record the Evidence quotes ("Constant-thickness, isotropic D matrix. Through-thickness integration
>   analytical …"); the flat kernels it describes still integrate in closed form.

### M-006: Per-Gauss-point layered constitutive evaluation

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** Shell kinematics module (`shell_kinematics.rs:44`) returns kinematic primitives only — no per-Gauss-point material evaluation hook; D matrix is computed once at element scope from the single material. No infrastructure for "compute D per Gauss point as a layered stack rather than a single isotropic relation."
- **Blocks:** M-005.
- **Note:** New code path; would need either a per-Gauss-point material callback or an unrolled per-ply integration scheme.

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-006's State (FICTION)
> holds; a per-point material lookup now exists for SOLIDS at element granularity only.** This is a dated
> audit snapshot (**Date:** 2026-05-12), so the bullets above are preserved as the record of what was
> measured then.
>
> - **The cited shell module is unchanged.** `pub fn shell_kinematics` is still at
>   `crates/reify-solver-elastic/src/shell_kinematics.rs:44` (no drift) and still returns kinematic
>   primitives only. Every shell kernel still computes D once per element call: `plane_stress_d` is
>   evaluated once in `shell_element_stiffness`, once in `shell_element_stiffness_mitc3_plus` and once in
>   `degenerate_stiffness_core` (there before its in-plane × ζ quadrature loops).
> - **New since this snapshot, solids only.** `pub trait MaterialField { fn material_at(&self, point) ->
>   AnisotropicMaterial }` (`crates/reify-solver-elastic/src/material_field.rs:116`; first landed
>   2026-05-27, commit `917dfe8c37`). Its module doc says the assembly hook samples ONE D per element at
>   the element centroid, and it is wired into the solid element entry points only (tet P1/P2, hex P1,
>   wedge P1 — see the M-003 overlay). That is a material lookup at element granularity, not a layered one.
> - **Per-Gauss-point layered evaluation — the mechanism this row names — is still absent.** No shell
>   kernel consults `MaterialField` (zero hits, see the M-003 overlay); the shell kernels compute one D per
>   element call and `MaterialField` is sampled once per element at the centroid.

### M-007: Per-ply stress and strain result fields in `ElasticResult`

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `ElasticResult` in `stdlib/solver_elastic.ri:295-316` has `displacement, stress, frame, max_von_mises, converged, iterations` only. `ShellStress` (`:352-356`) has `top, mid, bottom` — a 3-channel through-thickness shape designed for **single-material** outer/neutral/inner fibres, NOT per-ply (the comment at `:343-345` is explicit: "preserves the invariant that ShellStress always has all three channels populated even for solid-element results"). No precedent for `List<Field<...>>` or per-ply indexed field collections.
- **Blocks:** All composite-result consumers (GUI, multi-load-case envelopes).
- **Note:** PRD says "top, mid, bottom of each ply" — a 3 × N_plies result tensor, which has no analogue in the current result-data shape. Coupled to the `Field<X,Y>` in param position TODO (#3117) — every existing field-typed slot in `ElasticResult/ShellStress` is `Real` placeholder.

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-007's State (FICTION)
> holds; its field list, its quoted ShellStress invariant and its Real-placeholder Note are superseded.**
> This is a dated audit snapshot (**Date:** 2026-05-12), so the bullets above are preserved as the record
> of what was measured then.
>
> - **Anchors and field list.** The Evidence's `solver_elastic.ri:295-316` is now `structure def
>   ElasticResult` at `:504`. Its params are `displacement`, `stress`, `divergence`, `gradient`, `curl`,
>   `rotation`, `shear_angles`, `frame`, `shell_channels : ShellStress`, `max_von_mises`, `converged`,
>   `iterations`, `error_indicator`, `global_relative_energy_error` and `convergence_status`; none is
>   per-ply. `:352-356` is now `structure def ShellStress` at `:686`, still exactly `top`, `mid`, `bottom`:
>   a 3-channel through-thickness shape, not a per-ply one.
> - **The quoted invariant is RETIRED.** "ShellStress always has all three channels populated even for
>   solid-element results" no longer holds, and the sentence is gone from `solver_elastic.ri`. Task 4067
>   sets `ElasticResult.shell_channels` to `Value::Undef` on tet/solid results (the tet path in
>   `crates/reify-eval/src/compute_targets/elastic_static.rs` writes `("shell_channels", Value::Undef)`,
>   `:1604`). The comment block above `structure def ShellStress` states the new rule: "Do NOT fabricate
>   homogeneous top==mid==bottom for tets".
> - **The Note's "every existing field-typed slot … is `Real` placeholder" is SUPERSEDED.** It was true on
>   the audit date: `git show 8059aa59ba:crates/reify-compiler/stdlib/solver_elastic.ri` (a 2026-05-12
>   commit) has `ElasticResult` at `:295-316` and `ShellStress` at `:352-356` exactly as this row cites
>   them, with `displacement`, `stress`, `frame`, `top`, `mid` and `bottom` all declared `Real`. They were
>   tightened after the snapshot: `displacement` and `stress` on 2026-05-14 (task 3117, `e6517887d5`),
>   `frame` and `top`/`mid`/`bottom` on 2026-05-15 (task 3641, `df7c8d11cf`); both tasks are done. At this
>   SHA all six carry precise `Field<...>` types, recorded in the "Resolution note (tasks 3117 + 3641)"
>   comment above `ElasticResult` (`solver_elastic.ri:423`). The Note's coupling to the "`Field<X,Y>` in
>   param position TODO (#3117)" is superseded with it.
> - **"No precedent for `List<Field<...>>`" still holds.** `git grep -n 'List<Field<' --
>   crates/reify-compiler/stdlib` returns no hit.

### M-008: `tsai_wu(...)` stdlib failure-criterion function

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `tsai_wu` grep hit anywhere in repo. Closest precedent: `von_mises_stress` field on `AnalysisResult` in `stdlib/analysis.ri:30,36` (a scalar field, not a function). No stdlib function precedent for "stress × allowables → failure index field" mapping.
- **Blocks:** M-011 (failure-index result field).
- **Note:** Requires both M-001 (allowables in `OrthotropicMaterial`) and M-007 (per-ply stress fields) to be wired before this function has well-defined inputs.

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-008's State (FICTION)
> holds; its "no stdlib function precedent" sentence overlooked `safety_factor`, and its `analysis.ri`
> anchor has drifted.** This is a dated audit snapshot (**Date:** 2026-05-12), so the bullets above are
> preserved as the record of what was measured then.
>
> - **State holds.** `git grep -n -P '\btsai_?wu\b|\bTsaiWu\b' -- crates` returns no hit. The claim is
>   scoped to `crates/`: the audit's "anywhere in repo" is not re-asserted, because the PRD under `docs/`
>   names it.
> - **Anchor drift.** `analysis.ri:30,36` is now `trait AnalysisResult` at `:34`, with `param
>   von_mises_stress : Stress` at `:35`.
> - **The Evidence's "No stdlib function precedent for stress × allowables → failure index field" did
>   not account for `safety_factor`.** That builtin (`crates/reify-builtins/src/registry.rs`, the `name:
>   "safety_factor"` row, arity 2) takes a stress tensor or stress-tensor field plus one scalar yield
>   strength and returns `yield/von_mises`, dimensionless: the reciprocal of a von Mises failure index. A
>   field argument yields a `Field<D, Real>` (`compute_safety_factor`,
>   `crates/reify-expr/src/analysis.rs`); `examples/fields_analysis.ri` calls it on a raw 3×3 tensor. It
>   predates the snapshot: `compute_safety_factor` is already present at `8059aa59ba` (a 2026-05-12
>   commit), and `git log -S'fn compute_safety_factor'` dates its first appearance to `e0cbb4a6da`
>   (2026-04-15). Nothing in the registry takes more than that one allowable: its Analysis-family rows
>   are `von_mises`, `max_shear`, `principal_stresses`, `safety_factor` and `stress_invariants`, and only
>   `safety_factor` has an allowable argument, so the five ply allowables of a Tsai-Wu criterion have no
>   analogue there.
> - **The Note's M-001 dependency is unchanged.** The allowables half is still absent: no word-bounded
>   `X_T`, `X_C`, `Y_T` or `Y_C` appears in `crates/reify-compiler/stdlib` (`S` is too short to grep
>   usefully); see the M-001 overlay. The per-ply-field half is M-007, also still absent.

### M-009: `hashin(...)` stdlib failure-criterion function

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `hashin` grep hit. Same shape as M-008.
- **Blocks:** M-011.
- **Note:** Hashin distinguishes fibre-tension/fibre-compression/matrix-tension/matrix-compression modes — output cardinality higher than scalar Tsai-Wu.

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-009's State (FICTION)
> holds.** This is a dated audit snapshot (**Date:** 2026-05-12), so the bullets above are preserved as
> the record of what was measured then. `git grep -n -P '\bhashin\b|\bHashin\b' -- crates` returns no hit.
> The grep is word-bounded on purpose: a bare `hashin` matches "hashing" and hits 36 files under `crates/`.

### M-010: `max_strain(...)` stdlib failure-criterion function

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `max_strain` grep hit. Same shape as M-008.
- **Blocks:** M-011.

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-010's State (FICTION)
> holds; the snapshot's literal "No `max_strain` grep hit" now has one non-symbol hit site.** This is a
> dated audit snapshot (**Date:** 2026-05-12), so the bullets above are preserved as the record of what
> was measured then.
>
> - `git grep -n -P '\bmax_strain\b' -- crates` returns four lines, all one site: the `let mut max_strain`
>   local in `crates/reify-solver-elastic/src/elements/degenerate_shell.rs` (`:1969-1990`), inside the
>   `#[cfg(test)]` module (opens at `:996`), in test
>   `degenerate_assumed_membrane_b_is_frame_objective_under_rigid_rotation`. It is a test-local
>   variable, not the composite failure criterion; no stdlib or runtime `max_strain` symbol exists.
> - As to `crates/`, the snapshot's literal grep claim was true when written: `git log -S'max_strain' --
>   crates` shows the token entering `crates/` in a single commit, `e0bbecd846` (2026-05-31, task 4069),
>   after the snapshot date.

### M-011: Per-failure-criterion failure-index field in `ElasticResult`

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `failure_index` grep hit. PRD says "plus failure-index field per failure criterion." `ElasticResult` (`solver_elastic.ri:295`) does not declare any failure-index cell; `Field<X,Y>` in param position TODO (#3117) still gates field-typed result cells.
- **Blocks:** GUI composite-result rendering (not yet PRD'd).
- **Note:** Cardinality grows with criterion count × ply count — UX/data-shape open question.

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-011's State (FICTION)
> holds; its #3117 gating clause is superseded.** This is a dated audit snapshot (**Date:** 2026-05-12), so
> the bullets above are preserved as the record of what was measured then.
>
> - **State holds.** `git grep -n -E 'failure_index|FailureIndex' -- crates` returns no hit, and
>   `structure def ElasticResult` (`solver_elastic.ri:504`) declares no failure-index cell.
> - **The #3117 gating clause is SUPERSEDED.** "`Field<X,Y>` in param position TODO (#3117) still gates
>   field-typed result cells" no longer holds: #3117 is done. The delta is recorded once, in the M-007
>   overlay, and is not restated here.
> - **A shape precedent now exists on `ElasticResult`.** `error_indicator : Option<Field<Point3<Length>,
>   Pressure>> = none` (`solver_elastic.ri:624`) is an optional scalar indicator field; the source comment
>   documents it as a per-element stress-norm error indicator for visualisation, `none` when not computed.

### M-012: Inter-laminar shear stress recovery (equilibrium post-processing)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No post-processing equilibrium-recovery pass in `crates/reify-solver-elastic/` (only `error_estimator.rs` and direct stress evaluation `shell_result.rs`). PRD acknowledges "Standard but not free in implementation."
- **Blocks:** Practical composite analysis (delamination is the dominant failure mode per PRD).
- **Note:** PRD-flagged open issue; mentioned but neither task nor code stub exists.

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-012's State (FICTION)
> holds; the Evidence's parenthetical list of the crate's stress post-processing is incomplete.** This is
> a dated audit snapshot (**Date:** 2026-05-12), so the bullets above are preserved as the record of what
> was measured then.
>
> - **State holds.** `git grep -n -P 'inter_?laminar|Interlaminar' -- crates` returns no hit.
>   `git grep -n -E 'pub fn [a-z_0-9]*(stress|recover)' -- crates/reify-solver-elastic/src` lists the
>   crate's stress entry points, and none is equilibrium-based: `element_stress_p1` and
>   `element_stress_p2` (`result.rs`), `shell_element_stress` (`shell_result.rs`) and
>   `membrane_stress_delta` (`membrane_load.rs`) evaluate σ directly from the displacement, and
>   `recover_nodal_stress_p1` is volume-weighted nodal averaging. The doc comment of
>   `shell_element_stress` still gives transverse shear as "uniform across layers".
> - **The parenthetical is incomplete.** "(only `error_estimator.rs` and direct stress evaluation
>   `shell_result.rs`)" omits `recover_nodal_stress_p1` (`result.rs:427`), which `error_estimator.rs`
>   consumes (its module doc: "not the full superconvergent patch-recovery (SPR) least-squares fit"), and
>   the per-element σ recovery in `buckling_kernel.rs`, which calls `element_stress_p1` /
>   `element_stress_p2`. The `recover_nodal_stress_p1` omission predates the snapshot rather than
>   following it: `git log -S'fn recover_nodal_stress_p1'` dates the function to `0c86fd8c68`
>   (2026-05-10).

### M-013: Layup helpers (symmetric, balanced, quasi-isotropic constructors)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No grep hit. PRD: "Helpers for symmetric, balanced, and quasi-isotropic layups."
- **Blocks:** Convenience layer; not load-bearing.
- **Note:** Sugar around M-004; whether stdlib fn or constructor variants is open.

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-013's State (FICTION)
> holds.** This is a dated audit snapshot (**Date:** 2026-05-12), so the bullets above are preserved as
> the record of what was measured then. The Evidence's "No grep hit" named no pattern. The patterns
> re-run here are `git grep -n -P '\blayup|\bLayup|quasi_isotropic|symmetric_layup|balanced_layup' --
> crates`, which returns no hit, and the case-insensitive `git grep -n -i -E
> 'layup|quasi.?isotropic|symmetric.?lamin|balanced.?lamin' -- crates`, which returns none either.

### M-014: Tabular layup import helper (external JSON/TOML/spreadsheet)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `ImportHelper`, `read_toml`, `import_csv`, `json_load` grep hit in `crates/reify-compiler/stdlib/` or `crates/reify-eval/`. Adjacent infrastructure: `field_import_provenance.rs` for VDB/CSV ingestion, but that targets `Field<X,Y>` not structure-of-structs literal data. PRD calls this an open design question ("lean: import helper for tabular cases").
- **Blocks:** Not load-bearing; deferred-of-deferred.
- **Note:** Cross-cuts a broader open question about whether Reify gains a generic stdlib-data-from-file mechanism.

> **CORRECTION 2026-09-30 (task #7237, re-verified against main `28e442d0ac`) — M-014's State (FICTION)
> holds.** This is a dated audit snapshot (**Date:** 2026-05-12), so the bullets above are preserved as
> the record of what was measured then.
>
> - `git grep -n -P 'ImportHelper|read_toml|import_csv|json_load' -- crates` returns no hit. The audit
>   scoped its grep to `crates/reify-compiler/stdlib/` and `crates/reify-eval/`; this one covers all of
>   `crates/`.
> - The adjacent infrastructure is where the Evidence left it:
>   `crates/reify-eval/src/field_import_provenance.rs` exists at this SHA and already existed at
>   `8059aa59ba` (a 2026-05-12 commit). Its module doc still describes the provenance record for an
>   imported field.
> - `crates/reify-compiler/stdlib/io.ri` declares only traits, structures and enums, and no `fn` appears
>   in it. The Note's open question, a generic stdlib-data-from-file mechanism, is not answered by
>   anything this overlay found.

## Cross-PRD breadcrumbs

- **`structural-analysis-shells.md` (v0.4)** — this PRD's hard prerequisite. Mid-surface extraction, MITC3+ kinematics, `ShellStress` shape, `@shell` annotation all live there. Status per parent PRD: "design resolved + decomposed (2026-05-05) — deferred."
- **`structural-analysis-fea.md` (v0.3)** — gates the entire FEA stack including `ElasticResult`, `ElasticOptions`, solver loop. Composite extends `ElasticResult` shape.
- **`multi-load-case-fea.md` (v0.3.x)** — PRD says "composes with multi-load-case" for per-load-case envelopes. Envelope helpers (`envelope_von_mises`, `linear_combine`) in `fea_multi_case.ri` are scalar/single-stress-field — extending to per-ply, per-criterion envelopes is an additional cross-cut.
- **`fea-gui-rendering-shells.md` (v0.4)** — PRD says "composes with" for per-ply visualisation; sibling PRD is itself deferred.
- **`structural-analysis-progressive-damage.md`** — PRD seeds this hypothetical follow-on; not filed.
- **GR-001 (structure-constructor runtime eval)** — every proposed stdlib structure (`OrthotropicMaterial`, `Laminate`, `Ply`, instances of starter library like `T300_5208`) hits this gap. No mechanism in this PRD is unblocked by GR-001's resolution alone, but every mechanism is blocked by it.
- **TODO(field-in-param, task #3117)** — per-ply stress/strain/failure-index result fields all need `Field<X,Y>` in param position, same as existing `ElasticResult.stress/frame/displacement`.
- **Task 2227 (`List<TraitObject>` call-site conformance)** — done; partially relevant if `List<Ply>` is typed as `List<TraitObject>`-of-`Ply`. If `Ply` is a concrete struct (not a trait object), the call-site conformance check for `List<<ConcreteStruct>>` in param position is not confirmed wired by audit memory.
