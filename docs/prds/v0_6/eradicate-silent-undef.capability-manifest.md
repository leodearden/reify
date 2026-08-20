# Capability manifest — eradicate-silent-undef.md

Authored at decompose time 2026-07-24 (same session as the PRD). Binds each
leaf's user-observable signal to substrate evidence per the /prd gates
(G3+G6 mechanized). Machine-readable twin:
`eradicate-silent-undef.capability-manifest.yaml` (task ids stamped at
`commit_planning`).

**Probe provenance.** All baseline probes executed this session against the
2026-07-22 debug binary (`target/debug/reify`) on main `0d70ef1d5b`; captured
outputs quoted in PRD §2. The D3 `.ri` overlay verification workflow is N/A
for this pure-Rust/CLI PRD (per `procedural_prd_d3_verify_workflow_is_ri_only`);
the generic manual G3 checks below were run directly instead. The two PRD
fixtures parse and run cleanly (module headers present, no ERROR nodes — the
only novel "syntax" in this PRD is CLI flags, not grammar).

## α — Unexplained-undef backstop

- `backstop-predicate-substrate` → PASS. `check_no_stale_undef` free fn +
  `Engine` wrapper exist and compute exactly the root-undef/no-cause
  predicate: `crates/reify-eval/src/invariants.rs:106` / `:268`; currently
  consumed only by the debug-gate corpus harness
  (`crates/reify-eval/tests/no_stale_undef_invariant_gate.rs`).
- `capture-flag-substrate` → PASS. `set_capture_undef_causes`
  (`crates/reify-eval/src/engine_admin.rs:2334`); tracer + formatter
  (`crates/reify-eval/src/undef_tracer.rs:50`, `format_undef_cause`).
- `silent-baseline-real` (signal premise) → PASS. Probe: `reify eval` on
  `fixtures/silent_undef_generate_geometry.ri` → `P.holes = [undef, undef,
  undef, undef]`, **no note**, exit 0; `reify check` → `All constraints
  satisfied.`, exit 0.
- `census-gap-real` → PASS. `Value::is_undef` exists
  (`crates/reify-ir/src/value.rs:1591`); **no** `contains_undef` — a List of
  undef elements evades the notes loop's `v.is_undef()` filter (probe: the
  printed `P.holes` produced no note even though the loop ran).
- `cli-skip-site` → PASS. Empty-cause `continue` in `cmd_eval`'s undef-notes
  loop (`crates/reify-cli/src/main.rs`, "if causes.is_empty() { continue; }").
- `diagnostic-code-home` → PASS. `DiagnosticCode` registry in
  `crates/reify-core/src/diagnostics.rs` (W_/E_ codes; task 2255/3416
  precedent) — `W_UNDEF_UNEXPLAINED` is a new coded variant there.

## β — `reify check` reports undef causes

- `check-capture-gap-real` (signal premise) → PASS. grep: zero
  `set_capture_undef_causes` calls in `cmd_check` (only `cmd_eval`
  `main.rs:1570/1588`, LSP `analysis.rs:106`); probe: `reify check` on
  `fixtures/silent_undef_unbound_param.ri` prints no undef information while
  `reify eval` prints `note: P.d is undef (because: P.w unbound)`.
- `notes-machinery-upstream` → PASS. DAG-direction: α (backstop + census
  helper) is a hard prereq of β; the tracer/formatter substrate is landed.
- `explain-undef-flag-precedent` → PASS. `--explain-undef` parsing exists in
  `cmd_eval` (`main.rs`, Q2/§8.4 comment block) — β extends the same shape to
  check.

## γ — MemberResolutionFailed provenance

- `variant-slot-exists` → PASS. `UndefCause` enum
  (`crates/reify-ir/src/value.rs:3855`, 5 variants, `Eq+Hash`, side-map
  recorded — A1 transparency) — additive variant is enum-churn-free by the
  same pattern task γ(#4323-era) used for `OpContractFailed`.
- `silent-seam-real` (signal premise) → PASS. #5360's probe-anchored repro
  (2026-07-23, main `0408733da2`): 2-level nested-sub member read →
  `Parent.echo = undef` + `Parent.m.relay = undef`, zero diagnostics.
  Functional fix owned by #5360 (pending-high); γ is additive provenance.
- `landing-order-robustness` → PASS by construction. γ's e2e is disjunctive
  (value determined OR cause note names the member path — never silent), so
  it stays green whichever of γ/#5360 lands first; shared-file serialization
  via worktree locks (`unfold.rs`, `engine_eval.rs` in both file sets).

## δ — CollectionEvalFailed provenance

- `variant-slot-exists` → PASS (same enum evidence as γ).
- `sink-substrate` → PASS. reify-expr undef-cause sink exists:
  `push_op_contract_failure` (`crates/reify-expr/src/lib.rs:620/2898`),
  `with_undef_cause_sink` (consumed in `engine_eval.rs`
  `record_op_contract_failures`) — δ records through the same channel from
  the lambda/collection path.
- `silent-seam-real` (signal premise) → PASS. Probe this session:
  `generate(4, |i| cylinder(...))` → `[undef ×4]` under a green check
  (fixture committed). Functional fix owned by #5385 (pending-high); same
  disjunctive-test robustness as γ.

## ε — check Error-severity exit gate + seed allowlist

- `severity-gate-precedent` → PASS. `cmd_eval`'s Severity::Error exit gate
  (`main.rs:1661` region) and `build_is_success` (`main.rs:2286`, task 4458)
  — ε converges check on the same rule via a shared helper.
- `outlier-baseline-real` (signal premise) → PASS. `finish_check`
  (`main.rs:2719`) decides exit from constraint outcome alone; #5386's
  probe-anchored repro: `error: sub-component "j" references unknown
  structure "InternalJig"` printed, exit 0. Rejection-mechanism check: the
  asserted post-ε behaviour (exit 1) is delivered BY ε — the baseline
  silent-accept is the documented gap, bound here as motivation AND as the
  gate's test fixture shape.
- `bolt-ons-present` → PASS. `GdtIllegalModifier` code-match escalation
  (`main.rs:~672`) + `E_DFM_` message-prefix escalation
  (`dfm_has_error_diagnostic`, `main.rs:~688/2580`) — both match
  Error-severity diagnostics only, so the general gate subsumes them;
  deletion is behaviour-preserving for those classes.
- `healthy-path-error-exists` → PASS. Code-less Error "no registered compute
  trampoline" emitted on kernel-less FEA check (`engine_compute.rs:654`,
  `engine_admin.rs:1519`, `engine_eval.rs:8301`; named by the DFM bolt-on's
  own comment and by #5311) — the known seed allowlist entry; its
  disposition is owned by #5311 (pending).
- G7 waiver recorded (PRD §9): `error-severity-exits-nonzero` — temporary
  enumerated exemptions are the Leo-decided burn-down mechanism, ratchet +
  PTODO-guarded, cleared by ζ.

## ζ — burn allowlist to zero

- `producers-upstream` → PASS. DAG-direction: ζ ← ε (mechanism + seeded
  entries) and ζ ← #5311 (trampoline demotion) — no capability owed by a
  downstream task.
- `disposition-always-exists` → PASS by policy. INV-SF-2 corollary: an
  Error expected on a healthy path is by definition demotable/recodable, so
  ζ can always converge to an empty list without deep path work (deep fixes
  spawn follow-ups and re-cite, per PRD §11.1).

## η — PDIAG codes-mandatory detector + severity policy

- `pattern-module-substrate` → PASS. reify-audit per-pattern module layout
  (`crates/reify-audit/src/{ptodo,puntested,pdead,...}.rs`) + PTODO
  hard-gate infra precedent (`tests/infra/test_reify_audit_ptodo.sh`,
  classification-manifest registration).
- `target-corpus-real` (signal premise) → PASS. Investigation census: ~362
  `Diagnostic::error/warning` ctor sites in reify-eval, 67 `with_code` —
  the per-file baseline ratchet has a real, non-empty denominator.
- `drift-guard-same-diff` → bound as a delivered check: η's new
  `tests/infra/test_reify_audit_pdiag.sh` must register its
  `run-all-classification.manifest` bucket row in the same diff (overlay
  rule; esc-4914-162 precedent).

## ι — `--deny undef`

- `flag-parsing-substrate` → PASS. cmd_eval/cmd_check hand-rolled flag
  loops already parse `--strict`/`--explain-undef`/`--purpose`; `--deny`
  is the same shape.
- `exit-0-baseline-real` (signal premise) → PASS. Probe: `reify eval` on
  the unbound fixture exits 0 while printing `P.d = undef`.
- `census-upstream` → PASS. DAG-direction: ι ← α (contains_undef census),
  ι ← β (check-side census) — deny binds to each command's default census.
