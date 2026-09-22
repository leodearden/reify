# Capability manifest — `trampoline-param-drop-closure`

Mechanizes G3 + G6 (and records the G7 walk) per leaf, per `.claude/skills/prd/project.md`.
PRD: `docs/prds/v0_6/trampoline-param-drop-closure.md`, committed on `main` as `fd341ea8b0`.

**All evidence gathered and re-verified 2026-08-31 against `main` at `fd341ea8b0`.** The PRD's own
code anchors are dated against `a31dc6a055`; per the shared cite-by-symbol rule a dated snapshot's
anchors are provenance and were not re-anchored. Everything below cites by symbol and file, never by
`path:line`.

This file is the human-readable twin of
`docs/prds/v0_6/trampoline-param-drop-closure.capability-manifest.yaml`, which carries the same
bindings plus their `delivered_check` descriptors. **Keep the two in sync by hand** — `commit_planning`
stamps them together once at landing, and a later correction to a check is three writes: the `.yaml`
descriptor, this `.md` row, and the producer task's complete `metadata.delivered_checks` list.

## Verdict

**The D3 substrate-verification workflow BLOCKED this batch, and the block was real.** Sixteen agents
(Enumerator → Prover ‖ Adversary → Synthesize) ran over the four honor leaves β, γ, ε, ζ. Two leaves
came back with genuine falsifications — not harness noise — and both were re-confirmed against source
before being acted on. **ζ's PRD disposition was falsified outright** and the leaf was re-scoped from an
*honor* leaf to a *not_applicable* leaf; **ε's boundary row B9 was shown to be unobservable as written**
and its signal was restated. Both tasks were rewritten before the batch was flipped. Eleven bindings now
carry a **CORRECTION APPLIED**, **DECOMPOSE-TIME FINDING** or **DECOMPOSE-TIME FALSIFICATION** marker.

The workflow's remaining `UNPROVABLE` / `FAIL` rows were inspected individually and are harness
artifacts, not premise failures: the β rows record probe fixtures the Prover wrote, ran and reported
measurements from, which the harness then re-ran against paths that no longer existed (`No such file or
directory`); the γ rows carry affirmative text confirming every premise. Where a claim was decisive it
was verified directly against source rather than taken on the agent's word — `solve_eigen_dense`'s
`opts.` reads, `mechanism_modal_analysis`'s declaration site, `EigenSolverOptions::default()`, and the
`force_peak` fill loop were all read by hand.

Every mechanical `delivered_check` in the sidecar was **executed against `main` at decompose time and
confirmed RED** — an `expect: present` check finds nothing today, an `expect: absent` check finds its
target today. A check that is already green is a bug in the check, not evidence of delivery; two were
caught that way during authoring and rewritten (an em-dash mismatch that made γ's comment check
unmatchable, and a λ status-token check that matched the PRD's own §10 prose). A third was withdrawn
after ζ was re-scoped: it asserted the `EigenSolverOptions{n_modes: padded_size, ..Default::default()}`
construction would *disappear*, which ζ no longer changes — left in place it could never have gone
green and would have blocked dependent leaf κ forever with a false `DEP_CAPABILITY_NOT_DELIVERED`.

## Leaf → task map

| Label | Task | Title |
|---|---|---|
| α | #7079 | E_PARAM_NOT_HONORED / W_PARAM_NOT_APPLICABLE DiagnosticCode variants + the honored/ignored/not_applicable declaration mechanism (INV-PD-1 C1-C3) |
| β | #7080 | vertical slice - ElasticOptions fully declared; max_iter and cg_tolerance honored in elastic_static |
| γ | #7081 | buckling honors supports, PointLoad.point/.direction, PressureLoad/Gravity |
| δ | #7082 | LoadCase.options on the buckling_multi_case path - honor it or make the drop loud; never silent |
| ε | #7083 | honor TOTSShaper/RevoluteTOTSShaper actuator_limits; delete the force_limit phantom read; decide the keying/arity contract |
| ζ | #7084 | mechanism_modal - declare tol/max_iters/element_order not_applicable (the dense direct eigensolve has no iterative budget) |
| η | #7085 | reify-audit --pattern PDROP + allowlist baseline + owning-task liveness lane |
| η′ | #7086 | PDROP gate-test drift-guard registration |
| θ | #7087 | RULING - wire or delete target_fidelity on FDMSliceOptions and AsPrintedOptions |
| κ | #7088 | docs-truth - solver-option doc chunk, best-practices exemplar, cheatsheet index, discoverability |
| λ | #7089 | PRD close - task-id backfill verification, terminal stamp, AS-AUTHORED freeze header |

Leaf **ι** carries no task: it was **discharged at decompose time**, which is what decision D4
(“every allowlist entry has a live owner, filed by this PRD's own decomposition”) asks for. The five
tasks it filed are listed under *The ι discharge* below.

## Decompose-time findings

These are the eleven places the PRD's text and `main` disagreed. Each is binding on the leaf named.
The first two are **falsifications** — a disposition the PRD states that the code cannot deliver.

1. **ζ — `tol`/`max_iters` are NOT honorable; PRD §7 row A9 is falsified (G6 branch 3).** The most
   consequential finding here. `run_mechanism_modal` calls `solve_eigen_dense` *unconditionally* — no
   size or conditioning branch, in deliberate contrast to the FEA modal path in the same file, which
   does branch dense-vs-shift-invert. `solve_eigen_dense` reads **only** `opts.n_modes`, never
   `opts.tol` or `opts.max_iters`, and hardcodes `n_converged: 0` with the documented reason that the
   direct path has no iterative budget to report. C1 defines *honored* as "reaches the kernel **and
   changes the result**"; an iterative budget on a direct solver cannot change the result at any value.
   ζ is re-scoped to a `not_applicable` leaf — it still delivers the PRD's §1 guarantee, via the
   `W_PARAM_NOT_APPLICABLE` arm. Declaring them honored would have been a false-green of exactly the
   class C1 invariant 1 exists to prevent.
2. **ε — B9 is unobservable as written; the signal is restated (G6 branch 3).** Joints with index ≥ 1
   have `force_peak == 0` structurally, so the per-joint *difference* B9 asserts cannot appear: the SQP
   constraint vector is per joint, and a limit on a joint whose peak force is zero is a permanently
   inactive constraint. Restated against joint 0, where the constraint is active. If the j ≥ 1 inertness
   holds at implementation time it is a second defect of this PRD's own class, and ε files it as its own
   live owner rather than papering over it. ε also gains scope: the `actuator_limits` keying rule and
   arity contract are undecided today and a mismatched, out-of-range list is silently accepted.
3. **δ — `LoadCase.options` cannot be “honored” on the buckling path (G6 branch 3).** PRD §7
   dispositions A4 as *honor — `multi_case.rs` already does it*. But `buckling_multi_case.rs` drops it
   under an explicit design decision DD-4: `LoadCase.options` is typed `Option<ElasticOptions>`, the
   wrong knob type for buckling. δ's deliverable is restated to the PRD's own §1 guarantee — the drop
   must stop being **silent** — with `not_applicable` + the DD-4 reason as the recommended
   classification.
4. **β — `reify check` cannot deliver B2/B3 today (G6 branch 3).** `cmd_check` decides its exit from
   constraint outcomes plus two per-code bolt-ons, so an Error-severity diagnostic raised elsewhere
   prints `error:` and still exits 0 (INV-SF-2's own recorded evidence). The general gate is #5403
   (in-progress); routing check through build so it *sees* trampoline diagnostics is #5748. B2/B3 are
   restated against `reify eval`, which already gates on `Severity::Error` (task 4458). No edge wired,
   and C3 means no existing `.ri` file trips the new Error, so #5403 landing later is not a regression.
5. **α — the `E_*`/`W_*` mnemonics are not renderable identifiers (G6 branch 3).** `DiagnosticCode`
   has no `Display`, no `as_str`, no `FromStr` and no exhaustiveness test. A “the codes round-trip”
   signal would be a false premise. The machine identity is the serde PascalCase variant name; the
   mnemonic reaches a user only if embedded in the message text, per the `E_DUP_MEMBER_KEY:` precedent.
6. **η — the liveness lane's mechanism is PTODO's, not `fused_memory_client`.** PRD §3 names the HTTP
   client *and* “the same mechanism PTODO's liveness lane β uses”. Those are two different mechanisms
   and the second clause is the operative one: PTODO reads the task store directly, read-only, via
   rusqlite. PDROP copies that, reusing `is_terminal_status` verbatim.
7. **η — `--format markdown` is not a `reify-audit` flag.** It is an argument of the `/audit` *skill*.
   The PRD's η row says `/audit --pattern PDROP`, which is correct as written; this is recorded so no
   implementer binds evidence to a binary flag that does not exist.
8. **η — enumeration completeness is the landing risk (C4a).** The repo declares ~16 `@optimized`
   trampoline targets; leaves α–ζ declare five of them. η must choose between completing the remaining
   eleven declarations or shipping a ratcheting not-yet-declared list, or it lands RED on main on day one.
9. **γ — the port source is modal, not elastic; and the comment defect is doubled.**
   `target_node_set` resolves geometry handles over a *realized* boundary association, which the
   buckling trampoline (a synthetic in-trampoline grid) does not have; modal's
   `build_dirichlet_bcs`/`support_targets` is the correct source for both A1 and A2.
   `examples/buckling_column_p2.ri` carries the same pin-pin comment contradiction as the smoke file,
   so γ's scope covers both.
10. **ζ — `mechanism_modal(...)` is not a callable, and the omega fallback hides that.** The stdlib
    function is `mechanism_modal_analysis`; `modal::mechanism_modal` is only the `@optimized`
    compute-target id, and the PRD's B4 row uses the wrong one. An unknown function name is *silently
    accepted*: `reify check` exits 0 with "All constraints satisfied." and zero diagnostics, `reify eval`
    exits 0 binding `undef` with only a `note:`. This is **cross-cutting** — every leaf's fixture must
    assert its result is not `undef`, or a name typo reads as green.
11. **ζ — two-default divergence (C3).** `EigenSolverOptions::default()` is `tol` 1e-8 / `max_iters`
    1000, which is what the PRD's §2.1 A9 row cites — but the *declared* `ModalOptions` defaults are
    1e-9 and 200. C3 compares against the **structure_def's** declared default, so using the library
    values would make the diagnostic fire on the default and stay silent on a deliberate change.

## The G6 note on B8, mechanized

The PRD carries an explicit G6 note forbidding a textbook buckling constant, and this manifest is its
enforcement. `crates/reify-solver-elastic/tests/euler_column_pin_pin.rs` is the calibration reference
and already carries four landed BC families with their own analytic references and tolerances: pin-pin
(k=1, bound < 0.10, observed 9.21%), fixed-free (k=2, bound < 0.11, observed 10.02%), fixed-pin
(k ≈ 0.6992, bound < 0.10, observed 8.82%) and fixed-guided (k=0.5, bound < 0.09), all at nx=ny=8,
nz=160. The pin-pin reference sits 0.79 percentage points under its own bound. P1-tet bending lock
floors slender-column accuracy at ~6.8–10% regardless of mesh density, and pointwise Dirichlet BCs
realize an effective fixed-pin k ≈ 0.67–0.70 rather than the textbook 0.5. **B8's assertion is
therefore relational** — a ratio against the current pin-pin number at the same mesh, in the predicted
direction and rough magnitude — and never agreement with k=2. This is the `esc-3453-5/6` class; the
bound-below-floor half of that incident is what the numeric-floor rule exists to prevent.

## The G7 walk

Walked against `docs/legibility/design-invariants.md` (INV-SF-1..7, INV-AD-1..4), every task in the
batch, not only leaves.

- **INV-SF-3 `declared-intent-consumed-or-diagnosed`** — this PRD *is* an instance of enforcing SF-3
  at the trampoline seam; contract C1's three-set equality is the mechanism and PDROP is the gate.
- **INV-SF-5 `placeholders-owned-and-loud`** — the allowlist entries are placeholders and contract C4c
  makes a dead owner a gate failure. This is why the ι discharge filed live owners rather than leaving
  `#2911`/`#2998`/`#4149` cited while terminal.
- **INV-SF-6 `diagnostics-carry-codes`** — both new diagnostics carry `DiagnosticCode` variants by
  construction. γ additionally codes the code-less `#4245` in-plane-force warning it ports.
- **INV-SF-2 `error-severity-exits-nonzero`** — finding 2 above. Not a violation introduced here; the
  signal was restated onto the surface that already honours the invariant.
- **INV-AD-4 `boundaries-declare-angle-convention`** — **one obligation, not waived.** ε marshals
  `RevoluteTOTSShaper`'s angular limits (`Scalar<AngularVelocity>`, `Rate<AngularVelocity>`,
  `List<RevoluteJointLimit>`) across the `.ri`→Rust f64 boundary, where the typed distinction does not
  reach. ε adds the greppable convention-declaring comment beside the marshalling site.
- **INV-SF-1, INV-SF-4, INV-SF-7, INV-AD-1..3** — no hit. No leaf introduces a root undef, a
  permanently-indeterminate outcome, new grammar, or an Angle-typed quotient.

**No G7 waivers were recorded.** No task in the batch carries `metadata.g7_waivers`.

## The ι discharge

Decision D4 requires every allowlist entry to name a **live** owner, and contract C4c makes a dead
owner a gate failure — so an entry citing a `done` task reds the gate this PRD builds. Three of the
PRD's named entries had terminal owners, and a fourth gap was found at decompose:

| Allowlist entry | Was owned by | Now owned by | Kind |
|---|---|---|---|
| `ElasticOptions.mesh_size` | **#2911** (`done`) | **#7074** | code — plumb the declared override to the mesher. Zero reads exist anywhere in the workspace today. |
| `ElasticOptions.element_order`, elastic path | **#2911** (`done`) | **#7075** | code — buckling and modal already honor it (#4052 / #4066); only `elastic_static` does not. |
| `target_quantity_of_interest` | **#2998** (`done`) | **#7076** | ruling — #2998 *deliberately* ratified it as an accepted-but-ignored DWR forward hook, so the question is honor / ratify permanently / delete, not “fix a bug”. |
| `BucklingOptions.mode` / `.sigma` / `.auto_dense` | **#4149** (`done`) | **#7077** | ruling — **not in the PRD's §7 table.** Contract C1's union-equality forces these three into the buckling declaration, and C4c fails them on their dead owner. Found at decompose. |
| `TractionLoad` / `BodyForce`, elastic + buckling | — (split, unowned) | **#7078** | operator — author the split PRD `traction-and-body-force-loads`. |

`force_tet` / `require_hex_wedge` already have a live owner in **#4746** (pending; note it sits behind
a phantom-done chain via #2987, so it is live but not close to landing — allowlisted, not adopted), and
`ModalOptions.sigma` → `shift_frequency` in **#6097** (pending). Neither needed a new task.

**No dependency edge is wired from any leaf onto any owner task, deliberately.** The gate requires each
owner to be *live*; an owner reaching `done` is precisely what reds C4c. An edge would invert the
contract.

## Per-leaf bindings

### α — task #7079

*trampoline param-drop α: E_PARAM_NOT_HONORED / W_PARAM_NOT_APPLICABLE DiagnosticCode variants + the honored/ignored/not_applicable declaration mechanism (INV-PD-1 C1-C3)*

- **diagnosticcode-additive-variant-substrate** — capability->producer (substrate) - DiagnosticCode in crates/reify-core/src/diagnostics.rs is #[non_exhaustive], carries 191 variants, and has no exhaustive match-on-self anywhere in the workspace. Verified 2026-08-31. Minting two variants is additive and non-breaking. **PASS**. — `delivered_check`: grep `/ParamNotHonored/` **present** in `crates/reify-core/src/diagnostics.rs` (executed at decompose: RED today).

- **severity-carrier-substrate** — capability->producer (substrate) - Diagnostic::error / Diagnostic::warning fix Severity and the fluent .with_code(DiagnosticCode) attaches the code; verified 2026-08-31. So E_PARAM_NOT_HONORED = Diagnostic::error(..).with_code(..) and W_PARAM_NOT_APPLICABLE = Diagnostic::warning(..).with_code(..). No new type is required. **PASS**. — `delivered_check`: grep `/ParamNotApplicable/` **present** in `crates/reify-core/src/diagnostics.rs` (executed at decompose: RED today).

- **mnemonic-is-prose-not-a-renderable-identifier** — CORRECTION APPLIED (G6 branch 3, end-to-end capability). DiagnosticCode has NO Display, NO as_str, NO code_str, NO FromStr and no exhaustiveness test - verified 2026-08-31. A signal asserting the two codes 'round-trip' as the strings E_PARAM_NOT_HONORED / W_PARAM_NOT_APPLICABLE would therefore be a FALSE PREMISE. The only machine identity is the serde PascalCase variant name; the E_/W_ mnemonic reaches a user only if embedded in the message text, the house precedent being the 'E_DUP_MEMBER_KEY: ' message prefix. Alpha's signal is restated accordingly: the variants exist, serialize to their PascalCase names, and the mnemonic appears in the emitted message. **PASS**. — `delivered_check`: **manual** (negative structural fact (absence of Display/FromStr on DiagnosticCode); an expect:absent on a bare token would violate the reify overlay's scoping rule. The positive half is covered by the two variant checks above.).

- **house-precedent-shape-wired** — capability->producer (substrate, wired-on-main) - buckling_unsupported_option_diagnostics in crates/reify-eval/src/compute_targets/buckling.rs is the workspace's only existing diagnostic of this kind. It is called on the production path from solve_buckling_trampoline's final step and attaches DiagnosticCode::BucklingOptionUnsupported - not test-only. Its central unsupported_diag template plus its PRESENT-and-non-default firing rule is exactly contract C2 + C3, so alpha ports a shape rather than inventing one. **PASS**. — `delivered_check`: **manual** (substrate fact, already true on main; asserting it as a delivered_check would be vacuous. The behavioural twin is the two DiagnosticCode variant checks above.).

- **default-comparison-rule-is-non-breaking** — C3 (default-comparison) - every consumed structure_def declares its defaults inline, so C3 has a real source. Measured 2026-08-31 across the tracked .ri corpus: every BucklingOptions and ModalOptions call site spells its params AT their declared defaults (every sigma is 0.0), so the uniform-Error rule of D2 turns NO existing example red. That is what makes 'Error, not Warning' landable without a migration wave. **PASS**. — `delivered_check`: **manual** (corpus-wide property measured at decompose time; the durable enforcement is leaf eta's PDROP gate, not a pattern.).

- **positional-vs-by-name-ctor-binding-is-contested-prose** — hazard recorded (alpha edits option structure_defs). examples/buckling_column_p2.ri carries a long header asserting BucklingOptions arguments bind POSITIONALLY in declaration order regardless of name: labels - which would make adding or reordering a param silently re-bind every existing call site. Task #6484 (pending) exists precisely because that claim is FALSE: task 4522 landed a by-name binder, re-probed 2026-08-31. Alpha must not add params on the strength of the stale comment, and must not rely on positional order either; #6484 is retiring the comments. File contention only, no dependency wired. **PASS**. — `delivered_check`: **manual** (cross-task prose-correctness record; #6484 owns the comment retirement.).

- **pdiag-is-not-on-main** — cross-PRD reality check (INV-SF-6). docs/prds/v0_6/eradicate-silent-undef.md owns INV-SF-6 and its PDIAG enforcement, but PDIAG is NOT on main as of 2026-08-31: crates/reify-audit/src/pdiag.rs, crates/reify-audit/pdiag-baseline.txt and docs/notes/diagnostic-severity-policy.md are all absent, and the work lives unlanded on branch task/5405 (#5405, in-progress). Alpha therefore owns COMPLIANCE - both new diagnostics carry codes by construction - and must NOT bind evidence to 'PDIAG is green'. No dependency edge is wired from alpha: compliance does not need the enforcer to exist. Leaf eta DOES carry a real edge to #5405, for a different reason (shared registration surfaces). **PASS**. — `delivered_check`: **manual** (cross-PRD posture record; the mechanical half is the coded-diagnostic requirement already asserted by the two variant checks above.).


### β — task #7080

*trampoline param-drop β: vertical slice - ElasticOptions fully declared; max_iter and cg_tolerance honored in elastic_static*

- **max-iter-and-cg-tolerance-both-dropped** — capability->producer (substrate, the defect being fixed) - crates/reify-eval/src/compute_targets/elastic_static.rs builds its CgSolverOptions with a hardcoded tolerance of 1e-6 and max_iter set to the pub(crate) const SOLVER_MAX_ITER = 2000. Verified 2026-08-31. The drop is TWO-wide: max_iter (declared default 1000, so the hardcode also contradicts the declaration) and cg_tolerance (declared default 0.000001, numerically equal to the hardcode today, so only a user-supplied DIFFERENT value is lost). **PASS**. — `delivered_check`: grep `/max_iter: SOLVER_MAX_ITER/` **absent** in `crates/reify-eval/src/compute_targets/elastic_static.rs` (executed at decompose: RED today).

- **c1-union-equals-elasticoptions-param-set** — C1 invariant 1 (the union must EQUAL the param set, not be a subset) - RESOLVED AT DECOMPOSE so beta does not discover it at dispatch. ElasticOptions declares 17 params. Measured 2026-08-31 by grepping each field-name string across crates/reify-eval/src, crates/reify-expr/src and crates/reify-stdlib/src: HONORED TODAY (10) - threads, shell_threshold, shell_voxel_size, shell_branch_prune_ratio, shell_force, deterministic, adaptive, target_accuracy, max_refinement_iterations, max_dofs. HONORED BY BETA (2) - max_iter, cg_tolerance. IGNORED, each needing a live owner (5) - mesh_size (zero reads anywhere in the workspace), element_order on the elastic path (read only in buckling.rs and modal_ops.rs, never in elastic_static.rs), target_quantity_of_interest, force_tet, require_hex_wedge. 10 + 2 + 5 = 17, so the union closes with NO unowned residue. **PASS**. — `delivered_check`: **manual** (set-equality judgment over a structure_def param list; the durable mechanical twin is leaf eta's PDROP C4a check ('param in no set').).

- **ignored-entry-owners-are-live** — C4c / D4 (every allowlist entry names a live owner) - both historical owners are TERMINAL: #2911 (done) specified mesh_size, element_order, max_iter and cg_tolerance and shipped only the declarations; #2998 (done) deliberately ratified target_quantity_of_interest as accepted-but-ignored in v0.4. New live owners were filed by this decomposition (the iota discharge - see the .md manifest for the ids); force_tet and require_hex_wedge already have a live owner in #4746 (pending). DELIBERATELY NO dependency edge is wired from beta or eta onto any owner task: the gate requires each owner to be LIVE, and an owner reaching done is precisely what reds C4c - an edge would invert the contract. **PASS**. — `delivered_check`: **manual** (task-liveness property, enforced at run time by leaf eta's PDROP liveness lane (is_terminal_status against the task store), never by a static pattern.).

- **error-severity-exit-is-eval-and-build-today-not-check** — DECOMPOSE-TIME G6 FINDING (branch 3, end-to-end capability) - the PRD's boundary rows B2/B3 assert that a declared-ignored param at a non-default value makes `reify check` exit nonzero, and that the same param at its default keeps it at exit 0. Measured 2026-08-31, `reify check` cannot deliver that today: cmd_check's finish_check decides its exit from constraint outcomes alone plus two per-code bolt-ons (a GdtIllegalModifier code match and an E_DFM_ message-prefix match), so an Error-severity diagnostic raised anywhere else prints `error:` and still exits 0 - this is INV-SF-2's own recorded evidence. The general Severity::Error exit gate for cmd_check is task #5405's sibling #5403 (in-progress, high), and routing check through build so it SEES trampoline diagnostics is #5748. Separately, an Error-severity diagnostic returned inside ComputeOutcome::Completed does not by itself fail the solve. RESOLUTION, per G6 resolution (c) - change the asserted configuration so the claim becomes true: B2/B3's signal is restated against `reify eval` (equivalently `reify build`), which ALREADY gates exit on Severity::Error generally (task 4458, cited as INV-SF-2's house pattern) and which definitely runs the compute trampoline. `reify check` converges onto the same behaviour for free once #5403 and #5748 land. NO dependency edge is wired: the restated signal needs neither task, and C3's default-comparison rule means no existing .ri file trips the new Error, so #5403 landing later introduces no regression and needs no allowlist entry for these codes. **PASS**. — `delivered_check`: **manual** (cross-task capability-routing correction; the behavioural twin is the two DiagnosticCode variant checks under alpha, plus beta's own eval-level e2e.).

- **non-convergence-is-user-observable** — signal premise (G6 branch 3) - classify_convergence is called on the production path in elastic_static.rs with SOLVER_MAX_ITER, and ConvergenceStatus is a real result surface, so iteration-budget exhaustion is reportable rather than internal. RESIDUAL RECORDED rather than hidden: whether a CI-practical cantilever mesh actually needs MORE than 50 CG iterations was not measured at decompose time - and then it WAS. Direct measurement against target/release/reify during verification: a fea_cantilever_smoke.ri-shaped design with ElasticOptions(max_iter: 50) today returns converged: true, iterations: 719. So the mesh genuinely needs far more than 50 iterations, the solve runs straight past the requested budget, and B1's signal IS observable at 50. The same run also confirms the drop end-to-end from the user surface, not just by inspection. The assertion remains 'the solver used the user's budget', not the literal 50. **PASS**. — `delivered_check`: **manual** (an explicitly unmeasured observability premise with a stated adjustment rule; pinning a pattern now would assert the answer.).


### γ — task #7081

*trampoline param-drop γ: buckling honors supports, PointLoad.point/.direction, PressureLoad/Gravity*

- **supports-discard-is-real** — capability->producer (substrate, the defect) - crates/reify-eval/src/compute_targets/buckling.rs discards its supports slot with a bare `let _ = &value_inputs[5];` under a comment stating that BCs are hardcoded to pin-pin and that 'Support-driven BC selection is deferred (no live task)'. solve_buckling's signature doc pins slot [5] as `supports : List<Support>`, and crates/reify-compiler/stdlib/solver_buckling_fns.ri declares that parameter REQUIRED, with no default and no ignored-caveat in its doc block. Verified 2026-08-31. **PASS**. — `delivered_check`: grep `/Support-driven BC selection is deferred/` **absent** in `crates/reify-eval/src/compute_targets/buckling.rs` (executed at decompose: RED today).

- **bc-port-source-is-modal-not-elastic** — CORRECTION APPLIED (capability->producer, wrong-layer risk). PRD section 7 names modal's build_dirichlet_bcs / support_targets for finding A1 and elastic_static's target_node_set plus #4245's direction handling for A2. Verified 2026-08-31: all of them exist as production symbols - build_dirichlet_bcs, support_targets and simply_supported_pin_pin_bcs in crates/reify-eval/src/modal_ops.rs (with the unit test build_dirichlet_bcs_selects_pin_pin_vs_clamp), and loads_supports_to_bc_node_sets plus target_node_set in crates/reify-eval/src/compute_targets/elastic_static.rs. BUT they are structurally different designs: target_node_set resolves geometry handles over a REALIZED boundary association, while support_targets works from string face names over analytic beam coordinates. The buckling trampoline builds a SYNTHETIC grid in-trampoline and holds no BoundaryAssociation, so the MODAL mechanism is the correct port source for both halves, and A2's point/direction must be mapped onto the synthetic grid's named faces rather than routed through target_node_set. **PASS**. — `delivered_check`: **manual** (design-direction correction; the behavioural twin is the supports-discard check above and the relational buckling assertion below.).

- **fixed-free-is-reachable-in-the-kernel** — capability->producer (substrate) - the buckling kernel ALREADY supports non-pin-pin end conditions, so gamma plumbs supports through to a working capability rather than adding one. crates/reify-solver-elastic/tests/euler_column_pin_pin.rs carries four landed, calibrated BC families each with its own analytic reference: pin-pin (k=1, bound < 0.10, observed 9.21%), fixed-free (k=2, bound < 0.11, observed 10.02%), fixed-pin (k about 0.6992, bound < 0.10, observed 8.82%) and fixed-guided (k=0.5, bound < 0.09), all at nx=ny=8, nz=160. **PASS**. — `delivered_check`: **manual** (substrate fact, already true on main; asserting it would be vacuous. It is the calibration reference for the numeric-floor capability below.).

- **buckling-signal-is-relational-not-a-textbook-constant** — NUMERIC FLOOR (G6 branches 1+2 - the esc-3453-5/6 class). The PRD carries an explicit G6 note on B8 and this binding is its mechanization. Two MEASURED hazards forbid an absolute textbook-constant assertion. (1) P1-tet BENDING LOCK floors slender-column accuracy at roughly 6.8-10% regardless of mesh density - an exponential fit to the measured data yields a 6.8% asymptote (#4052/#4066). (2) Pointwise Dirichlet BCs realize an effective fixed-pin k of about 0.67-0.70, not the textbook fixed-fixed 0.5. The pin-pin reference itself sits at 9.21% against a 10% bound - 0.79 percentage points of headroom. B8's assertion is therefore RELATIONAL: with supports honored, the fixed-free result must move off the discarded-supports pin-pin number in the predicted direction (lower; the analytic ratio is 0.25) and rough magnitude, asserted as a RATIO against the current pin-pin number at the SAME mesh. Any absolute bound gamma chooses must be justified against the fixed-free 11%-bound / 10.02%-observed floor at the mesh actually used, and must NEVER assert agreement with k=2. Only element_order: ElementOrder.P2 reaches below 5%. **PASS**. — `delivered_check`: **manual** (the floor and the relational form are a design constraint on the test gamma writes; the pinned calibration numbers live in crates/reify-solver-elastic/tests/euler_column_pin_pin.rs and must be re-read, not copied, at implementation time.).

- **smoke-comment-contradiction-is-real-and-doubled** — capability->producer (substrate) - CONFIRMED and WIDENED 2026-08-31. examples/buckling_column_smoke.ri says 'Support: pin-pin - fixed supports at both ends (lateral restraint)' on the line above a single FixedSupport(target: "base") that becomes a 1-element LoadCase.supports list. The comment describes the BCs the trampoline hardcodes, not the ones the file declares - an inconsistency the discard currently hides. examples/buckling_column_p2.ri carries the BYTE-IDENTICAL contradiction. The PRD names only the smoke file, so gamma's scope is widened to both. **PASS**. — `delivered_check`: grep `/fixed supports at both ends/` **absent** in `examples/buckling_column_smoke.ri`, `examples/buckling_column_p2.ri` (executed at decompose: RED today).

- **smoke-fixture-is-numerically-embedded-twice** — blast radius (same-diff obligation) - examples/buckling_column_smoke.ri is include_str!-embedded by crates/reify-eval-fea-tests/tests/buckling_smoke.rs and by crates/reify-eval-fea-tests/tests/buckling_persistent_cache_round_trip.rs, both of which pin its numeric result, and it is pinned in the GUI grammar drift ledger. Honoring supports CHANGES the asserted critical load - fixed-free is about a quarter of pin-pin - so both embeds must be re-baselined in gamma's own diff. Separately, examples/*.ri has NO carve-out arm in verify.sh's decide_scope and falls to the conservative default, so gamma's diff runs the full workspace gate: expected, not a defect. **PASS**. — `delivered_check`: **manual** (blast-radius record; the two embeds are named so they are edited in the same diff rather than discovered as a post-merge red.).

- **pressureload-on-buckling-is-unmeasured** — OPEN PREMISE, carried deliberately (PRD section 12 open question 4). Whether buckling's geometric-stiffness formulation can consume a PressureLoad without new kernel work was never measured. The elastic side has the port source - #4264 bridged PressureLoad and #4439/#4440 bridged Gravity through extract_loads to apply_body_force - but buckling-side feasibility does not follow from that. Gamma MEASURES this first and, if it needs kernel work, moves PressureLoad to the split PRD and allowlists it against that PRD's owner rather than forcing it. Scoped as a decision inside gamma, not an assumed capability. **PASS**. — `delivered_check`: **manual** (an explicitly unmeasured premise with a stated resolution branch; binding a pattern now would assert the answer.).

- **malformed-ptodo-cites-in-the-touched-region** — blast radius (ratchet) - crates/reify-eval/src/compute_targets/buckling.rs's extract_total_load carries the deferral cite 'task theta/3457' and elastic_static.rs's #4245 in-plane-force warning cites 'task 4245'. Both are PROSE forms that resolve to PTODO malformed-cite, and both sit in gamma's edit region. Touching them surfaces a live fingerprint absent from crates/reify-audit/ptodo-baseline.txt, which is the ENFORCED severity-blind merge gate. Gamma fixes them to the #NNNN form in its own diff rather than re-baselining. The #4245 warning is additionally a code-less Diagnostic::warning - the exact prior art for W_PARAM_NOT_APPLICABLE - and should gain a code when ported. **PASS**. — `delivered_check`: grep `/task θ/3457/` **absent** in `crates/reify-eval/src/compute_targets/buckling.rs` (executed at decompose: RED today).


### δ — task #7082

*trampoline param-drop δ: LoadCase.options on the buckling_multi_case path - honor it or make the drop loud; never silent*

- **multi-case-honors-it-elastic-side** — capability->producer (substrate, the port source) - crates/reify-eval/src/compute_targets/multi_case.rs resolves a per-case override inside solve_multi_case_trampoline (Option(Some(x)) becomes the per-case value, anything else falls back to the shared default), with an interpreted twin resolve_load_case_options in crates/reify-expr/src/lib.rs. Confirmed wired-on-main 2026-08-31. **PASS**. — `delivered_check`: **manual** (substrate fact, already true on main; the deliverable check is the loudness capability below.).

- **buckling-multi-case-drop-is-a-TYPED-drop-not-an-oversight** — DECOMPOSE-TIME G6 FINDING (branch 3, end-to-end capability). PRD section 7 dispositions A4 as 'honor - multi_case.rs already does it'. Measured 2026-08-31, that premise does not survive contact: crates/reify-eval/src/compute_targets/buckling_multi_case.rs passes the shared BucklingOptions to every case under an explicit design decision DD-4, because LoadCase.options is typed Option<ElasticOptions> - the WRONG KNOB TYPE for buckling - and the module doc says so. So 'honor it' as literally written is unreachable without either a per-case BucklingOptions surface (new stdlib type work, out of proportion to this PRD) or an ElasticOptions-to-BucklingOptions mapping rule that does not exist. RESOLUTION, per G6 resolution (b) - weaken the assertion to what is achievable and record the stronger property: delta's deliverable becomes the PRD's own section 1 guarantee rather than section 7's verb - the drop must stop being SILENT. The recommended classification is not_applicable carrying the DD-4 type-mismatch reason, since a structurally inapplicable param is exactly contract C1's not_applicable category, yielding W_PARAM_NOT_APPLICABLE. If delta instead judges it ignored, delta files a live owner for the per-case-buckling-options work in its own diff. The signal is disjunctive - honored OR loud, never silent - which is the house shape already used by eradicate-silent-undef's leaves gamma and delta. **PASS**. — `delivered_check`: grep `/ParamNotApplicable|ParamNotHonored/` **present** in `crates/reify-eval/src/compute_targets/buckling_multi_case.rs` (executed at decompose: RED today).


### ε — task #7083

*trampoline param-drop ε: honor TOTSShaper/RevoluteTOTSShaper actuator_limits; delete the force_limit phantom read; decide the keying/arity contract*

- **force-limit-is-a-phantom-field** — capability->producer (substrate, the defect) - CONFIRMED ON BOTH HALVES 2026-08-31. crates/reify-stdlib/src/trajectory/trampoline.rs reads field_f64(shaper_data, "force_limit", 1000.0) inside input_shape_tots. A repo-wide `git grep -n force_limit -- '*.ri'` returns ZERO hits: force_limit is a field on NO .ri structure anywhere. The declared field is actuator_limits - List<JointLimit> on TOTSShaper and List<RevoluteJointLimit> on RevoluteTOTSShaper, both in crates/reify-compiler/stdlib/trajectory.ri - and the per-joint force field inside JointLimit is named max_force, not force_limit. So the 1000.0 default ALWAYS wins and every joint's limit is silently uniform. A comment in the same Rust file concedes the uniform-placeholder convention. **PASS**. — `delivered_check`: grep `/field_f64\(shaper_data, "force_limit"/` **absent** in `crates/reify-stdlib/src/trajectory/trampoline.rs` (executed at decompose: RED today).

- **actuator-limits-is-declared-and-actually-supplied** — capability->producer (substrate) - actuator_limits is declared on both shaper structures in crates/reify-compiler/stdlib/trajectory.ri and is actually supplied by callers: examples/trajectory/printer_print_envelope.ri passes actuator_limits: [JointLimit(joint: 0.0, max_force: 1000N)] and examples/trajectory/tots_optimal_ptp.ri passes a non-empty list. The data reaching the trampoline is real, not hypothetical - so honoring it changes an observable answer. **PASS**. — `delivered_check`: **manual** (substrate fact, already true on main; the deliverable check is the phantom-read deletion above.).

- **b9-as-written-is-not-observable-signal-restated** — DECOMPOSE-TIME FALSIFICATION (G6 branch 3). Boundary row B9 asserts that two joints at DIFFERENT actuator limits produce a per-joint-different shaped trajectory. Adversarial verification reports, and the code path corroborates, that joints with index >= 1 have force_peak == 0 STRUCTURALLY, so that difference cannot appear. Mechanism: in crates/reify-stdlib/src/trajectory/tots.rs the per-joint peak is filled from the flattened RNEA torque with a .take(n_joints) over mechanism.links built by build_rnea_links_for_sample (crates/reify-stdlib/src/trajectory/simulate.rs), and the trampoline's own comment concedes that value_to_mechanism_model uses a uniform PLACEHOLDER convention for inertia. The SQP constraint vector is per joint, so a limit on a joint whose force_peak is 0 is a permanently inactive constraint. RESOLUTION: the signal is restated against JOINT 0, where force_peak is non-zero and the constraint is active - a max_force well below the current uniform 1000 N default produces an observably longer optimized duration than the same design at a high limit, where today both runs are byte-identical. If the j >= 1 inertness is confirmed at implementation time it is a SECOND defect of this PRD's own class and epsilon files it as its own live owner rather than papering over it. **PASS**. — `delivered_check`: **manual** (the restated signal is a two-run eval comparison on joint 0; the j >= 1 inertness is an instruction to verify and file, not a capability this leaf delivers.).

- **keying-and-arity-contract-is-undecided-and-silently-accepted** — DECOMPOSE-TIME FINDING, SCOPE ADDED (INV-SF-3 declared-intent-consumed-or-diagnosed). JointLimit.joint is declared joint : Real - a placeholder-typed joint id per INV-SF-5 - and nothing today defines how a limit is KEYED to a joint, nor what happens when the list length does not match the joint count. Measured: a mismatched, out-of-range limit list is SILENTLY ACCEPTED. Honoring actuator_limits without settling this merely MOVES the silence: an author who mis-keys a limit, or supplies three limits for two joints, still gets a plausible-but-wrong trajectory. Epsilon must decide the contract - positional by index or keyed by the joint field, and what an arity mismatch or out-of-range id does - and make a violation loud in this PRD's own vocabulary. In scope: it is the PRD's own section 1 guarantee applied to the param this leaf honors. **PASS**. — `delivered_check`: **manual** (a design decision this leaf must make and enforce; the enforcing diagnostic's exact shape is chosen at implementation time.).

- **per-joint-difference-must-be-observable** — signal premise (G6 branch 3) - B9 asserts that two joints at DIFFERENT limits produce a per-joint-different shaped trajectory. The uniform max_force is threaded into profile_to_joint_waypoints, so the value does reach per-joint construction and a per-joint difference is structurally reachable. RESIDUAL RECORDED: whether the shaped-trajectory result surface exposes a per-joint difference through reify eval was not measured at decompose time. If the output collapses joints, epsilon restates the signal against whatever per-joint surface does exist - it must NOT fall back to a synthetic-input unit test, which G2 rejects as a leaf signal. **PASS**. — `delivered_check`: **manual** (an explicitly unmeasured observability premise with a stated fallback that stays inside the G2 signal vocabulary.).

- **angular-boundary-must-declare-its-convention** — G7 - INV-AD-4 `boundaries-declare-angle-convention`. RevoluteTOTSShaper's limits are angular: velocity_limit is Scalar<AngularVelocity>, acceleration_limit is Rate<AngularVelocity>, and actuator_limits is List<RevoluteJointLimit>. Honoring them per joint marshals angular values across the .ri-to-Rust f64 boundary in crates/reify-stdlib/src/trajectory/trampoline.rs, where the typed distinction does not reach. INV-AD-4 requires that boundary to NAME its convention (rad / deg / cycles) in a greppable contract comment - a silent rad=1 SI erasure is a defect even when the number it produces is correct. Epsilon adds the declaring comment beside the marshalling site rather than retyping anything; the house precedents are eigenvalue_to_frequency_hz and the spring_rate_for_lumped_dof refusal guard. NOT waived - the obligation is one comment and epsilon is the change that creates the boundary. **PASS**. — `delivered_check`: **manual** (a same-diff authoring obligation; the reviewer check is that the marshalling site names its angle convention.).

- **file-contention-with-6240** — file-lock contention, NOT a dependency - #6240 (pending) owns the wrong-kind joint-limit pairing diagnostic and declares crates/reify-compiler/stdlib/trajectory.ri plus two compiler tests. Its subject is ctor-time dimension-kind pairing; epsilon's is param-drop-to-kernel. The two are independent within the file. Recorded so the sequencing is visible; no add_dependency wired. **PASS**. — `delivered_check`: **manual** (scheduling and file-lock observation, not a delivered capability.).


### ζ — task #7084

*trampoline param-drop ζ: mechanism_modal - declare tol/max_iters/element_order not_applicable (the dense direct eigensolve has no iterative budget)*

- **tol-and-max-iters-are-NOT-HONORABLE-prd-disposition-falsified** — DECOMPOSE-TIME FALSIFICATION (G6 branch 3) - THE MOST CONSEQUENTIAL FINDING IN THIS MANIFEST. PRD section 7 row A9 dispositions mechanism_modal's tol/max_iters as HONOR. Adversarially verified and then independently re-confirmed against source 2026-08-31: that is not achievable. run_mechanism_modal (crates/reify-eval/src/modal_ops.rs) calls solve_eigen_dense UNCONDITIONALLY as its sole eigensolve - no size or conditioning branch, in deliberate contrast to the FEA modal path in the same file, which DOES branch dense-versus-shift-invert. And solve_eigen_dense (crates/reify-solver-elastic/src/eigensolve.rs) reads ONLY opts.n_modes: its sole opts. reads are the n_take truncation and the converged comparison. It never touches opts.tol or opts.max_iters, and hardcodes n_converged: 0 with the documented reason that the direct path has no iterative budget to report. Contract C1 defines honored as 'reaches the kernel AND CHANGES THE RESULT'; an iterative budget on a DIRECT solver cannot change the result at any value. RESOLUTION (G6 (b), weaken to what is achievable): zeta becomes a NOT_APPLICABLE leaf for tol and max_iters alongside element_order. It still delivers the PRD's section 1 guarantee, via the W_PARAM_NOT_APPLICABLE arm rather than the honor arm - the same correction shape as sibling delta. Declaring them honored would have been a false-green of exactly the class C1 invariant 1 exists to prevent. **PASS**. — `delivered_check`: **manual** (the deliverable is the not_applicable declaration and its diagnostic, asserted by the ParamNotApplicable check below; the falsified 'honor' half is deliberately NOT bound to a check, because zeta must NOT change the EigenSolverOptions construction.).

- **mechanism_modal-is-not-a-callable-false-green-vector** — DECOMPOSE-TIME FINDING (G6 branch 4, rejection/false-green). The identifier the PRD's boundary row B4 and the original leaf text both use, mechanism_modal(...), IS NOT A CALLABLE. The stdlib function is mechanism_modal_analysis (crates/reify-compiler/stdlib/modal_mechanism_fns.ri); modal::mechanism_modal is only the @optimized compute-target id. Verified 2026-08-31. This matters because reify's omega fallback SILENTLY ACCEPTS an unknown function name - measured: reify check exits 0 with 'All constraints satisfied.' and ZERO diagnostics, reify eval exits 0 with the binding undef and only a note: line. A fixture written against mechanism_modal(...) would pass while testing NOTHING. This is a CROSS-CUTTING hazard, not a zeta-only one: every leaf's fixture must assert its result is not undef so a name typo cannot read as green. **PASS**. — `delivered_check`: **manual** (a fixture-authoring rule; the mechanical twin would be a grep for the correct callable in a fixture whose filename is chosen at implementation time.).

- **two-default-divergence-c3-uses-the-declared-default** — DECOMPOSE-TIME CORRECTION (contract C3). EigenSolverOptions::default() is n_modes 10, tol 1e-8, max_iters 1000, and the PRD's section 2.1 row A9 cites those library values - but the DECLARED surface defaults in ModalOptions (crates/reify-compiler/stdlib/modal_analysis.ri) are tol = 0.000000001 (1e-9) and max_iters = 200. C3 fires on a value differing from the STRUCTURE_DEF's declared default, so the comparison must use 1e-9 and 200. Getting this backwards would make the diagnostic fire on the declared default and stay silent on a deliberate change - exactly inverted. **PASS**. — `delivered_check`: **manual** (a numeric-source correction to the C3 comparison; the values live in the .ri declaration and must be read there, not copied from here.).

- **b4-is-not-expressible-in-the-prd-gate-probe-harness** — DECOMPOSE-TIME FINDING (probe-vector limitation, measured). B4's observable - exit 0 with W_PARAM_NOT_APPLICABLE firing - cannot be expressed in the prd-gate probe harness as it stands. The ir probe kind maps exit 0 to ABSENT UNCONDITIONALLY, before its match clause is consulted, so a warning-at-exit-0 can never be observed PRESENT by an ir probe; and the check probe kind never runs the trampoline at all - a live reify check on a modal fixture prints 'no registered compute trampoline (falling back to body-inlining)' on stderr and exits 0. Zeta asserts its behaviour through reify eval plus a Rust integration test that inspects the emitted diagnostics directly, and does NOT attempt an ir probe row. Recorded because two probe bindings authored during verification were vacuous for exactly this reason. **PASS**. — `delivered_check`: **manual** (a harness limitation governing how zeta's test is written, not a capability of the code.).

- **modal-knob-drop-is-real** — capability->producer (substrate, the defect) - CONFIRMED 2026-08-31. run_mechanism_modal in crates/reify-eval/src/modal_ops.rs destructures extract_eigen_knobs as (requested_n_modes, _, _, _), discarding tol, max_iters and sigma, and then applies n_modes only as a post-hoc TRUNCATION rather than as a solve input. The same function builds EigenSolverOptions with n_modes: padded_size and ..Default::default(), and that Default is what silently replaces the user's values. The contrast case sits in the same file: run_modal_analysis destructures all four returns and passes them through. **PASS**. — `delivered_check`: **manual** (CORRECTED AT DECOMPOSE: this was originally an expect:absent check on the EigenSolverOptions{n_modes: padded_size, ..Default::default()} construction. That check asserts a change zeta no longer makes - the dense solver ignores tol/max_iters, so the construction stays as it is and the check could never go green, permanently blocking dependent leaf kappa with a false DEP_CAPABILITY_NOT_DELIVERED. Zeta's deliverable is the not_applicable declaration, asserted by the ParamNotApplicable check below.).

- **no-double-landing-of-6097** — DAG-direction plus scope disjointness (the brief's named hazard). #6097 (pending) renames ModalOptions.sigma to shift_frequency : Frequency = 0Hz, has the solver convert lambda = (2*pi*f)^2, and ALREADY carries the modal 'declared but not yet honored' warning as its own numbered item 4, explicitly mirroring the buckling guard. Read via get_task on 2026-08-31 and confirmed verbatim. Zeta's residual is therefore EXACTLY tol + max_iters + the element_order not_applicable declaration; sigma/shift_frequency is 6097's, not zeta's. A real add_dependency edge zeta -> #6097 is wired rather than a prose ordering, because 6097 also edits crates/reify-eval/src/modal_ops.rs - the contention is genuine as well as semantic. **PASS**. — `delivered_check`: **manual** (dependency-graph property, wired via add_dependency and checked by the scheduler.).

- **c1-union-equals-modaloptions-param-set** — C1 invariant 1 (union EQUALS the param set) - ModalOptions declares 8 params: n_modes, boundary_conditions, damping, sigma, tol, max_iters, reference_direction, element_order. On the mechanism_modal path: n_modes is honored as a truncation; damping has been honored since #6875 (done - mechanism_modal_analysis used to drop its RayleighDamping descriptor, and that is the precedent shape for this whole leaf); tol and max_iters are honored BY zeta; sigma is owned by #6097; and THREE params are not_applicable - element_order (meaningless in a lumped generalized-coordinate model, and documented NOWHERE today, which is the PRD's section 2.2 finding) plus boundary_conditions and reference_direction, both already documented as meaningless in the lumped model. The PRD names only element_order as the not_applicable member; zeta must classify all three or the union is a subset and C1 invariant 1 fails. **PASS**. — `delivered_check`: grep `/ParamNotApplicable/` **present** in `crates/reify-eval/src/modal_ops.rs` (executed at decompose: RED today).

- **user-tol-must-be-observable-on-the-lumped-path** — signal premise (G6 branch 3) - RESIDUAL RECORDED. The mechanism_modal path is a LUMPED generalized-coordinate model whose eigenproblem is small (n_dof equals the number of bodies) and is always solved by the dense path, so a different tol may not change the reported modes at all. Zeta must establish an observable difference before freezing the signal - an absurd tol that fails to converge, or the reported iteration/convergence surface - and if none exists on the dense path, restate the signal around the W_PARAM_NOT_APPLICABLE half rather than asserting an unobservable numeric change. Naming this at decompose is the cheapest place to catch it. **PASS**. — `delivered_check`: **manual** (an explicitly unmeasured observability premise with a stated fallback; binding a pattern now would assert the answer.).


### η — task #7085

*trampoline param-drop η: reify-audit --pattern PDROP + allowlist baseline + owning-task liveness lane*

- **pattern-extension-is-live-substrate** — capability->producer (substrate) - reify-audit carries a Pattern enum in crates/reify-audit/src/lib.rs with 12 variants plus a per-pattern module family (p1_producer_orphan.rs, p2_consumer_stub.rs, p5_phantom_done.rs, pdead_dead_code.rs, ptodo.rs, puntested.rs, player.rs, pdssentinel.rs, pdoccover.rs), so PDROP extends live substrate. NOTE: the enum's own doc comment makes the P<N><Name> shape NORMATIVE for downstream prefix routing, so the conforming variant name is PDrop. There is NO FromStr and NO clap - the --pattern token set is a hand-rolled matches! literal list in crates/reify-audit/src/bin/reify-audit.rs, which must gain the token, the error string, both usage strings, a run_pdrop dispatch predicate and a dispatch arm. Verified 2026-08-31. **PASS**. — `delivered_check`: grep `/PDrop/` **present** in `crates/reify-audit/src/lib.rs` (executed at decompose: RED today).

- **pattern-token-reaches-the-cli** — capability->producer (the CLI half of the same registration) - a Pattern variant alone does not make --pattern PDROP work: the binary validates tokens against a literal matches! set and rejects anything else. Both halves are required for the /audit skill to reach the detector. **PASS**. — `delivered_check`: grep `/PDROP/` **present** in `crates/reify-audit/src/bin/reify-audit.rs` (executed at decompose: RED today).

- **liveness-lane-precedent-is-ptodo-not-the-http-client** — CORRECTION APPLIED (capability->producer, wrong-mechanism risk). PRD section 3 says liveness 'is resolved through the existing fused_memory_client the audit crate already carries ... the same mechanism PTODO's liveness lane beta uses'. Measured 2026-08-31, those are two different mechanisms and the second clause is the operative one: PTODO's liveness lane does NOT use FusedMemoryClient. It opens the task store directly READ-ONLY via rusqlite (tasks_db_path, honoring the REIFY_PTODO_TASKS_DB override, and open_tasks_db with SQLITE_OPEN_READ_ONLY) and queries status and metadata for a single id under the master tag. Both mechanisms exist in the crate; PDROP must copy the PTODO one. The live-versus-terminal predicate to reuse verbatim is ptodo.rs's is_terminal_status - done or cancelled - together with its fail-soft contract, where a missing store degrades the lane with one stderr breadcrumb and leaves the exit class untouched. **PASS**. — `delivered_check`: **manual** (mechanism-selection correction; the behavioural twin is the PDrop registration checks above and the drift-guard row under eta-prime.).

- **ratchet-precedent-and-fingerprint-grammar** — capability->producer (substrate) - PTODO supplies the ratchet precedent PDROP should copy rather than re-derive: one canonical fingerprint function producing '<path> :: <kind> :: <normalized text>', a committed crates/reify-audit/ptodo-baseline.txt, a generator bin picked up by cargo autobin discovery (so no Cargo.toml edit), a shell ratchet in tests/infra carrying a scan-evidence vacuity floor, and a Rust well-formedness test over the baseline. PRD section 12 open question 3 proposes that the allowlist IS the baseline; see the enumeration-completeness capability below for why that choice is load-bearing rather than cosmetic. **PASS**. — `delivered_check`: **manual** (substrate fact, already true on main; the deliverable checks are the registration ones above.).

- **enumeration-completeness-is-the-landing-risk** — DECOMPOSE-TIME FINDING (contract C4a 'param in no set' versus landing green). The repo declares roughly sixteen @optimized trampoline targets: solver::elastic_static, solver::buckling, solver::buckling_multi_case, solver::multi_case, solver::form_find, solver::form_find_free, solver::membrane_load, modal::free_vibration, modal::mechanism_modal, modal::transient_response, modal::displacement_at, trajectory::input_shape, trajectory::simulate, dynamics::inverse_dynamics, fdm::slice and fdm::as_printed_material_r_fast. Leaves alpha through zeta deliver C1 declarations for FIVE of them. If PDROP enumerates all sixteen and reds on any param in no set, eta lands RED on main on day one. Eta must therefore pick ONE of two landable shapes and record the choice. Shape 1: complete the declarations for the remaining eleven trampolines inside eta's own scope - which includes free_vibration's ModalOptions declaration, the C1 invariant-3 worked case the PRD itself names, where free_vibration honors element_order while mechanism_modal declares it not_applicable. Shape 2: enumerate only trampolines carrying a declaration marker, plus a committed ratcheting list of not-yet-declared trampolines that must not grow. Shape 2 is the PTODO/PDIAG house posture and the cheaper landing; shape 1 is the stronger end state. Either lands green; neither is optional. **PASS**. — `delivered_check`: **manual** (a design choice with two acceptable resolutions, recorded so it is made deliberately at implementation time rather than discovered at merge.).

- **pdiag-branch-collision-real-edge-wired** — DAG-direction - #5405 (in-progress) is landing PDIAG on branch task/5405 and touches the EXACT same registration surfaces PDROP needs: the Pattern enum in crates/reify-audit/src/lib.rs, the --pattern token set and dispatch in crates/reify-audit/src/bin/reify-audit.rs, and the crate's tests. It also lands the baseline-ratchet and per-site-escape design PDROP should copy. A real add_dependency edge eta -> #5405 is wired. **PASS**. — `delivered_check`: **manual** (dependency-graph property, wired via add_dependency and checked by the scheduler.).

- **format-markdown-is-not-a-binary-flag** — CORRECTION APPLIED (G3, false-substrate guard). --format markdown is NOT a reify-audit flag: parse_args accepts only --task, --pre-done, --since, --pattern, --tasks-file, --fused-memory-url, --runs-db, --project-root, the --jcodemunch family, --no-jcodemunch, --help and --version, and hard-rejects anything else as an unknown flag. --format markdown is an argument of the /audit SKILL, which renders the report itself. The PRD's eta row says '/audit --pattern PDROP reports the live allowlist', which is the skill and is therefore correct as written - this binding exists so no implementer binds evidence to a binary flag that does not exist. The binary emits a JSON Finding array on stderr and a human summary on stdout, with per-run artifacts under data/audit-runs/. **PASS**. — `delivered_check`: **manual** (negative substrate fact recorded to prevent a false evidence binding downstream.).


### η′ — task #7086

*trampoline param-drop η′: PDROP gate-test drift-guard registration*

- **manifest-row-is-the-forcing-function** — capability->producer (substrate) - tests/infra/run-all-classification.manifest declares '<test_basename> <bucket>' for every discovered tests/infra/test_*.sh, with exactly three bucket values: pool, intra-run-serial and host-exclusive. tests/infra/test_run_all_classification.sh asserts that the symmetric declared-versus-discovered difference is EMPTY via a shared accessor library, and separately asserts that guard is itself non-vacuous by re-running it against a fixture manifest missing a row. So a new tests/infra/test_reify_audit_pdrop.sh WITHOUT a manifest row reds immediately. PTODO's own split is the precedent to mirror: the cargo-running ratchet is intra-run-serial, the hermetic fixture-repo probes are pool. **PASS**. — `delivered_check`: grep `/pdrop/` **present** in `tests/infra/run-all-classification.manifest` (executed at decompose: RED today).

- **registration-must-not-be-prose-ordered** — DAG-direction (the overlay's worked failure, esc-4914-162). Task 4914 landed a gate-resident smoke binary without its drift-guard registrations, turning main RED for every subsequent merge, because the registration was ordered after the test-adding task by PRD prose only rather than by a hard edge. Eta-prime is wired as a real add_dependency on eta and its deliverable IS the registration; the overlay additionally permits the same-diff form. What is REJECTED is a sibling ordered only by prose. **PASS**. — `delivered_check`: **manual** (dependency-graph property, wired via add_dependency and checked by the scheduler.).

- **nextest-and-harness-baseline-need-no-row** — capability->producer (substrate) - NEGATIVE RESULT RECORDED so no phantom registration work is filed. Verified 2026-08-31. (a) .config/nextest.toml needs NO entry: reify-audit is outside the occt test-group package filter, and a new crates/reify-audit/tests/pdrop.rs inherits the default profile's 20-minute ceiling; an override is required only for a genuinely longer binary. (b) scripts/check-harness-baseline-registration.sh applies only to the five consolidatable crates - reify-cli, reify-syntax, reify-kernel-occt, reify-eval, reify-compiler - and reify-audit is not among them, which is why ptodo.rs, pdoccover.rs, pdssentinel.rs and player.rs already sit as standalone tests there. (c) tests/infra/test_no_new_wallclock_upper_bounds.sh scopes to tests/infra/*.sh only and has no registry; its escape is an inline wallclock:allow comment on the offending line. (d) scripts/heavy-test-filter-lib.sh classifies heavy by membership in one positive filterset expression, so a PDROP test is gate-resident by default and adding an atom would be an opt-OUT, not a registration. **PASS**. — `delivered_check`: **manual** (a negative substrate finding; asserting absence of rows nobody needs would be a vacuous check.).

- **manifest-edit-escalates-scope** — blast radius - tests/infra/run-all-classification.manifest, its accessor library and its guard are declared LOAD-BEARING verify-pipeline artifacts in the manifest's own header, so eta-prime's edit routes through the full --scope all gate. Expected, not a defect. File contention to be aware of: #6354 (pending) also edits this manifest for the PDOCCOVER/PDSSENTINEL skill-doc registration - contention only, no dependency wired. **PASS**. — `delivered_check`: **manual** (scheduling and scope observation, not a delivered capability.).


### θ — task #7087

*trampoline param-drop θ: RULING - wire or delete target_fidelity on FDMSliceOptions and AsPrintedOptions*

- **consumer-is-provably-unreachable** — capability->producer (substrate, the premise of the ruling) - CONFIRMED 2026-08-31. select_rungs is defined in crates/reify-fdm/src/as_printed.rs and re-exported from crates/reify-fdm/src/lib.rs, and a repo-wide git grep finds NO caller in any crates/*/src/* outside that re-export: the only callers are crates/reify-fdm/tests/as_printed.rs and crates/reify-eval/tests/fdm_progressive_refinement_e2e.rs, both test-only. A corroborating note in crates/reify-eval/src/compute_targets/fdm_slice.rs independently calls target_fidelity an inert no-op placeholder. This is the C-10 test-only shape, and it is exactly why decision D5 makes this a ruling rather than a build task. **PASS**. — `delivered_check`: **manual** (substrate fact, already true on main; theta's deliverable is a ruling, whose two acceptable outcomes are disjunctive and not expressible as one pattern.).

- **delete-branch-has-a-known-blast-radius** — blast radius, recorded so the ruling is made with its cost visible - target_fidelity is declared twice, on FDMSliceOptions in crates/reify-compiler/stdlib/fdm_slice.ri and on AsPrintedOptions in crates/reify-compiler/stdlib/fdm_as_printed.ri, and a type-identity test in crates/reify-compiler/tests/harness_langcore/prelude_sub_member_typing_tests.rs asserts that BOTH structures declare target_fidelity AT THE SAME TYPE. The delete branch therefore reds that test and must update it in the same diff. Adjacent but no seam: #5806 (deferred) owns the dimension-check half of the AsPrintedOptions readers and shares crates/reify-eval/src/compute_targets/as_printed_material.rs - contention only, no dependency wired. **PASS**. — `delivered_check`: **manual** (cost record attached to one branch of the ruling; asserting it would presuppose that branch.).

- **execution-path-is-a-human-ruling** — routing (execution_class) - PRD decision D5 states the disposition is 'a decision task, not a build task, and is filed as such'. Theta is filed task_kind=deterministic with metadata.execution_class=decision, which submit converts into an always-escalates pure gate whose dispatch action is a born-at-L2 escalation to a human. It is NOT a normal code leaf, and it is NOT filed operational-because-it-is-not-code, which Leo's 2026-08-24 ruling rejects. The implementing change follows the ruling and is filed at resolve time. **PASS**. — `delivered_check`: **manual** (task-routing property, expressed in the filed task's kind and metadata rather than in the tree.).


### κ — task #7088

*trampoline param-drop κ: docs-truth - solver-option doc chunk, best-practices exemplar, cheatsheet index, discoverability*

- **no-fea-or-solver-chunk-exists-so-this-is-net-new-content** — capability->producer (substrate) - MEASURED 2026-08-31. crates/reify-mcp/src/tools/chunks/ holds 17 chunks (collections, connect, constraints, enums, fields, functions, geometry, guards, occurrences, parameters, purposes, stdlib, structures, syntax, traits, types, units) and grep returns ZERO hits across ALL of them for ElasticOptions, max_iter, cg_tolerance, solve_buckling, actuator_limits and mechanism_modal. The only FEA-adjacent content is a one-line module-tree entry each for std.structural and std.analysis in stdlib.md. Kappa therefore writes NET-NEW content rather than editing - which is precisely the docs-truth gate's 'language surface landed without its doc leaf' failure, caught here instead of at a dogfood session. **PASS**. — `delivered_check`: **manual** (kappa may either extend stdlib.md or add a new chunk; pinning one path now would prejudge the placement. The reviewer check is: the affected chunk names the solver-option knobs, and every documented signature compiles as written in a smoke .ri.).

- **chunk-registry-is-hand-maintained-with-a-hardcoded-count** — blast radius (capability->producer) - chunks are NOT globbed and no build script is involved. crates/reify-mcp/src/tools/language_chunks.rs requires FOUR edits for a new chunk: an include_str! const, a TOPICS row, a get_chunk match arm, and a HARDCODED assert_eq! on available_topics().len() in its own inline unit test, which is a guaranteed RED if missed. A FIFTH edit lands in crates/reify-mcp/tests/reference_tools_tests.rs, whose duplicated ALL_TOPICS list has NO drift guard tying it to TOPICS, so a chunk added without touching it is silently untested there. If kappa extends stdlib.md instead of adding a chunk, none of these apply. **PASS**. — `delivered_check`: **manual** (conditional blast radius that applies only under the new-chunk branch of the placement decision above.).

- **exemplar-corpus-gates** — capability->producer (substrate) - an examples/best_practices/ addition is gated three ways, all confirmed 2026-08-31. It is auto-compiled by the recursive examples walk in crates/reify-compiler/tests/harness_compilation_surface/examples_smoke.rs - NOTE that the reify overlay's docs-truth section cites the stale path crates/reify-compiler/tests/examples_smoke.rs, which does not exist; the harness submodule path is the live one. It is catalogued bidirectionally against INDEX.md's '| Exemplar | Idiom | Anti-pattern it replaces |' table. And it is constraint-gated by crates/reify-eval/tests/harness_corpus_gates/best_practices_constraint_gate.rs, which requires every constraint to be Satisfied or pinned in an EXPECTED_INDETERMINATE allowlist with a documented reason. An FEA-derived constraint is likely Indeterminate on the pure value-eval surface - the clearance_oracle.ri precedent - so kappa should expect to need an allowlist pin. No existing exemplar covers FEA or solver options. **PASS**. — `delivered_check`: **manual** (the gates are substrate; the deliverable is the new exemplar plus its INDEX.md row, whose filename is chosen at implementation time.).

- **pdoccover-fabrication-lane** — hazard recorded (anti-fabrication) - crates/reify-audit/src/pdoccover.rs runs a fabrication lane over the chunk corpus: any call-shaped name documented in a chunk that exists nowhere in compiler or stdlib source is a High-severity fabricated-name finding. Kappa documents signatures that DO exist - solve_buckling, ElasticOptions, BucklingOptions, ModalOptions and both shapers are real stdlib declarations - so the lane should stay quiet. Kappa's acceptance nonetheless includes each documented signature compiling as written in a smoke .ri, which is the docs-truth gate's own requirement and the direct defence against the phantom-signature class (#5347/#5364). **PASS**. — `delivered_check`: **manual** (a hazard the acceptance criterion already covers behaviourally; a grep would only assert the detector exists, which is already true.).

- **discoverability-acceptance-is-intent-level** — signal premise (docs-truth gate requirement 4) - the leaf's signal includes intent-level findability: an author who knows the GOAL ('make the solver use my iteration budget', 'make the column's end conditions matter') but not the feature name reaches the knob from the chunk text or the corpus index line. The cheatsheet entry goes in the 'Probe-verified idioms - index' section of .claude/skills/reify-design/SKILL.md as a one-line pointer at the corpus file, never an inline playbook. **PASS**. — `delivered_check`: **manual** (a qualitative acceptance criterion by construction; mechanizing it would reduce it to 'a line exists', which is exactly the vacuity this gate guards against.).


### λ — task #7089

*trampoline param-drop λ: PRD close - task-id backfill verification, terminal stamp, AS-AUTHORED freeze header*

- **decomposition-rows-carry-real-ids** — capability->producer (decompose-close obligation 1) - section 10's leaf rows were backfilled with real task ids by this decompose session, in the same commit as this manifest. Lambda VERIFIES that each row still resolves and repairs any drift, rather than discovering at close time that leaf state is mechanically unresolvable from the document - the structural cause of the kernel-seam-contracts recurrence named in esc-6232-5. **PASS**. — `delivered_check`: **manual** (the backfill is already committed by this session, so a grep for it would be green before lambda runs; lambda's job is verification and repair, not creation.).

- **terminal-vocabulary-is-closed** — capability->producer (decompose-close obligation 2) - the terminal token set is CLOSED to exactly SHIPPED, SUPERSEDED and WITHDRAWN, matched case-insensitively on the first token after the Status label within the first ten lines, with ALL CAPS the preferred authoring form. SHIPPED requires every leaf terminal, with cancelled leaves tolerated alongside provided at least one landed. The header shape to copy is docs/prds/v0_6/data-carrying-enums.md and docs/prds/kernel-seam-contracts.md - NOT the #4438 or #3847 precedents, whose own output is non-conformant with this rule. The same header is applied to the .capability-manifest.md. **PASS**. — `delivered_check`: grep `/Status.{0,12}(SHIPPED|SUPERSEDED|WITHDRAWN)/` **present** in `docs/prds/v0_6/trampoline-param-drop-closure.md` (executed at decompose: RED today).

- **cancelled-dependency-disposition** — DAG-direction - lambda depends on every other leaf by real add_dependency edges. A cancelled sibling counts as satisfied for lambda's edge, since both SHIPPED and WITHDRAWN require lambda to stay dispatchable against one; if the scheduler treats a cancelled edge as unmet, the decompose steward removes it by hand and applies the stamp in a docs-only commit rather than leaving lambda permanently blocked. **PASS**. — `delivered_check`: **manual** (dependency-graph property plus its documented manual fallback.).


---

## Out-of-batch edges wired

| From | To | Why |
|---|---|---|
| ζ | **#6097** (pending) | 6097 owns `ModalOptions.sigma` → `shift_frequency` and already carries the modal not-yet-honored warning as its own item 4. It also edits `crates/reify-eval/src/modal_ops.rs`, so the contention is real as well as semantic. ζ adopts what 6097 lands; its own residual is the `not_applicable` declaration for `tol`/`max_iters`/`element_order` plus the `ignored` entry citing #6097 for `sigma`. |
| η | **#5405** (in-progress) | #5405 lands PDIAG on branch `task/5405`, touching the exact registration surfaces PDROP needs — the `Pattern` enum, the `--pattern` token set and dispatch, and the crate's tests — and lands the baseline-ratchet + per-site-escape design PDROP should copy rather than re-derive. |

## Contention recorded, no edge wired

| Leaf | Other task / PRD | Shared surface |
|---|---|---|
| α | **#6484** (pending) | Retires the stale “structure-ctor binding is POSITIONAL” comments, including in `examples/buckling_column_p2.ri`. The claim is false post-task-4522; α must not act on the stale comment. |
| γ | **#6484** (pending) | Same file, `examples/buckling_column_p2.ri`. |
| γ | **#6663** (pending, high) | The *modal* path's two-support BC fidelity defect. Adjacent but distinct — γ should disclaim the modal path and cite it rather than be read as owning it. |
| ε | **#6240** (pending) | `crates/reify-compiler/stdlib/trajectory.ri`; ctor-time dimension-kind pairing, independent of param-drop-to-kernel. |
| ζ | **#6875** (done) | The precedent shape — `mechanism_modal_analysis` used to drop its `RayleighDamping` descriptor where the FEA path honored it. ζ is written against post-#6875 code. |
| η′ | **#6354** (pending) | Also edits `tests/infra/run-all-classification.manifest`. |
| θ | **#5806** (deferred) | Shares `crates/reify-eval/src/compute_targets/as_printed_material.rs`; owns the dimension-check half, not `target_fidelity`. |
| all | `docs/prds/compute-fea-hardening.md` | Sibling PRD, **no seam** — both edit `crates/reify-eval/src/compute_targets/*`, but its INV-FEA-1 canonical-registration work and this PRD's param extraction are independent within those files. File-lock contention only; deliberately **not** wired as a dependency. |

## Cross-PRD posture

`docs/prds/v0_6/eradicate-silent-undef.md` owns **INV-SF-6** (`diagnostics-carry-codes`) and its
`PDIAG` enforcement; this PRD owns **compliance** for the two new codes. PDIAG is **not on `main`** as
of 2026-08-31 — `crates/reify-audit/src/pdiag.rs`, `crates/reify-audit/pdiag-baseline.txt` and
`docs/notes/diagnostic-severity-policy.md` are all absent, and the work lives unlanded on branch
`task/5405`. No edge is wired from α: compliance does not need the enforcer to exist. η's edge to
#5405 exists for a different reason (shared registration surfaces).

`traction-and-body-force-loads` is the **split** target (PRD §9). `TractionLoad` and `BodyForce` are
dropped in both the elastic and buckling paths and are unimplemented *load types*, not dropped knobs;
they were split out deliberately. Their allowlist entries are owned by the PRD-authoring task filed in
the ι discharge.

## Substrate note (G3)

**No novel `.ri` grammar.** Four fixtures covering the PRD's boundary-row syntax — a non-default
`ElasticOptions(...)`, a `ModalOptions(element_order: ElementOrder.P2, ...)`, a `solve_buckling(...)`
call with a supports list, and a `TOTSShaper(actuator_limits: [...])` — were extracted to
`/tmp/prd-gate-fixtures/` and each parsed with `tree-sitter parse --quiet`, exit 0, zero ERROR nodes.
Ephemeral decompose-time probes are not committed, per the fixture-location standard. The grammar gate
is a formality for this PRD, confirmed rather than assumed.
