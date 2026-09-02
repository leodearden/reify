# Capability manifest — `check-diagnostic-truthfulness` (PRD 2, units-gating program)

Mechanizes G3+G6 per leaf, per `.claude/skills/prd/project.md`. All evidence gathered/re-verified
2026-07-28 against main HEAD `638d97d8ab` (the PRD's own re-verification cited `dc83d4fd60`, a
few commits earlier — no drift found in any anchor re-checked below except the two file-path
moves noted under α and β).

**Decompose-time finding (binding on this manifest):** two of this PRD's four leaves collided
with already-pending, already-ratified tasks filed under a different, earlier PRD
(`docs/prds/v0_6/eradicate-silent-undef.md`, tasks 5399-5406, filed 2026-07-24/25) that target
the IDENTICAL mechanisms. Resolved per the established first-filed-owns-the-seam precedent
(mem0 `38f402a7-af16-4db8-ac04-4d30281c8e77`, the placement-relations-belt/eradicate-silent-undef
collision — same task 5403 was the owner there too): **α is adopted onto existing task 5311**
(filed 2026-07-20) and **γ is adopted onto existing task 5403** (filed 2026-07-24), both via a
minimal `update_task` addendum citing this PRD, rather than filed as fresh duplicate tasks. Only
**β** and **δ** are new tasks this session (5748, 5749). Deltas from the PRD's literal text are
recorded per-leaf below and in each adopted task's own addendum, per the "record deltas rather
than edit the committed PRD" convention — neither PRD's `.md` was edited.

| Label | Task ID | Provenance |
|---|---|---|
| α | 5311 | pre-existing (filed 2026-07-20, Stage-2 reconciliation finding); PRD-2 addendum appended |
| β | 5748 | new, filed this session |
| γ | 5403 | pre-existing (filed 2026-07-24, eradicate-silent-undef ε); PRD-2 addendum appended — **THE EXIT-GATE LEAF sibling PRDs wire onto** |
| δ | 5749 | new, filed this session |

---

## α — task 5311 (severity/code hygiene for the compute-trampoline diagnostic)

- **trampoline-emission-sites-wired** — capability→producer (substrate): four sites emit the `"@optimized target {:?}: no registered compute trampoline"` line. **PASS**, but this row's original evidence was MISLEADING and is corrected here (2026-09-01, esc-5311-3). It matched only the SHARED PREFIX, so it could not see that two of the four are SOFT (`engine_eval.rs` `evaluate_params_and_lets_unified` / `evaluate_let_bindings` — push the diagnostic, then body-inline; message carries `(falling back to body-inlining)`) and two are HARD (`engine_admin.rs` `dispatch_compute_node`, `engine_compute.rs` `run_compute_dispatch` — return `Err`, nothing falls back; the clause is deliberately omitted). Its "exact match to the PRD's own citations, zero drift" claim was also wrong: three of the four line numbers had drifted. Resolve by SYMBOL. Per the RULING in the PRD's D4, the `DiagnosticCode` is minted at all four; the `fns.is_empty()` severity predicate applies at the two SOFT sites ONLY.
- **compute-registry-emptiness-predicate-substrate** — capability→producer (substrate): `self.compute_registry.fns` exists and is read/written at `engine_admin.rs:1160-1190` (`register_compute_fn`/`compute_dispatch`), confirmed present this session — `.is_empty()` is a valid predicate on it. **PASS**.
- **diagnosticcode-additive-variant-substrate** — capability→producer (substrate): `DiagnosticCode` is `#[non_exhaustive]` (`crates/reify-core/src/diagnostics.rs:155-156`), confirmed this session; no `VARIANT_COUNT`-style exhaustiveness backstop found on this enum (existing minting-rationale comments at :3015/:3018/:3035/:3785 independently confirm the additive-variant precedent). **PASS**.
- **regression-test-substrate** — capability→producer (substrate): `crates/reify-cli/tests/harness_cli/cli_check.rs` exists (confirmed this session) — α's new regression test extends it; not a new gate-resident file, so no drift-guard registration is triggered. **PASS**.
- **file-path-drift (correction applied)** — 5311's original citation `crates/reify-cli/tests/harness_cli/cli_build_fea.rs` (the FEA trampoline-free locking test `check_fea_violated_constraint_is_not_gated`) has MOVED to `crates/reify-cli/tests/harness_cli/cli_build_fea.rs` (confirmed this session via `grep -rl`). Corrected in 5311's PRD-2 addendum; the test name itself is unchanged.
- **baseline-swallowed-trampoline-error-real** — signal premise (G6 branch 3, end-to-end/negative-regression shape): probe-set evidence (this session, committed probe-set + `prd-capability-check.py --json`, PASS verdict) — `reify check examples/fea_pressure_smoke.ri` exits 0 while printing `error: @optimized target "solver::elastic_static": no registered compute trampoline (falling back to body-inlining)` on stderr. Captured command/exit-code/stderr in the probe run; harness `synthesize` verdict `{"blocks": false}`. **PASS**.

## β — task 5748 (route sub-paths (a)/(c) through build(); merge diagnostics)

- **module_has_geometry-substrate** — capability→producer (substrate): `fn module_has_geometry` defined `crates/reify-cli/src/main.rs:2432`, already consumed by `cmd_eval` at `:1556` — confirmed present, wired-on-main (not test-only). **PASS**.
- **build-internally-calls-check-substrate** — capability→producer (substrate): `build_with_geometry_output` calls `self.check(module)` internally as its own first step (`crates/reify-eval/src/engine_build.rs:3949`), confirmed present this session — this is the structural fact D2's merge design depends on (build()'s BuildResult carries a stale check-style copy, not just realization-only diagnostics). **PASS**.
- **ordering-invariant-substrate** — capability→producer (substrate): the build()→tessellate_realizations()→check() ordering comment at `main.rs:577-589` confirmed present. **PASS**.
- **diagnostic-fields-partialeq-substrate** — capability→producer (substrate): `crates/reify-core/src/diagnostics.rs` — `Diagnostic { severity: Severity, message: String, code: Option<DiagnosticCode>, .. }` (struct at :3821); `Severity` derives `PartialEq` (:94), `DiagnosticCode` derives `PartialEq, Eq, Hash` (:152/:156) — confirmed this session; the manual tuple-comparison D2 requires is buildable exactly as the PRD asserts (Diagnostic itself derives only `Debug, Clone`, no blanket `PartialEq`). **PASS**.
- **baseline-flange-indeterminate-swallowed-real** — signal premise (G6 branch 3): probe-set evidence (this session) — `reify check examples/m5_geometry_flange.ri` exits 0, prints `INDETERMINATE BoltFlange#constraint[1]`, discards `centroid`/`moment_of_inertia` resolution errors to stderr. Harness verdict PASS (baseline confirmed real, not yet fixed). **PASS**.
- **baseline-mirror-check-blind-real** — signal premise (G6 branch 3): probe-set evidence (this session, synthetic fixture reproducing the exact `examples/best_practices/symmetry_mirror.ri:27` "WRONG — bare-0 origin" shape) — `reify check` on the fixture prints ONLY "All constraints satisfied.", exit 0; the compile-geometry-op error line is completely absent from check's stderr. Harness verdict PASS. **PASS**.

## γ — task 5403 (general Severity::Error check-exit gate; adopted, allowlist-based)

- **beta-upstream-wired** — DAG-direction: `add_dependency(5403, depends_on=5748)` — real edge, wired this session (confirmed via `add_dependency` response). **PASS**.
- **allowlist-mechanism-substrate** — capability→producer (substrate, inherited from 5403's own original 2026-07-24 manifest in `eradicate-silent-undef.capability-manifest.yaml`, re-affirmed not re-derived): `build_is_success`'s shape (`main.rs:2286-2288`) is the precedent the shared severity-gate helper mirrors. **PASS** (manual, inherited).
- **bolt-ons-subsumed-and-still-deletable** — capability→producer (this task, unchanged from original): `GdtIllegalModifier` code-match (~main.rs:672) and `dfm_has_error_diagnostic` (~:688/:2580) both match Error-severity diagnostics only, confirmed present this session (grep) — the general gate subsumes them without behavior change. **PASS**.
- **baseline-mirror-eval-rejection-mechanism-real** — signal premise (G6 branch 4, rejection-mechanism check): probe-set evidence (this session, `probe_kind: ir`) — `reify eval` on the identical mirror bare-origin fixture exits 1 with `error: failed to compile geometry operation: missing or non-Length argument 'ox' for mirror` in stderr. This is the rejection mechanism γ's post-β/post-γ signal targets; confirmed it exists and fires today (at eval layer; check layer is the gap this PRD closes). Harness verdict PASS. **PASS**.
- **delta recorded** — PRD-2's D3/D4 literal text describes a bare, allowlist-free unconditional gate with a hard `add_dependency` onto α (5311) "to avoid a real regression." 5403's KEPT (not rewritten) design already avoids that regression via its seeded `CHECK_ERROR_EXIT_ALLOWLIST` entry citing #5311 — so no new edge from 5403 onto 5311 was wired; see 5403's PRD-2 addendum for the full reasoning. The end-state (zero exemptions) converges once task 5404 lands (already depends on 5403 + 5311, pre-existing edges, unchanged).

## δ — task 5749 (docs-truth leaves)

- **gamma-zeta-upstream-wired** — DAG-direction: `add_dependency(5749, depends_on=5403)` and `add_dependency(5749, depends_on=5404)` — both real edges, wired this session. **PASS**.
- **getting-started-anchors-real** — capability→producer (substrate): `docs/getting-started.md` line anchors confirmed this session — `:8` (§1 sanity-check code block), `:46-48` (bare-`50` indeterminate teaching example), `:86-94` (`## Common issues` section, `:92-94` sub-range) all present verbatim, zero drift from the PRD's citations. **PASS**.
- **symmetry_mirror-comment-anchor-real** — capability→producer (substrate): `examples/best_practices/symmetry_mirror.ri:27` (`// WRONG — bare-0 origin`) and `:32` (`TRAP: that error does NOT appear under check`) both confirmed present verbatim this session. **PASS**.
- **cheatsheet-index-already-present** — capability→producer (substrate): `.claude/skills/reify-design/SKILL.md:72` indexes `symmetry_mirror.ri`, confirmed present this session — no new index line needed (δ's own text states this explicitly rather than silently skipping the sub-requirement). **PASS**.
- **bare-radius-teaching-example-unaffected-real** — signal premise (G6 branch 3, negative-regression claim): probe-set evidence (this session, synthetic fixture matching the doc's bare-radius shape) — `reify check` on a `Length` param set to a bare `50` (no unit) stays `INDETERMINATE Flange#constraint[0]` with a Warning (`operator undefined for these operand kinds`), exit 0 — no `Severity::Error` produced, so this PRD's γ gate does not touch this teaching example. Harness verdict PASS. **PASS**.

---

## D3 verification workflow — batch verdict

The `Workflow` tool (`{scriptPath: "scripts/prd-decompose-verify.mjs"}`) is not available in this
decompose session's toolset (subagent context; not a deferred/loadable tool here). Ran the
equivalent Enumerator→Prover‖Adversary→Synthesize pipeline manually against the deterministic
core the workflow wraps (`scripts/prd-decompose-verify.py` + `scripts/prd-capability-check.py`),
per the project overlay's own description of that script as "the testable core of the
Workflow-based decompose verification":

- **Enumerator** (manual): 5 premises extracted from the four leaves' signals — the α/β/γ/δ
  baseline claims enumerated above under "signal premise" / "rejection-mechanism" bindings.
- **Prover** (`scripts/prd-capability-check.py --json` against a committed probe-set): all 5
  probes evaluated against a fresh `target/release/reify` (binary timestamp confirmed newer than
  HEAD `638d97d8ab`) — **5/5 PASS**, 0 FAIL, 0 UNPROVABLE, 0 HARNESS_ERROR.
- **Adversary** (manual independent hunt): the substantive finding was the α/γ collision with
  pre-existing tasks 5311/5403 documented at the top of this manifest — an unlisted premise
  ("no conflicting pending task already targets this mechanism") that the PRD's own text did not
  enumerate. Resolved per the first-filed-owns-the-seam precedent rather than left as a blocking
  finding; no fixture-level falsification of any of the 5 Prover premises was found.
- **Synthesize** (`scripts/prd-decompose-verify.py synthesize`, run against the unioned Prover +
  Adversary records): `{"blocks": false, "blocking": [], "report": ""}`, exit 0.

**Verdict: batch does not block.** No FAIL/UNPROVABLE/HARNESS_ERROR. The one adversarial finding
(task collision) was resolved structurally (task adoption + addenda + dependency rewiring), not
by weakening any premise.
