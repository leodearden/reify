# PRD: Merge-gate health — collateral elimination, honest categorization, guard precision

**Status: committed 2026-07-19. Evidence base: `docs/notes/merge-verify-cpu-survey-2026-07.md`. Program milestone: task 5254.**

## 1. Consumer + user-observable surface (G1)

Consumers: (a) the merge gate itself — every `merge_request` lands faster and red gates blame the right party; (b) escalation triage (L1/L2) — `semaphore_timeout` escalations become trustworthy; (c) PRD `verify-retry-failed-only` (retry-set soundness depends on honest categories + a durable FLAKY ledger — cross-PRD dep); (d) the program milestone 5254.

Observable surfaces: runs.db `merge_verify` failure-category distribution; restart-window gate survival rate; the FLAKY ledger file accumulating across reseeds; guard failure messages that name the offending commit.

## 2. Problem statement (measured 2026-07-19)

57.8% of failed merge gates fail in the run_all infra pool, but deep RCA shows almost none are test flakes: ~24–27% of failed local gates are ONE infra race (the `_merge-verify` worktree gutted by restart-collateral — 96.9% failure rate for verifies dispatched within 90s of a restart), and the DF failure categorizer has 0/31 precision on `semaphore_timeout`, burning bounded same-SHA retries on deterministic reds (8 wasted full gates / 2.08 h in 30 h) and letting red-main incidents masquerade as flake storms. Separately, three real offenders landed through the gate while a deterministic guard was red (landing-discipline hole), and new infra tests can bypass the full gate entirely (trivial-pass hole — cause of the 2026-07-19 red-main).

## 3. Sketch of approach

Five workstreams; cross-repo seams follow "reify ships the primitive, DF wires the invocation".

**W1 — restart-collateral elimination (Defect A residual; DF-owned, one reify config leaf).**
- W1a (DF): startup survivor barrier — before the merge worker's first `reset_persistent_merge_worktree`, scan for live processes with cwd/fd/maps under `_merge-verify` and reap (killpg + bounded wait) or defer dispatch. Reify already ships the scan idiom (thin-warm-lane T-checks).
- W1b (reify yaml + DF sweep): enable `verify_use_cgroup_scope` for reify (`config.py:2481` default False; mechanism shipped, `verify.py:2890-2911`) + DF startup sweep of leftover `df-verify-*.scope` units. Kills the GNU-timeout pgroup-escape strands (`verify.py:2846-2849`).
- W1c (DF): contended-lease fail-open (`git_ops.py:1902-1911`) ⇒ bounded longer wait, then DEFER dispatch (requeue), never run unprotected.
- W1d (DF): converge laptop CLI verify span onto `lane_lock_path` (`cli.py:495-503`) — the 2685 treatment for the remote host (5034/5187 class).

**W2 — honest categorization (Defect B; DF-owned, dep on in-flight DF 2821).**
- W2a: dep edge on DF 2821 (positive-anchor `semaphore_timeout` on wrapper-emitted markers; the module docstring's own proposal at `verify_classify.py:198-203`). Do not duplicate.
- W2b (DF): extend DF 2756's `_VERIFY_ENV_BROKEN_RE` (`verify_classify.py:160-163`) with the A-collateral shapes (`couldn't read <file>`, `verify.sh: No such file or directory`, sub-second lint rc=127) — MUST land with/before W2a (A↔B interaction: today the mislabel's free retry self-heals restart collateral; after W2a, unextended patterns would hard-blame innocent branches).
- W2c (DF, conditional): if 2821 ships classifier-only, deterministic-red circuit-breaker at the retry seam (`merge_queue.py:1800-1812`): identical failing-set on retry (reuse `parse_per_test_results` + `^FAILED \S+\.sh` matcher) ⇒ deterministic; same set across different tasks/SHAs ⇒ main-health probe + loud escalation, not infra-hold.

**W3 — gate-bypass closure (reify-owned).**
- W3a: trivial-pass hole — `scripts/verify-pipeline-guard.sh requires-full-gate` must exit 0 for any `tests/infra/test_*.sh` path (manifest addition or glob special-case). Adding/renaming an infra test changes the gate suite itself — definitionally never config-only. Coordinate as dep/amendment of in-flight task 5252 (its files already neighbor the guard); do NOT file a competing task if 5252 absorbs it.
- W3b (DF): landing-discipline hole — three offenders (4861, 4923, 5141) landed while the wallclock guard was deterministically red; 5158's recorded RCA for ep3 was empirically falsified (pre-5148 detector already flagged 5141's asserts). Investigate + close the actual landing path (sticky per-task green, `include_infra=0` lever during storms, manual storm-landing) and add a final-tip full-infra re-verify before merge finalization for infra-guard-red candidates.

**W4 — guard precision + self-identifying collateral (reify-owned, small leaves).**
- W4a: wallclock guard — require a time-measurement signal (time-suffixed variable operand or elapsed/duration lexemes); stop letting prose "wall" + config-constant comparisons match (kills the 6-per-run ep3 false positives). Optional companion: diff-aware blame ("MAIN-LEVEL VIOLATION — pre-existing" vs added-line hard fail).
- W4b: `test_occt_flock_gate` Tests 14/15/22 — replace the `sleep 0.2` holder grace with a causal barrier (poll `flock -n` probe until holders confirmed), the technique Tests 19/21A already use. This is the ONLY confirmed live true flake in the top-14 census (recurred 07-19).
- W4c: `test_run_all_ambient_isolation` — baseline-red SKIP decoupling (stop double-counting every test_run_all red).
- W4d hermeticity bundle: tree-sitter plan-capture adoption (`test_tree_sitter_pipeline.sh:359,382` → `plan_capture_lib.sh`); proc_reaper pre-clean marker suffix (`:945`), load-scaled handshakes, loud lib-missing guards; PTODO `NEW_IN_LIVE` echo on failure (`test_reify_audit_ptodo.sh:251`); occt `_T*_PLAN` captures stop swallowing stderr.
- W4e: run_all emits a distinct `INTERRUPTED (worktree removed)` marker when INFRA_DIR vanishes mid-suite, so collateral self-identifies for W2b instead of producing N per-member FAILED lines.

**W5 — durable FLAKY ledger (cross-repo).**
- W5a (DF): export `REIFY_RUN_ALL_FLAKY_LEDGER=/home/leo/src/reify/data/verify-logs/flaky-ledger.jsonl` in the merge/background verify env (reify already honors the env; today's default dies in the lane on reseed, `run_all.sh:549-558`).
- W5b (reify, optional): extend Phase-2.5 serial-retry + ledger to `intra-run-serial` members (today pool-only, `run_all.sh:1043`), so serial members can be exonerated as flaky at all.

## 4. Pre-conditions / substrate (G3 — verified 2026-07-19)

All file:line anchors verified during the RCA sweep (12 agents); re-verify at decompose time against post-recovery main. Key: DF 2685/2315/2599 landed (lock convergence); DF 2821 in-progress claiming `verify_classify.py`; DF 2748/2756 done (insufficient by their own documented gaps); reify 5221/5223 done; 5251 in-progress (FALSE-PREMISE: its load-flake framing is contradicted by the census — rescope to its real residue per feedback_false_premise playbook); 5252 in-progress (manifest rows + throughput note bump in-branch); 5166 infra-hold (MG-B5 sentinel fix rides its existing dry-run proposal via /unblock — NOT a leaf here).

## 5. Resolved design decisions

- No dynamic step reordering (oracle advantage ≤15s/failed gate; static order already contract-guarded). The only ordering change (lint-before-release) belongs to PRD `merge-gate-riders`, not here.
- No de-flake work on `test_verify_semaphore_e2e`, `test_cpu_admit`, `test_agent_cargo_shim`, `test_cpu_load_governance` — census shows zero live defects; their reputations were collateral + categorizer artifacts.
- Measurement-class tests stay host-exclusive (empirically settled); logic tests use fixture-injected PSI.
- W2b lands with/before W2a (interaction ordering is a hard dependency edge, not prose).

## 6. Out of scope

retry_failed_only (PRD `verify-retry-failed-only`); compile-cost work (PRD `merge-gate-compile-cost`); riders incl. lint-order swap and release-pass skip (PRD `merge-gate-riders`); offline-lane enablement (item-4 checklist, config-only); the #5120 registry_drift fix itself (red-main recovery, in flight).

## 7. Cross-PRD seams (G4)

| Seam | Owner |
|---|---|
| FLAKY-ledger durability (W5) | THIS PRD produces; `verify-retry-failed-only` consumes (dep edge) |
| Failure categories (W2) | DF 2821 produces the classifier; THIS PRD owns W2b/W2c riders + the dep edges |
| `tests/infra` full-gate coverage (W3a) | task 5252 (extend/dep — no competing task) |
| MG-B5 sentinel | task 5166's existing proposal via /unblock |
| Sweep env_transient retry-forever | W2 (the sweep's categorizer path) — `merge-gate-riders`' release-skip precondition consumes the resulting sweep-health signal |
| release-sensitive list | PRD `merge-gate-compile-cost` (not touched here) |

## 8. Decomposition plan (one bullet = one leaf; signals sketched, finalize at decompose)

DF-side leaves filed with `project_root=/home/leo/src/dark-factory`; reify leaves here. Every leaf wires into milestone 5254.
- W1a startup survivor barrier — signal: induced-restart harness test shows post-restart verify survives with a planted straggler; runs.db restart-window failure rate → ~0 over next 20 restarts.
- W1b reify yaml `verify_use_cgroup_scope: true` + DF scope sweep — signal: `df-verify-*.scope` visible during a live verify; kill test reaps the full tree.
- W1c lease-timeout defer — signal: unit test = contended lock ⇒ requeue event, zero unprotected runs.
- W1d laptop lock convergence — signal: laptop verify holds `<lane_dir>.lock` (probe via flock -n failure during run).
- W2b ENV_TRANSIENT pattern extension — signal: replayed 5164/5071 logs classify env_transient, not semaphore_timeout/test_failure. (Dep: lands with/before 2821's flip.)
- W2c circuit-breaker (conditional on 2821 scope) — signal: replayed #5120 pair yields deterministic + main-health escalation, no second full gate.
- W3a verify-pipeline-guard infra-test coverage — signal: `requires-full-gate tests/infra/test_x.sh` exits 0; new-infra-test branch takes full gate in a fixture repro.
- W3b landing-discipline closure — signal: fixture branch with guard-red cannot finalize; incident-replay documentation of which lever leaked 5141.
- W4a wallclock precision — signal: ep3's 6 false-positive asserts pass un-annotated; a real `elapsed<N` still fails.
- W4b occt holder barrier — signal: 200× looped Tests 14/15/22 under load green; ledger shows no new occt_flock FLAKY events over a trailing week.
- W4c ambient_isolation decoupling — signal: with test_run_all forced red, ambient_isolation reports SKIP-baseline-red, one FAILED name not two.
- W4d hermeticity bundle — signal: per-item unit demos (plan-capture retry under SIGPIPE injection; marker-suffixed pre-clean no longer matches foreign fixture; PTODO failure output lists fingerprints).
- W4e INTERRUPTED marker — signal: simulated INFRA_DIR removal mid-suite yields the marker + distinct exit path.
- W5a ledger env wiring (DF) + W5b serial-member Phase-2.5 (reify) — signal: ledger file exists in main checkout and gains entries across two reseeded merge verifies; a serial member can appear as FLAKY.
- Admin: rescope 5251 (update_task, per its real residue); confirm 5252 carries the manifest rows + note bump (no-op leaf if already landed by recovery).

## 9. Open (tactical) questions

- W1a scan placement: DF harness startup vs merge-worker first-dispatch hook (prefer the latter — narrower).
- W1b rollout: flip for merge role only first, or all roles at once?
- W2c signature store: reuse shadow-baseline storage or a small sidecar table?
- W3b needs its own mini-RCA of the three landing paths before the fix leaf is final — keep as investigate-then-fix pair.
- W4a lexeme spec: exact operand grammar (reuse `_wc_var_sfx` branch) — settle in-leaf.
