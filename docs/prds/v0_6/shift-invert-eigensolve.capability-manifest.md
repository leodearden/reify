# Capability manifest — `shift-invert-eigensolve`

**PRD:** `docs/prds/v0_6/shift-invert-eigensolve.md` · **Machine twin:** `shift-invert-eigensolve.capability-manifest.yaml`

**Measured against main `c8bfba6087` (2026-09-03).** Every binding below was verified first-hand in the
authoring session; none is inherited from the chartering record. Two of them **correct** that record —
see β/`sparse-lu-…` (LDL^T is unavailable; LU is the factorization) and δ/`modal-already-plumbs-…`
(modal is already wired to the dead field, so the two paths are not symmetric).

Mechanizes G3 (substrate) + G6 (premise validity) per leaf. **21 bindings, all PASS — no binding blocks
the batch.** Mechanical (`grep`) checks are copied into producer `metadata.delivered_checks` by
`commit_planning`; `manual` checks stay sidecar-only and are excluded from the dispatch gate.

Every mechanical pattern below was confirmed **currently absent** on main (measured counts: `sp_lu` 0,
`shift_skipped_modes` 0, `pub sigma` in `buckling_kernel.rs` 0, `solver::critical_load` 0), so each is a
genuine landing signal rather than a vacuous match.

---

## α — eigensolver shift contract + dense-path selection

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `dense-path-computes-full-spectrum-so-selection-is-the-only-change` | substrate, wired-on-main. `solve_eigen_dense` densifies (K, B), runs faer `gevd_real` (QZ), recovers λ for **all** n, then sorts by \|λ\| and takes `n_modes`. Honoring σ there is a sort-key change — no factorization, no new failure mode, exact skipped count free. Reached from three live sites: `solve_generalized_eigen`'s small-model regime and singular-K fallback, plus `try_solve_eigen_shift_invert`'s Lanczos-floor fallback. Not test-only. | PASS | grep `shift_skipped_modes` present in `eigensolve.rs` |
| `eigensolveroptions-sigma-is-a-dead-field-this-leaf-revives` | G6 premise, re-measured at `c8bfba6087`. `grep -n sigma` over `eigensolve.rs` returns **exactly three** lines — defaults doc, `pub sigma: f64`, `Default` impl. No solver function reads it. Premise true, not stale. | PASS | manual — negative structural fact about main *before* landing; a bare-token `expect: absent` would be violently over-broad (`InitialStress3.sigma`, element stress). Positive half covered above. |
| `three-diagnostic-variants-are-additive-substrate` | substrate. `DiagnosticCode` is a plain enum with rustdoc-documented `W_`/`E_` mnemonics; adding variants is additive. This leaf mints all three the PRD fixes normatively — `ShiftSkippedModes` (W), `ShiftAtEigenvalue` (E), `FirstModeNotInShiftedResult` (E) — so sibling wiring leaves γ and δ do not both edit `diagnostics.rs` and serialize against each other. None exists today (measured count 0). | PASS | grep `FirstModeNotInShiftedResult` present in `diagnostics.rs` |
| `boundary-tests-are-gate-resident-and-need-same-diff-registration` | substrate. `reify-solver-elastic` binaries are gate-resident **by default** — `REIFY_HEAVY_NEXTEST_FILTER` excludes only `determinism`, `analytical_validation`, `modal_benchmarks`. A new boundary-test binary joins the merge gate immediately under `.config/nextest.toml`'s 1200 s default. Registration is a **same-diff** obligation (esc-4914-162 failure shape), never a prose-ordered sibling. | PASS | manual — conditional on whether the tests exceed the default ceiling; the merge gate reds on an unregistered slow binary. |

## β — Lanczos/LU shifted factorization

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `sparse-lu-with-partial-pivoting-exists-and-is-a-stiffnessop-dropin` | **CORRECTION to the chartering record, which proposed LDL^T.** faer 0.24 ships **no** sparse LDL^T solver — `faer::sparse::solvers` declares exactly `Llt`, `Qr`, `Lu`. `sp_lu()` ("with partial (row) pivoting") exists on both `SparseColMat` and `SparseRowMat`; `impl SolveCore<T> for Lu` and `impl ShapeCore for Lu` both exist, so `Lu` drops in behind `StiffnessOp::solve_in_place` exactly as `Llt` does. LDL^T is reachable only via the low-level **unpivoted** `factorize_{simplicial,supernodal}_numeric_ldlt`, which breaks down on indefinite matrices without regularization. | PASS | grep `sp_lu` present in `eigensolve.rs` |
| `luerror-arity-matches-the-existing-none-vs-panic-discipline` | substrate. `LuError` carries exactly `SymbolicSingular{index}` and `Generic(FaerError)`, mirroring the `SparseLltError` Numeric/Generic split `try_solve_eigen_shift_invert` already discriminates on. The existing contract ("`None` means exactly one thing; a resource failure panics") extends without a new error vocabulary. | PASS | manual — upstream structural fact, already true. Behavioural cover: BT5. |
| `sigma-zero-preserves-the-landed-goldens-structurally` | G6, inverted (asserts **no** movement). §5.1's `sigma == 0.0` special case makes this code-path identity, not a tolerance. Four landed suites pin numbers here: `euler_column_pin_pin.rs` (4 BC families, bounds 0.09–0.11), `buckling_smoke.rs`, `buckling_persistent_cache_round_trip.rs`, and the modal goldens. Consistent with the reify byte-identity ruling: byte-identity is wrong for *iterative* claims, right for *same-calls-same-order* — the identical argument the in-file `SparseStiffnessOp` comment already makes. | PASS | manual — proven by those suites staying green, not by a grep. Bound to BT1. |

## γ — buckling kernel + trampoline wiring

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `bucklingkerneloptions-has-no-sigma-and-both-call-sites-hardcode-zero` | G6 premise, re-measured. `BucklingKernelOptions` declares exactly five fields and no shift input; both `solve_buckling_kernel` arms (P1, P2) construct `EigenSolverOptions` with a literal `sigma: 0.0`. This leaf **adds** an input rather than threading one. | PASS | grep `pub sigma` present in `buckling_kernel.rs` |
| `shiftskippedmodes-warning-reaches-the-production-trampoline` | wired-on-main entry path. `solve_buckling_trampoline` is registered as `"solver::buckling"` in the `compute_targets` dispatch table and already assembles a `Vec<Diagnostic>` on the production path. `DiagnosticCode` is a plain additive enum. | PASS | grep `ShiftSkippedModes` present in `compute_targets/buckling.rs` |
| `only-the-sigma-arm-retires-mode-and-auto_dense-belong-to-7179` | G4 seam made mechanical. Three arms share one `unsupported_diag` template; only the `sigma` arm retires. Scoped to that arm's own `kernel_note` string, **not** a bare `sigma` token, per the overlay's `expect: absent` scoping rule — a bare token would match the P1/P2 stress variables in the same file. | PASS | grep `has no shift-origin input yet` **absent** in `compute_targets/buckling.rs` |
| `g2-signal-is-a-mode-set-difference-not-a-silenced-warning` | G2, explicitly guarded. A committed fixture **pair** differing only in the σ value, compared through `reify eval` — never "the warning stopped firing". Vehicle is `eval`, not `check` (check exits 0 on Indeterminate; the check-time engine carries no kernel). Each fixture states its solver path: below `n = max(64, 2·n_modes)` only the dense path is reached. | PASS | manual — a difference between two files is not a single-file grep. Mechanical half is the two checks above. |

## δ — modal trampoline wiring

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `modal-already-plumbs-the-shift-to-the-dead-field` | **CORRECTION to the chartering record's symmetric framing.** `extract_eigen_knobs` returns `(n_modes, tol, max_iters, sigma)` and its caller writes `sigma` straight into the `EigenSolverOptions` literal at the `free_vibration` site. Modal's wiring is already complete up to the dead field, so this leaf's work is the λ-space conversion plus diagnostics — materially less than buckling's. | PASS | grep `ShiftSkippedModes` present in `modal_ops.rs` |
| `independence-from-6097-is-real-not-asserted` | G4 seam; no edge in either direction (Leo, 2026-09-01). Surface-agnostic read: `shift_frequency : Frequency` → λ = (2πf)² via #6097's inverse of `eigenvalue_to_frequency_hz`; `sigma : Real` → λ directly. α and β never learn which surface fed them. The leaf must **check** #6097's landed state, not assume it. | PASS | manual — the branch depends on main's state at implementation time. The invariant true in both branches is the check above. |
| `modal-has-no-unsupported-option-warning-today` | G3 substrate + scope correction. `reify-eval-fea-tests/tests/` contains `buckling_option_unsupported.rs` and **no** modal equivalent, so the warning this leaf retires exists only if #6097 landed first. If not, the retirement is a no-op — the leaf must not manufacture a warning in order to retire it. | PASS | manual — conditional on #6097; an unconditional `expect: absent` would pass vacuously. |

## ε — result provenance + helper refusal

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `three-helpers-read-modes-zero-blind-and-fail-unconservatively` | G6 premise, measured. `critical_load` = `modes[0].eigenvalue * reference_load`; `safety_factor_buckling` = `modes[0].eigenvalue`; `first_frequency` = `modes[0].frequency` under a comment asserting "`modes[0]` is the fundamental mode". None sees σ. At σ≠0 each reports a mid-spectrum value as the first mode — **higher** than truth, the unconservative direction. A correctness defect, hence Error not Warning. | PASS | manual — the variant is minted by α; a grep here would report α's landing, not this leaf's. The trampoline check below is this leaf's signal. |
| `optimized-trampoline-conversion-is-existing-substrate` | substrate, wired-on-main. A pure `.ri` body cannot raise a diagnostic (constraints are scalar predicates over a struct's own cells), so refusal must be Rust-side. `@optimized` has 55 stdlib uses including `solve_buckling` and `modal_analysis` in the very files edited. `Value::Undef` was considered and **rejected** — it is the silent-failure sentinel INV-SF-1 exists to eradicate. | PASS | grep `solver::critical_load` present in `solver_buckling_fns.ri` |
| `both-result-structs-are-frozen-and-the-assertions-move-with-them` | G3 substrate + blast radius. `BucklingResult` asserted at **exactly 4** param cells (`buckling_stdlib_compile.rs`); `ModalResult` at **exactly 6** (`modal_options_validation_tests.rs`). Both assertions move **in this diff**. Named so it is not discovered at merge. | PASS | manual — the assertion is a numeric literal inside a test message; grepping it would pin the fix's shape, not the capability. The compiler suite is the cover. |

## ζ — PDROP C1 flip + cite retirement

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `the-c1-declaration-surface-is-owned-elsewhere-so-check-do-not-assume` | G4 seam. The C1 declaration mechanism is #7079's, the buckling declaration #7081's, the detector #7085's — none guaranteed landed. This leaf flips whatever exists and otherwise records the honored state for the eventual declaration; it must not invent the mechanism. **No fourth C1 disposition**: Leo's 2026-09-01 ruling forbids a ratified set, and `not_applicable` is reserved for semantically meaningless params, which a working shift origin is not. | PASS | manual — the declaration syntax belongs to #7079 and is not fixed at this PRD's authoring time. |
| `the-7178-cite-retirement-is-not-vacuously-satisfiable` | PTODO/PDROP contract. `#7178` does **not** appear in tracked source today — #7081 and #6097 add the cites when they land. An unconditional `expect: absent` would pass vacuously right now, which is the exact vacuity class this PRD closes. | PASS | manual — real cover is the PTODO fingerprint ratchet (`tests/infra/test_reify_audit_ptodo.sh`), which reds on a cite to a terminal task. |

## η — exemplar corpus + discoverability

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `fea-has-zero-doc-chunk-presence-so-the-chunk-leaf-is-waived` | **Docs-truth waiver evidence, measured.** No file under `crates/reify-mcp/src/tools/chunks/` mentions buckling, modal, eigen, FEA or any `solve_*` function — the 17 chunks cover collections, connect, constraints, enums, fields, functions, geometry, guards, occurrences, parameters, purposes, stdlib, structures, syntax, traits, types, units. A chunk edit here would be a one-paragraph orphan in a topic that does not exist. The gate's other three obligations **are** delivered; the chunk obligation is waived with this rationale plus a filed follow-up for FEA chunk coverage as a whole. | PASS | grep `spectral_shift` present in `examples/best_practices/INDEX.md` |
| `examples-corpus-is-compile-gated-so-the-exemplar-cannot-rot` | substrate, wired-on-main. `examples/best_practices/` is auto-compile-gated by `crates/reify-compiler/tests/examples_smoke.rs`. Expected cost, not a defect: `examples/*.ri` has no carve-out arm in `verify.sh`'s `decide_scope`, so this leaf's diff runs the full workspace gate. | PASS | manual — an existing harness property; a check on it would be vacuous. |

## θ — PRD close

| Capability | Binding | Verdict | Check |
|---|---|---|---|
| `terminal-vocabulary-is-closed-and-this-leaf-is-the-only-stamper` | Overlay "PRD terminal status" — terminal vocabulary is exactly `{SHIPPED, SUPERSEDED, WITHDRAWN}`, first token after the `Status` label, matched case-insensitively. Without a decompose-filed close leaf nothing ever stamps one, which is why only 5 of ~350 tracked PRDs ever reached a terminal status, each by retroactive hand-fix. Depends by real edges on every sibling; a `cancelled` sibling counts as satisfied. | PASS | grep `Status:** **SHIPPED` present in the PRD `.md` |
