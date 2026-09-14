# Goal-oriented (dual-weighted) error estimation: honor `ElasticOptions.target_quantity_of_interest`

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-09-11 · **Approach:** B + H (contract + two-way boundary tests)

**Code anchors** verified against main `191d6c85ba` (2026-09-11). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Provenance:** chartered by task **#7177** under Leo's ruling of 2026-09-01 (esc-7076-1), authored in the
`/prd` session that discharges **esc-7177-3**, on Leo's authorisation of 2026-09-11. The six-item scope
in #7177's brief is the charter; every measurement in §3 was re-taken first-hand in this session at the
anchor SHA, and the draft was put through an adversarial design review whose corrections are folded in
(the point-functional regularization of §5.1, the fallible estimator seam of §5.5, the CLI-reachability
finding of §3). It continues, and closes, the deferral recorded in
`docs/prds/v0_4/a-posteriori-error-estimation.md` ("Rejected DWR — needs Reify-language syntax for a
quantity of interest + an adjoint solve of comparable cost; real v0.5+"). Its dependency gate, **#4909**
(real size-field remeshing that consumes the Dörfler marking), is merged (`f963ac9811`).

**Normative substrate:** `docs/legibility/design-invariants.md` — this PRD **consumes** **INV-PD-1**
(`declared-param-reaches-kernel`, owned by `docs/prds/v0_6/trampoline-param-drop-closure.md`) by moving
`target_quantity_of_interest` out of its `ignored` allowlist into `honored`, and **INV-PD-2**
(`result-fields-populated-or-owned`) for the two result fields it adds. It establishes no new invariant.
Full G7 walk: §9.1.

---

## 1. Goal

One user-observable guarantee:

> A designer who names the **one number they care about** — deflection near the tip, normal stress near
> a fillet — gets an adaptive solve that spends refinement **where it moves that number**, reports the
> number together with an **estimate of its own error**, and stops when that error is below the accuracy
> they asked for. A quantity of interest that cannot be honored is an **Error**, never a silently dropped
> knob.

Today none of that exists. `ElasticOptions.target_quantity_of_interest : Option<QoIDescriptor>` is
declared, `QoIDescriptor` has **zero variants**, `none` is its only reachable value, and the eval layer
never reads the field. The only adaptive estimator is global energy-norm (Zienkiewicz–Zhu), which makes
the **whole** displacement field converge even when the designer needs one localized number.

After this PRD:

- **A QoI is nameable.** `QoIDescriptor` gains `LocalDisplacement { at, radius, direction }` and
  `LocalNormalStress { at, radius, normal }` — coordinate-addressed, parametric, re-resolvable on every
  refined mesh, and **regularized** (a mean over a small ball, never a point delta — §5.1).
- **A dual-load assembler and a dual-weighted indicator exist** in `reify-solver-elastic`; the dual
  **solve** runs beside the primal in `solve_cantilever_fea`, reusing the already-assembled,
  row-eliminated `K` (§5.3).
- **The dual-weighted indicator steers Dörfler marking** beside the Z-Z indicator, selected by the QoI.
- **The result carries the answer.** `ElasticResult.qoi` holds the QoI value on the **final** mesh plus
  its signed error estimate, readable by an ordinary `match`; `qoi_relative_error` is the dimensionless
  quantity the loop converged, readable by `unwrap_or` like `global_relative_energy_error`.
- **Nothing is dropped silently.** A QoI on a path that cannot evaluate it, or without `adaptive: true`,
  is a coded Error; the two existing code-less adaptive warnings gain `DiagnosticCode`s.
- **The vacuity is discharged.** The param leaves the PDROP C1 `ignored` set and #7177 stops being its
  owner.

## 2. Consumers

| Consumer | What it consumes |
|---|---|
| **#7085** — PDROP detector + owning-task liveness lane | Resolves #7177's liveness. When leaf ε lands the param is `honored` and carries no allowlist entry. |
| **#7080** — trampoline param-drop leaf β (`ElasticOptions` C1 declaration) | Writes the elastic C1 declaration. Its `target_quantity_of_interest` row cites this PRD's leaf ε (#7457) as owner (amended at decompose; it previously cited terminal #7076). |
| **#7086 / `/audit`** — PDROP gate consumers | Read the flipped declaration. |
| **`docs/prds/v0_4/a-posteriori-error-estimation.md`** | Its "DWR future-proofing" hook becomes the DWR driver, exactly as that PRD promised. |
| **Every author of a data-carrying enum** | Leaf α lifts the v1 "payload values must be compile-time literals" restriction. Two stdlib sites already work around it: `result.ri`'s `ok_or` (returns the bare param instead of `Err { error: err }`) and `dynamics.ri`'s `JointForceValue` marker-trait encoding ("revisit if non-constant payload construction … lands"). |
| FEA authors | "How accurate is *this* number, and refine until it is accurate enough." |

In-engine seam: §3.4 ComputeNode dispatch (`docs/prds/v0_3/engine-integration-norm.md`) — the
`solver::elastic_static` trampoline reached through the existing dispatch table. Leaf α extends an
existing `CompiledExprKind` family, not a new seam.

## 3. Measured state

All measured 2026-09-11 at `191d6c85ba`, by three enumeration passes, first-hand probes, and an
adversarial re-verification.

**The param is uninhabitable, unread, and undiagnosed.** `enum QoIDescriptor {}` (stdlib
`solver_elastic.ri`) has zero variants; `extract_adaptive_params` (`elastic_static.rs`) reads exactly
`adaptive`, `target_accuracy`, `max_refinement_iterations`, `max_dofs`; there are **zero** hits for
`target_quantity_of_interest` in `crates/reify-eval/` and `crates/reify-solver-elastic/`; the only
cites of `#7177` in tracked source are two rows of `trampoline-param-drop-closure.md`. **Four** tests
pin today's shape and move with leaf γ: the three `QoIDescriptor` stub pins
(`qoi_descriptor_enum_is_an_empty_stub`, the `Option<Enum("QoIDescriptor")>` member-type pin, the
`OptionNone` default pin) and the **`ElasticResult` exactly-13-param-cell assertion**, all in
`crates/reify-compiler/tests/harness_geometry_solver/solver_elastic_tests.rs`.

**No adjoint substrate.** `git grep` for `adjoint|dual|goal.oriented|dwr|quantity of interest` over
both crates finds only "self-adjoint" (eigensolver symmetry) and "dual-mode/dual-source" (engine
concepts). No dual solve, no dual load assembler, no goal functional.

**The primal solve is Jacobi-PCG, assembled in `reify-eval`, and discards its system.**
`solve_cantilever_fea` (`elastic_static.rs`) calls the solver crate's `assemble_global_stiffness`,
builds `f`, applies `apply_dirichlet_row_elimination` (zero row, zero column, unit diagonal, pin RHS —
the constrained `K` stays **symmetric**; every `DirichletBc.value` on this path is `0.0`), runs
`solve_cg_with_warm_state`, and returns `CantileverFeaSolve { u, coords, tet_connectivity, nodal_stress,
… }`. `K`, `f` and the `DirichletBc` set are locals of that `reify-eval` function; `solve_cg` borrows `K`,
so a second solve on `(&k, &g)` needs no solver-crate API change. There is no factorization to reuse: **a
dual solve is a second CG solve of the same cost as the primal**, the cost the v0.4 PRD priced.

**Both adaptive lanes call the same primal.** `CantileverAdaptiveProblem` (uniform, #4902) and
`RealizedAdaptiveProblem` (localized, #4909) both implement `AdaptiveProblem::solve_and_estimate` as
`solve_cantilever_fea` + `compute_zz_indicator`; both run `deterministic: true` (cold-start CG).
Whatever the primal returns, both lanes see.

**The seam is estimator-blind, infallible, and energy-norm-named.** `AdaptiveProblem` (`adaptive.rs`)
is `fn solve_and_estimate(&mut self) -> AdaptiveEstimate` + `fn refine(…) -> Result<(), Self::Error>`;
only `refine` can fail, so `run_adaptive_refinement` has **no exit for an estimation error**.
`AdaptiveEstimate { global_indicator, per_element, n_dofs }` documents `global_indicator` as "global
relative energy-norm error", `RefinementBudget.target_accuracy` as "relative energy-norm error target",
and the loop compares them directly; `mark_dorfler` is called on `per_element` unconditionally.

**Recovery is the substrate the indicator needs, and it is all present.** `element_stress_p1`,
`recover_nodal_stress_p1` (volume-weighted patch average), `compute_zz_indicator`'s per-element
`η_e² = V_e · Δσᵀ S Δσ` (`energy_density_voigt` over `compliance_matrix`, positive definite on symmetric
tensors), `barycentric_p1`, `locate_element_p1` (lowest containing index wins; `tol` is a
**scale-invariant barycentric slack**, documented as never to be scaled by edge length),
`interpolate_p1_at_point`, `apply_point_load`. **Absent:** face adjacency, interior residual, face-jump
`[[σ·n]]`, any linear-functional-of-`u` assembler (the only precedent is the test-private
`mean_tip_deflection` in `analytical_validation.rs`).

**A single-node point functional is mesh-divergent, and the repo has measured it.**
`analytical_validation.rs`'s module doc records that a single-node force is "a discrete point singularity
whose local displacement spike grows without bound as the mesh refines (measured P1 error went 2.3 % →
3.4 % → 6.2 % → 10.3 % from 12³ to 24³)", and reads the cantilever's deflection as a **face mean** for
exactly that reason. A point-displacement QoI's dual load is that single-node force. §5.1 regularizes.

**A remesh preserves neither node indices nor B-rep attribution.** The #4909 lane forces the uniform
fallback whenever a selector-resolved BC is present, and every `VolumeMesh` the gmsh refiner returns
carries `boundary: None`. Anything that addresses the QoI by face selector or node set is
**unresolvable after the first refinement**; §5.1 makes the QoI coordinate-addressed.

**What `reify eval` can reach today (probed, `target/debug/reify` of 2026-09-08 and `target/release/reify`
of 2026-09-01, identical results).** The **dims** overload with `adaptive: true` runs the uniform lane
end-to-end from the CLI. On the committed fixture `tests/prd-gate/fixtures/dwr_cantilever_energy.ri`
(`target_accuracy: 0.02`, `max_refinement_iterations: 2`), a **cold-cache** run reports
`info: adaptive refinement finished at 90375 DOFs on a 240×4×24 grid`,
`convergence_status: ConvergenceStatus::NotConverged`, `global_relative_energy_error: Some(0.10577012532863311)`
— the C1 baseline every later run of that fixture is held to. (A one-iteration scratch probe of the same
box reported 14157 DOFs on a 120×2×12 grid and 0.181; different budget, same lane.) The **body** overload
does **not** run from the CLI: it registers no gmsh kernel (`warning: … no gmsh kernel registered (call
ensure_gmsh_kernel())`), the realization falls back, and the trampoline never solves. The localized lane
is reachable only from the Rust e2e harness (`solve_elastic_static_body_e2e.rs`, `ensure_gmsh_kernel`,
coordinate BCs). **Consequence:** CLI-observable signals use the dims overload and the uniform lane; the
localized lane is asserted from Rust.

**`reify eval` is served by a persistent FEA cache, and diagnostics are not replayed on a hit.** The
elastic result is persisted under `~/.cache/reify/fea` (`REIFY_CACHE_DIR` overrides;
`crates/reify-eval/src/compute_persist.rs`). A second run with the same cache dir returns byte-identical
stdout and **zero bytes of stderr**: the `info:`/`warning:` lines above appear only on a miss. An
`ElasticOptions` knob edit does reach the cache key (`target_accuracy` 0.02 → 0.01 produced a second
entry), but `target_quantity_of_interest` is unconstructible today, so its membership in the key is
**unverified** — §5.7 makes it a leaf-γ obligation, and every eval-stderr assertion in this PRD runs
against a fresh `REIFY_CACHE_DIR`. The missing replay itself is pre-existing and not this PRD's (§7).

**Reading the result today.** A `match` over an `Option<T>` with `some(binder)`/`none` arms does **not
parse** — `match_pattern` in `tree-sitter-reify/grammar.js` admits variant-binding patterns, bare
identifiers and `_`, never a call form, and `Option` is the built-in `Value::Option`, not an enum. The
stdlib reads options through `unwrap_or` / `is_some` / `map_or` (`option_recovery.ri`,
`examples/m6_fallback_recovery.ri`). A plain enum with a unit variant, by contrast, is matched with the
shipped DCE patterns; a bare-variant *value* must be type-qualified (`QoIEstimate.NoQoi`) while patterns
stay unqualified. Both forms were probed at the anchor SHA and pass `reify check` and `reify eval`
(`tests/prd-gate/fixtures/dwr_qoi_readback.ri`). §5.6 is shaped by this.

**Data-carrying enum payloads must be compile-time literals.** Probed with `reify check`: a
`LocalDisplacement { at: Point3<Length>, radius: Length, direction: Vector3<Dimensionless> }`
declaration with `param q : Option<QoIDescriptor> = none` **compiles**; constructing it with
`at: point3(length, …)` — or even the all-literal `dz: -1.0`, a unary-minus *expression* — **parses**
(grammar gate exit 0, zero ERROR nodes) but `reify check` exits 1 with `error: non-constant payload value
for field '…' … is not yet supported`. The restriction is `variant_construct.rs`'s `Literal`-only payload
arm, whose comment defers "the runtime constructor node" to a follow-up **citing no task id**;
`crates/reify-mcp/src/tools/chunks/enums.md` documents it under "Payload limits". **No live task owns
lifting it** (task search, 2026-09-11). Leaf α owns it here. Bare-variant construction resolves a name by
**silent first match across every in-scope enum** (`variant_construct.rs`, pinned by
`local_result_enum_coexists_with_prelude_result_and_prelude_wins_first_match`); two cross-enum
duplicates already exist (`Cubic`, `Triangular`), and the stdlib name-collision gate indexes enum
*names*, not variant names.

**The two adaptive warnings carry no code.** In `elastic_static.rs`'s adaptive branch: the
**non-isotropic-material fallback** warning and the **knobs-set-but-`adaptive: false`** warning are both
`Diagnostic::warning(…)` with `code: None`. `DiagnosticCode` has no `Elastic*`, `Adaptive*` or `Qoi*`
variant; `BucklingOptionUnsupported` is the workspace's only coded knob diagnostic. `adaptive: true` with
`element_order: P2` degrades to P1 upstream of the estimator — `volume_mesh_to_solver_mesh` returns
`None` for a non-P1 mesh and the synthetic P1 box is built instead — so the adaptive branch is
P1-isotropic in practice, with `compute_zz_indicator`'s P1 assert as the last line of defence.

**PDROP has not landed.** #7079, #7080 and #7085 are pending; zero hits for `ParamNotHonored`/`PDROP`
in the Rust tree. #7080's description still names **#7076** (terminal) as this param's owner — a stale
cite this decompose corrects.

## 4. What DWR is, and why the recovery form

The primal problem is `a(u, v) = f(v)`; its P1 Galerkin solution is `u_h`, with **homogeneous** Dirichlet
data (true on this path — §3). A quantity of interest is a bounded linear functional `J(u)`. Its dual
(adjoint) problem is `a(z, v) = J(v)` — "how sensitive is `J` to a residual introduced at each point".
Two identities drive everything below:

1. **Error representation.** `J(u) − J(u_h) = J(e_u) = a(z, e_u) = a(z − z_h, e_u) = a(e_z, e_u)`,
   by Galerkin orthogonality `a(e_u, z_h) = 0` for the dual solved on the **same** P1 space with the
   same homogeneous constraints. The QoI error is the energy inner product of the two errors.
   **This needs `J` bounded on `H¹`.** A point value `d·u(x)` in 3D is not — its load is a Dirac delta —
   which is the divergence §3 measured. §5.1 therefore defines every shipped QoI as a **mean over a
   ball**, an `L²` functional, for which (1) holds and the effectivity has a limit to converge to.
2. **Discrete reciprocity.** With `K` symmetric after elimination, `J(v) = gᵀv`, and `g` zeroed at the
   constrained DOFs (where `u_h = 0`, so `gᵀu_h` is unchanged): `K z_h = g` ⟹
   `J(u_h) = gᵀu_h = z_hᵀ K u_h = z_hᵀ f`. The QoI evaluated directly equals the load evaluated on the
   dual, to CG tolerance. This is BT1.

**Two ways to make (1) computable.** The *residual* form writes `a(e_z, e_u)` as
`Σ_K ρ_K(u_h)(z − z_h)` with interior residuals and face jumps of `σ_h·n`, then approximates `z − z_h`
by a higher-order or recovered dual. The *recovery* form (Cirak & Ramm's reciprocal-theorem estimator)
replaces both errors by their stress-recovery approximations:

```
η_K  =  V_K · (σ*_u − σ_{u,h})_Kᵀ · S · (σ*_z − σ_{z,h})_K        (Voigt; S = compliance)
J(u) − J(u_h)  ≈  Σ_K η_K                                            (signed)
```

`σ*` is the volume-weighted nodal patch recovery `recover_nodal_stress_p1` already computes and
`σ_{·,h}` the elementwise constant P1 stress. (`compute_zz_indicator` spells its difference
`σ_h − σ*`; the form is bilinear, so the sign convention cancels.) **This PRD chooses the recovery form:**

- It is the **bilinear generalization of the landed Z-Z indicator**: with `z_h = u_h` it is exactly
  `η_e² = V_e · Δσᵀ S Δσ` from `compute_zz_indicator`. One recovery mechanism, two indicators, and a
  boundary test that pins them to each other (BT2).
- It needs **only existing substrate** (§3). The residual form needs face adjacency, interior residuals,
  a face-jump integrator and a higher-order dual — none exists, and P1 interior residuals are trivially
  zero, so all its information would sit in face jumps built from scratch.
- Its per-element contribution is **signed**; the sum is a genuine estimate of `J(u) − J(u_h)`, not a
  bound.

Marking uses `|η_K|`; convergence uses the conservative `Σ_K |η_K|` (§5.4). Nonlinear QoIs (von Mises)
fit the seam by linearization `J'(u_h)` — the trait in §6 takes `u_h` for that reason — but none ships
here (§8).

## 5. Design decisions (resolved; do not re-open)

### 5.1 QoI surface — ball means, coordinate-addressed, parametric, linear

```reify
enum QoIDescriptor {
    // Mean of d · u over the ball of radius `radius` centred at `at`.
    LocalDisplacement { at: Point3<Length>, radius: Length, direction: Vector3<Dimensionless> },
    // Mean of n · σ · n over the same ball (normal stress on the plane with normal `normal`).
    LocalNormalStress { at: Point3<Length>, radius: Length, normal:    Vector3<Dimensionless> },
}
```

Construction is the shipped unqualified brace form, in `param` defaults and constructor arguments:

```reify
param length : Length = 1000mm
let result = solve_elastic_static(material, length, width, height, [tip_load], [mount],
    ElasticOptions(adaptive: true, target_accuracy: 0.02,
        target_quantity_of_interest: LocalDisplacement {
            at: point3(length, width / 2.0, height / 2.0), radius: 20mm,
            direction: vec3(0.0, 0.0, -1.0) }))
```

- **Regularized, not pointwise.** The functional is the volume-weighted mean over the contributing
  element set `E = { K : centroid(K) ∈ ball(at, radius) }`, falling back to `E = { K ∋ at }` (the one
  element containing `at`, by `locate_element_p1`) when no centroid lies in the ball. With the P1
  one-point rule, `J(u_h) = Σ_{K∈E} V_K q_K / Σ_{K∈E} V_K` where `q_K = d·ū_K` (the nodal mean of the
  four `d·u_i`) or `nᵀσ_K n`; the dual load is `g_i += (V_K / V_E)(¼)d` for each node `i` of each
  `K ∈ E` (displacement) or the analogous `g_e = B_eᵀ D (n⊗n)` weights (stress), obtained by applying
  `element_stress_p1` to the twelve unit displacements — the columns of a linear map are the images of
  the basis. `E` is never empty, `g` is never zero for a non-zero direction, and as `h → 0` the
  functional converges to the true ball mean — an `L²` functional, bounded on `H¹`, so identity (1)
  holds and effectivity has a limit. `radius` is a **required, positive** payload field; `radius ≤ 0mm`
  is `E_QoiUnresolvable`. A default "small" radius was rejected: it would silently reintroduce the
  point delta whose divergence §3 measured.
- **Coordinate-addressed, not selector-addressed** (§3): `E` is re-derived on every mesh from
  coordinates; nothing about the QoI is an index into a mesh that will be thrown away. Face-restricted
  QoIs ("mean over this face") need a face re-resolution the refiner cannot give today and are out of
  scope (§8) — the ball mean is the regularized neighbour that *is* buildable.
- **Parametric** — `at: point3(length, …)` tracks `param length`. This is why leaf α exists.
- **Linear functionals only**, so identities (1) and (2) hold exactly. `LocalNormalStress` is linear
  because P1 element stress is `σ_K = D B_K u_K`.
- **Variant names are unique across the stdlib, and that uniqueness is unenforced** (§3: silent
  first-match resolution, no variant-name gate). `LocalDisplacement`, `LocalNormalStress`,
  `DisplacementEstimate`, `NormalStressEstimate` have zero hits today; leaf γ lands a test asserting each
  appears in exactly one `enum_def` of the concatenated stdlib, since nothing else will. The `Local`
  prefix also keeps the variant visually distinct from the paren-constructed `PointLoad(…)` structure
  that sits in the same `solve_elastic_static(…)` call.
- `direction`/`normal` are **normalized by the extractor**; a zero vector is `E_QoiUnresolvable`.

### 5.2 Indicator — recovery-form dual weighting; Z-Z is its self-dual special case

Per §4. `compute_dual_weighted_indicator(primal_elems, dual_elems, mesh, material) ->
DualWeightedIndicator { per_element_signed, qoi_error_estimate, qoi_error_bound }` with
`qoi_error_estimate = Σ η_K` (signed) and `qoi_error_bound = Σ |η_K|`. It shares
`recover_nodal_stress_p1` and the compliance contraction with `compute_zz_indicator`; the quadratic form
becomes a bilinear form. **`compute_zz_indicator` itself is not refactored** — its goldens stay
byte-identical — and BT2 pins the special case instead.

**Marking weights.** DWR marks on `|η_K|`, an energy-homogeneous weight — the standard bulk criterion
for dual-weighted estimators ("mark until the marked contributions reach `θ` of the total estimate").
Z-Z marks on `η_e`, a square root. Dörfler's prefix rule is not invariant under squaring, so at the same
`θ = 0.5` the DWR marked set is smaller and more concentrated than Z-Z's on the same field. **That is
deliberate**: the two modes' marked-set sizes are not comparable and no test may assert they are.

### 5.3 Dual solve — the same constrained `K`, a second CG solve, homogeneous data

`solve_cantilever_fea` keeps its assembled, row-eliminated `K` alive past the primal solve and, when a
QoI is present, assembles `g` (`QuantityOfInterest::dual_load`), **zeroes `g` at every constrained DOF**
— the dual always has homogeneous Dirichlet data, whatever the primal prescribed; the column-into-RHS
term of `apply_dirichlet_row_elimination` does not apply because `K` is already eliminated, so this is
one loop, not a "half" of that function — and runs `solve_cg` on `(&k, &g)` with the primal's
`CgSolverOptions` and `SolverMode`. Cost: one primal-equivalent solve per iteration. No warm-start across
meshes (a remesh preserves no DOF numbering); the primal already accepts that. Dual non-convergence is
`W_QoiDualNotConverged` and the estimate is still reported — the posture the primal takes with
`converged: false`.

### 5.4 Convergence quantity — conservative relative bound; signed estimate reported

`AdaptiveEstimate.relative_error` in QoI mode is `qoi_error_bound / |J(u_h)|`. Conservative on purpose:
signed cancellation between elements can make `|Σ η_K|` small while the field is far from converged,
and a stopping rule must not be fooled by cancellation. The **signed** sum is what the designer wants for
correcting the number (`J ≈ J(u_h) + estimate`), so it is reported in `ElasticResult.qoi`.
`target_accuracy` keeps its name and default and is re-documented as "relative error target for the
selected estimator"; no second accuracy knob.

**Degenerate `J(u_h)`.** When `|J(u_h)|` is below the documented floor (§11 Q3) the ratio is not finite
and must not reach the loop: `solve_and_estimate` returns `Err(QoiDegenerate)`, the loop stops, the
trampoline emits `W_QoiDegenerate` naming the floor, reports the last completed solve as
`NotConverged { TargetMissed }`, populates `qoi` (value and absolute error are still meaningful) and sets
`qoi_relative_error` to `none` with that recorded cause. A non-finite payload is never written (C7).

### 5.5 Selection seam — the QoI selects the estimator; the loop becomes fallible

No indicator trait. `solve_and_estimate` already owns "solve and estimate"; the two production
implementations branch on `Option<&QoiSpec>`: absent → Z-Z; present → primal + dual + dual-weighted
indicator, **and Z-Z as well**, so `global_relative_energy_error` keeps its honest meaning in QoI mode
(§5.6). A trait would be a third abstraction for two implementations that share one seam; rejected.

Two changes to `adaptive.rs` are authorized, and only these:

1. `fn solve_and_estimate(&mut self) -> Result<AdaptiveEstimate, Self::Error>`, and
   `run_adaptive_refinement` propagates the error exactly as it propagates `refine`'s. Today the seam
   is infallible, so a QoI that becomes unresolvable on iteration 3 would have **no exit** — an
   implementer would have to panic or loop on a garbage dual, the silent shape §5.7 closes. The test
   stubs' `Infallible` error type makes the migration mechanical. The eval-side error type becomes an
   enum over `RefineError | QoiError`.
2. `AdaptiveEstimate.global_indicator` is renamed `relative_error` (its energy-norm meaning was baked
   into the name; six read sites, all in `adaptive.rs` and the two impls), documented as "the
   estimator-defined dimensionless quantity compared with `target_accuracy`", and the struct gains
   `qoi: Option<QoiEstimate { value, error_estimate, error_bound }>`. `RefinementBudget.target_accuracy`'s
   doc moves with it. `per_element` is documented as non-negative marking weights (`η_e` for Z-Z, `|η_K|`
   for DWR; §5.2).

`mark_dorfler`, the budget checks and the stall rule are untouched.

### 5.6 Result surface — a typed value, not an SI-erased scalar

```reify
enum QoIEstimate {
    NoQoi,                                                    // no QoI was requested (declared-absent)
    DisplacementEstimate { value: Length,   error: Length   },
    NormalStressEstimate { value: Pressure, error: Pressure },
}
structure def ElasticResult {
    …
    param convergence_status : ConvergenceStatus = Converged { final_indicator: 0.0 }
    // Appended AFTER convergence_status: structure constructors bind positionally.
    param qoi : QoIEstimate = QoIEstimate.NoQoi     // FINAL-mesh value + SIGNED error estimate
    param qoi_relative_error : Option<Real> = none  // Σ|η_K| / |J(u_h)|, what the loop converged
}
```

- A single `Option<Real>` was rejected because the two QoIs have different dimensions and `Real` is
  genuinely dimensionless; a dimensioned `Option<Scalar>` was considered — `Value::Scalar` carries its
  dimension at runtime, as `max_von_mises` does — but a single field whose dimension changes with the
  descriptor cannot be typed statically in `.ri`, so a `constraint` over it cannot be written. The
  enum is typed per variant.
- **`qoi` is an enum, not an `Option<enum>`** (§3: `match` cannot destructure an `Option`, and nested
  destructuring is unsupported). `NoQoi` is the typed declared-absent value — the `ConvergenceStatus`
  precedent for a result field that is never `Undef` — and the shipped DCE `match` reads the rest.
  `qoi_relative_error` stays `Option<Real>` because that is exactly `global_relative_energy_error`'s
  shape and read path, and its degenerate case (§5.4) needs a real `none`.
- **Read-back form**, verified at the anchor SHA (`tests/prd-gate/fixtures/dwr_qoi_readback.ri`):
  ```reify
  let tip : Length = match result.qoi {
      DisplacementEstimate { value: v, error: e } => v,
      NormalStressEstimate => 0mm,
      NoQoi => 0mm
  }
  let rel : Real = unwrap_or(result.qoi_relative_error, 0.0)
  ```
  Leaf ζ's exemplar demonstrates it. Payload dot-access is out of scope (§8).
- `qoi.value` reflects the **final refined mesh** — the whole point — unlike `displacement`/`stress`,
  which by #4902's ratified contract reflect the seed mesh (§8).
- **`global_relative_energy_error` keeps its meaning in QoI mode**: the Z-Z indicator is computed on
  every iteration alongside the DWR one (the seed-mesh Z-Z pass already runs for `error_indicator`) and
  populates it honestly. It is never the QoI ratio under an energy-norm name. `error_indicator` stays the
  Z-Z stress-error field (§7). In QoI mode `convergence_status.Converged { final_indicator }` carries
  `qoi_relative_error`.
- **Three producer arms** write the a-posteriori fields — `aposteriori_adaptive_fields`,
  `aposteriori_nonadaptive_default_fields`, and the non-isotropic fallback arm — and all three must
  write the two new fields (`some`/`none`) for C7's "no third state" to hold; the engine builds
  `ElasticResult` as an explicit `StructureInstance`, so declaring the params is necessary, not sufficient.
  The exactly-13-cell assertion moves to 15 in the same diff.

### 5.7 Loudness — a QoI that cannot be honored is an Error; existing warnings gain codes

One principle: **a requested QoI is either evaluated or refused with a code; it is never `none` with a
shrug.**

| Situation | Response |
|---|---|
| QoI set, `adaptive: false` | `E_QoiRequiresAdaptive` (Error). The one-solve-no-refine idiom is `adaptive: true, max_refinement_iterations: 0`, which reports the estimate honestly as `NotConverged { MaxIterations }`. **Why an Error beside a Warning in the same arm:** the budget knobs are tuning for a loop that is not running; a QoI is a request for a *number* the result will not contain. Their asymmetry is deliberate, and §8 pins that the knob warning's severity is not retyped here. |
| QoI set on a path that cannot evaluate it — shell route, non-isotropic material, `radius ≤ 0`, `at` outside every element on any iteration, zero `direction`/`normal` | `E_QoiUnresolvable` (Error) naming the reason. A silently zero dual would make every `η_K = 0` and report "converged" instantly — the exact silent-failure shape this PRD closes. |
| `|J(u_h)|` below the floor | `W_QoiDegenerate`; §5.4. |
| QoI set, uniform fallback lane (gmsh unavailable, selector BCs, or the dims overload) | Evaluated. The estimate is lane-agnostic; only the *marking* is discarded, which the existing fallback warning already reports. |
| Dual CG does not converge | `W_QoiDualNotConverged`; estimate reported. |
| A QoI edit against a warm persistent FEA cache | **Must miss.** `target_quantity_of_interest` (every payload field) enters the cache key exactly as the other `ElasticOptions` knobs do, and an Error outcome is never served from cache. Leaf γ verifies both; the missing diagnostic replay on a hit is pre-existing (§7). |
| Existing non-isotropic-material fallback warning | gains `DiagnosticCode::ElasticAdaptiveMaterialUnsupported`; text unchanged. |
| Existing knobs-set-but-`adaptive: false` warning | gains `DiagnosticCode::ElasticAdaptiveKnobInert`; text unchanged. |

**Normative names**, fixed here and bound by the manifest: `QoiRequiresAdaptive` (E),
`QoiUnresolvable` (E), `QoiDualNotConverged` (W), `QoiDegenerate` (W),
`ElasticAdaptiveMaterialUnsupported` (W), `ElasticAdaptiveKnobInert` (W). All minted in leaf γ, the
only leaf that emits them. `E_PARAM_NOT_HONORED` is **not** this PRD's to emit — it is PDROP's
(#7079/#7080); leaf ε flips the declaration that PDROP reads.

## 6. The contract (H component)

Seam: `reify-solver-elastic` (assembler + indicator + estimate carrier) ↔ `reify-eval`
(`solve_cantilever_fea`, which owns `K` and runs both solves, and the two `AdaptiveProblem` impls).

```rust
/// Borrowed P1 tet mesh view — the f64 coordinates + connectivity the eval side
/// already holds (`VolumeMesh` stores f32 vertices and is not the solve mesh).
pub struct P1TetMeshRef<'a> { pub coords: &'a [[f64; 3]], pub tets: &'a [[usize; 4]] }

/// A bounded linear functional of the displacement field, re-resolvable on any mesh.
pub trait QuantityOfInterest {
    /// J(u_h) on this mesh. Err when the QoI cannot be resolved here (C4).
    fn evaluate(&self, mesh: P1TetMeshRef<'_>, material: &IsotropicElastic, u: &[f64]) -> Result<f64, QoiError>;
    /// g with J(v) = gᵀv, length 3·n_nodes, NOT yet zeroed at constrained DOFs (the caller
    /// owns the BC set). Takes u_h so a linearized nonlinear functional fits later. Err per C4.
    fn dual_load(&self, mesh: P1TetMeshRef<'_>, material: &IsotropicElastic, u: &[f64]) -> Result<Vec<f64>, QoiError>;
    /// Which `QoIEstimate` variant (hence dimension) `evaluate` returns.
    fn kind(&self) -> QoiKind;
}
pub struct DualWeightedIndicator { pub per_element_signed: Vec<f64>, pub qoi_error_estimate: f64, pub qoi_error_bound: f64 }
pub fn compute_dual_weighted_indicator(primal: &[StressElement<'_>], dual: &[StressElement<'_>], mesh: &VolumeMesh, material: &IsotropicElastic) -> DualWeightedIndicator;
pub struct QoiEstimate { pub value: f64, pub error_estimate: f64, pub error_bound: f64 }
pub struct AdaptiveEstimate { pub relative_error: f64, pub per_element: Vec<f64>, pub n_dofs: usize, pub qoi: Option<QoiEstimate> }
pub trait AdaptiveProblem { type Error; fn solve_and_estimate(&mut self) -> Result<AdaptiveEstimate, Self::Error>; fn refine(&mut self, marked: &[usize]) -> Result<(), Self::Error>; }
```

- **C1 — Absent QoI is the identity.** With no QoI, `solve_cantilever_fea`, both lanes and every
  landed a-posteriori golden take the same calls in the same order as before this PRD. Structural: the
  dual branch is behind `Option::None`.
- **C2 — Reciprocity.** For every shipped QoI and every mesh, `evaluate(u_h) == fᵀz_h` within
  `10·cg_tolerance·|J(u_h)|` (identity (2); the slack is derived from the shared `CgSolverOptions` and
  moves with it).
- **C3 — Self-dual reduction.** When the primal load **is** the QoI's dual load, `f = F·g` with `F = 1`
  and both solves cold-started in `SolverMode::Deterministic`, `z_h` is **bit-identical** to `u_h` and
  `per_element_signed` equals `compute_zz_indicator`'s `η_e²` elementwise, bit-for-bit; for `F ≠ 1` or
  a warm start the agreement is within `10·cg_tolerance / (relative magnitude of Δσ)`, and the test
  must state that derivation rather than "to rounding". Consequently every `η_K ≥ 0` and the estimate
  is `≥ 0` — the minimum-potential-energy inequality `fᵀu_h ≤ fᵀu` in discrete form, a property of the
  quadratic form, not a physical claim.
- **C4 — Unresolvable is typed.** `radius ≤ 0`, a zero direction, or a point outside every element at
  `locate_element_p1`'s documented barycentric slack returns `QoiError`, never a zero `g`, never `NaN`.
- **C5 — Signs and bounds.** `per_element_signed` may be negative; `per_element` handed to `mark_dorfler`
  is `|η_K|`; `qoi_error_bound ≥ |qoi_error_estimate|`; `relative_error = qoi_error_bound / |J(u_h)|`.
- **C6 — Re-resolution.** `evaluate` and `dual_load` are called afresh on every mesh the loop produces;
  no QoI state survives a `refine`.
- **C7 — Result population (INV-PD-2).** When a QoI was requested and the solve completed,
  `ElasticResult.qoi` is a `DisplacementEstimate`/`NormalStressEstimate` with **finite** payloads and
  `qoi_relative_error` is `some(…)` finite, or `none` with the `W_QoiDegenerate` cause recorded (§5.4);
  when no QoI was requested `qoi` is `NoQoi` and `qoi_relative_error` is `none` (declared-absent, the
  `error_indicator` convention). There is no third state.

### Two-way boundary tests

| # | Scenario | Preconditions | Asserts |
|---|---|---|---|
| **BT1** | Reciprocity | any fixture pencil, both QoI kinds, seed mesh | `evaluate(u_h)` equals `fᵀz_h` within `10·cg_tolerance·|J|` (C2) |
| **BT2** | Z-Z is the self-dual case | a purpose-built solver-crate fixture whose primal load is the QoI's own `dual_load` (`f = g`, `F = 1`), `SolverMode::Deterministic`, cold start | `z_h` bit-identical to `u_h`; `per_element_signed` equals `η_e²` elementwise; every `η_K ≥ 0`; `qoi_error_estimate > 0` (C3). Pins the algebraic reduction only — never a convergence claim |
| **BT3** | Patch test is exact, and the dual is real | uniform-stress primal on the existing Z-Z patch-test mesh **plus a non-trivial** `LocalDisplacement` dual (`Σ|z_h| > 0`) | every `η_K` is `0` within absolute `1e-12` (the existing test's tolerance) **and** BT1 holds on the same fixture — so the zero is P1 exactness, not a zero dual |
| **BT4** | Unresolvable is typed | `at` outside the body; `direction = vec3(0,0,0)`; `radius: 0mm` | `QoiError`, no panic, no zero `g` (C4); through `reify eval`, `E_QoiUnresolvable` with non-zero exit |
| **BT5** | Selection really switched (CLI) | committed **dims-overload** fixture pair `tests/prd-gate/fixtures/dwr_cantilever_{energy,qoi}.ri`, identical except the QoI, uniform lane, **fresh `REIFY_CACHE_DIR`** | the QoI run reports `qoi = DisplacementEstimate { … }` and `qoi_relative_error = some(finite)`; its `convergence_status` is either `Converged { final_indicator }` with `final_indicator` equal to `qoi_relative_error`, or `NotConverged { reason }` with `qoi_relative_error` still populated (the committed budget does **not** guarantee convergence: the energy twin ends `NotConverged` at 0.10577 against a 0.02 target); the energy twin's stdout is byte-identical to today's cold-cache baseline (C1); a second QoI run against the first's cache dir is a cache **miss** for the QoI, i.e. the QoI enters the key |
| **BT6** | Localized lane with a QoI (Rust) | body overload, coordinate BCs (no `target:`), `ensure_gmsh_kernel`, extends `body_adaptive_solve_runs_the_gmsh_realized_localized_lane` | the localized lane ran with a QoI (the "adaptive refinement finished at … DOFs; the gmsh-realized" diagnostic), `qoi` populated; the DWR-vs-Z-Z marked-set overlap is **recorded** in the test log, not asserted (§5.2) |
| **BT7** | Runtime payload | `Rect { width: w, height: w * 2.0, sign: -1.0 }` with `param w`, plus a multi-line construction whose last field is a quantity literal followed by a newline | `reify eval` constructs the value and a `match` reads the fields back; `reify check` no longer emits "non-constant payload value"; the multi-line case's value is unchanged by the line break (INV-SF-7) |
| **BT8** | Degenerate QoI | a QoI whose `J(u_h)` is identically zero by symmetry | `W_QoiDegenerate`, `qoi_relative_error = none`, `qoi` populated, loop terminated, exit 0 (§5.4) |

## 7. Cross-PRD relationship and seam ownership

| Seam | Owner | Note |
|---|---|---|
| C1 declaration mechanism, `E_PARAM_NOT_HONORED` | **#7079** (PDROP α) | Not this PRD's. Leaf ε flips whatever declaration exists; if none, records the honored state for #7080 to write. |
| `ElasticOptions` C1 declaration | **#7080** (PDROP β) | Cites leaf ε as this param's owner (amended at decompose from stale #7076). No dependency edge in either direction — an edge would invert the contract (#7080's own text). |
| PDROP detector + liveness | **#7085** | Consumes ε's flip. |
| Runtime payload construction for data-carrying enums | **this PRD, leaf α** | Extends `docs/prds/v0_6/data-carrying-enums.md` (SHIPPED) beyond its recorded v1 limit; that PRD is not re-opened. The `enums.md` chunk "Payload limits" is corrected in α's diff. |
| `ElasticResult.error_indicator` (Pa-valued Z-Z stress error, seed mesh, GUI channel) | **#4910 / #4906**, landed | **Unchanged** in QoI mode. A signed DWR visual channel is §8. |
| Seed-mesh primary fields vs refined-mesh a-posteriori fields | **#4902's ratified contract** | Inherited, not changed: `qoi` joins the refined-mesh side. |
| `ElasticOptions.mesh_size` / elastic `element_order` | **#7074 / #7075** | Untouched; the adaptive branch is P1 by construction (§3). |
| Adaptive-vs-uniform convergence-rate validation | **#3002** | Untouched. Leaf δ measures QoI effectivity, not energy-norm rates. |
| `solver::multi_case` (`LoadCase.options`) | **out of scope** | No adaptive plumbing there; the QoI is unreachable through it. Its PDROP declaration (#7082) decides that param's disposition on that path. |
| CLI gmsh-kernel registration for the body overload | **unowned; pre-existing** | §3. Not created here; CLI signals use the dims overload. Named so nobody re-derives it at dispatch. |
| Persistent FEA cache does not replay diagnostics on a hit | **unowned; pre-existing** (`compute_persist.rs`) | §3. An INV-SF-3 gap in its own right — a cached result's warnings vanish on the second run — but not created here. This PRD only requires the QoI to enter the key and never to serve an Error from cache. |
| `match` over `Option<T>` with `some(x)`/`none` patterns | **absent, and not needed** | §3/§5.6. `qoi` is a plain enum; options are read with `unwrap_or`/`is_some`. No grammar work is chartered. |
| Selector-addressed / face-restricted QoIs | **unowned; out of scope** | §8. |

No reciprocal-ownership statement exists in any of these.

## 8. Out of scope

- **Face- or region-restricted QoIs** ("mean over this face"). Needs a face re-resolution the refiner
  cannot provide (`boundary: None` after remesh). The ball mean is the buildable neighbour.
- **Nonlinear QoIs (von Mises, principal stress).** Fit the seam via `J'(u_h)`; no variant ships here.
- **Residual-form DWR / higher-order dual.** Rejected in §4 on substrate grounds; revisit only if the
  effectivity leaf δ measures is unacceptable.
- **A signed DWR per-element visual channel.** `error_indicator` stays Z-Z (§7).
- **Refined-mesh primary fields.** #4902's contract; the §7a resample grid that #4910 depends on.
- **Coarsening, p-refinement, multi-QoI, QoI on shells, QoI through `multi_case`, QoI from the CLI on
  the body overload** (the gmsh-kernel registration gap, §3/§7).
- **Payload dot-access, partial binding, qualified payload construction** — `enums.md`'s other limits
  are untouched by leaf α, which lifts exactly the literal-only rule.
- **Retyping the existing adaptive warnings' severity.** They gain codes; severity and text stay.

## 9. Pre-conditions

All verified at `191d6c85ba`; re-verify at implementation time.

- **#4909 merged** (`f963ac9811`). **Present.**
- Data-carrying enums with named-field payloads, `Option<Enum>` params, `some(Variant {…})` — **present**
  (declaration compiles; bare and `some(…)` construction parse with zero ERROR nodes).
- `Point3<Length>` / `Vector3<Dimensionless>` / `Length` as payload field types — **present**.
- Runtime (non-literal) payload construction — **absent**; queued as leaf α, hard upstream of γ.
  `StructureInstanceCtor` (`reify-ir/src/expr.rs`) and its `reify-expr` eval arm are the template.
- `locate_element_p1`, `barycentric_p1`, `element_stress_p1`, `recover_nodal_stress_p1`,
  `apply_point_load`, `apply_dirichlet_row_elimination`, `solve_cg` borrowing `K` — **present**.
- `Value::Enum { type_name, variant, payload }` producer path (`convergence_status_to_value`) and the
  inline `Value::Scalar { si_value, dimension: DimensionVector::PRESSURE }` form — **present**.
- `DiagnosticCode` is a plain additive enum with rustdoc mnemonics — **present**.
- The four shape pins (§3) — **present, and must move**.
- The dims overload reaches the adaptive lane from `reify eval` — **present** (probed, cold cache).
- The `NoQoi`-enum `match` read-back and `unwrap_or` over `Option<Real>` — **present** (probed:
  `tests/prd-gate/fixtures/dwr_qoi_readback.ri` passes `reify check` and `reify eval`). `match` over an
  `Option` is **absent** and deliberately not used.
- No new `.ri` grammar. **`grammar_confirmed = true` for every leaf**.

### 9.1 Design-invariant walk (G7)

No leaf violates an invariant in `docs/legibility/design-invariants.md`; no waiver is needed.

- **`nothing-vacuous-and-unowned`** — this PRD is the discharge of one census row. It adds two result
  fields and one result enum, all gated by PVAC (chartered) and populated-or-`none` per C7.
- **INV-SF-1 `undef-has-provenance`** — no path leaves a cell `Undef`; absence is `none` with a
  documented reason (C7, §5.4).
- **INV-SF-2 `error-severity-exits-nonzero`** — the two `E_` codes are `Severity::Error` and are asserted
  through `reify eval`, which gates exit on severity; `reify check` converges when #5403/#5748 land.
- **INV-SF-3 `declared-intent-consumed-or-diagnosed`** — §5.7 is this invariant applied to one knob.
- **INV-SF-4 `indeterminate-attributable-transient`** — the one degraded outcome (`qoi_relative_error =
  none`) carries a typed cause, `W_QoiDegenerate`, naming the runtime condition (`|J(u_h)|` below the
  floor); it is transient (a different QoI or load clears it) and never a permanent Indeterminate.
- **INV-SF-5 `placeholders-owned-and-loud`** — leaf α retires the blanket, task-less "deferred follow-up"
  comment in `variant_construct.rs` by implementing it; leaf γ removes the stub-enum prose.
- **INV-SF-6 `diagnostics-carry-codes`** — six codes minted; two code-less warnings retrofitted.
- **INV-SF-7 `parse-is-value-faithful`** — leaf α changes what a payload *expression* means (from
  rejected to evaluated) inside brace-delimited, comma-separated fields; the grammar is unchanged, but
  BT7 carries an adjacency-regression case (a quantity literal ending a line inside a multi-line
  construction) so a future juxtaposition change cannot silently alter a payload value.
- **INV-AD-1..4** — no angle crossing: directions and normals are dimensionless vectors,
  `LocalNormalStress` is a stress, and nothing crosses a process boundary.
- **INV-PD-1 / INV-PD-2** — consumed as stated in the header; C7 is the PD-2 obligation.

## 10. Decomposition plan

Seven leaves, filed 2026-09-11 and committed together. Greek labels are authoring handles; the real task
ids are stamped on each row below. Cite the id, never the status.

**α — Runtime payload construction for data-carrying enums.** `α #7456`
Replace `variant_construct.rs`'s literal-only payload arm with a `CompiledExprKind::VariantCtor`
(shape of `StructureInstanceCtor`: compiled field expressions in declaration order, result type
`Type::Enum`), evaluated in `reify-expr` into `Value::Enum { payload }`; the all-literal case stays
constant-folded so every existing DCE golden is byte-identical. Update `enums.md` "Payload limits" and
the spec's DCE section (§3.8/§4.5) in the same diff (docs-truth). Retires the task-less deferral comment.
*Signal (BT7):* `reify eval` on `tests/prd-gate/fixtures/dce_runtime_payload.ri` evaluates the
param-dependent, negated-literal and multi-line constructions and a `match` reads the fields back;
`reify check` emits no "non-constant payload value". *Consumer:* γ, `result.ri`'s `ok_or`,
`dynamics.ri`'s `JointForceValue`, every DCE author.
*Same-diff obligations:* drift-guard registration for any new gate-resident test; if a Rust test
`include_str!`s the fixture, its basename joins `_RUST_COUPLED_RI_FIXTURES` in `scripts/verify.sh`.

**β — Dual-load assembler, dual-weighted indicator, fallible estimator seam.** `β #7452`
`P1TetMeshRef`, `QuantityOfInterest`, `LocalDisplacementQoi` / `LocalNormalStressQoi` (contributing-set
rule of §5.1; the stress functional's `g_e` from `element_stress_p1` over the twelve unit displacements),
`QoiError`, `compute_dual_weighted_indicator`, `QoiEstimate`, the `AdaptiveEstimate` rename/extension and
the fallible `solve_and_estimate` with `run_adaptive_refinement` propagating (§5.5 — the only authorized
changes to `adaptive.rs`). Lands **BT1–BT4** as gate-resident tests in the crate, with BT2's `f = g`
fixture built in Rust.
*Intermediate.* Unlocks γ. *Same-diff obligation:* nextest/heavy-filter registration if the boundary
binary exceeds the default ceiling (the crate is gate-resident by default).

**γ — DSL surface, eval threading, result fields, diagnostics.** `γ #7453`
`QoIDescriptor` gains its two variants; `QoIEstimate` (with `NoQoi`) declared; `ElasticResult` gains
`qoi : QoIEstimate = QoIEstimate.NoQoi` and `qoi_relative_error : Option<Real> = none` **after**
`convergence_status`; the four shape pins move deliberately (the
empty-stub test becomes a two-variant pin; 13 → 15 cells); a stdlib test asserts the four new variant
names are each declared exactly once; `extract_adaptive_params` reads `target_quantity_of_interest` into
`Option<QoiSpec>` (normalizing directions, validating `radius`); `solve_cantilever_fea` runs the dual
when present (§5.3); both `AdaptiveProblem` impls produce the dual-weighted **and** Z-Z estimates; all
three producer arms write the two new fields; the six `DiagnosticCode` variants of §5.7 minted and
emitted, including the code retrofit on the two existing warnings; `ElasticOptions`/`ElasticResult` doc
blocks rewritten (no "stub"/"v0.4 ignores" prose); `target_quantity_of_interest` verified to enter the
persistent FEA cache key and an Error outcome never served from cache (§5.7); the BT6 Rust e2e on the
body path.
*Signal (BT5 + BT4-eval + BT8):* `reify eval` (fresh `REIFY_CACHE_DIR`) on the committed dims-overload
pair `tests/prd-gate/fixtures/dwr_cantilever_{energy,qoi}.ri` behaves per BT5;
`tests/prd-gate/fixtures/dwr_qoi_without_adaptive.ri` exits non-zero with `E_QoiRequiresAdaptive`.
*Depends on α, β.* *Same-diff obligation:* `_RUST_COUPLED_RI_FIXTURES` registration for any fixture a
Rust test reads.

**δ — Effectivity study and validation gate.** `δ #7454`
On the `analytical_validation.rs` cantilever (Timoshenko reference; the module doc records the P1 lock
floor and the 3-D-vs-beam model offset — cite those numbers, do not re-guess) with a `LocalDisplacement`
QoI whose ball covers the tip-face neighbourhood `mean_tip_deflection` reads, and on the
`aposteriori_validation.rs` L-shaped domain with a QoI away from the corner: measure the effectivity
index `Σ η_K / (J_ref − J(u_h))` per iteration and the DWR-vs-Z-Z marked-set overlap, and record them in
`docs/notes/dwr-effectivity-2026.md`. **Gate assertions are relational only** (G6): (a) sign agreement
of the signed estimate with `J_ref − J(u_h)` on the cantilever; (b) `qoi_error_bound` decreases across
≥ 2 iterations (internal consistency, no reference needed); (c) `|J_ref − J|` on the final mesh not
larger than on the seed mesh **plus the cited model-offset floor**. Non-nested gmsh remeshing gives no
monotonicity guarantee, so no strict-decrease-of-`|J_ref − J|` is asserted. A gmsh-free build skips with
a printed reason, never a silent green. No effectivity band is asserted — §11 Q5.
*Signal:* the committed note plus the gate test green. *Depends on γ.*

**ε — PDROP C1 flip and cite retirement.** `ε #7457`
`target_quantity_of_interest` moves from the elastic C1 `ignored` set to `honored`; this task's own id —
transferred from #7177 at decompose, the #7263 precedent — stops owning anything; every cite of it in
tracked source is retired. Check what has landed (#7079/#7080/#7085) rather than assuming.
*Signal:* no cite of this task's id remains in tracked source; the elastic C1 declaration (if landed)
names the param `honored`; the PTODO fingerprint ratchet stays green; if #7085 has landed,
`/audit --pattern PDROP` reports the param honored with no allowlist entry. *Depends on γ.*

**ζ — Exemplar corpus, discoverability, docs-truth.** `ζ #7458`
`examples/best_practices/goal_oriented_refinement.ri` (dims overload; demonstrates the §5.6 read-back
`match` over `QoIEstimate` and `unwrap_or` over `qoi_relative_error`, the forms
`tests/prd-gate/fixtures/dwr_qoi_readback.ri` pins) + `INDEX.md` row + a one-line `.claude/skills/reify-design/SKILL.md` index entry. *Docs-truth:*
the enums chunk is corrected in α; the FEA/solver surface has **zero** chunk presence (re-measured
2026-09-11 — none of the 17 chunks mentions `solve_elastic_static`, `ElasticOptions`, `adaptive` or
`target_accuracy`), so the FEA chunk obligation is **waived** with this rationale and this leaf **files**
the FEA chunk-coverage follow-up task (the shift-invert η leaf #7264 carries the same obligation;
whichever lands first files it, the other cites it). *Signal:* an author who knows the goal ("refine
until my tip deflection is accurate to 2 %") but not the feature name reaches the mechanism from the
corpus index line. *Note:* `examples/*.ri` falls to `verify.sh`'s conservative default and runs the full
gate. *Depends on γ.*

**η — PRD close.** `η #7455`
Stamps the terminal `Status:` marker with the landed leaf ids, adds the AS-AUTHORED freeze paragraph and
the LIVE/AS-AUTHORED map, and applies the matching header to the capability manifest.
*Signal:* the committed header. *Depends on every other leaf.*

```
α ──┐
    ├──► γ ──┬──► δ ──┐
β ──┘        ├──► ε ──┼──► η
             └──► ζ ──┘
```

## 11. Open questions (tactical; resolvable at implementation time)

1. **Contributing-set rule at the ball boundary.** §5.1 uses "centroid in the ball"; an element
   straddling the sphere is in or out wholesale. A clipped-volume weight would be smoother but needs
   sphere–tet clipping; not worth it at v1. Record the rule in the `LocalDisplacement` doc block.
2. **Point-location tolerance.** Keep `locate_element_p1`'s documented **scale-invariant barycentric
   slack** (e.g. `1e-9`); never scale it by a length. Lowest containing index already wins.
3. **The `|J(u_h)|` floor for `W_QoiDegenerate`.** Relative to the QoI's own natural scale on the mesh
   (e.g. `1e-9 · max_i |d·u_i|` over the contributing nodes, or the analogous stress scale), never an
   absolute SI constant. Document the rule in the `qoi_relative_error` doc block.
4. **Dual CG tolerance.** Reuse the primal's `CgSolverOptions` verbatim; BT1's `10·tol` slack and BT2's
   bit-identity precondition are derived from that choice and must move with it.
5. **Effectivity band.** δ records measured effectivity; whether to gate on a band (e.g. `[0.5, 2]`) is
   filed as a follow-up from δ with the measured numbers attached — never asserted from intuition
   (`esc-3453-5/6` class).
6. **Diagnostic rustdoc wording.** Variant names and mnemonics are fixed (§5.7); the canonical-message
   rustdoc follows the `BucklingOptionUnsupported` block's shape.
7. **Cache-key membership test shape.** γ proves the QoI enters the key by running the energy and QoI
   twins against one fresh `REIFY_CACHE_DIR` and asserting the QoI run is a miss (its result differs), not
   by inspecting `compute_persist.rs` internals. Whether an Error outcome is ever written to the cache is
   checked the same way (a second run must re-emit `E_QoiRequiresAdaptive`).
8. **`QoiError` ↔ eval error enum.** The eval-side `AdaptiveProblem::Error` becomes an enum over
   `RefineError | QoiError`; the uniform lane, whose `refine` is `Infallible`, still needs the `QoiError`
   arm. Keep one enum for both lanes.
