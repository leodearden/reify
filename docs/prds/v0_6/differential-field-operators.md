# PRD: Differential field operators over Sampled fields (FEA-native + generic FD)

**Status:** authored 2026-06-11 · **Milestone:** v0_6 · **Approach:** B + H (high-stakes: FEA + ComputeNode dispatch + numerical)

**Provenance.** Spun out of `/unblock 4543` (2026-06-11, esc-4543-34/35). The blocked task tried to add `max/min/argmax/argmin` reductions for `FieldSourceKind::{Gradient, Divergence, Curl, Laplacian}` over Sampled inners "via the VonMises unwrap pattern." That is impossible: these are *differential* operators, their lambda slot holds the original source `Value::Field` (not a callable `Value::Lambda`), and `validate_differentiable_field` (`crates/reify-expr/src/calculus.rs:160-171`) hard-rejects every non-Analytical/Composed source. There is no neighbor-stencil finite-difference machinery in the codebase. This PRD owns that gap. Sibling dispositions: task **4543** → MaxShear/SafetyFactor pointwise reductions only; **4562** → PrincipalStresses only; **4561** → 2-arg bounded Analytical/Composed reduction. Those three are *not* in this PRD.

---

## 1. Consumer + user-observable surface

The architect's blocking note flagged the orphan-producer risk directly: *"no real pipeline ever constructs a Divergence/Laplacian-over-Sampled field."* An Explore sweep confirmed it — every `gradient(...)`/`divergence(...)` call site in `.ri` examples today passes an **Analytical** lambda field, and no consumer differentiates a Sampled field. This PRD resolves the gate by routing the dominant consumer through the FEA pipeline itself, where producer and consumer are the same dataflow.

**Phase 1 — FEA-native derivative channels (the real consumer).** The elastic solver already forms the displacement gradient: `element_stress_p1` (`crates/reify-solver-elastic/src/result.rs:72`) builds the physical shape-function gradients `∇x N_i = J⁻ᵀ·∇ξN_i` and computes strain `ε = B·u_e` to recover stress (σ = D:ε). The derivative information exists; today it is discarded after stress recovery. Phase 1 surfaces it as first-class result fields:

- **`ElasticResult.divergence`** : `Field<Point3<Length>, Real>` — volumetric strain (dilatation) `tr(ε) = ∂uₓ/∂x + ∂u_y/∂y + ∂u_z/∂z`. Scalar codomain.
- **`ElasticResult.gradient`** : `Field<Point3<Length>, Tensor<2,3,Real>>` — the displacement-gradient tensor ∇u (its symmetric part is strain). Dimensionless.
- **`ElasticResult.curl`** : `Field<Point3<Length>, Vector3<Real>>` — `∇×u` (twice the infinitesimal rotation vector). Dimensionless.

All three are emitted with **`source: FieldSourceKind::Sampled`** (mirroring the existing `displacement`/`stress` channels), so `sample()` and the scalar reductions work through the *existing* Sampled paths.

**User-observable signal (G2):** a stdlib `.ri` example solves a known load case and asserts `result.divergence` against the analytic dilatation (e.g. an axially-loaded bar: `tr(ε) = (1−2ν)σ/E`), runnable in CI. Plus `.gradient`/`.curl` sampled at a point.

**Engine-integration seam (overlay G1 §3.4):** the elastic-static result is produced through **ComputeNode dispatch** (`compute_targets/elastic_static.rs`, per `compute-node-contract.md`). The new channels extend that existing trampoline — no new seam.

**Phase 2 — generic neighbor-stencil FD (the general operator).** For *non-FEA* Sampled data (imported OpenVDB fields, `build_sampled_field` output, sampled-analytical fields) there is no kernel to recover from, so we provide a finite-difference primitive. Consumer: `gradient/divergence/curl/laplacian(sampled_field)` construction — today these return `Value::Undef` (the closed `validate_differentiable_field` path); Phase 2 makes them return a real differentiated Sampled field. Concrete near-term Laplacian consumer: **mean curvature of an SDF/OpenVDB field** (`∇²φ ≈ 2H` for `|∇φ|=1`), with `reify-shell-extract/medial.rs:283 gradient_at_index` as the central-difference precedent to mirror.

**User-observable signal (G2):** a stdlib `.ri` example builds a Sampled field of a known polynomial via `build_sampled_field`, takes `gradient`/`laplacian`, and `sample`s/`max`es it — exact to machine precision on the polynomial (see §6). Plus a sphere-SDF mean-curvature example asserting `∇²φ ≈ 2/R` within the FD floor.

---

## 2. Sketch of approach

**Phase 1 (FEA-native):**
1. `result.rs` — `recover_nodal_gradient_p1` (sibling of `recover_nodal_stress_p1`), reusing the `grads_phys` that `element_stress_p1` already forms. Per element: full ∇u = `grads_phys · u_e` (9 components); divergence = trace; curl = antisymmetric part. Volume-weighted nodal averaging, identical machinery to `recover_nodal_stress_p1` (which `error_estimator.rs` already reuses).
2. Result struct — add `nodal_gradient: Vec<[[f64;3];3]>` alongside `nodal_stress` (divergence/curl are linear projections of it, derivable at wrap time or stored).
3. Trampoline `compute_targets/elastic_static.rs` — extend the `resample_multi_nodal_to_grid` call (currently displacement stride-3 + stress stride-9) to also resample gradient (stride-9) / divergence (stride-1) / curl (stride-3); add `sampled_divergence_field` / `sampled_gradient_field` / `sampled_curl_field` helpers in `compute_targets/mod.rs` next to `sampled_disp_field`/`sampled_stress_field` (`source:Sampled`).
4. `crates/reify-compiler/stdlib/solver_elastic.ri` — declare the three new `ElasticResult` fields.

**Phase 2 (generic FD):**
5. `sampled_differential(sf, op) -> SampledField` in `reify-expr` (mirror `medial.rs` scheme): central difference O(h²) on grid interior, one-sided on boundaries, over `Regular1D/2D/3D` using `axis_grids`/`spacing`. Scalar input → gradient (vector out) / laplacian (scalar out); vector input → divergence (scalar out) / curl (vector out), differencing each extracted component.
6. `compute_gradient/divergence/curl/laplacian` (`calculus.rs`) — when `source == Sampled`, dispatch to (5) and return a **`source:Sampled`** output field (eager lowering), instead of `Undef`. Analytical/Composed inners keep their existing lazy callable-lambda path.
7. Multi-component `sample_at_point` (stride-n) — today scalar-only (`crates/reify-expr/src/sampled.rs`); needed to `sample()` a vector/tensor Sampled output.
8. Vector/tensor Sampled reduction by magnitude — `max(grad_field)` ⇒ `max |∇u|`; pointwise per-window magnitude projection, the VonMises pattern shape (`field_reductions.rs:203-247`). **Sequenced after task 4543** (same `field_reductions.rs` match region).

---

## 3. Pre-conditions

| Pre-condition | State | Handling |
|---|---|---|
| FEA kernel forms ∇u (B-matrix) | ✅ verified — `result.rs:72` `element_stress_p1` | reuse `grads_phys` |
| SampledField carries grid geometry for stencils | ✅ verified — `axis_grids`/`spacing`/`bounds`/`kind` (`value.rs:90-110`) | — |
| Central-diff stencil precedent | ✅ exists — `reify-shell-extract/medial.rs:283` | mirror scheme (no shared-code dep; different crate) |
| `sample_at_point` handles stride-n | ❌ scalar-only today | **Phase 2 task (7)** |
| 4543 reduction-arm region | pending (re-scoped this session) | **G4 seam: task (8) deps 4543** |
| No novel `.ri` grammar | ✅ field accessors + `gradient(...)` builtin both parse today | `grammar_confirmed=true`, no fixtures |

---

## 4. Resolved design decisions

- **D1 — Eager lowering (both phases).** `gradient/divergence/curl/laplacian(sampled_f)` constructs a `source:Sampled` output field at call time (kernel-recovered Phase 1, FD-computed Phase 2). *Rationale:* `sample()` and the scalar reductions then work through existing Sampled paths; the `Gradient/Divergence/Curl/Laplacian` `FieldSourceKind`s only ever wrap Analytical/Composed inners → **no differential reduction arms in `field_reductions.rs`**, which sidesteps the 4543/4562 match-arm seam entirely.
- **D2 — FEA derivatives via B-matrix recovery, not FD (Phase 1).** Higher fidelity: ∇u of a P1 element is the *exact* derivative of the FE interpolant (constant per element, superconvergent at recovery points). FD on a *resampled* grid would stack interpolation error + O(h²) truncation on a lossy shadow of u. Lower compute: `grads_phys` is already formed during stress recovery.
- **D3 — Vector/tensor Sampled reduction by magnitude.** `max(grad)`/`max(curl)` reduce by pointwise Euclidean magnitude (the VonMises projection shape). Divergence is scalar → reduces directly, no projection.
- **D4 — Generic FD scheme.** Central difference O(h²) interior, first/second-order one-sided at boundaries (mirror `medial.rs`). Accuracy contract pinned to polynomials where FD is **exact** (§6), never a guessed tolerance.
- **D5 — Laplacian: scalar-only, Phase 2, SDF-curvature consumer.** Excluded from the FEA spine: `∇²(linear)=0` per P1 element (degenerate), and the equilibrium-residual use case is already covered by `error_estimator.rs` (ZZ recovery). Its real near-term home is mean curvature of SDF/OpenVDB scalar fields via the generic stencil.
- **D6 — Dimensions.** `div(u)`, `∇u`, `∇×u` of a Length-valued displacement over a Length domain are all dimensionless (strain), via the existing `dim_quotient_type` logic in `calculus.rs`.

---

## 5. Boundary-test sketch (H component — two-way contracts)

High-stakes seams get contracts tested from *both* sides (overlay G5; `compute-node-contract.md` precedent):

- **Kernel ↔ Sampled encoding.** Contract: `recover_nodal_gradient_p1` emits per-node row-major data whose stride matches the channel's declared codomain (div=1, curl=3, gradient=9), `data.len() == grid_count * stride`. Test from the kernel side (recovery produces the right shape on a known mesh) **and** from the eval side (`sampled_divergence_field` round-trips to a `Value::Field` whose `codomain_type` arity equals the stride — the `extract_per_case_sampled_field` stride-assert pattern, `fea.rs:878`).
- **FD primitive ↔ reduction/sample.** Contract: a `source:Sampled` field produced by `sampled_differential` is indistinguishable (to `sample`/`max`/`argmax`) from any other Sampled field of the same codomain. Test the eager-lowered field both by direct `sample_at_point` and by `compute_extremum`, asserting they agree with the analytic derivative on a polynomial fixture.

---

## 6. Numeric premise validation (G6 — block)

Numerically heavy; premises pinned to **exactly representable** cases, no guessed bounds (overlay G6; cautionary corpus: esc-3453 buckling 5%-vs-bending-lock, esc-3770 natural-cubic-spline).

- **Phase 1 (native).** Uniform-strain patch test (the `element_stress_p1_uniaxial_strain_patch_test` pattern): under constant strain every element's recovered ∇u is identical, so nodal averaging is **exact** → `result.divergence` matches `tr(ε)` to machine precision. Floor assertion: `|recovered − analytic| < 1e-12` on the patch.
- **Phase 2 (generic FD).** Central difference is **exact** on its order-matched polynomial: linear field → `gradient` exact; quadratic field → `laplacian`/`divergence` exact (2nd central difference of a quadratic is its constant 2nd derivative). Fixtures use such polynomials → assert `< 1e-12`. For a non-polynomial control (e.g. `sin`), assert **O(h²) convergence** under grid refinement, not an absolute tolerance. SDF sphere mean-curvature: assert `∇²φ ≈ 2/R` within the documented one-sided-boundary floor away from the boundary band.

---

## 7. Out of scope

- Laplacian over FEA displacement (P1-degenerate; needs P2 elements or a `div(recovered-grad)` composition) → **deferred bookmark**.
- Thermal / scalar-potential field *sources* (no such solver exists yet) — Laplacian's heat-source and Laplace-equation use cases wait on those.
- Analytical/Composed differential *reduction* (global extrema over a callable lambda) → owned by **task 4561** (2-arg bounded form).
- Shell-element derivative channels (`frame`/`shell_channels` path) — tet/solid path first.
- P2-element native gradient channels — the P1 path lands first; P2 (`element_stress_p2` exists, `result.rs:199`) is a follow-on.

---

## 8. Cross-PRD relationship + seam owners (G4)

| Seam | Other side | Owner | Resolution |
|---|---|---|---|
| `field_reductions.rs` vector-magnitude reduction arm | tasks **4543** (MaxShear/SafetyFactor), **4562** (PrincipalStresses) — same match region | **this PRD** task (8), deps **4543** | sequence after 4543; magnitude projection is a new pointwise kind, no conflict once 4543's arms land |
| Sampled vs Analytical/Composed differential reduction | task **4561** (bounded 2-arg) | 4561 owns Analytical/Composed; **this PRD** owns Sampled (eager lowering) | disjoint by source kind; documented in both |
| `ElasticResult` extension | `structural-analysis-fea` (v0_3), `structural-analysis-shells` (v0_4) | **this PRD** | additive result fields; tet/solid path only (shells out of scope §7) |
| Generic stencil scheme | `reify-shell-extract/medial.rs` | mirror, no shared code | different crate; copy the central/one-sided scheme, cite as precedent |

---

## 9. Decomposition plan (one bullet per leaf → observable signal)

**Phase 1 — FEA-native spine:**
- **α (producer+wire, divergence):** `recover_nodal_gradient_p1` + result-struct field + trampoline wrap + `solver_elastic.ri` `.divergence` accessor. *Signal:* stdlib `.ri` solves an axial bar; `max(result.divergence)` ≈ analytic dilatation `(1−2ν)σ/E` in CI (patch-test exact, §6).
- **β (gradient + curl channels):** native `.gradient` (Tensor) + `.curl` (Vector3) accessors via the same recovery. *Signal:* `.ri` samples `result.gradient`/`result.curl` at a point and asserts against the analytic ∇u (depends on multi-component sample, task ε).
- **γ (vector-magnitude reduction):** `max/argmax` over vector/tensor Sampled fields by pointwise magnitude. *Signal:* `.ri` `max(result.gradient)` returns max `|∇u|`. **deps 4543** (G4 region).

**Phase 2 — generic FD:**
- **δ (stencil primitive):** `sampled_differential` central/one-sided FD over `Regular1D/2D/3D`, scalar gradient + laplacian. *Signal:* covered by ε's CI example (producer; gated through ε per C-as-integration-gate).
- **ε (eager-lower scalar ops + close construction gap):** dispatch `gradient`/`laplacian(sampled_scalar)` → δ, returning `source:Sampled`. *Signal:* `.ri` builds a quadratic `build_sampled_field`, `max(laplacian(f))` = exact constant 2nd derivative; `gradient` exact on linear — CI.
- **ζ (vector-input FD + multi-component sample):** `divergence`/`curl(sampled_vector)` via δ; stride-n `sample_at_point`. *Signal:* `.ri` `sample(divergence(sampled_vector_field), p)` matches analytic divergence.
- **η (SDF mean-curvature consumer):** scalar-Laplacian applied to a sphere SDF. *Signal:* `.ri`/test asserts `∇²φ ≈ 2/R` within the FD floor.
- **θ (integration gate — CRITICAL):** end-to-end CI example exercising Phase 1 (`result.divergence` engineering quantity) and Phase 2 (generic `gradient`/`laplacian`) together; the G2-bearing gate for the batch.

**Deferred bookmark:** Laplacian over FEA displacement (P2 elements / `div(recovered-grad)`), promoted when a consumer materializes.

---

## 10. Open (tactical) questions

- Store `nodal_gradient` as the full stride-9 tensor and derive div/curl at wrap time, vs. store all three pre-projected? (Memory vs. recompute; either satisfies the contract.)
- Generic-FD home crate: a new `sampled_fd` module in `reify-expr` vs. promoting `medial.rs:gradient_at_index` to a shared location. Default: new module in `reify-expr` (avoids a `reify-shell-extract` dep edge); revisit if duplication bites.
- One-sided boundary order for Laplacian (first-order one-sided vs. ghost-node extrapolation) — pick at η's design step against the sphere-SDF floor.
- Whether γ's magnitude reduction should also expose the winning component index (argmax over entries), or just the domain coord — tactical, decide with 4562's PrincipalStresses argmax convention.
