# Capability manifest — Differential field operators over Sampled fields

Mechanizes G3 + G6 for `docs/prds/v0_6/differential-field-operators.md`. One row per **leaf** capability asserted by a task's user-observable signal, bound to evidence. Any binding resolving to `declared-only | test-only | producer-downstream | producer-absent | fixture-ERROR | bound≤floor` **blocks the batch**. All bindings below resolve PASS.

Substrate verified against working tree at PRD-commit `b392fe13fb`. Line numbers are at-verification snapshots; the stable symbol names are the durable links (PRD line citations have drifted, as CLAUDE.md warns — symbols re-resolved here).

Evidence vocabulary: `grep:file:sym wired` (symbol present on a production path) · `producer:<task>` (capability built by a sibling/upstream task, dep wired) · `grammar:<precedent>` (existing parsing syntax) · `floor:<bound> vs <hazard>` (G6 numeric floor).

---

## Phase 1 — FEA-native spine

### α — `recover_nodal_gradient_p1` + result field + trampoline + `.divergence` accessor
*Signal:* stdlib `.ri` solves an axial bar; `max(result.divergence)` ≈ analytic dilatation `(1−2ν)σ/E` in CI.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Producer writes real `Value::Field{source: Sampled}` for divergence (not `Undef`) | `producer:α` — new `recover_nodal_gradient_p1` (sibling of `grep:result.rs:356 recover_nodal_stress_p1`) + new `sampled_divergence_field` helper mirroring `grep:compute_targets/mod.rs:59 sampled_disp_field` / `:72 sampled_stress_field`; production Sampled emit precedent `grep:elastic_static.rs:787,816 source: FieldSourceKind::Sampled` wired | PASS |
| ∇u recovery reuses already-formed B-matrix grads | `grep:result.rs:107 grads_phys` (built inside `element_stress_p1`, result.rs:72) wired | PASS |
| `.divergence` field accessor parses | `grammar:.stress/.displacement` accessor precedent in `solver_elastic.ri` (same accessor syntax; `grammar_confirmed`) | PASS |
| Numeric: `\|recovered − analytic\| < 1e-12` | `floor:1e-12 vs machine-eps` — **uniform-strain patch test** (constant strain ⇒ every element's recovered ∇u identical ⇒ nodal averaging exact). Deliberately avoids the P1-tet **bending-lock** hazard (§G6) by using a constant-strain patch, not a slender beam | PASS |

### β — native `.gradient` (Tensor) + `.curl` (Vector3) channels
*Signal:* `.ri` samples `result.gradient`/`result.curl` at a point and asserts against the analytic ∇u.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Producer writes `Field{Sampled}` for gradient (stride-9) + curl (stride-3) | `producer:β` — same `recover_nodal_gradient_p1`; new `sampled_gradient_field` / `sampled_curl_field` helpers next to the wired `sampled_disp_field`/`sampled_stress_field` | PASS |
| Multi-component (stride-n) `sample_at_point` to sample a tensor/vector field | `producer:ε` (introduces stride-n `sample_at_point`; β→ε dep wired). Today scalar-only at `grep:sampled.rs:78 sample_at_point` | PASS |
| `.gradient`/`.curl` accessors parse | `grammar:` accessor precedent | PASS |
| Numeric: matches analytic ∇u on patch | `floor:1e-12 vs machine-eps` — same constant-strain patch (exact) | PASS |

### γ — vector/tensor-magnitude reduction `max/argmax(result.gradient)`
*Signal:* `.ri` `max(result.gradient)` returns max `|∇u|`.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Per-window Euclidean-magnitude projection in `compute_extremum`/`compute_argextremum` | `producer:γ` — new pointwise-magnitude kind mirroring `grep:field_reductions.rs:203 project_von_mises_sampled`; reduce sites `grep:field_reductions.rs:118 compute_extremum`,`:285 compute_argextremum` wired | PASS |
| Same `field_reductions.rs` match region as MaxShear/SafetyFactor — sequenced after | `producer:4543` (cross-PRD, γ→4543 dep wired; G4 seam §8 — magnitude is a *new* pointwise kind, no conflict once 4543's arms land) | PASS |
| A vector/tensor `result.gradient` Sampled field to reduce | `producer:β` (γ→β dep wired) | PASS |

## Phase 2 — generic FD

### δ — `sampled_differential` central/one-sided FD over `Regular1D/2D/3D`
*Signal:* **none standalone — producer; gated through ε** (C-as-integration-gate). `consumer_ref = ε, ζ`.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| SampledField carries grid geometry for stencils | `grep:value.rs:101 spacing`,`:105 axis_grids`,`:45-47 SampledKind::{Regular1D,Regular2D,Regular3D}` wired | PASS |
| Central/one-sided difference scheme precedent | `grep:medial.rs:666 gradient_at_index` wired (mirror scheme, no shared-code dep — different crate) | PASS |
| Named downstream consumer (not an orphan producer) | `producer→ε,ζ` — δ's signal is ε's CI example; consumers dep-wired | PASS |

### ε — eager-lower `gradient`/`laplacian(sampled_scalar)` + close construction gap + stride-n sample
*Signal:* `.ri` builds a quadratic via `build_sampled_field`; `max(laplacian(f))` = exact constant 2nd derivative; `gradient` exact on linear — CI.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Sampled branch of `compute_gradient`/`compute_laplacian` returns real `Field{Sampled}` instead of `Undef` | `producer:ε` edits the closed path `grep:calculus.rs:139 validate_differentiable_field` + dispatch `grep:calculus.rs:186 compute_gradient`,`:388 compute_laplacian` (sites wired; ε replaces the `source==Sampled ⇒ Undef` return with a δ-dispatch eager-lowering) | PASS |
| Quadratic/linear Sampled fixture | `grep:engine_eval.rs:1309 build_sampled_field` wired | PASS |
| stride-n `sample_at_point` (to sample the linear-field gradient vector) | `producer:ε` — introduced here (its own signal needs it); consumed by β, ζ | PASS |
| Dimensionless codomain of `div/∇/∇²` over Length-over-Length | `grep:calculus.rs:53 dim_quotient_type` wired (D6) | PASS |
| Numeric: laplacian exact on quadratic, gradient exact on linear | `floor:1e-12 = exact-arithmetic` — **order-matched polynomial**: 2nd central diff of a quadratic *is* its constant 2nd derivative; 1st central diff of a linear *is* its constant slope. Exactly representable (§6), not a guessed tolerance | PASS |

### ζ — vector-input FD (`divergence`/`curl(sampled_vector)`) + multi-component sample
*Signal:* `.ri` `sample(divergence(sampled_vector_field), p)` matches analytic divergence.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Sampled branch of `compute_divergence`/`compute_curl` → δ, returns `Field{Sampled}` | `producer:ζ` edits `grep:calculus.rs:247 compute_divergence`,`:317 compute_curl` (sites wired) | PASS |
| Stencil primitive | `producer:δ` (ζ→δ dep wired) | PASS |
| stride-n `sample_at_point` for curl (vector) output; divergence output is scalar (existing sample) | `producer:ε` (ζ→ε dep wired) | PASS |
| Numeric: divergence of a linear vector field exact | `floor:1e-12 = exact-arithmetic` (order-matched, §6) | PASS |

### η — SDF mean-curvature consumer `∇²φ ≈ 2/R`
*Signal:* `.ri`/test asserts `∇²φ ≈ 2/R` for a sphere SDF within the FD floor.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Scalar Laplacian over a sampled SDF | `producer:ε` (η→ε dep wired — eager-lowered scalar laplacian) | PASS |
| Sphere-SDF Sampled field | `grep:engine_eval.rs:1309 build_sampled_field` wired (sphere SDF constructible) | PASS |
| Numeric: `∇²φ ≈ 2/R` | `floor:O(h²)-interior, boundary-band-excluded` — sphere SDF is **non-polynomial**, so FD is *not* exact; assert **interior** agreement to the documented O(h²) truncation floor (and/or O(h²) convergence under refinement), boundary band excluded. G6-safe (§6: convergence/interior-floor, never a guessed absolute machine-precision tol) | PASS |

### θ — integration gate (CRITICAL) — Phase 1 + Phase 2 end-to-end
*Signal:* end-to-end CI `.ri` exercising Phase 1 (`result.divergence` engineering quantity) and Phase 2 (generic `gradient`/`laplacian`) together. **The G2-bearing gate for the batch.**

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Phase-1 engineering quantity reachable end-to-end | `producer:α` (θ→α dep wired) | PASS |
| Phase-2 generic ops reachable end-to-end | `producer:ε,ζ,η,γ` (θ deps wired) | PASS |
| Two-way boundary contracts (H component, §5) | kernel↔Sampled stride round-trip (`grep:fea.rs:891 extract_per_case_sampled_field` stride-assert precedent) + FD↔reduction/sample indistinguishability — named as θ's H-signal | PASS |

---

## Gate summary

- **G1** — every mechanism has a named consumer; Phase 1 routes producer=consumer through ComputeNode dispatch (overlay §3.4), Phase 2 closes the `gradient/.../laplacian(sampled)` → `Undef` construction gap and the SDF-curvature consumer; θ integrates both. No orphan.
- **G3 / grammar** — no novel `.ri` grammar; all builtins + accessors parse today. `grammar_confirmed=true`.
- **G4** — sole cross-PRD edge **γ→4543** (shared `field_reductions.rs` region). 4561 (Analytical/Composed bounded reduction) and 4562 (PrincipalStresses) are **disjoint by source kind** — no dep edge (§8); boundaries kept.
- **G5** — high-stakes (FEA + ComputeNode + numerical) ⇒ B+H; θ is the integration gate, §5 names two-way boundary tests.
- **G6** — every numeric premise pinned to an **exactly-representable** case (uniform-strain patch, order-matched polynomial → `1e-12`) or an **O(h²)/convergence** floor (sphere SDF), never a guessed bound. Bending-lock / boundary-band hazards explicitly side-stepped.

**No FAIL bindings — batch clears the manifest gate.**
