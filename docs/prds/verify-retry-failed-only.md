# PRD: `retry_failed_only` — narrowed merge-gate retry (re-run only what didn't pass)

**Status: committed 2026-07-19. Evidence base: `docs/notes/merge-verify-cpu-survey-2026-07.md`. Program milestone: task 5254. Shape: B+H (cross-repo protocol seam).**

Cross-repo pattern: **reify ships the primitive, dark-factory wires the invocation.** reify owns the verify.sh / run_all / gui *consumption* of a retry-subset; DF owns the `merge_request` API field, retry-set *construction*, budget policy, shadow-baseline merging, and the honest event marker.

---

## §1 — Consumer + user-observable surface (G1)

**Consumers of the reify primitive:**
- (a) The DF merge worker's **autonomous failure path** — the M1 bounded classified-infra-transient retry (`orchestrator/merge_queue.py:1787-1902`, `CategoryPolicy.is_infra_transient`). On a retryable transient, the worker re-runs the *narrowed* subset instead of the full ~45-min gate.
- (b) **Operator `merge_request` resubmits** — a new `retry_failed_only: bool` argument on `merge_request` (`escalation/src/escalation/server.py:1093`), following the `verified_green` caller-vouched-bool precedent (`server.py:1099`).
- (c) The reify verify pipeline itself, at the `verify_env` seam — DF sets retry env vars that `scripts/verify.sh` honors.
- (d) Program milestone **5254** — the terminal integration-gate task wires into it.

**User/operator-observable surfaces:**
- A deliberately-flaked merge gate whose **retry log shows only the failed subset running** (a fraction of 20,280 tests + no recompile) and whose `merge_verify` event / `runs.db` row carries **`retry_scope: failed_only`** with per-suite subset sizes.
- The gate **lands** after the narrowed retry.
- Drift-guard infra test (`tests/infra/test_verify_retry_failed_only.sh`) demonstrating the subset-run + the soundness refusals in CI.

This PRD introduces no *new engine seam* (no `engine-integration-norm.md §3` entry needed) — it is verify-pipeline plumbing consuming existing capabilities.

---

## §2 — Problem statement + savings basis (G6)

From `docs/notes/merge-verify-cpu-survey-2026-07.md`:
- Passing merge gate: **median ~44–49 min wall** (p90 ~68); **2.26 gates per landed merge** (14d); failed gates = 24% of gate hours.
- **Compile+link ≈ 55–65% of gate CPU.** A retry reuses the `_merge-verify` lane's retained warm `target/`, so the recompile is a fast incremental no-op — **this is the dominant win**, independent of how much the test set shrinks.
- Debug pass alone: 20,280 tests / 1,145 binaries; release pass ~10,400 / ~500. run_all infra ~390s; gui vitest median 80s.

**Savings (cited verbatim, not inflated):** `~2.5 h/14d direct + head-of-line latency + makes retry budget >1 affordable.` Do not assert larger numbers than the survey's.

Today a single suspected-transient test failure costs a **full** re-verify (full compile + full 20,280-test run + full infra + gui). The M1 bounded infra-transient retry already exists but re-runs everything. `skip_verify` is unconditionally False on every success path (`merge_queue.py:11374`) — there is deliberately no green cache and this PRD does **not** add one: a retry is a *narrowed re-verify*, never a skip.

---

## §3 — Substrate (G3 — verified 2026-07-19; re-verify at decompose against post-recovery main)

All anchors re-verified at author time against `main@fc47d707b3`.

| Capability | Status | Evidence |
|---|---|---|
| `merge_request` caller-vouched-bool precedent | **exists** | `server.py:1099` `verified_green: bool = False` |
| DF per-test result parsing | **exists** | `merge_shadow.py:171 parse_per_test_results` (nextest + libtest formats) |
| M1 bounded classified-infra-transient retry seam | **exists** | `merge_queue.py:1787-1902` |
| Shadow warm per-test baseline store | **exists** | `on_result` callback `merge_queue.py:1556,1583-1587` → `merge_shadow.py` |
| Tree-pin / rebase-forces-full-verify (M4) | **exists** | `_reverify_rebased_tree` `merge_queue.py:79,1685`; any rebase full-verifies |
| nextest exact-name filterset | **exists** | `-E 'test(=<exact>)'` runs *exactly* the named tests (verified live: 2-of-6 subset ran) |
| run_all per-member re-invoke (Phase 2.5) | **exists** | `tests/infra/run_all.sh:1030-1049` serial retry-once re-runs individual failed pool members |
| `emit_nextest_pass` retry-env insertion point | **exists** | `scripts/verify.sh:1138` (nextest command construction) |
| run_all member-subset knob | **absent → new** | run_all has only `--scope host-infra`; no member-subset selector |
| gui failed-spec forwarding | **absent → new** | verify.sh gui block runs all specs |

### §3.1 — Falsified premise: nextest record/`-R` round-trip is NOT operational in 0.9.136

The survey proposed nextest experimental **record/rerun** (`NEXTEST_EXPERIMENTAL_RECORD=1`, `-R latest`) as the sound mechanism. **Author-time empirical verification falsified this for the installed `cargo-nextest 0.9.136`:**
- The `-R/--rerun <RUN_ID_OR_RECORDING>` flag parses (help: *"Rerun tests that failed or didn't complete… New tests… are also included by default"* — the sound semantics) **but there is no way to *create* a recording**: no `--record` flag, no `record` subcommand exposed (even with the env set), and the `[store]` / `store.enabled` config keys are rejected as unknown.
- After a normal run with `NEXTEST_EXPERIMENTAL_RECORD=1` set, `-R latest` reports **`error: no recorded runs exist`**. The record-*writer* is compiled in (binary strings show `prune`/`export`/retention) but unreachable in this build's surfaced CLI.

**Consequence — design pivots to the filter-file mechanism** (the brief's other named option, `REIFY_VERIFY_RETRY_FILTER_FILE`). This is *strictly better* than depending on `-R`: it is **version-stable** (no experimental feature that "may change" or is not even enabled), **ARG_MAX-safe** (subset read from a file, not the command line), and reuses the per-test map DF already parses. The record path is recorded as a future simplification bookmark (§11) — **not** a dependency.

No novel *reify grammar/DSL* substrate is assumed — the reify grammar gate is **N/A** for this pure-infrastructure PRD.

---

## §4 — The retry contract (H component — what crosses the seam)

Two attempts against **one pinned tree OID**. The tree OID is `git rev-parse HEAD:` (or the merge worker's frozen-tip tree hash) of the branch under verification.

### §4.1 — Attempt-0 (fresh, full verify) — unchanged behaviour + one addition
- verify.sh runs the full gate exactly as today (full compile, both nextest passes, run_all, gui).
- **Addition:** verify.sh stamps the verified **tree OID** into a small sidecar under the retained lane `target/` (e.g. `target/reify-verify-attempt.json` = `{tree_oid, profiles, timestamp}`). This is the only new attempt-0 artifact; it survives reseed because it lives under `target/` (`git clean -xfd -e target`).
- DF captures attempt-0's per-suite results via existing paths (`parse_per_test_results` + the nextest/run_all/gui logs).

### §4.2 — Retry (narrowed) — what DF sends into `verify_env`
DF constructs the **sound subset per suite** and sets:

| Suite | env DF sets | reify (verify.sh) guarantee |
|---|---|---|
| nextest (per profile: debug, release) | `REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE=<path>` (+ `_DEBUG` / `_RELEASE` variant) — newline-delimited **exact** test IDs = the sound subset | `emit_nextest_pass` appends `-E 'test(=id1) \| test(=id2) \| …'` built from the file so **exactly** those tests run; nothing else. |
| run_all infra | `REIFY_RUN_ALL_MEMBER_SUBSET="<member.sh basenames>"` | run_all runs **only** those members (reusing the Phase-2.5 per-member invoke path); all other members skipped. |
| gui vitest | `REIFY_GUI_RETRY_SPECS="<failed spec paths>"` | the gui block runs `npm test -- <specs>` (via `scripts/gui-test.sh -- <specs>`), only the named specs. |
| all suites | `REIFY_VERIFY_RETRY_TREE_OID=<oid>`, `REIFY_VERIFY_RETRY_SCOPE=failed_only` | verify.sh refuses a subset run unless the sidecar tree OID **matches** (else full-verify, loud); emits the honest marker (§4.4). |

**Sound-subset definition (per suite):**
- **nextest** (fail-fast-cancel): subset = **{did-not-pass}** = failed ∪ **not-started** (cancelled by fail-fast) ∪ new. *A failed-**only** `-E` filter is UNSOUND* because fail-fast leaves later tests un-run (survey: `5570/9823 tests run`); the not-started tests must be included. On a **tree-pinned** retry there are **no new tests**, so subset = {all planned − passed}. DF computes this from `parse_per_test_results` + the attempt-0 plan/list (DF-owned; see §8 D2).
- **run_all** (runs to completion, no cross-member fail-fast): subset = **{failed members}** — complete by construction, no not-started concern. Members are hermetic (H2/4924), so a subset run is side-effect-free.
- **gui vitest** (runs to completion): subset = **{failed spec files}** — complete by construction.

### §4.3 — Fallback-to-full is mandatory and loud
verify.sh **must full-verify (never silently skip or silently narrow)** and log a distinct reason when, under `REIFY_VERIFY_RETRY_SCOPE=failed_only`, any of:
- the attempt sidecar is absent, or its `tree_oid` ≠ `REIFY_VERIFY_RETRY_TREE_OID` (rebase / stale lane — routes through the existing M4 full path anyway);
- a suite's filter file / subset env is set but empty or unreadable;
- the subset size exceeds a safety ceiling (a subset ≈ the whole suite is a construction bug — prefer the honest full run).

This is the **rejection-mechanism** (G6 branch 4): the drift test authors each of these conditions and observes verify.sh actually fall back.

### §4.4 — Honest events (mining stays truthful)
verify.sh emits a `@@REIFY_RETRY_SCOPE=failed_only@@` marker plus per-suite subset counts (same emitter family as the clock-stop markers). DF captures it into the `merge_verify` event + `runs.db` row as `retry_scope: failed_only` with subset sizes (§8 D5). A narrowed retry is therefore **never** miscounted as a full green gate by the survey's runtime mining.

---

## §5 — Soundness invariants (encode as PRD-level, tested at the seam)

1. **Tree-pinned.** Subset retry only on identical tree OID; any rebase ⇒ full verify via the existing `_reverify_rebased_tree` (M4) path. verify.sh enforces via the sidecar (§4.3); DF enforces independently (belt-and-suspenders).
2. **Sound subset.** nextest subset = {did-not-pass} (incl. not-started); run_all/gui subset = {failed} (complete). Never failed-only for nextest.
3. **FLAKY-ledgered.** Every *failed-attempt-0 → passed-on-retry with no code change* outcome is a flake and is appended to the **durable** FLAKY ledger. This requires the ledger to survive lane reseed — **owned by PRD `merge-gate-health` (W5a)**; consumed here via a dependency edge (§8). Without durable ledgering, narrowed retries would erase the flake record on reseed and corrupt mining.
4. **Shadow-safe.** A subset retry produces a *partial* per-test map; DF must **merge attempt-0 + retry per-test maps** before storing the warm shadow baseline (`on_result` / `merge_shadow.py`), else the next cold-shadow compare sees phantom divergence.
5. **Category-gated.** Autonomous (M1) narrowed retry fires **only** for genuinely-retryable classes — gated on DF **2821**'s classifier fix (positive-anchored `semaphore_timeout`; the 0/31-precision categorizer must not label a deterministic red as retryable). Deterministic reds never get a narrowed retry; they surface.
6. **Honest events.** `retry_scope: failed_only` + subset sizes on every narrowed retry (§4.4).

---

## §6 — Resolved design decisions

1. **Mechanism = filter-file, not nextest `-R`/record.** Forced by §3.1 (record inoperable in 0.9.136). Version-stable, ARG_MAX-safe, reuses DF's existing per-test parse. `-R`/record is a future-simplification bookmark only (§11), never a dependency.
2. **Autonomous M1 application: YES — but category-gated + tree-pinned + ledgered.** The worker applies `retry_failed_only` on its existing M1 bounded retry, *conditioned on* DF 2821 confirming a retryable class, the tree being unchanged, and the flake being ledgered. This is safe because 2821 prevents a deterministic red from ever entering the retry path, and every narrowed-green retry leaves an honest audit trail. Operator `merge_request(retry_failed_only=True)` is the manual counterpart.
3. **Subset carried by file, expression built inside verify.sh.** DF writes exact test IDs to a file; `emit_nextest_pass` reads it and constructs the `-E 'test(=…) | …'` expression. Keeps the CLI bounded and the DF/reify contract a plain newline list. Exact (`test(=…)`) matching only — never substring (a substring match could pull in unintended tests, violating soundness).
4. **run_all/gui subset = {failed}, nextest subset = {did-not-pass}.** The asymmetry is intrinsic: run_all/gui run to completion; nextest fail-fast-cancels. Encoded in §4.2.
5. **No green cache, no `skip_verify`.** Consistent with `merge_queue.py:11374`. A retry always *runs* its subset against the warm target.
6. **New infra test carries its own drift-guard registrations same-diff.** `tests/infra/test_verify_retry_failed_only.sh` (task δ) must land with its `tests/infra/run-all-classification.manifest` bucket row and `test_no_new_wallclock_upper_bounds.sh` registration in the **same** diff (overlay drift-guard rule; the esc-4914-162 A3-before-A6 class). Not a downstream sibling.

---

## §7 — Out of scope

- FLAKY-ledger **durability** implementation (owned by PRD `merge-gate-health` W5a; consumed here).
- Failure-**classifier** correctness (owned by DF 2821; consumed here as a gating dependency).
- Compile-cost reduction (test-harness consolidation, eval B/D, release-sensitivity, ENGINE_VERSION_HASH) — PRD `merge-gate-compile-cost`.
- Lint-order swap, run_all content-addressed skip, release-pass delta-conditional — PRD `merge-gate-riders`.
- Restart-collateral / honest-categorization / gate-bypass (PRD `merge-gate-health` W1–W4).
- Offline-lane enablement (config-only checklist item).

---

## §8 — Cross-repo / cross-PRD seam ownership (G4)

**reify owns (ships the primitive):**

| Leaf | Mechanism |
|---|---|
| α | `emit_nextest_pass` honors `REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE` (per profile) + tree-OID sidecar guard + loud full-fallback |
| β | run_all `REIFY_RUN_ALL_MEMBER_SUBSET` member-subset knob (reuse Phase-2.5 invoke) |
| γ | gui block `REIFY_GUI_RETRY_SPECS` failed-spec forwarding |
| δ | reify-side boundary test `tests/infra/test_verify_retry_failed_only.sh` + honest `retry_scope` marker + same-diff drift-guard registrations |

**DF owns (wires the invocation):**

| Leaf | Mechanism |
|---|---|
| D1 | `retry_failed_only: bool` on `merge_request` + `MergeRequest` threading (`server.py:1093`, `verified_green` precedent) |
| D2 | worker **retry-set construction** — {did-not-pass} per nextest profile from `parse_per_test_results` + attempt-0 plan; {failed} run_all members; {failed} gui specs; write the filter files + set the reify retry envs in `verify_env`; tree-OID gate |
| D3 | retry **budget policy** + **M1 autonomous** application, category-gated on DF 2821 |
| D4 | **shadow-baseline map merge** (attempt-0 ∪ retry per-test maps) before storing the warm baseline (`on_result` / `merge_shadow.py`) |
| D5 | `merge_verify` **event marker** `retry_scope: failed_only` + subset sizes → `runs.db` |

**Seam table:**

| Seam | Direction | Mechanism | Owner | Status |
|---|---|---|---|---|
| `merge_request.retry_failed_only` | reify (operator/worker) → DF | API field + threading | dark-factory (D1) | queued |
| retry-set construction | DF → reify verify_env | filter files + retry envs | dark-factory (D2) | queued |
| verify.sh retry consumption | DF sets → reify honors | `REIFY_VERIFY_RETRY_*` envs | **this PRD (α/β/γ)** | queued |
| honest event marker | reify emits → DF records | `@@REIFY_RETRY_SCOPE@@` → `retry_scope` | reify emits (δ) / DF records (D5) | queued |
| FLAKY-ledger durability | **this PRD consumes** ← produced by | `merge-gate-health` **W5a** | `merge-gate-health` (dep edge) | **blocked-on: W5a not yet filed** |
| failure categories | **this PRD gates on** ← produced by | classifier | **DF 2821** (dep edge) | in-progress |
| program milestone | terminal → milestone | dependency into 5254 | reify (ε → 5254) | queued |

**Reciprocal-ownership check:** `merge-gate-health.md §7` reads *"FLAKY-ledger durability (W5) | THIS PRD produces; verify-retry-failed-only consumes (dep edge)"*. This PRD matches (consumes). No contested seam. DF 2821 owns the classifier; this PRD owns only the *dep edge + category-gated consumption*. No fourth contested pair introduced.

> **Decompose-time sequencing note (load-bearing).** The FLAKY-ledger dependency targets `merge-gate-health` **W5a**, which is **not yet a filed task** (that PRD is committed but undecomposed as of 2026-07-19). A real `add_dependency` edge cannot be wired until W5a exists. Decompose this PRD **after** (or jointly with) `merge-gate-health`, so the terminal task's W5a edge is a real edge, not prose. DF 2821 exists (`dark_factory:2821`) and 5254 exists — those edges wire immediately.

---

## §9 — Boundary-test sketch (two-way; faces both producer and consumer)

The integration-gate task (δ) names this as its observable signal (closes into G2).

### 9.1 — reify side (verify.sh looks *inward* at what it was handed)
| Scenario | Precondition | Postcondition |
|---|---|---|
| B1 nextest subset applied | filter file = 2 exact IDs, sidecar OID matches | exactly those 2 tests run; summary shows the rest skipped |
| B2 tree-pin refusal | sidecar OID ≠ `REIFY_VERIFY_RETRY_TREE_OID` | full verify runs; distinct `retry refused: tree drift` log line |
| B3 missing/empty subset refusal | scope=failed_only, filter file absent/empty | full verify; distinct `retry refused: no subset` log line |
| B4 run_all member subset | `REIFY_RUN_ALL_MEMBER_SUBSET="test_x.sh"` | only `test_x.sh` runs; others reported skipped |
| B5 gui spec subset | `REIFY_GUI_RETRY_SPECS="src/__tests__/foo.test.ts"` | only that spec runs |
| B6 honest marker | any narrowed retry | `@@REIFY_RETRY_SCOPE=failed_only@@` + subset sizes on stdout |

### 9.2 — DF side (worker looks *outward* at the subset it constructs)
| Scenario | Precondition | Postcondition |
|---|---|---|
| C1 sound nextest subset | attempt-0 fail-fast at test N of M | constructed subset = {failed ∪ not-started}, **not** {failed} |
| C2 category gate | attempt-0 category = deterministic-red (2821) | **no** narrowed retry; failure surfaces |
| C3 shadow map merge | subset retry produces partial map | stored warm baseline = attempt-0 ∪ retry maps; no phantom divergence |
| C4 event honesty | narrowed retry lands | `runs.db` row carries `retry_scope: failed_only` + sizes |

---

## §10 — Decomposition plan (one bullet = one leaf; signals sketched, finalized at decompose)

reify leaves filed here (`project_root=/home/leo/src/reify`); DF leaves filed with `project_root=/home/leo/src/dark-factory`. **Every workstream's terminal task wires into milestone 5254.** verify.sh / run_all touch the verify pipeline → full `--scope all` gate (never trivially config-only; `verify-pipeline-guard.sh requires-full-gate`).

**reify:**
- **α — verify.sh nextest retry-subset consumption.** `metadata.files: ["scripts/verify.sh"]`. Signal: scripted repro — record a fixture attempt-0, write a 2-ID filter file, retry runs exactly those 2 (nextest summary), and an OID-mismatch forces a full run with the refusal log line. `grammar_confirmed: N/A`.
- **β — run_all member-subset knob.** `metadata.files: ["tests/infra/run_all.sh"]`. Signal: `REIFY_RUN_ALL_MEMBER_SUBSET="test_x.sh"` runs only that member; drift guard asserts the subset is honored.
- **γ — gui failed-spec forwarding.** `metadata.files: []` (verify.sh gui block + possibly `scripts/gui-test.sh` — let BRE acquire). Signal: gui block with `REIFY_GUI_RETRY_SPECS` runs only the named specs via `gui-test.sh -- <specs>`.
- **δ — reify boundary test + honest marker + drift-guard registrations (INTEGRATION-GATE, reify terminal).** Adds `tests/infra/test_verify_retry_failed_only.sh` **and** its `run-all-classification.manifest` row + `test_no_new_wallclock_upper_bounds.sh` registration in the same diff. `metadata.files: []` (multiple files, same diff). Signal: the infra test green in CI, exercising B1–B6 + emitting the `retry_scope` marker. Depends on α, β, γ. **Wires into 5254.**
- **ε — operator-observable e2e demo (deterministic/operational).** Deliberately flake a merge gate, resubmit via `merge_request(retry_failed_only=True)`, and observe: retry log shows the subset run, `runs.db` shows `retry_scope: failed_only`, the gate lands. `execution_class: operational`. Depends on δ + D1–D5 + `merge-gate-health` W5a (ledger durability) + DF 2821 (classifier). **Wires into 5254.** *(May be folded into 5254's own verification at decompose if the coordinator prefers a single terminal; keep as the named e2e signal regardless.)*

**dark-factory (filed with `project_root=/home/leo/src/dark-factory` at decompose):**
- **D1 — `retry_failed_only` on `merge_request` + threading.** Signal: unit test — the arg reaches the worker's retry path; default False is a no-op.
- **D2 — retry-set construction + verify_env wiring + tree-OID gate.** Signal: unit test — given an attempt-0 fail-fast map, the constructed nextest subset = {failed ∪ not-started}; the reify retry envs + filter files are populated; a rebased tree routes to full verify.
- **D3 — budget policy + M1 autonomous application (category-gated on 2821).** Signal: unit test — a retryable-class attempt-0 triggers a narrowed M1 retry; a deterministic-red class does not.
- **D4 — shadow-baseline map merge.** Signal: unit test — a subset retry's partial map is merged with attempt-0's before storage; no phantom shadow divergence.
- **D5 — `merge_verify` event marker.** Signal: `runs.db` row for a narrowed retry carries `retry_scope: failed_only` + subset sizes.

**Dependency edges to wire at decompose:**
- α, β, γ → δ (δ depends on all three). δ → ε. D1 → D2 → D3; D2 → D4; D2 → D5.
- ε depends on: δ, D1–D5, `dark_factory:2821`, `merge-gate-health` W5a (**file W5a first**).
- δ → 5254; ε → 5254.
- Cross-project edges use the qualified `dark_factory:<id>` form (routes to `metadata.external_deps`).

---

## §11 — Open (tactical) questions

- **`-E` expression size ceiling.** For a large not-started set (fail-fast early in a 20,280-test pass), the `-E 'test(=…) | …'` expression is large but file-sourced and under ARG_MAX. Settle the exact safety-ceiling count (§4.3) and whether to chunk vs full-fallback above it — measure against a real fail-fast-early log.
- **gui subset transport.** `gui-test.sh -- <specs>` already forwards vitest args; confirm the failed-spec paths DF parses from the vitest reporter match `gui-test.sh`'s expected spec-path form.
- **Sidecar vs DF-supplied OID as the source of truth.** §4 uses both (reify sidecar + DF env, must agree). Confirm no race where the lane sidecar is stale from a prior consumer (the lane-lock at acquire should prevent it — verify against `seed-warm-lane.sh --lane-lock`).
- **ε as a standalone leaf vs folded into 5254.** Decide at decompose whether the e2e demo is its own task or 5254's verification body.
- **G7 decompose walk.** `docs/legibility/design-invariants.md` is not yet on disk (advisory walk only at author time — no lock-step duplication, no prose-only contract, no silent fail-soft: §4.3 makes every fallback loud). Run the normative G7 walk at decompose once the doc lands.
