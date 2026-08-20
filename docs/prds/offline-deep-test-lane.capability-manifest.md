# Capability manifest — `offline-deep-test-lane` (Part A)

Mechanizes G3 + G6 per leaf for `docs/prds/offline-deep-test-lane.md`. Each binding ties a leaf's
asserted capability to **evidence** (grep/command/file). Any **FAIL** binding blocks queueing until
resolved. Verified against main @ `0113758b11`, 2026-07-01.

**Domain notes.** This PRD is **shell/config/test-partition infra**, not DSL or result-field
production. Therefore the reify field-population sentinel (`Value::Undef`) and grammar-fixture
checks are **N/A by construction** (no `.ri` syntax, no result fields) — recorded per binding rather
than silently skipped. The live G6 risk is the one **numeric floor** in the new gate smoke (A3).

---

## A1 — `heavy` nextest filter (single source of truth)

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `.config/nextest.toml` present; nextest 0.9.x supports binary/test filter expressions + `--run-ignored all`. The heavy binaries all resolve on disk — `crates/reify-solver-elastic/tests/{determinism,analytical_validation,modal_benchmarks}.rs`, `crates/reify-eval/tests/{buckling_smoke,tensegrity_t0a,fea_diagnostics_e2e}.rs` (verified `ls`, this session). |
| **Anti-orphan / wired** | **PASS** | Consumed within Part A: A2 (offline role selects it), A4 (gate negates it), A6 (asserts it). Not a producer with no consumer. |
| **Resolve-to-disk (anti-mismatch)** | **PASS (by A6)** | A6 extends the existing `tests/infra/test_nextest_slow_priority.sh` resolve-to-disk pattern to every `heavy` atom → a typo'd/dangling filter becomes a CI failure, not a silent coverage hole. |
| Grammar-fixture | **N/A** | No novel `.ri` syntax. |

## A2 — `DF_VERIFY_ROLE=offline` role in `scripts/verify.sh`

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `verify.sh` role `case` at ~L414 with error "want task\|merge" at ~L432; `CARGO_PRIO` idle-class idiom already used for `task`/`merge` (`nice`/`ionice` probed at L411-413). Adding an `offline` arm is symmetric. |
| **Anti-orphan / wired** | **PASS** | Consumed by A5 (`run-offline-deep.sh` executes the role) — a real reify-local caller on-main, not only the not-yet-existing DF worker. |
| **Negative-assertion (jobserver-detach)** | **PASS (by A6)** | A2 asserts the offline plan leaves `CARGO_MAKEFLAGS` unset (draws from neither task nor merge FIFO); A6/`--print-plan` observes the emitted env, not a promise. |
| Numeric floor | **N/A** | Role plumbing asserts no numeric bound. |

## A3 — thin gate smoke binary (`solver_gate_smoke.rs`)

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | New test binary in an existing crate; the smoke reuses existing solver entry points already exercised by `analytical_validation.rs` / `determinism.rs`. |
| **Numeric floor (G6)** | **PASS — bound pinned to an already-passing tolerance** | Determinism 1-vs-2 threads asserts **exact bit-stability** (equality — no numeric floor to clear). The one analytical benchmark uses a **coarse** tolerance pinned to an existing green bound (`cantilever_beam_p1_tip_deflection_within_5pct_of_timoshenko` already passes at 5% — safely **above** the P1-tet bending-lock floor of ~9–10% only for slender columns; the smoke uses a non-slender cantilever at 5%, the known-good regime). **Do NOT** author a tighter bound (e.g. 1% / a slender P1 column) — that reproduces esc-3453 (5% < 9–10% bending lock). |
| **Anti-orphan / wired** | **PASS** | Consumed by the gate: runs under `not (heavy)` because it is a distinct binary outside the `heavy` pattern (A4/A6 assert the partition). |
| Field-population | **N/A** | Test asserts values against analytical closed-forms; produces no `Value::Field`. |

## A4 — `REIFY_GATE_EXCLUDE_HEAVY` knob-gated gate exclusion (default 0)

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | Gate roles already build the nextest `selector` per-role (`verify.sh` ~L861); negating it behind an env check is a local addition. Env-var read makes the knob settable from `dark-factory-orchestrator.yaml` with no code change (the flip-seam contract, PRD §6). |
| **Additive-default (anti-regression)** | **PASS (by A6)** | Default `0` ⇒ gate plan unchanged; A6 asserts the knob-off plan is identical to today's + the smoke, and the knob-on plan is `not (heavy)`. The strictly-additive-on-landing invariant (§8) is observed, not assumed. |
| **Anti-orphan / wired** | **PASS (cross-repo, tracked)** | Consumer of the `1` value is Part B's `flip-gate-exclude-heavy` (dark-factory-orchestrator.yaml), wired via a real cross-project `add_dependency` edge at decompose (PRD §6). This is the accepted cross-repo seam class (reify ships the seam + default; DF pulls it) — the seam has a **named owner** (dark-factory), so it is not an unclaimed orphan. |
| **Negative-assertion (strict `1`)** | **PASS (by A6)** | A6 asserts any value other than exactly `1` (unset/empty/`0`/garbage) leaves the gate at the full set — a silent-accept of a malformed knob that *excluded* heavy would be a coverage hole. |

## A5 — `scripts/run-offline-deep.sh` one-shot runner

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | Thin wrapper over A2's role; no new substrate. |
| **Anti-orphan / wired (executable, not `--print-plan`-only)** | **PASS** | This *is* the anti-orphan evidence for A2: the `offline` role has a real reify-local caller that **executes** (not merely prints) the heavy set off-gate. Precedent this defends against: C-10 `selector_vocabulary_v2` (22+ fns, zero dispatch consumers). |

## A6 — `tests/infra/test_verify_offline_partition.sh` drift-guard

| Check | Verdict | Evidence |
|---|---|---|
| **Substrate exists (G3)** | **PASS** | `tests/infra/` harness (`test_helpers.sh`) + the registry `scripts/verify-pipeline-infra-tests.txt` exist; `test_nextest_slow_priority.sh` is the resolve-to-disk precedent to extend. |
| **Executable partition (anti-tabulation)** | **PASS — this leaf's whole job** | The guard **runs** `verify.sh --print-plan` under offline / knob-on / knob-off and diffs the actual emitted plans; it does not tabulate an unexecuted promise. Blocks the batch if the partition has overlap or an orphan (a heavy test running nowhere). |
| **Wired into the pipeline** | **PASS** | Registered in `scripts/verify-pipeline-infra-tests.txt` so a change to `.config/nextest.toml` / `verify.sh` selects it (`select_infra_tests()`); it also load-bears the `--scope all` plan per the CLAUDE.md verify-pipeline-guard rule (correctly routes Part A through the full gate). |

---

## Summary

| Leaf | Blocking verdict |
|---|---|
| A1 heavy filter | **PASS** |
| A2 offline role | **PASS** |
| A3 gate smoke | **PASS** (numeric floor pinned to an already-green 5% non-slender bound) |
| A4 flip knob | **PASS** (default-additive; cross-repo consumer named & dep-edged) |
| A5 runner | **PASS** (executable consumer — anti-orphan for A2) |
| A6 partition drift-guard | **PASS** (executable, not tabulated) |

**No FAIL bindings. Batch is clear to queue** once the decompose session runs the D3 substrate-verify
workflow (`scripts/prd-decompose-verify.mjs`) to confirm these bindings executably per-leaf.
The single G6 hazard (A3's numeric floor) is neutralized by pinning the smoke's tolerance to an
existing green bound; the manifest flags the trap (esc-3453) so decompose does not re-tighten it.
