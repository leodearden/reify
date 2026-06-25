# Capability manifest — infra-test wall-clock de-flake

Mechanizes G3 (substrate exists) + G6 (premise valid) for each leaf of
`infra-test-wallclock-deflake.md`. Every binding below is **PASS** — substrate
verified by grep on 2026-06-25, or queued as an upstream producer with the dep
wired. No `declared-only` / `rejection-absent` / `bound≤floor` bindings → batch
not blocked.

This PRD asserts **no** numeric/closed-form premises (G6 branches 1/2 N/A) — it
*removes* absolute wall-clock bounds. The G6-relevant premise per leaf is
"the structural marker the rewritten assertion checks is actually emitted on the
production code path" — bound to a `grep:file:line` below. The non-vacuous
mandate (still RED under broken wiring) is each leaf's two-way boundary signal.

| Leaf | Asserted capability | Evidence binding | Verdict |
|---|---|---|---|
| **T1** | shared acquire core is behavior-preserving + emits opt-in ordered ACQUIRE/RELEASE events, no-op when env unset | refactor target exists: `grep:scripts/lib_test_semaphore.sh:117-162` (shuffle/deadline/FD-9) ⇄ `grep:scripts/cargo-test-occt-gated.sh:148-224` (mirror); two-way boundary = existing `test_test_run_semaphore.sh` + `test_occt_flock_gate.sh` stay green | PASS (producer:self; new event-log is this leaf's deliverable) |
| **T2** | Section B exempt-path marker; Section C exit-75; Section A causal ordering | `grep:scripts/lib_test_semaphore.sh:60` (`bypass (role=merge) — no slot acquired`); `grep:scripts/verify.sh:1134` (`FAILED (exit 75): test-run semaphore acquire`); Section A ordering `producer:T1` (event-log, dep wired) | PASS |
| **T3** | occt deadline → exit 75; serialization ordering | exit-75 path in `scripts/cargo-test-occt-gated.sh` (mirror of lib:159); ordering `producer:T1` (if R) else generous-T ceiling (no marker needed) | PASS |
| **T4** | bypass-in-stderr + exit-75 already structural | `grep:tests/infra/test_test_run_semaphore.sh:131` (Test 9 bypass assert) + `:170` (Test 11 exit-75 assert) already present; T4 only drops redundant timing Tests 8/12 | PASS |
| **T5** | cpu-admit bypass + fail-open markers | `grep:scripts/cpu-admit.sh:134` (`bypass (role=merge)`), `:132` (timestamp-bumped variant), `:142` (`WARNING — fail-open — kernel lacks …`) | PASS |
| **T6** | shim STUB_CARGO stdout sentinel | `grep:tests/infra/test_agent_cargo_shim.sh:60` (stub echoes `STUB_CARGO $*`), captured `SHIM_STDOUT` `:108` | PASS |
| **T7** | no absolute discriminator (generous T / skip-gate) | `quiet_box_met` skip-gate pattern exists `grep:tests/infra/load_tolerance_lib.sh:146`; technique C/T removes the ceiling — no marker asserted | PASS (no capability claim) |
| **T8** | warm-lane trio in-class? (audit) | audit-gated — no capability asserted until findings; out-of-class residue handed to warm-lane owner | PASS (investigation leaf) |
| **T9** | guard fires RED on a planted wall-clock-upper-bound violation, GREEN on clean suite | the guard's planted-violation fixture is its own evidence (built by the leaf) | PASS (self-evidencing) |

**Induced-load acceptance substrate (all timing-bearing leaves):** `tests/infra/cpu_load_fixture.sh` exists (`grep:tests/infra/cpu_load_fixture.sh:2`, task 4634 done) — each leaf proves green under it at PSI `some~50%`.

**Precedent (approach already blessed on main):** #4756 (`9bc73aa`, done) decoupled the cancellation wall-clock SLA → iteration-count assertion; #4107 (`1f87535`, done) is the band-aid this PRD's T3 supersedes.
