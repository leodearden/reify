# Shift-invert eigensolve: honor `BucklingOptions.sigma` and `ModalOptions.shift_frequency`

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-09-03 · **Approach:** B + H (contract + two-way boundary tests)

**Code anchors** verified against main `c8bfba6087` (2026-09-03). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Provenance:** chartered by task **#7178** under Leo's ruling of 2026-09-01 (esc-7077-1), authored in the
`/prd` session that discharges **esc-7178-1**. Every measurement in §3 was taken first-hand in that
session and re-verified at the anchor SHA above. The design decisions in §5 were made by Leo in that
session and are recorded here, not re-opened.

**Normative substrate:** `docs/legibility/design-invariants.md` — this PRD **consumes** **INV-PD-1**
(`declared-param-reaches-kernel`, owned by `docs/prds/v0_6/trampoline-param-drop-closure.md`) by moving
two params out of its `ignored` allowlist into `honored`. It establishes no new invariant. Its full
G7 walk is §9.1; the consumed set is **INV-PD-1**, **INV-PD-2**, **INV-SF-1**, **INV-SF-2**,
**INV-SF-3**, **INV-SF-5**, **INV-SF-6** and **INV-AD-4**.

---

## 1. Goal

One user-observable guarantee:

> Setting a non-zero spectral shift **changes which modes the solver returns** — and if that shift
> means the result can no longer answer "what is the first mode?", every helper that claims to answer
> it says so with an Error instead of returning a plausible wrong number.

Today neither half holds. `BucklingOptions.sigma` is warned-and-dropped; `ModalOptions.sigma` is
plumbed the whole way to a dead struct field and silently discarded. `EigenSolverOptions.sigma` is
constructed and never read by anything.

After this PRD:

- **The shift is real.** A non-zero `sigma` / `shift_frequency` assembles and factors `K − σB` and
  returns the eigenvalues nearest σ, not the eigenvalues nearest zero.
- **The unconservative read is closed.** A shifted solve records whether it skipped modes; the three
  `modes[0]` helpers (`critical_load`, `safety_factor_buckling`, `first_frequency`) raise a coded
  Error rather than reporting a mode from the middle of the spectrum as if it were the first.
- **The vacuity is discharged.** Both params leave the PDROP C1 `ignored` set and #7178 stops being
  their owner.

## 2. Consumers

Every consumer is concrete and live at authoring time; verify status at implementation time rather
than trusting this list.

| Consumer | What it consumes |
|---|---|
| **#7085** — PDROP detector + owning-task liveness lane | Resolves #7178's liveness. When this PRD lands, both params move to `honored` and carry no allowlist entry. |
| **#7081** — trampoline param-drop leaf γ | Writes the `BucklingOptions` C1 declaration and cites #7178 for `sigma`. This PRD retires that cite. |
| **#6097** — `ModalOptions.sigma` → `shift_frequency : Frequency` | Adds a not-yet-honored modal warning citing #7178. This PRD retires it in the same diff that honors the param. |
| FEA authors | Targeted eigenvalue extraction: skipping spurious near-zero modes, accelerating convergence toward a hand-calculated load, inspecting a frequency band, reaching negative (reversed-load) buckling eigenvalues. |

In-engine seam: §3.4 ComputeNode dispatch (`docs/prds/v0_3/engine-integration-norm.md`) — both
surfaces are `@optimized` trampolines reached through the existing dispatch table. No new seam.

## 3. Measured state

All measured 2026-09-03 at `c8bfba6087`.

**`EigenSolverOptions.sigma` is a dead field.** `grep -n sigma` over
`crates/reify-solver-elastic/src/eigensolve.rs` returns **exactly three** lines: the defaults doc, the
declaration (`pub sigma: f64`, doc "Shift σ (reserved for shifted-inverse formulation; currently
0.0)"), and the `Default` impl. It is read by nothing — not `solve_eigen_shift_invert`, not
`try_solve_eigen_shift_invert`, not `lanczos_shift_invert`, not `solve_eigen_dense`, not
`check_eigen_options_and_shapes`.

**Despite the names, there is no shift anywhere.** `try_solve_eigen_shift_invert` factors **K itself**
via `k.sp_cholesky(Side::Lower)` and builds `CompositeShiftInvertOp { k_op, m_op, n }` — plain
`K⁻¹·M` inverse iteration. `lanczos_shift_invert` converts `μ → λ = 1/μ`. No `K − σB` assembly exists
anywhere in the workspace.

**The two paths are asymmetric, and the record understated this.**

| | plumbing | today's behaviour at σ≠0 |
|---|---|---|
| **Buckling** | `BucklingKernelOptions` has **five** fields (`n_modes`, `eigen_tol`, `eigen_max_iters`, `cg_tolerance`, `cg_max_iter`) — **no sigma input at all**. Both `solve_buckling_kernel` call sites construct `EigenSolverOptions { …, sigma: 0.0 }` as a literal. | Warns (`W_BucklingOptionUnsupported`, note text "the buckling kernel has no shift-origin input yet") and drops. The warning **cites no task id**. |
| **Modal** | `extract_eigen_knobs` (`crates/reify-eval/src/modal_ops.rs`) **does** read `sigma` and writes it into `EigenSolverOptions.sigma` at the `free_vibration` call site. | **Silently** dropped at the dead field. Modal has no unsupported-option warning today; #6097 adds one. |

So the modal path is *plumbed to a dead field*; the buckling path is *not plumbed but loud*. The
factorization work is shared; the wiring work is not.

**Nothing reds today.** Every tracked `.ri` call site spells the declared default:
`examples/buckling_column_p2.ri` is the only `BucklingOptions` site; the `ModalOptions` sites are
`examples/modal/*.ri`, `examples/trajectory/printer_print_envelope.ri`, `prj/printer_v01/printer.ri`
(three sites), `tests/prd-gate/fixtures/r3b_displacement_at_selector_grammar.ri`, and
`crates/reify-eval-fea-tests/tests/fixtures/clamped_clamped_beam_modes.ri` (three sites — a site the
2026-09-01 record did not list).

**Three stdlib helpers read `modes[0]` blind.** `critical_load(result, ref)` is
`result.modes[0].eigenvalue * reference_load`; `safety_factor_buckling` is `result.modes[0].eigenvalue`;
`first_frequency` is `result.modes[0].frequency` under a comment asserting "`modes[0]` is the
fundamental mode". None can see σ. §5.4 is what closes this.

**Result structs are pinned.** `BucklingResult` is asserted at *exactly 4* param cells by
`crates/reify-compiler/tests/buckling_stdlib_compile.rs`; `ModalResult` at *exactly 6* by
`crates/reify-compiler/tests/modal_options_validation_tests.rs`. Adding a provenance field touches
both assertions — named blast radius, not a blocker.

## 4. What σ is, and why anyone sets it

This section exists because the shift is not self-explanatory and every future reader of this PRD will
need it.

Both paths solve `K φ = λ B φ`. What λ *means* differs:

- **Buckling** (`B = −K_g`, from the pre-stress solve): λ is a **dimensionless load multiplier** —
  multiply the reference load by λ and the structure buckles. λ < 0 is buckling under the *reversed*
  load.
- **Modal** (`B = M`): λ = ω² = (2πf)², units s⁻². `eigenvalue_to_frequency_hz(λ) = √λ/(2π)` in
  `crates/reify-stdlib/src/modal/free_vibration.rs` is the existing crossing; #6097 adds the inverse.

Krylov methods find the **largest |μ|** fastest. Shift-invert hands Lanczos the operator
`(K − σB)⁻¹B`, whose eigenvalues are `μ = 1/(λ − σ)`. Largest |μ| ⟺ smallest |λ − σ| ⟺ **λ nearest
σ**. Today σ is hardwired 0, the operator is `K⁻¹B`, and only the modes nearest zero are reachable.

Four reasons to set σ ≠ 0:

1. **Skip junk near zero.** Mechanisms and unrestrained rigid-body DOFs produce spurious λ≈0 modes
   that consume every `n_modes` slot.
2. **Speed.** Lanczos converges fastest near σ. A hand calculation that puts the buckling load near
   λ≈3.5 makes σ=3.5 converge in a fraction of the iterations of crawling up from 0.
3. **Inspect a band.** "What modes live near 300 Hz?" → `shift_frequency: 300Hz`.
4. **Reach negative eigenvalues** (buckling). At σ=0 the sort is by |λ|, so λ=−2 and λ=+3 compete on
   equal terms; a negative σ targets the reversed-load side deliberately.

**Why this is a factorization-strategy change, not parameter threading.** K is SPD after boundary
conditions. By Sylvester's inertia law, `K − σB` acquires one negative eigenvalue for every eigenvalue
σ passes. Cholesky cannot factor an indefinite matrix — the existing
`.expect("eigensolve: K must be SPD; …")` is exactly what fires. **σ above the first eigenvalue is the
normal case, not an edge case**: reasons 1 and 3 are inherently above λ₁ and reason 2 straddles it.
Only σ *below* the first mode keeps the matrix SPD.

## 5. Design decisions (resolved; do not re-open)

### 5.1 Factorization dispatch — σ=0 verbatim, else Cholesky-then-LU

**Substrate (verified, and it corrects the chartering record):** faer 0.24 ships **no sparse LDL^T
solver**. `faer::sparse::solvers` declares exactly `Llt`, `Qr`, `Lu`. `sp_lu()` — LU with **partial row
pivoting** — exists on both `SparseColMat` and `SparseRowMat`, returns `Result<Lu<I,T>, LuError>`, and
`Lu` implements `SolveCore<T>`, so it is a drop-in behind `StiffnessOp::solve_in_place` exactly as
`Llt` is today. LDL^T exists only as the low-level *unpivoted* `factorize_simplicial_numeric_ldlt` /
`factorize_supernodal_numeric_ldlt`, which breaks down on indefinite matrices without regularization.
**The factorization is LU, not LDL^T.** #7178's suggestion of LDL^T predates this measurement.

```
if sigma == 0.0 {
    k.sp_cholesky(Side::Lower)        // today's path, unchanged
} else {
    let a = shifted(k, b, sigma);     // K - σB
    match a.sp_cholesky(Side::Lower) {
        Ok(llt)         => llt,       // σ below the first mode: cheap, and skipped = false
        Err(Numeric(_)) => a.sp_lu()?,// indefinite: LU, and skipped = true
        Err(Generic(e)) => panic!(..),// resource/index failure — existing discipline
    }
}
```

Rationale: the σ==0 guard means **no landed golden moves** — `euler_column_pin_pin.rs` (four BC
families with pinned tolerances), `buckling_smoke.rs`, `buckling_persistent_cache_round_trip.rs` and
the modal goldens are structurally untouched, because σ=0 takes the same faer calls in the same order.
The Cholesky-first attempt is not merely an optimization for reason 2 above: **its success/failure IS
the correctness discriminator of §5.4**, so it is load-bearing, not a perf tweak.

`LuError` carries exactly two arms, matching the existing `SparseLltError` discipline:
`SymbolicSingular { index }` → the typed failure of §5.3; `Generic(FaerError)` → panic naming the
resource failure, never mistaken for a domain fact about the model.

### 5.2 Selection by |λ−σ|; presentation order by |λ|

These are **two different rules and no leaf may conflate them.**

- **Selection** — *which* `n_modes` eigenvalues come back — is by ascending **|λ − σ|**. This is what
  makes the shift mean anything, and it is Lanczos's natural output (largest |μ|).
- **Presentation order** — the order of `result.modes` — is ascending **|λ|**, as today.

At σ=0 the two coincide and the behaviour is identical to today's. At σ≠0 a 300 Hz band inspection
returns 295, 298, 301, 310 Hz — a mode table an engineer can read — rather than the proximity order
301, 298, 310, 295.

The `|λ|` (absolute, not signed) convention is today's and is preserved unchanged: with negative
eigenvalues present, λ=−2 still sorts before λ=+3.

### 5.3 σ at an eigenvalue — Error, with perturbation deferred as opt-in

`K − σB` is singular when σ lands on an eigenvalue. Detection is **two-part**, because faer's
`LuError::SymbolicSingular` reports only *structural* rank deficiency: partial-pivot LU on a
numerically tiny pivot returns `Ok` and yields garbage.

1. `LuError::SymbolicSingular` → the typed failure directly.
2. A post-factorization guard on the recovered spectrum (non-finite λ, or a back-substitution residual
   above a documented threshold) → the same typed failure.

The response is a **coded Error** — `DiagnosticCode::ShiftAtEigenvalue`, mnemonic
`E_ShiftAtEigenvalue` — naming the offending σ and telling the author to move it. **No
automatic perturbation.** Nudging σ and continuing is the silent-substitution class this PRD exists to
close; if it is ever wanted it arrives later as an explicit opt-in knob, never as a default.

### 5.4 Skipped-mode provenance — warn at solve, Error at use

This is the design decision with the largest safety consequence, so the reasoning is recorded in full.

**The defect.** `critical_load(result, ref)` returns `result.modes[0].eigenvalue * reference_load`. At
σ≠0 the returned window sits *around σ*, and every eigenvalue below σ is simply absent. So the helper
reports the multiplier of whichever mode was nearest the shift — which is **higher** than the true
first mode. The user is told a column holds 160 kN when it buckles at 41 kN. That is the
unconservative direction, and `first_frequency` fails the same way: a fundamental reported at 300 Hz
that is really at 30 Hz means driving a machine straight through a resonance.

**The correctness condition is exactly decidable, for free.** By Sylvester's inertia law the number of
eigenvalues below σ equals the number of negative eigenvalues of `K − σB`, and a Cholesky of
`K − σB` succeeds **iff** that matrix is positive definite **iff** no eigenvalue lies between zero and
σ. (For buckling's indefinite `B = −K_g`, "positive definite" means σ lies in the eigenvalue-free
interval containing zero — i.e. no buckling mode has been passed in *either* direction, which is the
right condition.) So the §5.1 dispatch already computes the answer:

- **Cholesky succeeds** ⟹ nothing skipped ⟹ `modes[0]` genuinely is the first mode ⟹ helpers correct.
- **Cholesky fails `Numeric`** ⟹ at least one mode was passed ⟹ helpers would be wrong.

This is exact, not a heuristic, and costs nothing beyond the factorization already being performed.

**Precision limit — say "at least one", not a count.** The Cholesky/LU discriminator yields a
**boolean**. An exact count needs an inertia-revealing LDL^T, which faer's sparse LU does not expose.
The **dense path is the exception**: `solve_eigen_dense` computes the entire spectrum via QZ, so it can
count exactly. Diagnostic wording must therefore be boolean-safe on the Lanczos path and may be exact
on the dense path — one message template with an optional count, never two drifting messages.

**The response, at two levels:**

- **At solve time — a Warning.** A shifted solve that skipped modes emits
  `DiagnosticCode::ShiftSkippedModes` (mnemonic `W_ShiftSkippedModes`) naming σ and stating that the
  result is a *window*, not the bottom of the spectrum. Advisory: band inspection (reason 3) stays
  legal.
- **At use time — an Error.** `critical_load`, `safety_factor_buckling` and `first_frequency` raise
  `DiagnosticCode::FirstModeNotInShiftedResult` (mnemonic `E_FirstModeNotInShiftedResult`) when handed
  such a result. The message names the helper, σ, and the fact that the first mode is not in the
  result.

**Normative names.** All three `DiagnosticCode` variants this PRD introduces — `ShiftSkippedModes` and
`FirstModeNotInShiftedResult` here, `ShiftAtEigenvalue` in §5.3 — are **fixed by this PRD**, not left
to implementation taste: the capability manifest binds mechanical checks to them, and leaf α mints all
three in one place so the sibling wiring leaves γ and δ do not both edit `diagnostics.rs`.

Refusal therefore lands on the *incorrect use*, not on the legitimate solve. A user who shifts to
inspect a band and never asks "what is the first mode?" is warned once and otherwise unobstructed.

**Mechanism.** `EigenSolverResult` gains two fields, named normatively here:
`shift_skipped_modes: bool` (the C5 provenance boolean) and `shift: f64` (the σ actually used, so a
diagnostic can name it without the caller re-deriving it). The flag must then reach `.ri`, so
`BucklingResult` gains a fifth param and `ModalResult` a seventh, and the two frozen-shape assertions
(§3) move with them. Reify `constraint` clauses express
only scalar predicates over a struct's own cells and cannot express "this call is invalid", so the
three helpers convert from pure `.ri` bodies to `@optimized` trampolines that raise the diagnostic in
Rust. `@optimized` is well-worn substrate — 55 uses across the stdlib, including the neighbouring
`solve_buckling` and `modal_analysis` — so this is a conversion, not a new mechanism.

Returning `Value::Undef` instead was considered and **rejected**: `Undef` is the silent-failure
sentinel (`INV-SF-1`), which is the opposite of the loudness this PRD is for.

### 5.5 The dense path is a selection change, and lands first

`solve_eigen_dense` computes the **entire** spectrum with `gevd_real` and then sorts by |λ| and takes
the first `n_modes`. Honoring σ there is a **sort-key change and nothing else** — no factorization, no
new numerics, no new failure mode, and an exact skipped-mode count for free.

This is not a backwater. The dense path serves the small-model regime (`n ≤ max(64, 2·n_modes)`),
buckling's own Lanczos-floor fallback inside `try_solve_eigen_shift_invert`, and modal's singular-K
fallback under `DENSE_FALLBACK_MAX_DIM`. So §6's contract is defined once and implemented twice, and
the safe implementation lands first (leaf α) with the risky one held to it (leaf β).

**Consequence for fixture design, and it is a trap:** a fixture small enough to be quick may exercise
*only* the dense path and never touch Lanczos at all. G2 evidence must state which path it exercises
and the two wiring leaves must cover both.

## 6. The contract (H component)

`EigenSolverOptions.sigma` is the shift **in eigenvalue (λ) space** for both callers. Unit conversion
is the caller's job, not the eigensolver's (§7 seam table).

Every implementation of the generalized eigensolve must satisfy all six clauses:

- **C1 — σ=0 is the identity.** `sigma == 0.0` produces the same eigenvalues, the same order and the
  same code path as before this PRD. Structurally guaranteed by the §5.1 dispatch, not asserted as a
  numerical tolerance.
- **C2 — Selection.** The returned set is the `n_modes` eigenvalues of the pencil with smallest
  |λ − σ| that the method converged.
- **C3 — Order.** `eigenvalues` is ascending by |λ|, with `eigenvectors` columns permuted to match.
- **C4 — Back-shift.** Eigenvalues are returned in the original λ space (`λ = σ + 1/μ` on the Lanczos
  path), never in shifted or μ space.
- **C5 — Provenance.** `EigenSolverResult.shift_skipped_modes` reports whether any eigenvalue of the
  pencil lies between zero and σ and is absent from the returned set, and `EigenSolverResult.shift`
  carries the σ used. `false` is only ever reported when that has been *established* (Cholesky
  success, or a full-spectrum count), never assumed.
- **C6 — Singularity.** A singular or numerically-degenerate `K − σB` is a typed failure carrying σ,
  never a panic, never a silently perturbed solve, never a garbage spectrum.

### Two-way boundary tests

The seam is `eigensolve.rs` ↔ its two callers, and the contract is implemented twice. Boundary tests
face both ways:

- **BT1 — σ=0 identity, both implementations.** For a fixture pencil, dense and Lanczos at σ=0
  reproduce the pre-PRD eigenvalues and order.
- **BT2 — cross-implementation agreement.** For a pencil sized to be solvable both ways, dense and
  Lanczos at the same σ≠0 return the same eigenvalue *set* to solver tolerance, the same order (C3),
  and the same C5 provenance boolean.
- **BT3 — selection really moved.** At a σ above λ₁ the returned set differs from the σ=0 set. This is
  the mechanical form of the G2 signal at the solver layer.
- **BT4 — provenance is honest, both directions.** σ below λ₁ ⟹ `skipped == false` and `modes[0]`
  equals the σ=0 first mode. σ above λ₁ ⟹ `skipped == true`.
- **BT5 — singular shift.** σ placed on a known eigenvalue of a small analytic pencil produces the C6
  typed failure, not a panic and not a finite-looking wrong answer.
- **BT6 — caller-facing.** Both trampolines convert their surface value into λ-space σ correctly, and
  a non-zero shift changes the mode set visible from `reify eval`.

## 7. Cross-PRD relationship and seam ownership

| Seam | Owner | Note |
|---|---|---|
| `BucklingOptions.mode`, `BucklingOptions.auto_dense` | **#7179** | Explicitly **not** this PRD. #7179 dispatches dense vs shift-invert, builds `E_BucklingInvalidMode`, and decides the orphaned `auto_dense` threshold. |
| `docs/prds/v0_5/buckling-eigensolver.md` phantom-done residue (§5 `mode` paths, §4 `E_BucklingInvalidMode`, §14 Q6) | **#7179** | Named here only so nobody re-absorbs it. |
| `ModalOptions.sigma` → `shift_frequency : Frequency` retype | **#6097** | **No dependency edge in either direction** (§7.1). |
| `TractionLoad` / `BodyForce` on the buckling path | **#5800** | Untouched. |
| Buckling `supports` / `PointLoad.point` / `.direction` | **#7081** | Untouched. This PRD does not enter `extract_total_load` or the BC path. |
| PDROP detector + liveness lane | **#7085** | Consumes this PRD's C1 flip. |
| INV-PD-1 contract and the C1 set vocabulary | `trampoline-param-drop-closure.md` | This PRD is a consumer, and adds no fourth disposition. |
| Sign convention for a negative "critical load" | **unowned; out of scope** | §8. |

### 7.1 Why there is no edge with #6097, and how leaf δ stays independent

#6097 renames and retypes the modal surface; this PRD honors whatever surface exists. Neither blocks
the other, and wiring an edge would be wrong in both directions.

Leaf δ therefore reads the shift **surface-agnostically**, and must *check* rather than assume:

- If #6097 has landed — `shift_frequency : Frequency` — convert with λ = (2πf)², using the inverse of
  `eigenvalue_to_frequency_hz` that #6097 adds beside it in
  `crates/reify-stdlib/src/modal/free_vibration.rs`, and retire #6097's not-yet-honored warning in the
  same diff.
- If it has not — `sigma : Real`, documented as dimensionless eigenvalue units — pass the value
  through as λ directly, and leave the surface alone.

Either way `EigenSolverOptions.sigma` receives λ-space, and leaves α/β never learn which surface fed
them. The conversion lives at the trampoline, not in the eigensolver.

## 8. Out of scope

- **Buckling `mode` and `auto_dense`** — #7179.
- **Automatic σ perturbation** — deliberately deferred (§5.3).
- **An exact skipped-mode count on the Lanczos path** — needs an inertia-revealing factorization faer
  does not expose (§5.4).
- **Multiple shifts / spectrum sweeps / `modes_near(part, n, freq)`** — a single shift per solve. The
  convenience helper #6097's text mentions exists nowhere in the tree and is not built here.
- **B-orthogonal or (K−σB)-orthogonal Lanczos.** The existing path hands faer's
  `partial_self_adjoint_eigen` an operator that is self-adjoint in a non-Euclidean form; this PRD
  preserves that arrangement rather than replacing the Krylov method. See §11 Q1.
- **The signed-`|λ|` convention for a negative first mode.** With `|λ|` ordering, a reversed-load mode
  at λ=−0.5 already sorts ahead of a forward mode at λ=+4.1e4 and `critical_load` already returns a
  negative Force **today at σ=0**. Pre-existing, unchanged here, and worth its own task (§11 Q3).
- **FEA doc-chunk coverage.** §10 leaf η, waiver recorded.

## 9. Pre-conditions

All verified at `c8bfba6087`; re-verify at implementation time.

- `faer::sparse` `sp_lu()` on `SparseRowMat`, `Lu: SolveCore<f64>`, `LuError::{SymbolicSingular, Generic}` — **present**.
- `@optimized` trampoline mechanism — **present** (55 stdlib uses).
- `DiagnosticCode` is a plain enum with rustdoc-documented mnemonics; adding variants is routine — **present**.
- `BucklingResult` exactly-4 and `ModalResult` exactly-6 frozen-shape assertions — **present, and must move** (§3).
- No new `.ri` grammar. Every construct this PRD needs (`param` on a `structure def`, `@optimized`, ctor
  field spelling) is already in use at the sites being edited. **`grammar_confirmed = true`** for every leaf.

### 9.1 Design-invariant walk (G7)

No leaf violates an invariant in `docs/legibility/design-invariants.md`; no waiver is needed. The
non-obvious ones, and what each obliges:

- **INV-AD-4 `boundaries-declare-angle-convention`** — the one that is easy to miss. Leaf δ's
  λ = (2π·f)² is an **angular convention crossing** (Hz, cycles/s → rad/s → λ), and the invariant's own
  house pattern names `eigenvalue_to_frequency_hz` as a *declared-bridge* precedent. δ's conversion is
  that bridge's mirror image and must be equally declared: it uses the inverse helper #6097 adds beside
  it in `crates/reify-stdlib/src/modal/free_vibration.rs`, carrying the same doctrine-citing comment
  shape, rather than re-deriving 2π inline at the trampoline. **A bare inline `2.0*PI*f` in
  `modal_ops.rs` would be a silent crossing and an INV-AD-4 defect even though the number is right.**
  Buckling is unaffected — λ there is a dimensionless load multiplier and no angle is crossed.
- **INV-SF-1 `undef-has-provenance`** — §5.4 rejects `Value::Undef` as the helper-refusal mechanism for
  exactly this reason; the refusal is a coded Error.
- **INV-SF-2 `error-severity-exits-nonzero`** — `E_ShiftAtEigenvalue` and
  `E_FirstModeNotInShiftedResult` are `Severity::Error` and must make `reify eval` exit non-zero. A
  refusal that exits 0 is not a refusal.
- **INV-SF-3 `declared-intent-consumed-or-diagnosed`** and **INV-SF-5 `placeholders-owned-and-loud`** —
  this PRD is the discharge of both for these two params: the intent becomes consumed, and the loud
  placeholder is retired together with its owner (leaf ζ).
- **INV-SF-6 `diagnostics-carry-codes`** — all three new diagnostics carry `DiagnosticCode` variants,
  minted in leaf α.
- **INV-PD-2 `result-fields-populated-or-owned`** — leaf ε's new `BucklingResult` / `ModalResult`
  provenance params must be **populated on the production path**, never left `Undef`. A provenance flag
  that is itself absent would re-open the defect it exists to close.

## 10. Decomposition plan

Eight leaves, filed 2026-09-03 and committed together. Greek labels are authoring handles; the real task ids are stamped on each row below. Cite the id, never the status — the id is immutable and queryable, a status word rots the moment the task moves.

**α — Eigensolver shift contract + dense implementation.** `α #7258`
Establishes §6 C1–C6 as the eigensolver's documented contract; adds the `EigenSolverResult` provenance
fields; implements selection-by-|λ−σ| with order-by-|λ| in `solve_eigen_dense` (pure sort-key change,
exact skipped count); lands BT1–BT5 as the harness both implementations are held to. **Mints all three
`DiagnosticCode` variants** (`ShiftSkippedModes`, `ShiftAtEigenvalue`, `FirstModeNotInShiftedResult`)
here rather than in the leaves that emit them, so the sibling wiring leaves γ and δ do not both edit
`crates/reify-core/src/diagnostics.rs` and serialize against each other under the narrow file locks.
*Intermediate.* Unlocks β, γ, δ, ε.
*Same-diff obligation:* any new gate-resident test binary in `reify-solver-elastic` carries its
`.config/nextest.toml` slow-timeout override or heavy-filter entry in this diff — the crate is
gate-resident by default and only the binaries named in `scripts/heavy-test-filter-lib.sh` are excluded.

**β — Lanczos/LU shifted factorization.** `β #7259`
`K − σB` assembly; the §5.1 dispatch; `StiffnessOp`'s SPD doc relaxed and the adapter generalized over
`Llt`/`Lu`; back-shift λ = σ + 1/μ; the §5.3 two-part singular detection as a typed failure. Must pass
α's BT1–BT5 unchanged, and BT2 cross-implementation agreement is the acceptance.
*Intermediate.* Depends on α. Unlocks γ, δ.

**γ — Buckling kernel + trampoline wiring.** `γ #7260`
`BucklingKernelOptions` gains `sigma`; both `solve_buckling_kernel` call sites pass it instead of the
`0.0` literal; the trampoline reads `BucklingOptions.sigma`, passes it through, emits
`W_ShiftSkippedModes` and the §5.3 Error, and **retires the `sigma` arm of
`buckling_unsupported_option_diagnostics` only** — the `mode` and `auto_dense` arms are #7179's and
must survive. Updates `buckling_option_unsupported.rs` cases (a)/(e) accordingly.
*Signal:* `reify eval` on the committed fixture pair
`tests/prd-gate/fixtures/shift_invert_buckling_{unshifted,shifted}.ri` returns a **different mode
set** at a non-zero `sigma` than the otherwise byte-identical fixture at `sigma: 0.0`, and emits no
`W_BucklingOptionUnsupported` for `sigma`. Each fixture states in a header comment which solver path
it exercises (§5.5).
*Depends on β.*

**δ — Modal trampoline wiring.** `δ #7261`
`extract_eigen_knobs`'s shift read becomes surface-agnostic per §7.1 and reaches
`EigenSolverOptions.sigma` as λ-space; `W_ShiftSkippedModes` and the §5.3 Error surfaced; #6097's
not-yet-honored modal warning retired **if and only if** it has landed.
*Signal:* `reify eval` on the committed fixture pair
`tests/prd-gate/fixtures/shift_invert_modal_{unshifted,shifted}.ri` returns a different mode set at a
non-zero shift than at the default, with the returned frequencies clustered near the requested shift.
*Depends on β.*

**ε — Result provenance + helper refusal.** `ε #7262`
`BucklingResult` gains a fifth param and `ModalResult` a seventh carrying the C5 flag; both
frozen-shape assertions updated; `critical_load`, `safety_factor_buckling` and `first_frequency`
convert to `@optimized` trampolines — targets `solver::critical_load`, `solver::safety_factor_buckling`
and `modal::first_frequency`, registered in the `compute_targets` dispatch table alongside
`solver::buckling` — raising `E_FirstModeNotInShiftedResult`.
*Signal:* `reify eval` on a shifted fixture calling `critical_load` errors with the coded diagnostic
naming the helper and σ; the same call on an unshifted fixture returns exactly the value it returns
today.
*Depends on γ, δ.*

**ζ — PDROP C1 flip and cite retirement.** `ζ #7263`
Both params move from the C1 `ignored` set to `honored`; #7178 stops being their owner; every `#7178`
cite in tracked source is removed. Check what has actually landed rather than assuming — #7081 writes
the buckling C1 declaration and #7085 builds the detector, and neither is guaranteed to be in place.
*Signal:* no `#7178` cite remains in tracked source; the C1 declarations name both params `honored`;
the PTODO fingerprint ratchet stays green. If #7085 has landed, `/audit --pattern PDROP` additionally
reports both params honored with no allowlist entry.
*Depends on γ, δ, ε.*

**η — Exemplar corpus + discoverability.** `η #7264`
The G2 fixtures graduate to `examples/best_practices/spectral_shift.ri` with its `INDEX.md` line and
a one-line index entry in `.claude/skills/reify-design/SKILL.md`.
*Docs-truth waiver, recorded:* the overlay's doc-chunk leaf is **waived**. Measured 2026-09-03: the
FEA/solver surface has **zero** presence in `crates/reify-mcp/src/tools/chunks/` — no chunk mentions
buckling, modal, eigen or any `solve_*` function — so a chunk edit here would be a one-paragraph
orphan in a topic that does not exist. This leaf files a follow-up task for FEA chunk coverage as a
whole rather than seeding it sideways.
*Signal:* an author who knows the goal ("only look at modes near a frequency I care about") but not the
feature name reaches the mechanism from the corpus index line.
*Note:* touching `examples/*.ri` has no carve-out arm in `verify.sh`'s `decide_scope` and falls to the
conservative default, so this diff runs the full workspace gate. Expected, not a defect.
*Depends on γ, δ.*

**θ — PRD close.** `θ #7265`
Stamps the terminal `Status:` marker with the landed leaf ids, adds the AS-AUTHORED freeze paragraph
and the LIVE/AS-AUTHORED map, and applies the matching header to the capability manifest.
*Signal:* the committed header.
*Depends on every other leaf.*

```
α ──► β ──┬──► γ ──┬──► ε ──► ζ ──► θ
          └──► δ ──┘        │       ▲
                   └──► η ──┴───────┘
```

## 11. Open questions (tactical; resolvable at implementation time)

1. **Non-Euclidean self-adjointness.** `faer::partial_self_adjoint_eigen` orthogonalizes in the
   Euclidean inner product, but `K⁻¹B` is self-adjoint in the K-inner product, and `(K − σB)⁻¹B` in
   the `(K − σB)` form — which is not an inner product at all once that matrix is indefinite. The
   existing σ=0 path already relies on this arrangement and its goldens pass. Leaf β should **measure**
   whether convergence quality degrades at large σ rather than predict it, and if it does, the
   response is a convergence diagnostic (`converged: false` already exists on `EigenSolverResult`),
   not a Krylov rewrite — that is out of scope per §8.
2. **Residual threshold for §5.3 part 2.** The numerically-singular guard needs a concrete threshold.
   Derive it from the pencil's scale at implementation time; do not import a constant from another
   solver.
3. **Negative critical load.** Whether `critical_load` should report a signed Force or the smallest
   *positive* multiplier is a pre-existing convention question (§8), surfaced but not created by this
   work. File it as its own task from leaf ε rather than folding it in.
4. **Diagnostic rustdoc wording.** The three `DiagnosticCode` variant names (`ShiftSkippedModes`,
   `ShiftAtEigenvalue`, `FirstModeNotInShiftedResult`) and their `W_`/`E_` mnemonics are **fixed** by
   §5.3–§5.4 and bound by the capability manifest — do not rename them. What remains open is the
   canonical-message-form rustdoc each variant carries, which follows the neighbouring
   `BucklingOptionUnsupported` block's shape.
