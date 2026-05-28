# PRD: Buckling Eigensolver + Geometric Stiffness K_g

Status: contract (resolves the eigensolver+K_g slice of `docs/prds/v0_5/structural-stability-buckling.md`).
Authored 2026-05-12 in interactive session under the audit-derived G1/G2/G3/G4/G5/META gates.

Resolves cluster C-22 / gap **GR-024** per `docs/architecture-audit/gap-register.md`.

## §0 — Purpose and supersession

This document is the **contract** for the buckling kernel surface: the eigensolver
(faer-rs operator-form Lanczos with shift-invert), the geometric stiffness matrix `K_g`
assembly for P1 tet elements, the `solve_buckling` stdlib entry, the `BucklingResult`
value shape, and the GUI mode-shape-frame implementation slice. It addresses GR-024
named in `docs/architecture-audit/gap-register.md` (cluster C-22 in
`phase-3-files-synthesis.md`).

The parent PRD `docs/prds/v0_5/structural-stability-buckling.md` continues to own the
broader user-facing buckling product (rationale, deferral framing, future non-linear
buckling, future imperfection-sensitivity workflow). This PRD does **not** supersede it;
this PRD resolves the kernel-surface slice of it. The parent's
"Sketch of approach" and "Open design questions" sections are answered authoritatively
here; the parent gains a cross-link in a companion-correction task.

The audit's dominant failure mode — "incomplete/ill-formed implementation chain" (see
`preferences_implementation_chain_naming` memory) — is what this contract is designed to
prevent for the eigensolver+K_g seam. Resolution mode is approach **B + H** per
`preferences_implementation_chain_portfolio`: vertical-slice decomposition under
design-first / contracts / two-way boundary-test discipline. Justified at G5 in §13.

## §1 — Foundation gates

This PRD's surface is gated on three landed/resolving foundations:

1. **GR-001** — struct-constructor runtime evaluation
   (`docs/prds/v0_3/structure-instance-runtime.md`). Required so that
   `BucklingOptions(...)`, `BucklingResult { ... }`, `Mode { ... }` evaluate at runtime
   instead of producing `Value::Undef`. Inherited by every FEA-stack PRD; cited and
   moved on.
2. **GR-002 — ComputeNode contract** (`docs/prds/v0_3/compute-node-contract.md`, landed).
   Provides per-Engine `ComputeDispatchRegistry`, `CancellationHandle`, `OpaqueState`
   transfer, atomic completion, significance-filter integration. This PRD registers
   `solver::buckling` against that contract.
3. **FEA stack engine integration** — `solve_elastic_static` reaches end-to-end via the
   ComputeNode contract DAG (`compute-node-contract.md` §8 task η). Buckling requires
   a linear-static pre-stress solve as its first phase; the same trampoline shape is
   reused. Gated on `compute-node-contract.md` task η landing.

A fourth implicit gate is the `Field<X,Y>`-in-`param`-position TODO tracked by task
**#3117**. This PRD applies the same `Real`-placeholder workaround that `ElasticResult`
uses for its `displacement` / `stress` fields. When #3117 lands, the placeholder fields
flip to their natural typed form without surface API change.

## §2 — Goal and motivating signal

A user writes:

```
material = Steel_AISI_1045
column   = box(width = 20 mm, height = 20 mm, length = 800 mm)

load    on column.face("top")    = 1 kN downward
support on column.face("bottom") = fixed

result = solve_buckling(column, material, [load], [support])
critical = critical_load(result)            // ≈ π² E I / L²  → eigenvalue × 1 kN
```

The user observes:

- **CLI:** `reify check buckling_column.ri` evaluates the file. `result.modes[0].eigenvalue`
  is within tolerance of the closed-form Euler value `π² E I / (k L)²` for the BC class
  (k = 1.0 pin-pin; 2.0 fixed-free; 0.5 fixed-fixed). The CLI prints the
  eigenvalue + mode-shape-norm summary line.
- **Viewport:** Opening the file in `reify gui` renders the column. After the buckling
  solve completes, the viewport offers a **Modes panel** listing the n_modes eigenvalues.
  Selecting a mode animates the column shape by sweeping a phase parameter from −1 to +1
  at displacement-field × scale. Animation runs at a steady frame rate
  (target 30 fps) with the undeformed geometry rendered as a faint reference.
- **LSP:** Hovering `BucklingResult` / `Mode` / `solve_buckling` shows the documented
  stdlib signatures + a brief description.

That motivating signal is the leaf observable that the integration-gate task in §13
asserts on. Every other task in the DAG either prepares or extends it.

## §3 — Resolved design decisions (2026-05-12)

The parent PRD §"Open design questions" are answered here:

| Question | Resolution |
|---|---|
| Linear vs. non-linear buckling | **Linear (eigenvalue) only** for v0.5. Non-linear (Riks / arc-length) deferred to a future PRD if demand emerges. Same lean as parent. |
| Number-of-modes default | **n_modes = 10.** Generous default; handles symmetric-structure degeneracy without user thinking; ~2× Krylov iterations vs n=5 is not a hot path. |
| Imperfection sensitivity | **Out of scope for v0.5.** Future-PRD pointer. Real value is for shells, which are out of scope for v0.5 K_g; composes with `mesh-morphing.md` which has its own gaps. |
| Reference-load magnitude | **λ *is* the safety factor.** `safety_factor_buckling(result, applied_load)` trivially collapses to `result.modes[0].eigenvalue` (the `applied_load` argument is informational; kept in the signature for future-flexibility). |
| Multi-step / load-following | Out of scope. Each load case is a separate eigenvalue problem; per-case envelope handled by `MultiCaseBucklingResult`. |

Additional decisions resolved in this session:

| Question | Resolution |
|---|---|
| Element coverage for K_g | **P1 tet only** for v0.5. P2 tet + P1 hex/wedge K_g are mechanical extensions deferred to a follow-up PRD or to the hex/wedge meshing PRD's decomposition. |
| Shell-element K_g | **Out of scope; stubbed with `E_BucklingShellNotImplemented` diagnostic citing task 3392.** Bare-MITC3 has the same flat-facet under-prediction that motivated MITC3+ (shells audit M-005); shipping shell-K_g on bare-MITC3 would replicate the band-widening pattern documented in task 3034. The diagnostic catches the failure shape early and points the user at the prereq. |
| Multi-load-case coupling | **Parallel `MultiCaseBucklingResult` struct** with `cases : Map<String, BucklingResult>`. Sibling to `MultiCaseResult`. Composes with `multi-load-case-fea.md` patterns. |
| Solver mode default | **Shift-invert Lanczos at σ=0**, using SPD K's Cholesky factor for the inner solve. Override via `BucklingOptions.mode` (String-typed; see §4). Dense generalized EVD fallback when total DOF ≤ ~200 (via faer's `linalg/gevd` QZ surface). |
| GUI ownership of mode-shape animation | **GR-016 owns the `mode-shape-frame` channel contract; this PRD owns the implementation slice** (backend emitter + frontend `BucklingPanel` animator). GR-016's deferred bookmark task λ is replaced by this PRD's §13 phase-9 task. |
| Geometric-multiplicity / degenerate-mode handling | **Default n_modes=10 + Lanczos with deflation** (faer's `self_adjoint_eigen` already block-deflates internally). v0.5 does not pursue block-Lanczos; n_modes=10 is enough to catch one mode-pair degeneracy in typical fixtures. |
| Cancellation poll granularity | **Between Lanczos iterations + between major phases** (linear-static pre-stress / K_g assembly / eigensolve / mode-shape post-process). 100 ms SLA per `compute-node-contract.md` §2; Lanczos iterations on typical FEA-scale K are well under this budget. |

## §4 — Surface contract (stdlib `fn` + value shapes)

**Stdlib entry.** Declared in `crates/reify-compiler/stdlib/solver_buckling.ri`:

```
@optimized("solver::buckling")
fn solve_buckling(
    body:      Body,
    material:  ElasticMaterial,
    loads:     List<Load>,
    supports:  List<Support>,
    options:   BucklingOptions = BucklingOptions.default,
) -> BucklingResult
```

Inputs match `solve_elastic_static` (PRD `structural-analysis-fea.md` §"Sketch")
verbatim except for the `options` type. `Load` / `Support` carry the same drift
inherited from the FEA / multi-load-case PRDs (kind-tagged Maps from builtin
constructors per audit-brief givens); this PRD does not re-design that surface.

**Options struct.** Declared adjacent to the fn:

```
structure def BucklingOptions {
    n_modes:    Integer = 10
    mode:       String  = "shift_invert"   // "shift_invert" | "dense" | "lanczos_no_shift"
    sigma:      Real    = 0.0              // shift origin in eigenvalue units
    tol:        Real    = 1.0e-8           // Lanczos convergence tolerance
    max_iters:  Integer = 1000             // hard cap; default chosen for ~10⁵ DOF problems
    auto_dense: Bool    = true             // if true, fall back to dense GEVD when DOF ≤ ~200
}
```

`mode` is **String-typed** for v0.5 (precedent: existing options structs use string
discriminants until ADT enums land in Reify grammar; the audit grammar gate makes this
the conservative choice). Validated against the allowlist at trampoline entry; invalid
values produce `Diagnostic::E_BucklingInvalidMode`.

**Result struct.** Declared in `crates/reify-compiler/stdlib/solver_buckling.ri`:

```
structure def Mode {
    eigenvalue:  Real                                       // load multiplier; dimensionless
    mode_shape:  Field<Point3<Length>, Vector3<Length>>     // displacement field
}

structure def BucklingResult {
    modes:        List<Mode>
    converged:    Bool
    iterations:   Integer
    pre_stress:   ElasticResult              // the linear-static solve that fed K_g
}
```

**Field-in-param caveat.** Per the same workaround `ElasticResult` uses today
(`solver_elastic.ri` precedent; audit `findings/structural-stability-buckling.md` M-003,
M-005), `Mode.mode_shape` is encoded with a `Real`-placeholder field on disk until
task #3117 lands. The runtime path produces `Value::Map` shaped to expose
displaced-position samples to the GUI; once #3117 lands, `mode_shape` flips to its
natural typed form without surface-API change.

**Result-interpretation helpers** (stdlib pure functions, no trampoline):

```
fn critical_load(result: BucklingResult) -> Force
    // First-mode eigenvalue × reference load magnitude.
    // Reference load magnitude derived from the `pre_stress` field's stored load magnitudes.

fn mode_shape(result: BucklingResult, n: Integer) -> Field<Point3<Length>, Vector3<Length>>
    // result.modes[n].mode_shape (subject to #3117 placeholder).

fn safety_factor_buckling(result: BucklingResult, applied_load: Force) -> Real
    // v0.5 semantics: result.modes[0].eigenvalue. `applied_load` retained for signature
    // forward-compatibility (future non-linear buckling may use it).
```

## §5 — Eigensolver kernel contract

**Crate location.** `crates/reify-solver-elastic/src/eigensolve.rs` (new module). Lives
alongside the elastic solver because K assembly + Cholesky factorization are shared
substrate. No new crate.

**Generalized eigenvalue problem.** Buckling solves
`(K + λ K_g) φ = 0`, equivalently `K φ = −λ K_g φ`, a generalized symmetric eigenvalue
problem with `A = K` (SPD after BCs applied), `B = −K_g` (symmetric, possibly indefinite).
Smallest |λ| → critical buckling load multiplier.

**Mode = "shift_invert" (default).**

1. Factor `K = L Lᵀ` once via faer's sparse Cholesky (existing CG warm-state factorization
   path is the precedent; symbolic factorization is reused if a warm CompT slot is
   present).
2. Build a `LinOp<f64>` (faer trait) that on `apply(out, v)` computes `out ← L⁻ᵀ L⁻¹ (−K_g v)`
   = one Cholesky back-solve + one K_g matvec. K_g is held as `SparseRowMat`.
3. Call `faer::operator::self_adjoint_eigen::partial_self_adjoint_eigen` with that LinOp,
   `n_eigval = n_modes`, `tol = options.tol`, `restarts = options.max_iters / max_dim`,
   `min_dim = n_modes`, `max_dim = max(2*n_modes, 32)`.
4. Recover physical eigenvalues from the returned Krylov-space eigenvalues by reciprocal
   transformation (shift-invert at σ=0 inverts the spectrum; physical λ = 1 / λ_krylov).
5. Recover physical eigenvectors by L back-solve on the Krylov eigenvectors.

**Mode = "dense".** Used when `auto_dense` is true and `dof ≤ ~200`, or when explicitly
requested. Calls faer's `linalg/gevd` (QZ for real generalized eigenproblems) on the
densified K and K_g. Returns the full spectrum; selects the n_modes smallest by |λ|.

**Mode = "lanczos_no_shift".** Bare Lanczos against `(−K_g, K)` regular pair via
`partial_self_adjoint_eigen` without shift-invert. Diagnostic-only mode for solver
debugging; converges to large-magnitude eigenvalues first (wrong order) so flagged
`W_BucklingNoShiftReversedOrder` at trampoline.

**Cancellation discipline.** The trampoline polls `cancellation.is_cancelled()` at:

- Between linear-static pre-stress and K_g assembly.
- Between K_g assembly and eigensolve.
- Inside the Lanczos `LinOp::apply` callback — once per matvec. Each matvec is one
  Cholesky back-solve, well under 100 ms for problems up to ~10⁵ DOF.
- Between eigensolve and mode-shape post-process.

On `Cancelled` return, the prior cache entry stays; the slot is cleared per
`compute-node-contract.md` §2 semantics.

**Warm state.** `OpaqueState` carries the sparse Cholesky symbolic factorization of K
(reusable across small parameter perturbations that don't change the sparsity pattern)
and, optionally, the Lanczos restart vector. First call: no prior state. Repeat call
with same sparsity: factor reused; Lanczos restarted with prior eigenvector basis as
warm start. `cost_per_byte` reported per `compute-node-contract.md` §5.

**Convergence reporting.** `BucklingResult.converged = true` iff all n_modes eigenvalues
satisfy the tolerance criterion. `BucklingResult.iterations` reports the total Krylov
iterations across restarts. Diagnostic `W_BucklingNotConverged` emitted on partial
convergence — `modes` contains the converged subset, length < n_modes.

## §6 — K_g element-kernel contract

**Crate location.** `crates/reify-solver-elastic/src/elements/tet_p1_geometric.rs`
(new module). Sibling to the existing `tet_p1.rs` elastic-stiffness kernel.

**Formulation.** Per-element geometric stiffness for the standard small-strain
linear-buckling formulation:

```
K_g_e = ∫_Ω_e (Bᴺᴸ)ᵀ σ_e Bᴺᴸ dV
```

where `Bᴺᴸ` is the non-linear (geometric) strain-displacement matrix and `σ_e` is the
element-averaged Cauchy stress from the pre-stress linear-static solve. For P1 tet,
the standard derivation gives a closed-form 12×12 block per element; same Gauss
quadrature surface (1 point at centroid) as the elastic K. Implementation parallels
`tet_p1.rs::element_stiffness_matrix` row-for-row.

**Global assembly.** New function `assembly::global::assemble_geometric_stiffness(...)`
parallel to the existing `assemble_global_stiffness`. Takes the pre-stress
`ElasticResult.stress` field and the same mesh; returns `SparseRowMat<usize, f64>` in
the same DOF ordering as K.

**Shell stub.** Function `assembly::global::assemble_geometric_stiffness_shell(...)` is
declared with the same signature pattern but returns
`Err(Diagnostic::E_BucklingShellNotImplemented {
    cite_task: "3392",
    cite_findings: "structural-analysis-shells.md M-005 (bare-MITC3 vs MITC3+ DRIFT)",
})`. The trampoline routes to this on shell-classified bodies. Unit-test fixture pins
the diagnostic shape.

**Hex / wedge stub.** Same pattern. Diagnostic `E_BucklingHexWedgeNotImplemented` cites
`hex-wedge-meshing.md`.

## §7 — Multi-load-case coupling

**Parallel struct.** Declared in `crates/reify-compiler/stdlib/solver_buckling.ri`:

```
structure def MultiCaseBucklingResult {
    cases:  Map<String, BucklingResult>
}
```

Composes with the existing multi-load-case PRD shape. The optional companion stdlib
helper:

```
@optimized("solver::buckling_multi_case")
fn solve_buckling_load_cases(
    body:     Body,
    material: ElasticMaterial,
    cases:    List<LoadCase>,
    options:  BucklingOptions = BucklingOptions.default,
) -> MultiCaseBucklingResult
```

**Envelope helpers** (stdlib pure functions):

```
fn envelope_critical_load(mcbr: MultiCaseBucklingResult) -> Force
    // min over cases of critical_load(case_result).

fn worst_buckling_case(mcbr: MultiCaseBucklingResult) -> String
    // argmin over cases of critical_load.
```

**Why parallel rather than a field on `MultiCaseResult`.** Two analysis kinds with
distinct result shapes; folding them risks compounding the `Field`-in-param TODO
across two product types. The parallel shape mirrors the elastic / buckling split
at the single-case level.

**Out of scope for this PRD.** Cross-case buckling envelope rendering in the GUI
(static; uses existing infrastructure) is left to a small follow-up task; the
worst-case modes per case are accessible via `result_for(mcbr, case_name)` reuse
of the existing multi-load-case accessor.

## §8 — GUI seam: mode-shape-frame implementation

**Contract owner.** `docs/prds/v0_3/gui-event-channel-inventory.md` (GR-016) §3 owns the
channel contract:

- Channel name: `mode-shape-frame`
- Payload: `{ mode_index: u8, phase: f32, displaced_positions: Vec<f32> }`
- Producer: backend buckling-solver post-process
- Consumer: frontend `BucklingPanel` animator

GR-016's deferred bookmark task λ ("Add mode-shape-frame channel + producer when
buckling lands in v0.5+") is **replaced** by this PRD's §13 phase-9 task; GR-016's
prereq line "structural-stability-buckling decomposition activates" is the trigger.

**Backend.** New Tauri command + IPC emitter in `gui/src-tauri/src/buckling.rs` (new
module). On `solve_buckling` completion the engine emits one `mode-shape-frame`
event per (mode_index, phase) tuple in a steady cadence (30 fps target;
animation phase ∈ [−1, +1] sampled at the frame rate, then looped). `displaced_positions`
is the undeformed vertex positions + `phase × scale × mode_shape` evaluated at each
vertex. Scale is computed at the backend to normalize peak displacement to a viewport
fraction (default ~10% of bounding-box diagonal).

**Frontend.** New `gui/src/panels/BucklingPanel.tsx` component subscribed to the
`mode-shape-frame` channel and the `engine-state` channel (existing). Shows:

- Mode list (eigenvalue + thumbnail).
- Currently-selected mode index.
- Play/pause toggle, scale slider, undeformed-overlay toggle.
- Three.js mesh whose positions are updated from `displaced_positions` per frame.

**Frame budget.** The animator must hold 30 fps for meshes up to ~100 K vertices
(matches the FEA solver's documented working range). Frame-rate degradation under
that threshold emits `W_BucklingAnimationFrameDrop` to the diagnostic stream.

## §9 — Boundary-test sketch (cross-crate, facing both ways)

Tests live in `crates/reify-solver-elastic/tests/eigensolve_*.rs` (kernel-side,
unit-to-integration), `crates/reify-eval/tests/buckling_smoke.rs` (engine-side,
through-ComputeNode), `gui/src-tauri/tests/buckling_ipc.rs` (Tauri-side
backend-emitter), and `gui/tests/buckling_panel.spec.ts` (frontend Playwright).
Boundary-test discipline is mandated by G5/B+H; tests cross every named seam from
each side.

### 9.1 Kernel-side (eigensolver + K_g looking outward at the FEA crate)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Pinned-pinned Euler column.** Square box (20×20×800 mm), Steel_AISI_1045, 1 kN tip load, both ends pinned. | P1 tet mesh; elastic K assembled; pre-stress solve converged. | `result.modes[0].eigenvalue × 1 kN` is within **10%** of `π² E I / L²` (observed 9.2% at `nx=ny=8, nz=160`). Mode shape's largest displacement component aligns with one in-plane axis; transverse displacement at mid-span is the maximum (half-sine shape). |
| **Fixed-free cantilever column.** Same geometry; bottom fixed, top free, tip load. | Same. | `eigenvalue × 1 kN` within **11%** of `π² E I / (2L)²` (effective-length factor k=2; observed 10.0%). Mode shape is quarter-sine. |
| **Fixed-pin column.** Same geometry; bottom face fully clamped (all 3 DOFs/node), top face laterally clamped (`u_x=u_y=0`, `u_z` free/node). | Same. | `eigenvalue × 1 kN` within **10%** of `π² E I / (k L)²` with **k≈0.6992 (fixed-pin)**; observed `k_eff≈0.670`, 8.8% at current mesh. |

> **P1-tet accuracy note (G6 / esc-3453-5,6).** The 5% bounds and the **fixed-fixed (k=0.5)** third variant originally stated here are *not achievable* at practical mesh density: P1-tet bending lock at this slenderness (L/r≈138) yields 9–10% error on every BC variant, and pointwise Dirichlet BCs impose no rotational restraint, so a clamped-clamped attempt realizes **fixed-pin (k≈0.6992)**, not fixed-fixed. Bounds reconciled to the shipped test (`euler_column_pin_pin.rs`: pin-pin 10%, fixed-free 11%, fixed-pin 10%). MPCs (`u_z` equal across the top face) **do** realize a true fixed-fixed (k=0.5) BC — verified in `euler_column_pin_pin.rs::fixed_guided_euler_column_within_nine_percent` (constraint satisfaction is bit-exact). The MPC alone does **not** reach 5%, however: P1-tet bending lock floors the error at ~6.8% (asymptote of `error = a + b/nx²`; 8.46% at nx=ny=10), so the MPC variant is bounded at **9%** to match the P1-tet tolerance family. Reaching the original 5% requires P2-tet (quadratic) K_g — tracked as a follow-up (esc-3813-117).
| **n_modes degeneracy on square cross-section.** Pinned-pinned column with square box (20×20×L). | Same. | First two eigenvalues are within tolerance of each other (in-plane / out-of-plane mode pair). `modes[0].mode_shape` and `modes[1].mode_shape` are orthogonal in displacement space. |
| **Shell input emits clean diagnostic.** Call `solve_buckling` on a shell-classified body. | Body classifier returns Shell. | Trampoline returns `ComputeOutcome::Failed { diagnostics: [E_BucklingShellNotImplemented { cite_task: "3392", ... }] }`. No panic. |
| **Cancellation under design loop.** Synthetic large-DOF column; auto-resolve drives a non-structural param; rapid input changes mid-solve. | Trampoline registered; ≥ 100 ms per Lanczos iteration. | Cancellation observed within 2× poll budget (≤ 200 ms). Prior cache entry intact. No orphaned solver threads. |
| **Warm state across small perturbation.** Solve column; perturb length by 1 mm; re-solve. | Same mesh sparsity pattern. | Second solve reuses symbolic factorization (verified via OpaqueState lifecycle test). Total wall-clock ≤ 0.5× cold-start. |
| **Dense fallback at tiny DOF.** Solve `auto_dense = true` problem with 50 DOF. | `dof ≤ 200`. | `BucklingResult.converged = true`; iterations = 0 (dense path doesn't iterate). Result agrees with shift-invert path on the same problem to 8 digits. |
| **Non-converged path.** Pathological problem (`max_iters = 5`, large DOF). | As above. | `BucklingResult.converged = false`. `modes` contains the converged subset (length 0–9). `W_BucklingNotConverged` in diagnostics. |

### 9.2 Engine-side (ComputeNode dispatch + stdlib looking inward)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Round-trip `solve_buckling`.** `.ri` file declares Euler column; `solve_buckling(...)` returns BucklingResult; first eigenvalue within tolerance. | GR-001 + GR-002 + FEA stack landed; trampoline registered as `solver::buckling`. | Engine inspection confirms a ComputeNode with `target = "solver::buckling"`. ElasticResult pre-stress also resolved (separate inner ComputeNode under §5 step 1). Cache hit on re-run. |
| **Multi-load-case shape composition.** `solve_buckling_load_cases(body, mat, [case_a, case_b])`. | Same; LoadCase ctor lands via GR-001. | `MultiCaseBucklingResult.cases` has 2 entries; `worst_buckling_case(mcbr)` returns the lower-eigenvalue case name. |
| **Significance filter at result boundary.** Re-dispatch with tolerance-equivalent (bit-different) load magnitudes. | `is_opted_in("solver::buckling")` returns true. | `FilterOutcome::Equivalent`; no downstream recompute. |
| **GUI mode-shape-frame round-trip.** Open buckling .ri in `reify gui`; select mode 0; observe animation. | `BucklingPanel.tsx` mounted; backend emitter wired. | At least 30 `mode-shape-frame` events per second; `displaced_positions` length matches vertex count; frontend animates smoothly. |
| **Frame-drop diagnostic.** Force 1 M-vertex mesh (synthetic). | As above. | `W_BucklingAnimationFrameDrop` emitted; animator continues at degraded rate; no crash. |

## §10 — Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/structural-analysis-fea.md` | this consumes | K assembly, Cholesky factorization, ElasticResult, faer machinery | FEA PRD | foundation gate (gated on FEA stack reaching ComputeNode integration) |
| `docs/prds/v0_3/compute-node-contract.md` | this consumes | `ComputeDispatchRegistry`, `CancellationHandle`, `OpaqueState`, atomic completion, significance filter | contract; shipped | this PRD adds `solver::buckling` target per §6 consumer policy |
| `docs/prds/v0_3/structure-instance-runtime.md` | this consumes | Struct-ctor runtime evaluation (`BucklingOptions(...)`, `BucklingResult { ... }`, `Mode { ... }`) | structure-instance-runtime PRD | foundation gate (GR-001) |
| `docs/prds/v0_3/multi-load-case-fea.md` | this composes with | `LoadCase` struct, envelope helpers, accessor patterns (`case_names`, `result_for`) | multi-load-case PRD | sibling design; this PRD authors parallel `MultiCaseBucklingResult` per §7 |
| `docs/prds/v0_3/gui-event-channel-inventory.md` (GR-016) | this implements | `mode-shape-frame` channel + payload | GR-016 owns the channel contract; this PRD owns the implementation slice | replaces GR-016's deferred bookmark task λ |
| `docs/prds/v0_3/fea-gui-rendering.md` | this extends | Deformed-shape rendering pipeline; adds time-domain animation primitive | this PRD (animation primitive); fea-gui-rendering (underlying renderer) | extends; no contested ownership |
| `docs/prds/v0_4/structural-analysis-shells.md` | this stubs out | Shell-element K_g | this PRD stubs; full implementation deferred to MITC3+ work | task 3392 cited in diagnostic |
| `docs/prds/v0_3/hex-wedge-meshing.md` | this stubs out | Hex / wedge K_g | this PRD stubs; full implementation deferred to hex-wedge work | follow-up PRD if/when needed |
| `docs/prds/v0_3/mesh-morphing.md` | future-PRD pointer | Imperfection seeding (mode-shape × small-amplitude perturbation) | future-PRD | out of scope for v0.5 |
| `structural-analysis-modal.md` (unfiled) | future shared infra | faer-rs Lanczos infrastructure (modal: `(K − ω²M)φ = 0`) | unfiled | comment in §5 only |

No reciprocal "the other owns it" pairs surfaced in this PRD's design.

## §11 — Pre-conditions for activating

- **GR-001 resolved** — `docs/prds/v0_3/structure-instance-runtime.md` landed; `BucklingOptions(...)`,
  `BucklingResult { ... }`, `Mode { ... }`, `MultiCaseBucklingResult { ... }`,
  `LoadCase { ... }` runtime ctors yield `Value::StructureInstance`.
- **ComputeNode contract** — `compute-node-contract.md` §8 task η (FEA round-trip) landed.
  `solver::elastic_static` is registered and exercised by an `.ri` smoke fixture.
- **FEA stack engine integration** — `solve_elastic_static` reaches end-to-end.
- **GR-016 channel contract** — `gui-event-channel-inventory.md` decomposition queued
  and at least the channel-registration scaffold landed (this PRD owns the
  `mode-shape-frame` emitter + animator; GR-016 owns the channel surface).
- **Task #3117 (Field-in-param)** — not strictly required (Real-placeholder workaround
  unblocks shipping); when it lands, the `Mode.mode_shape` field flips to its typed
  form. Tracked as a forward-looking dependency, not a gate.

## §12 — Out of scope

- **Non-linear (Riks / arc-length) buckling.** Future-PRD pointer; same as parent PRD.
- **Imperfection-sensitivity workflow.** Future-PRD pointer; gated on shells K_g.
- **Shell-element K_g.** Stubbed with `E_BucklingShellNotImplemented`; full impl deferred
  to MITC3+ work (task 3392).
- **Hex / wedge K_g.** Stubbed; future-PRD or extension of hex-wedge meshing decomp.
- **P2 tet K_g.** Mechanical extension of P1; deferred to follow-up.
- **Thermal buckling / dynamic instability / flutter.** Separate physics.
- **Modal (vibration) analysis.** Shares the eigensolver but is a different formulation
  (mass matrix M instead of K_g). Future PRD; left as a comment that this PRD's
  eigensolver shape is generalization-friendly.

## §13 — Integration DAG (proposed; not yet filed)

Decomposition style: **B (vertical slice) + H (design-first / interface contracts /
boundary tests)** per `preferences_implementation_chain_portfolio`. G5 justification:

- Cross-crate blast radius: `reify-solver-elastic`, `reify-compiler/stdlib`,
  `reify-stdlib`, `reify-eval`, `gui/src-tauri`, `gui/src` (frontend) — **6 crates / dirs**.
- Mechanism count: ~12.
- High-stakes seams: FEA + ComputeNode dispatch + GUI event channel + GR-001.

Each leaf names its **user-observable signal** per G2. Producer-only tasks closed in
isolation are not acceptable (`feedback_task_chain_user_observable`).

### Phase 1 — Foundation supplements

- **Task α** — `BucklingOptions`, `BucklingResult`, `Mode`, `MultiCaseBucklingResult`
  stdlib structure_defs declared in `solver_buckling.ri`.
  - **Observable signal:** Stdlib compile test `crates/reify-compiler/tests/buckling_stdlib_compile.rs`
    parses the structure_defs and confirms type resolution matches the expected shape
    (using the Real-placeholder for `Mode.mode_shape` per #3117 workaround).
  - **Prereqs:** GR-001 landed.
  - **Crates touched:** reify-compiler/stdlib.

### Phase 2 — Eigensolver kernel slice (no engine integration yet)

- **Task β** — `eigensolve.rs` shift-invert Lanczos + dense fallback against synthetic
  test pairs.
  - **Observable signal:** Unit test in `crates/reify-solver-elastic/tests/eigensolve_synthetic.rs`
    constructs a hand-built `(K, B)` pair with a known spectrum; asserts that
    shift-invert Lanczos recovers the smallest 5 |λ| within 1e-8 tolerance.
    Dense-fallback test on a 50-DOF pair agrees with shift-invert to 8 digits.
  - **Prereqs:** None (purely kernel-internal).
  - **Crates touched:** reify-solver-elastic.

### Phase 3 — K_g element kernel slice

- **Task γ** — P1-tet K_g element-stiffness function + global assembly.
  - **Observable signal:** Unit test in `crates/reify-solver-elastic/tests/kg_p1_tet.rs`
    asserts (a) per-element K_g symmetry, (b) per-element K_g rank for trivial input
    stress = 0 (rank 0), (c) coarse-mesh column K_g + K passes Euler eigenvalue
    sanity at 10% tolerance.
  - **Prereqs:** β.
  - **Crates touched:** reify-solver-elastic.

### Phase 4 — Vertical-slice solver assembly (kernel-level, no stdlib yet)

- **Task δ** — `solve_buckling_kernel(...)` Rust API: linear-static pre-stress → K_g
  assembly → eigensolve → mode-shape recovery.
  - **Observable signal:** Integration test
    `crates/reify-solver-elastic/tests/euler_column_pin_pin.rs` constructs a
    Steel_AISI_1045 box (20×20×800 mm), applies pin-pin BCs + 1 kN tip load, and
    asserts `result.modes[0].eigenvalue × 1 kN` within **10%** of `π²EI/L²` (P1-tet
    bending lock — see §9.1). Same test file also covers the fixed-free (11%) and
    fixed-pin (10%) BC variants. The third variant is **fixed-pin, not fixed-fixed**:
    P1-tet pointwise Dirichlet imposes no rotational restraint, so the plain-Dirichlet
    third variant is fixed-pin. A fourth variant
    (`fixed_guided_euler_column_within_nine_percent`) uses a top-face `u_z`-equality MPC
    to realize true fixed-fixed (k=0.5); the MPC is verified correct but P1-tet bending
    lock floors error at ~6.8% (→ bounded **9%**, 8.46% at nx=ny=10). The original 5%
    needs P2 K_g (esc-3813-117 follow-up). Bounds corrected per esc-3453-5/6 (G6) and esc-3813-117.
  - **Prereqs:** β, γ.
  - **Crates touched:** reify-solver-elastic.

### Phase 5 — Stdlib + trampoline (first end-to-end through ComputeNode)

- **Task ε** — `fn solve_buckling` stdlib decl + `@optimized("solver::buckling")`
  trampoline wrapping `solve_buckling_kernel`. Result-interpretation helpers
  (`critical_load`, `mode_shape`, `safety_factor_buckling`).
  - **Observable signal:** `examples/buckling_column_smoke.ri` declares a steel column,
    calls `solve_buckling`, and prints `critical_load(result)`. `reify check
    examples/buckling_column_smoke.ri` evaluates the file and the printed value
    matches the analytical Euler load within 5%. CLI evaluation confirms; re-running
    hits the ComputeNode cache (dispatch-count instrumentation).
  - **Prereqs:** α, δ, **plus compute-node-contract.md §8 task η landed** (FEA round-trip
    proves the trampoline shape works; `solver::buckling` is a sibling registration).
  - **Crates touched:** reify-compiler/stdlib, reify-stdlib, reify-eval (registration only).

### Phase 6 — Shell + hex/wedge stub diagnostics

- **Task ζ** — `E_BucklingShellNotImplemented` (cite 3392) + `E_BucklingHexWedgeNotImplemented`
  (cite hex-wedge-meshing) emitted when `solve_buckling` is called on non-tet body.
  - **Observable signal:** Test fixture `.ri` that calls `solve_buckling` on a
    shell-classified body; CLI evaluation emits the named diagnostic with the citation
    payload. Same for hex/wedge body. No panic.
  - **Prereqs:** ε.
  - **Crates touched:** reify-solver-elastic, reify-stdlib (trampoline error path).

### Phase 7 — Multi-load-case coupling

- **Task η** — `MultiCaseBucklingResult` + `solve_buckling_load_cases` trampoline +
  `envelope_critical_load` + `worst_buckling_case` envelope helpers.
  - **Observable signal:** `.ri` fixture declares 2 LoadCases, calls
    `solve_buckling_load_cases`, prints `worst_buckling_case(mcbr)` and
    `envelope_critical_load(mcbr)`. CLI evaluation confirms case-name + critical-load
    match the per-case singletons. Re-run hits per-case cache.
  - **Prereqs:** ε. Plus `multi-load-case-fea.md` task 3005 (solve_load_cases) landed for
    the per-case dispatch pattern precedent.
  - **Crates touched:** reify-compiler/stdlib, reify-stdlib, reify-solver-elastic.

### Phase 8 — Significance filter integration

- **Task θ** — `is_opted_in("solver::buckling")` added; significance filter applies
  at the BucklingResult boundary same as ElasticResult.
  - **Observable signal:** `.ri` design loop with `param thickness : Length = auto`
    and a buckling-constraint. Auto-resolve drives thickness; only re-evaluates
    downstream when the buckling result differs beyond tolerance. Test
    instrumentation pins the no-recompute path.
  - **Prereqs:** ε.
  - **Crates touched:** reify-eval (significance_filter.rs allowlist).

### Phase 9 — GUI mode-shape-frame implementation

- **Task ι** — Backend `mode-shape-frame` emitter (Tauri command + IPC) + frontend
  `BucklingPanel.tsx` animator.
  - **Observable signal:** Open `examples/buckling_column_smoke.ri` in `reify gui`;
    after the solve completes, BucklingPanel appears in the side panel with the 10
    modes listed. Selecting a mode triggers animation; debug-MCP `screenshot_window`
    captures three frames during the animation cycle showing displacement progression.
    Playwright test `gui/tests/buckling_panel.spec.ts` asserts (a) panel mounts,
    (b) modes list renders, (c) animation frame rate ≥ 25 fps for the 800-mm column
    fixture.
  - **Prereqs:** ε. Plus GR-016 decomposition phase 1 (channel-registration scaffold
    landed); this task **replaces** GR-016's deferred bookmark task λ.
  - **Crates touched:** gui/src-tauri (new buckling.rs), gui/src (new BucklingPanel.tsx).

### Phase 10 — Persistent-cache hookup

- **Task κ** — Buckling results survive engine restart per persistent-fea-cache pattern.
  - **Observable signal:** Solve buckling for column; exit engine; restart; re-open
    file. First evaluation hits persistent cache (no trampoline call; verified via
    `--verbose` dispatch-count instrumentation). Result matches.
  - **Prereqs:** ε. Plus `compute-node-contract.md` §8 task ι (persistent-cache hookup
    for ComputeNode) landed.
  - **Crates touched:** reify-eval (persistent_cache extension), reify-solver-elastic
    (OpaqueState serialization for Cholesky factorization).

### Phase 11 — Companion correction tasks

- **Task μ** — `structural-stability-buckling.md` (parent PRD) prose update: §"Sketch of
  approach" gains a cross-link to this PRD; §"Open design questions" entries are
  marked resolved (with pointers to §3 of this PRD); §"Pre-conditions" updated
  to point at this PRD's foundation gates.
  - **Observable signal:** `git diff docs/prds/v0_5/structural-stability-buckling.md`
    shows the prose updates; doc lint passes; no code changes.
  - **Prereqs:** None (independent doc edit).
  - **Crates touched:** docs/prds/v0_5/.

- **Task ν** — `gui-event-channel-inventory.md` (GR-016) prose update: §3 table row for
  `mode-shape-frame` flips from DEFERRED to ACTIVE; task λ marked superseded with a
  pointer to this PRD's task ι.
  - **Observable signal:** `git diff docs/prds/v0_3/gui-event-channel-inventory.md`
    shows the prose update; reference to this PRD's §13 task ι in the supersession
    note.
  - **Prereqs:** None (independent doc edit).
  - **Crates touched:** docs/prds/v0_3/.

- **Task ξ** — `architecture-audit/gap-register.md` GR-024 Notes update: cross-link
  to this PRD as the resolution document.
  - **Observable signal:** GR-024 entry's Disposition / Notes field cites this PRD;
    git diff matches.
  - **Prereqs:** None.

### Dependency view

```
α (stdlib decls; GR-001-gated)
                                              μ (parent PRD update; independent)
β (eigensolve) ──┐                            ν (GR-016 update; independent)
                 ├─→ δ (kernel slice) ─→ ε ─┬─→ ζ (stub diag)
γ (Kg P1 tet) ──┘                           ├─→ η (multi-case)
                                            ├─→ θ (significance filter)
                                            ├─→ ι (GUI animator) ─┐
                                            └─→ κ (persistent cache)
                                                                  │
                              GR-016 phase-1 ─────────────────────┘
                              (channel scaffold)
                                                                  ξ (gap-register;
                                                                     independent)
```

External gates (foundation): GR-001 resolution gates α; `compute-node-contract.md` §8
task η gates ε; `multi-load-case-fea.md` task 3005 gates η; GR-016 phase-1 gates ι;
`compute-node-contract.md` §8 task ι gates κ.

## §14 — Open questions (surfaced but not decided in this session)

These are **tactical** — choices a downstream architect can make either way without
producing an architecturally-inferior result. Per META gate boundary.

1. **Cholesky factorization reuse heuristic.** OpaqueState carries the symbolic
   factorization. When is it safe to reuse the numeric factorization too (faster path,
   no eigenvalue-affecting changes)? Default: reuse only the symbolic factor; numeric
   factor is recomputed each time. Sharpening is profile-driven. **Suggested resolution:**
   ship symbolic-only reuse; flag numeric-reuse as a future optimization. Decide during
   task β.

2. **Lanczos restart strategy.** faer's `partial_self_adjoint_eigen` accepts a
   `restarts` parameter that controls thick-restart behavior. v0.5 default is
   `max_iters / max_dim`. Sharpening across the test fixtures will likely refine this.
   **Suggested resolution:** ship the default; tune empirically during task β.
   Decide during task β.

3. **Pre-stress load magnitude convention.** §3 resolved that λ *is* the safety factor
   when `applied_load == reference_load`. The `applied_load` argument to
   `safety_factor_buckling` is currently informational. Future non-linear buckling may
   want it; for v0.5 we accept the redundancy. **Suggested resolution:** none needed
   now. Decide if/when non-linear buckling PRD is authored.

4. **Animation phase parameterization.** §8 uses phase ∈ [−1, +1] sampled at 30 fps.
   Alternative: phase as `sin(2π t / period)` for a smooth back-and-forth. **Suggested
   resolution:** linear phase ramp `[−1, +1, −1]` is simpler; smoothness can be added
   later. Decide during task ι.

5. **Mode-shape scale normalization.** §8 normalizes peak displacement to ~10% of bbox
   diagonal. User may want explicit scale control. **Suggested resolution:** ship
   default + scale slider in the BucklingPanel UI; default value tuned by inspection.
   Decide during task ι.

6. **`auto_dense` threshold.** §3 picked DOF ≤ ~200. faer's dense gevd is O(n³); 200
   DOF is ~8 ms on commodity hardware. Could raise to 1000 (~1 s) for the convenience
   of dense path's exact-spectrum guarantee. **Suggested resolution:** ship 200; tune
   later. Decide during task β.

7. **n_modes = 10 default frame budget.** GUI BucklingPanel renders 10 mode thumbnails.
   For ≫10 modes (user override) the UI could paginate or scroll. **Suggested
   resolution:** ship scroll; paginate later if requested. Decide during task ι.

## §15 — Glossary

- **K** — elastic stiffness matrix (SPD after BCs). Shipped today for P1/P2 tets and
  bare-MITC3 shells.
- **K_g** — geometric stiffness matrix, stress-dependent (symmetric, possibly
  indefinite). **NEW in this PRD** for P1 tets only.
- **(K + λ K_g) φ = 0** — linear-buckling generalized eigenvalue problem.
- **Shift-invert** — spectral transformation: solve `(K − σ B)⁻¹ B φ = μ φ` instead of
  `K φ = λ B φ`. Lanczos converges to large-|μ|, which corresponds to small |λ − σ|
  in original problem. With σ = 0, finds smallest |λ| — the critical buckling
  multiplier.
- **Lanczos** — Krylov subspace method for symmetric / self-adjoint eigenproblems;
  faer's `operator::self_adjoint_eigen` is the operator-form variant.
- **Arnoldi** — non-symmetric Krylov method; not used in this PRD (buckling K_g is
  symmetric).
- **`LinOp<T>`** — faer trait for matrix-free operators; takes
  `apply(out: MatMut, v: MatRef, par: Par, stack: &MemStack)`. Shift-invert is built
  as a `LinOp` that internally does one Cholesky back-solve per matvec.
- **Mode** — eigenvalue + eigenvector pair, packaged as `Mode { eigenvalue, mode_shape }`.
- **`mode-shape-frame`** — GUI event channel (owned by GR-016) carrying one frame of
  mode-shape animation: `{ mode_index, phase, displaced_positions }`.
