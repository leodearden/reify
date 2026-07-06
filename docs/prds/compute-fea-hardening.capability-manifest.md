# Capability manifest — compute-fea-hardening

Mechanizes G3 + G6 for `docs/prds/compute-fea-hardening.md` (decompose 2026-07-06). One block per
**leaf** task, binding each capability its user-observable signal asserts to evidence. Any FAIL
value blocks the batch. All evidence re-verified against live `main` @ 2026-07-06 (post the 9
intervening commits on `elastic_static.rs` since the survey's 2026-07-05 line numbers).

G6 numeric-domain (FEA) hazards: **no new closed-form/accuracy bound is asserted anywhere in this
PRD** — no branch-1/branch-2 exposure. All leaf signals are branch-3 (end-to-end capability:
"a diagnostic appears instead of a crash", "a hint diagnostic appears") or branch-4 (rejection
mechanism: "the architecture test fails on a bypass", "the grep gate fails on regression").
Grammar gate: **N/A** — no novel `.ri` syntax introduced by any of the four scopes.

Evidence vocabulary:
- `grep:<file>:<line> wired` — symbol/pattern present on main at the named line, re-verified live.
- `producer:<label> upstream` — capability delivered by an upstream task in the transitive dep closure.
- `capability-exists` — the substrate the task's mechanism will use is already present on main.

---

## A5 — compute-trampoline-registration architecture test

**Signal:** grep-based architecture test + `scripts/check-compute-trampoline-registration.sh` fails
if a fourth engine-construction site bypasses `register_production_compute_fns`.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Exactly 3 known call sites exist to assert against | `grep:crates/reify-cli/src/main.rs:1229 wired`, `grep:gui/src-tauri/src/engine.rs:1031 wired`, `grep:crates/reify-eval/src/test_runner.rs:109 wired` (re-verified live 2026-07-06) | PASS |
| `register_production_compute_fns` exists for the script to grep for | `producer:A1 upstream` (A1 is a same-batch prerequisite, wired via `add_dependency`) | PASS |
| A precedent script-wiring pattern exists to copy | `grep:scripts/check_event_inventory.sh wired`, `grep:tests/infra/test_event_inventory_wired.sh wired` | PASS |
| DAG-direction | A1→A2/A3/A4→A5, all upstream of A5 | PASS |

---

## C1 — LSP posture doc + locking test

**Signal:** committed test pins no false violation/false pass for an FEA constraint under the LSP's
trampoline-free `Engine`.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `cmd_check`'s posture comment exists as the pattern to mirror | `grep:crates/reify-cli/src/main.rs:450-471 wired` | PASS |
| The locking-test pattern to copy exists and passes today | `grep:crates/reify-cli/tests/cli_build_fea.rs:147 wired` (`check_fea_violated_constraint_is_not_gated`, confirmed present at exactly this line) | PASS |
| `Satisfaction::Indeterminate` is the actual runtime state an unregistered FEA constraint reaches | `grep:crates/reify-lsp/src/diagnostics.rs:190,236 wired` (both filter on `Satisfaction::Violated`, confirmed live) | PASS |
| LSP test convention (inline `&str` source, no `.ri` fixture files) confirmed so C1 doesn't assume a missing fixture mechanism | `grep:crates/reify-lsp/src/diagnostics.rs:410-421 wired` (`valid_bracket_source_no_errors` uses `reify_test_support::bracket_source()`, an inline helper, not a file path) | PASS |

---

## C2 — LSP hint diagnostic

**Signal:** an FEA-bearing `.ri` file open in the LSP shows the hint diagnostic on its FEA
constraint (was: nothing).

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `Severity::Info` maps to a non-error LSP diagnostic severity (substrate for "non-noisy hint") | `grep:crates/reify-lsp/src/convert.rs:148 wired` (`Severity::Info => DiagnosticSeverity::INFORMATION`) | PASS |
| A per-constraint identity to key the once-per-document dedup on already exists | `grep:crates/reify-lsp/src/diagnostics.rs:190-199 wired` (`ConstraintCheckEntry.id`/`.label` used for exactly this kind of per-constraint identity today) | PASS |
| The producing fixture/posture doc exists | `producer:C1 upstream` | PASS |
| Task #5023 exists to cite as superseding consumer | `grep:fused-memory task 5023 wired` (confirmed present, status=deferred, metadata explicitly names "this also unifies the LSP FEA-constraint answer") | PASS |

DAG-direction: C2 depends on C1 (upstream). No FAIL.

---

## D1 — catch_unwind wrapper + regression tests

**Signal:** `reify build`/`reify eval` on a fixture that trips a compute-trampoline panic exits 1
with a `Failed`/error diagnostic instead of a Rust panic backtrace + process abort.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `invoke_compute_trampoline` is the single generic dispatch point for every registered `ComputeFn` (so the fix covers FEA/buckling/modal/shell-extract uniformly) | `grep:crates/reify-eval/src/engine_compute.rs:102-119 wired` (looks up `self.compute_registry.fns.get(target)` by string, confirmed generic) | PASS |
| `ComputeOutcome::Failed` exists and is already constructed on the production path (not a new variant) | `grep:crates/reify-eval/src/compute_targets/elastic_static.rs:437,469 wired` | PASS |
| The RED-test premise — bare `Value::Undef` at each arg actually panics today, not already guarded | `grep:crates/reify-eval/src/compute_targets/elastic_static.rs:3373,3745,3791 wired` (`other => panic!(...)` arms in `classify_material`/`extract_scalar_si`/`extract_loads`, confirmed reachable for a bare-Undef arg outside the body-path/AsPrintedZones-specific guards) | PASS |
| `std::panic::catch_unwind`/`AssertUnwindSafe` are stdlib, unconditionally available | `capability-exists` | PASS |

---

## D9 — validate-all-inputs gate (Result-refactor integration leaf)

**Signal:** `reify build` on an FEA fixture with a malformed/Undef input exits with a diagnostic
**naming the offending constraint/arg**, not a raw panic backtrace.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `classify_material` returns `Result` by the time D9 runs | `producer:D7 upstream` | PASS |
| `extract_scalar_si` returns `Result` by the time D9 runs | `producer:D2 upstream` | PASS |
| `extract_loads` returns `Result` by the time D9 runs | `producer:D8 upstream` | PASS |
| Insertion point is precisely identified (won't silently disturb the two pre-existing guards) | `grep:crates/reify-eval/src/compute_targets/elastic_static.rs:429-470,472-478 wired` (body-path guard, `AsPrintedZones`-Undef guard, then `classify_material` call — exact line range re-verified live) | PASS |
| Precedent for "early guard returns `ComputeOutcome::Failed` with empty diagnostics/structured_detail" to generalize | `grep:crates/reify-eval/src/compute_targets/elastic_static.rs:437,469 wired` | PASS |

DAG-direction: D9 depends on D2, D7, D8 (all upstream, all same-PRD intermediates). No FAIL.

---

## E1 — mesh_boundary.rs fail-closed guard

**Signal:** regression test feeding a NaN candidate distance asserts exclusion + a warn log line.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Exact unguarded site | `grep:crates/reify-kernel-gmsh/src/mesh_boundary.rs:537 wired` (`.min_by(|a,b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))`, confirmed live; function span 519-539 re-verified) | PASS |
| Fail-closed template to mirror exists and is already landed | `grep:crates/reify-kernel-gmsh/src/through_thickness.rs:170-200 wired` (finite-guard + `tracing::warn!` + early return, task #3178) | PASS |
| `tracing` is already a dependency of `reify-kernel-gmsh` (substrate for the warn call) | `grep:crates/reify-kernel-gmsh/src/through_thickness.rs:192 wired` (`tracing::warn!` already used in this exact crate) | PASS |

---

## E2 — adaptive.rs fail-closed guard

**Signal:** regression test feeding a NaN indicator asserts it is never marked + a warn log line.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Exact unguarded site | `grep:crates/reify-solver-elastic/src/adaptive.rs:273-278 wired` (function span 266-292 re-verified) | PASS |
| `mark_dorfler`'s zero-energy/empty-set edge cases are already tested (so a new "treat non-finite as zero-contribution" branch composes with documented behavior, not a fresh design) | `grep:crates/reify-solver-elastic/src/adaptive.rs:259-265 wired` (doc comment: empty slice / all-zero indicator vector → empty `Vec`) | PASS |

---

## E3 — interpolation.rs total_cmp

**Signal:** regression test feeding a NaN centroid coordinate asserts the BVH build completes
without panicking.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Exact unguarded site | `grep:crates/reify-solver-elastic/src/interpolation.rs:387-391 wired` (`build_recursive`, function starts :337, re-verified) | PASS |
| `f64::total_cmp` is stdlib, unconditionally available, no MSRV gap (stabilized Rust 1.62) | `capability-exists` | PASS |

---

## E4 — modal_ops.rs frequency-sort fail-closed guard

**Signal:** regression test feeding a NaN frequency asserts the resort is skipped (order preserved)
+ a warn log line.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Exact unguarded site | `grep:crates/reify-eval/src/modal_ops.rs:441-446 wired` (found via this PRD's own repo-wide grep sweep — not in the survey's original 3) | PASS |
| The ascending-frequency contract this sort enforces is documented (so "skip the resort" is a principled fallback, not a silent behavior change) | `grep:crates/reify-eval/src/modal_ops.rs:432-440 wired` (doc comment states the eigensolver's own |λ|-ascending order already satisfies the contract in the normal case) | PASS |

---

## E5 — modal_ops.rs nearest_node fail-closed+total_cmp guard

**Signal:** regression test feeding a NaN node coordinate asserts a deterministic pick + a warn log line.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Exact unguarded site | `grep:crates/reify-eval/src/modal_ops.rs:2796-2797 wired` (`nearest_node`, function span 2784-2801, confirmed production code — not behind `#[cfg(test)]`, which starts at :2859) | PASS |
| `nearest_node` is genuinely reached on a production path (not test-only), justifying the graceful-not-panicking choice | `grep:crates/reify-eval/src/modal_ops.rs:2762-2763 wired` (called from `simply_supported_pin_pin_bcs`, itself production BC-realization code) | PASS |

---

## F1 — NaN-ordering grep gate

**Signal:** boundary-test sketch's last row — a synthetic unguarded site added to a scratch copy is
caught by the gate during task execution; the committed gate fails CI on any future regression.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| All 5 sites are clean (fail-closed or `total_cmp`) by the time the gate is authored, so it can be deny-all from day one | `producer:E1,E2,E3,E4,E5 upstream` | PASS |
| Precedent script + wiring-test pattern to copy exists | `grep:scripts/check_event_inventory.sh wired`, `grep:tests/infra/test_event_inventory_wired.sh wired` | PASS |
| The pattern to grep for is precisely specified (won't false-negative on the `total_cmp` sites it must NOT flag) | `grep:crates/reify-solver-elastic/src/interpolation.rs:390 wired` (post-E3, this becomes `f64::total_cmp`, textually distinct from `.unwrap_or(...Ordering::Equal...)`) | PASS |

DAG-direction: F1 depends on E1-E5 (all upstream). No FAIL.

---

## Summary

11 leaves, 0 FAIL bindings. 11 intermediates (A1-A4, D2-D8) each have a named downstream consumer
within the batch (A1→A2/A3/A4; A2/A3/A4→A5; D2→D5/D6/D7/D9; D3→D7; D4→D6; D5→D6; D6→D7; D7→D9;
D8→D9) — no orphan intermediate. No FAIL/UNPROVABLE binding blocks the batch.
