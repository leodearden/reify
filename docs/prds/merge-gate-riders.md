# PRD: Merge-gate riders — lint-phase ordering, run_all content-addressed skip, sweep-gated delta-conditional release pass

**Status:** active (author 2026-07-19). Program: merge-verify-cpu-reduction; release-gate milestone task **5254**.
**Evidence base:** `docs/notes/merge-verify-cpu-survey-2026-07.md` (12-agent survey, 2026-07-19; all savings figures below are quoted from it, not re-derived). All file:line anchors re-verified against main `fc47d707b3` at author time.
**Shape:** B + H (verify.sh is the highest-blast-radius script in the repo; every behavior change lands with its contract/doc/drift-guard updates in the SAME diff).

## 1. Goal

Three ratified riders on the merge gate (operator-approved 2026-07-19; the third operator-signed "Include, sweep-gated"). Each trims wasted merge-gate wall/CPU without narrowing what a landed main has been proven against:

1. **Lint phase ordered before the test phase in the DF merge pipeline** — failure-attribution: lint p/c=60 vs release nextest p/c=21 (survey §Failure attribution); today a lint-only-red merge burns the full ~50-min test phase before the ~4-min lint phase runs.
2. **run_all content-addressed member skip (merge tier)** — ~117 infra tests (~5–8 min) run on every merge; most are drift guards over verify-pipeline artifacts that did not change since the last green main run.
3. **Delta-conditional release-pass skip** — the ~17-min release nextest pass (at ~27 gates/day) is skipped when the merge delta's reverse compile closure ∩ release-sensitive set = ∅, **activated only once the background main-tip sweep is demonstrably healthy** (green on cadence + alerting on failure). The coverage claim of this rider is **conditional on the sweep backstop — never absolute**.

## 2. Consumers (G1)

- **Every merge-gate run** consumes all three mechanisms (DF merge pipeline via `dark-factory-orchestrator.yaml` commands; manual `scripts/land.sh` path via `hooks/pre-merge-commit` — the manual path deliberately gets **no** skips, see fail-open rules).
- **DF classification + mining** (`data/orchestrator/runs.db`, `data/verify-logs/`) consume the plan markers and per-member skip log lines.
- **The release-skip activation leaf** consumes the sweep-health signal produced by PRD `merge-gate-health.md` W2 (DF 2821 + DF 2831) — a real dependency edge, not prose.
- No in-engine (reify kernel/dispatch) seam is touched; the engine-integration catalogue is N/A.

## 3. Rider 1 — lint before the expensive test tail

### 3.1 The two variants, evaluated (design question resolved here)

The survey's supportable-swap figure (~22s/failed gate) prices the **reify-side intra-plan variant**: emit the clippy block inside the merge test-phase plan between the debug and release nextest passes (order by descending fails-per-CPU-hour: debug 131 → lint 60 → release 21). Evaluation:

- **Reify-side (intra-plan)**: the DF merge pipeline runs `test_command` / `lint_command` as **separate sequential processes** (`dark-factory-orchestrator.yaml:143-145`, `concurrent_verify: false` at `:159`; DF executes them in hard-coded test→lint→type order in `verify.py`'s sequential branch). Injecting clippy into the *test* action's plan for role=merge would (a) double-run clippy (the later lint phase re-runs it; mostly fingerprint-warm but not free), (b) break the test/lint action separation that `hooks/pre-merge-commit` and the plan-shape contracts (`tests/infra/test_verify_failfast_order.sh`, `test_occt_flock_gate.sh` Tests 17/17b at `:260-294`) are built on, and (c) buy only the release-pass tail (~22s/failed gate expected). The cache-warmth question (clippy check-profile artifacts between the two nextest passes) becomes moot under the chosen variant, and is recorded here as the rejected branch's open risk rather than resolved empirically.
- **DF-side (phase ordering)**: reorder the sequential phases **lint → test** and **short-circuit** (skip test+type as `CheckRun.skipped`) when lint fails, for the **merge role only**. This catches the lint-red class (4.8% of failures, survey) after ~4 min instead of after the ~50-min test phase — a strictly larger expected saving than the intra-plan swap — with **zero reify plan-shape change** (the failfast-order and occt 17/17b contracts stay byte-identical).

**Decision: DF-side phase ordering, merge-role-scoped, config-gated.** Task-role verifies keep test-first: the inner TDD loop's dominant failure is the test phase (debug nextest p/c=131 ≫ lint 60), and TDD agents want test output every iteration. Background-role sweeps keep test-first too (coverage, not latency). DF ships the mechanism behind a per-project module-config knob (`sequential_lint_first`, default off) applying only when `concurrent_verify: false` and role=merge; reify's yaml flips it on.

### 3.2 Semantics

- Order for merge-role sequential verifies: lint → test → type. On lint rc≠0: test and type legs are recorded as skipped (`CheckRun.skipped` exists for exactly this shape); the attempt fails; classification takes the lint path (lint/type legs already cannot classify `env_transient` by construction — DF `verify_classify` note).
- Green gates: both phases still run; total wall unchanged (sequential sum is order-invariant modulo minor cache interactions).
- Failure attribution on a lint-red merge loses the test-phase log for that attempt — accepted: the gate fails either way and the branch must fix lint first regardless.

## 4. Rider 2 — run_all content-addressed member skip (merge tier)

### 4.1 The soundness inversion (why the existing map is not reused for skipping)

`scripts/verify-pipeline-infra-tests.txt` maps artifact → test-glob for **selective inclusion** at task tier (over-running is safe, so per-test artifact sets may be incomplete). Skipping inverts the soundness direction: a skip is sound only if the declared artifact set is **complete** for that member. Therefore skip eligibility comes from a **new, explicitly complete closure manifest**, not the inclusion map:

- **`tests/infra/run-all-skip-closures.manifest`** — one row per skip-eligible member: `<test_basename> <path-or-glob> [<path-or-glob>…]` declaring the member's complete tracked-file dependency closure. Implicit members of every closure (by construction, not per-row): the member's own file, `tests/infra/test_helpers.sh`, `tests/infra/run_all.sh`, `tests/infra/run-all-classification-lib.sh`, `tests/infra/load_tolerance_lib.sh`, and the classification/ambient-vars manifests. A member with **no row never skips** (fail-open). Initial rows are conservative (broad globs like `scripts/*.sh tests/infra/*` are legal; a row equivalent to "everything" is equivalent to no skip on any real delta and still correct).
- **Under-declaration drift guard** (same diff): a check that every literal repo-relative path referenced in a manifested member's source text is covered by its declared closure; failure is a red gate member, not a silent narrow.

### 4.2 Skip decision (content-hash, not mtime)

Git is the content-addressed store. A member skips iff **all** hold:

1. The skip flag is present (see activation) and `DF_VERIFY_ROLE=merge`.
2. A state ledger entry exists giving the member's **last-executed-green main sha**.
3. `git diff --quiet <green-sha> HEAD -- <closure paths>` (tree content byte-identical) **and** the working tree is clean for those paths.
4. The member has a closure row (fail-open otherwise).
5. The backstop is not due: the member has executed within the last `REIFY_RUN_ALL_SKIP_MAX_AGE_HOURS` (default 24) **and** within the last `REIFY_RUN_ALL_SKIP_MAX_MERGES` (default 25) merge-tier runs.

Every decision is logged per member, no silent caps:
`SKIP (content-clean): <member> green=<sha> closure_ok` / `RUN (backstop-due): <member>` / `RUN (delta): <member> touched=<path>` / `RUN (unmapped): <member>`. A corrupt/absent state file ⇒ one loud line + full run (fail-open storm escape).

### 4.3 State ledger + two-key activation (the flaky-ledger amnesia lesson)

State (per-member `{member, green_sha, last_executed_at, merges_since}`) lives at the path in `REIFY_RUN_ALL_SKIP_STATE` — a **main-checkout durable path** wired in `dark-factory-orchestrator.yaml` `verify_env` (precedent: `REIFY_RUN_ALL_FLAKY_LEDGER` at yaml `:171`; the in-lane default dies on reseed, survey §FLAKY ledger). Updated only after a run where every executed member passed (executed members get the new sha; skipped members keep their entries).

The **opt-in flag** `REIFY_RUN_ALL_CONTENT_SKIP=1` is set **narrowly on verify.sh's merge-tier run_all plan line** (the `REIFY_AUDIT_NO_COLD_BUILD=1` idiom at `verify.sh:~1575-1605`), **never** ambient `verify_env` — so nested/fixture run_all invocations (`test_run_all.sh`, `test_run_all_ambient_isolation.sh` hostile loop) can never inherit skipping. Both new env vars get rows in `tests/infra/run-all-ambient-vars.manifest` (set-equality guard, task 5152) in the same diff.

`hooks/pre-merge-commit` (manual `land.sh` path) passes no flag ⇒ always full pool. Role=background (main-tip sweep) never skips ⇒ the sweep re-runs the full universe on cadence, a second backstop on top of §4.2(5).

### 4.4 INV-5 restated (INV-5′)

Old INV-5 (`docs/prds/run-all-pool-contention-tiering-fix.md:126`, `tests/infra/test_run_all_tiering.sh`): the full pool runs exactly once, at merge. New invariant:

> **INV-5′:** the full pool remains merge-tier-resident (plan shape unchanged); **within** the merge-tier invocation, every member runs on every merge whose delta (vs. that member's last green main run) touches its declared closure; every member without a declared closure runs on every merge; and every member runs at least once per `MAX_MERGES` merge-tier runs and once per `MAX_AGE_HOURS` regardless of deltas. Skips are per-member, content-hash-based, individually logged, and fail-open.

The tiering-PRD doc amendment, `test_run_all_tiering.sh` comment/contract updates, and the new skip-logic drift tests land in the **same diff** as the run_all.sh behavior change. The plan-shape assertions in `test_verify_failfast_order.sh` (`run_all.sh` line content) are re-checked in the same diff (the line gains one env token).

## 5. Rider 3 — delta-conditional release-pass skip (sweep-gated)

### 5.1 Mechanism

Seam: `scripts/verify.sh` release-pass emission (`add_test_passes`, release branch `:1218-1252`) + `scripts/release-scope-lib.sh` gaining a reverse-closure predicate that **reuses** `_reify_compile_closure` (`scripts/occt-scope-lib.sh`, the single shared dev-dep-accurate closure primitive; non-transitive dev-dep semantics per task 4938 — no second implementation).

When `REIFY_RELEASE_DELTA_SKIP=1` **and** `DF_VERIFY_ROLE=merge`:

1. Derive the merge delta: first-parent diff of the speculative merge commit (DF merge lane), or `HEAD..MERGE_HEAD` under `hooks/pre-merge-commit`. Underivable ⇒ **run the pass** (fail-open).
2. Map delta files → crates via the `affected-crates-lib.sh` conventions: any C4 global file (root `Cargo.toml`/`Cargo.lock`, `.cargo/**`, `tree-sitter-reify/**`, toolchain) ⇒ ALL ⇒ run. Any file not attributable to a crate and not classed non-crate (docs/gui-frontend/tests-infra per `_is_noncrate`) ⇒ run (fail-wide, C5).
3. Reverse compile closure of the changed crates ∩ `release_declared_set` (dynamic read of `scripts/release-sensitive-crates.txt` — membership owned by PRD `merge-gate-compile-cost`, see §8): empty ⇒ **omit the release nextest pass emission** and emit the plan marker **`RELEASE-PASS: skipped (delta-clean)`**; non-empty ⇒ emit as today.
4. The release **lib prebuild stays unconditionally** (`cargo build --release -p reify-cli`, `verify.sh:1575` — run_all's audit/CLI gate tests consume `target/release/reify`). Only release test-binary build+exec is skipped.
5. Role=background **never** skips (the sweep IS the backstop); role=task unchanged (release pass only runs under `--profile both` anyway); knob default **off**.

### 5.2 C2 carve-out (same diff as the verify.sh change)

`docs/prds/verify-scope-contract.md` C2 ("the merge gate never narrows", enforced at `verify.sh:608-616`, guarded by `test_verify_scope.sh:740-870`) gains a **profile-axis carve-out**: on a delta-clean merge, release-profile re-execution is deferred from merge-time to the **main-tip background sweep** (role=background, `--profile both`, running the full release-sensitive set on cadence — landed with task 5210). The carve-out text must state the conditionality explicitly: *a release-only breakage introduced by a delta-clean merge is caught by the sweep at main-tip cadence (hours), not at the gate, and only while the sweep is green-on-cadence and alerting on failure*. The crate-set axis (C1/C2 branch-diff narrowing) is untouched. The in-code comment block at `verify.sh:~1226-1236` ("do not fix this by narrowing the release pass") is amended in the same diff to name this carve-out and its precondition.

New drift guard (new file `tests/infra/test_verify_release_delta_skip.sh` + `run-all-classification.manifest` row, same diff — deliberately NOT edits to `test_verify_scope.sh`, whose MG-B5 region is owned by in-flight task 5166): fixture-injected delta (env override, hermetic — the REIFY_AFFECTED_CRATES_OVERRIDE idiom) asserting: (i) knob off ⇒ pass present (Tests 17/17b preserved); (ii) knob on + delta-clean fixture ⇒ marker present, release nextest line absent, reify-cli release prebuild still present; (iii) knob on + release-sensitive-touching fixture ⇒ pass present, no marker; (iv) knob on + role=background ⇒ pass present; (v) knob on + underivable delta ⇒ pass present. The test manages the knob env explicitly (never inherits ambient).

### 5.3 Activation precondition (HARD, dependency-edged)

The skip may only activate (yaml `REIFY_RELEASE_DELTA_SKIP=1`) once the background sweep is:
(a) demonstrably **green on cadence over a trailing window** (runs.db: background-role verifies over ≥7 days at the configured cadence, green), and
(b) **alerting on failure instead of silently retrying** — today the sweep is wedged retrying `env_transient` forever (DF categorizer bug; survey §DF categorizer). The fix is DF-side and owned by PRD `merge-gate-health.md` W2: **DF 2821** (positive-anchor classifier) + **DF 2831** (ENV_TRANSIENT collateral-shape extension). The activation leaf carries **real `add_dependency` edges** on both — never prose.

The activation leaf's verification protocol re-checks (a)+(b) live before flipping, and observes both G2 signals post-flip (a real delta-clean merge landing with the marker; a release-sensitive-touching merge with the pass present). If (b) turns out not to be fully delivered by 2821+2831 (alerting is downstream of honest classification), the leaf **escalates back to the merge-gate-health seam** rather than activating.

## 6. Contract section (H)

| # | Invariant | Enforced by |
|---|---|---|
| K1 | `sequential_lint_first` applies only when `concurrent_verify: false` ∧ role=merge; default off; lint-red short-circuits test+type as skipped; gate still fails | DF unit tests (task α); reify yaml seam guard (task β) |
| K2 | Reify merge/test plan shape is byte-unchanged by rider 1 | `test_verify_failfast_order.sh`, `test_occt_flock_gate.sh` 17/17b (untouched, stay green) |
| K3 | run_all skip: per-member, content-hash (git tree compare), logged per decision, fail-open on unmapped/underivable/corrupt-state/absent-env; own-file change always runs; INV-5′ backstop (24h / 25 merges) | new skip drift tests + `test_run_all_tiering.sh` (same diff, task γ) |
| K4 | Skip opt-in flag is plan-line-narrow (never ambient verify_env); state path env is ambient-data-only; both rows in `run-all-ambient-vars.manifest` | ambient-isolation set-equality guard (task 5152's test) + task γ diff |
| K5 | Release skip: role=merge only ∧ knob on ∧ derivable delta ∧ empty closure∩sensitive-set; marker string `RELEASE-PASS: skipped (delta-clean)` is a frozen machine-greppable constant; reify-cli release prebuild unconditional; background never skips | `test_verify_release_delta_skip.sh` (task ε, same diff) |
| K6 | C2 profile-axis carve-out documented with explicit sweep-conditionality; crate-set axis untouched | `verify-scope-contract.md` amendment (same diff, task ε) |
| K7 | Release-skip activation gated on sweep green-on-cadence + alert-on-failure | dependency edges ζ→{ε, dark_factory:2821, dark_factory:2831} + ζ's verification protocol |
| K8 | Closure reuse: reverse-closure via `_reify_compile_closure` only (no lock-step duplicate) | code review anchor in ε; drift caught by `test_affected_crates_lib.sh` family |

## 7. Boundary-test sketch (two-way)

| Scenario | Side | Pre | Post |
|---|---|---|---|
| Lint-red merge, knob on | DF | sequential merge verify, lint rc≠0 | test/type legs skipped, attempt fails, classification = lint path |
| Lint-green merge, knob on | DF | lint rc=0 | test+type run; result identical to today |
| Knob on, task role | DF | role=task | order unchanged (test first) |
| Yaml seam drift | reify | `lint_command`/`concurrent_verify`/knob rows in yaml | seam guard red if lint_command emptied/reordered semantics broken |
| Member skip, clean closure | reify | state entry green_sha, closure unchanged | `SKIP (content-clean)` line, member not executed, suite green |
| Member runs on delta | reify | closure member file changed vs green_sha | `RUN (delta)` line, member executes |
| Unmapped member | reify | no closure row | always executes |
| Backstop due | reify | last exec > 24h / > 25 merges | `RUN (backstop-due)`, member executes |
| Corrupt/absent state | reify | garbage state file / env unset | loud line + full pool run |
| Nested run_all | reify | fixture invocation without plan-line flag | no skips inside fixtures |
| Release skip, delta-clean | reify | knob on, fixture delta ∩ sensitive = ∅ | marker present, release nextest absent, prebuild present |
| Release pass, sensitive delta | reify | fixture delta touches sensitive closure | pass present, no marker |
| Background sweep | reify | role=background, knob on | pass present (never skips) |
| Live activation | both | ζ protocol | real merge log shows marker; sweep health evidenced in activation record |

## 8. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/merge-gate-health.md` (landed) | consumes | sweep-health signal (DF 2821 + DF 2831 categorizer/alerting) | merge-gate-health (W2) | edges wired at decompose (ζ) |
| `merge-gate-compile-cost` (sibling session, brief stage) | consumes | `scripts/release-sensitive-crates.txt` **membership** — that PRD owns ALL membership changes; THIS PRD owns skip logic only and reads the declared set dynamically | compile-cost PRD | no hard edge (dynamic read) |
| Task 5166 (infra-hold) | adjacent | `test_verify_scope.sh` MG-B5 sentinel + reify-ir release-list add | 5166 (via its /unblock proposal) | avoided: rider-3 tests live in a new file |
| `verify-retry-failed-only` (sibling session) | none direct | — | — | markers land in verify-logs both consume |
| Milestone 5254 | produces | terminal leaves wired as milestone deps | this PRD | wired at decompose |
| `run-all-pool-contention-tiering-fix.md` | amends | INV-5 → INV-5′ restatement | this PRD (γ, same diff) | queued |
| `verify-scope-contract.md` | amends | C2 profile-axis carve-out | this PRD (ε, same diff) | queued |

## 9. Decomposition plan

All riders touch verify-pipeline files (`scripts/verify-pipeline-paths.txt` manifest) — **full gate, never trivial-pass**. Every new `tests/infra/test_*.sh` carries its `run-all-classification.manifest` row in the same diff (gate-test registration rule).

- **α (dark-factory, intermediate → β):** `sequential_lint_first` module-config knob (default off) + merge-role sequential lint-first ordering + lint-red short-circuit (`CheckRun.skipped`) in `verify.py`; DF unit tests for K1 + classification path. *Signal:* DF tests demonstrate order + short-circuit + gate-still-fails; unlocks β. Files: `orchestrator/src/orchestrator/verify.py`, `orchestrator/src/orchestrator/config.py`.
- **β (reify, leaf):** flip `sequential_lint_first: true` in `dark-factory-orchestrator.yaml` + reify-side seam drift assertions (yaml contract: lint_command present/full-clippy, `concurrent_verify: false`, knob row) + safe restart (`scripts/orchestrator-redeploy-restart.sh`). *Signal:* a lint-red merge attempt's archived logs (`data/verify-logs/<task>/`) show lint phase first and test leg skipped; green merges unchanged. Depends: `dark_factory:α`.
- **γ (reify, intermediate → δ):** run_all content-addressed skip engine + `run-all-skip-closures.manifest` + under-declaration guard + backstop + per-member logging + plan-line flag in verify.sh merge block + ambient-vars manifest rows + INV-5′ restatement (tiering PRD doc + `test_run_all_tiering.sh`) + skip drift tests — one diff. *Signal:* fixture-driven: state fixture + unchanged closures ⇒ `SKIP (content-clean)` lines and members not executed; touched artifact ⇒ member runs; backstop-due ⇒ runs; absent state ⇒ full pool. Files: `tests/infra/run_all.sh`, `scripts/verify.sh`, `tests/infra/test_run_all_tiering.sh`, `tests/infra/run-all-ambient-vars.manifest`, `docs/prds/run-all-pool-contention-tiering-fix.md`.
- **δ (reify, leaf):** activation: wire `REIFY_RUN_ALL_SKIP_STATE` (main-checkout durable path under `data/verify-logs/`) in yaml `verify_env` + restart + observe. *Signal:* a real merge-gate log shows per-member SKIP lines with shas; state file accumulates on the main checkout; run_all wall reduction vs the ~390s baseline recorded. Depends: γ.
- **ε (reify, intermediate → ζ):** release-pass delta-skip implementation per §5.1 + C2 carve-out doc + verify.sh comment amendment + `tests/infra/test_verify_release_delta_skip.sh` (+ manifest row), knob default off — one diff. *Signal:* the five hermetic plan-fixture scenarios of §5.2 pass; Tests 17/17b stay green with knob off. Files: `scripts/verify.sh`, `scripts/release-scope-lib.sh`, `docs/prds/verify-scope-contract.md`.
- **ζ (reify, leaf):** sweep-gated activation per §5.3: verify (a)+(b), flip `REIFY_RELEASE_DELTA_SKIP=1` in yaml `verify_env`, restart, observe both live signals. *Signal:* a real delta-clean merge lands with `RELEASE-PASS: skipped (delta-clean)` in its archived plan; a release-sensitive-touching merge shows the pass present; the activation record cites the sweep-health evidence. Depends: ε, `dark_factory:2821`, `dark_factory:2831`. Escalates instead of activating if alerting is not live.

Milestone wiring: 5254 depends_on β, δ, ζ (reify terminals) and `dark_factory:α`.

G7 note (design-invariants walk; `docs/legibility/design-invariants.md` absent on main — walked against the five slugs): both skip mechanisms are suppressors and carry storm escapes (fail-open ladder, backstop cadence, role scoping, loud per-decision lines, kill knobs); all contracts are machine-checked same-diff (K1–K8); skip decisions log structured facts (shas, paths); decisions corroborate git content state, never mtime/snapshot; closure logic reuses the single shared primitive. No waivers required.

## 10. Out of scope

- `release-sensitive-crates.txt` membership changes (owned by `merge-gate-compile-cost`).
- Categorizer/sweep-alerting fixes (owned by `merge-gate-health` W2 / DF 2821 / DF 2831 — dep edges only).
- Dynamic step reordering inside verify.sh plans (rejected by the survey: oracle advantage ≤15s/failed gate; static order stays contract-guarded).
- retry_failed_only mechanics (PRD `verify-retry-failed-only`).
- The trivial-pass hole fix (task 5256 / DF 2823 / reify 5252) — complementary; rider 2 never skips a member whose own file changed, by construction.

## 11. Open questions (tactical)

1. **Backstop defaults** (24h / 25 merges): confirm against observed merge cadence during γ; both are env-tunable knobs, defaults chosen conservative. Decide in γ.
2. **Skip-state schema** (flat JSON vs JSONL append): implementer's choice in γ; must survive concurrent merge-tier writers on the two-host setup (flock or last-writer-wins with per-member rows).
3. **Where β's yaml seam assertions live** — extend `tests/infra/test_release_mode_in_test_command.sh` (existing yaml-contract test, no new manifest row) vs a new file (+ row). Decide in β.
4. **δ measurement source** — runs.db duration delta vs archived-log wall lines. Decide in δ.
5. **Initial closure-manifest coverage** — which of the ~117 members get rows in γ vs follow-up accretion (start with the biggest wall contributors; unmapped members simply keep running).
6. **DF knob spelling** (`sequential_lint_first` module-config bool vs role-list) — α's implementer follows DF config conventions.
