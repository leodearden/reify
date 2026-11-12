# confusion census 2026-11-12

Project: reify

## Method

```yaml
run_id: census-reify-20261112
as_of_sha: 2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4
since: 71ac9dfa85006956733a95bbeb6967e28075ee54
evidence:
  window:
  - '2026-10-13T00:00:00+00:00'
  - '2026-11-12T00:00:00+00:00'
  sessions_enumerated: 3520
  skipped_coded: 397
  skipped_zero_signal: 1765
  mined: 40
  ledger_rows: 548
verification:
  confirmed: 3
  weakened: 0
  refuted: 9
  unverified: 0
cost:
  miner_calls: 40
  verify_calls: 16
  synthesis_calls: 1
  probe_calls: 3
  embedding_calls: 1
  wall_clock_secs: 1882.461
inputs_consumed:
- review-reify-20261015
- review-all-reify-20261010
extra:
  inputs_consumed_note: '/hotspot-survey: no record matching plans/bug-hotspot-survey-<YYYY-MM-DD>[-<n>]-full-findings.json'
  screened:
    title: 0
    session: 0
    key: 0
    cross_instrument: 0
  attached: 0
  adjudicated: 14
  ledger_created_this_run: false
  ledger_state: ok
  ledger_pruned: 114
  ledger_error: null
  verify_normalisations:
    anchor_rewritten: 0
    anchor_from_remediation: 0
    anchor_from_title: 0
    tag_dropped: 0
    severity_missing: 0
    severity_substituted: 0
    severity_reason_missing: 0
    route_defaulted: 0
    remediation_rejected: 0
    reason_missing: 0
  reverify:
    due: 4
    offered: 4
    gone: 3
    present: 1
    inconclusive: 0
    deferred: 0
  slug_rejections:
    count: 0
    values: []
    values_omitted: 0
```

## Saturation

- batches: 2
- stop reason: saturated
- operator batch cap: 50 batch(es) (not reached -- mining stopped by: saturated)
  - batch 0: dup_rate=0.90 (total=20, succeeded=20, failed=0, saturated=True)
  - batch 1: dup_rate=1.00 (total=20, succeeded=20, failed=0, saturated=True)

## Screened

- per rule: title 0, session 0, key 0, cross_instrument 0
- inputs consumed: review-reify-20261015, review-all-reify-20261010
- inputs note: /hotspot-survey: no record matching plans/bug-hotspot-survey-<YYYY-MM-DD>[-<n>]-full-findings.json

## Adjudication

- model text-embedding-3-small; attach at cosine >= 0.9, cluster at >= 0.85; at most 10 queue cluster(s) per run
- embeddings: 1 request(s), 9 embedded, 901 cached, 19 pruned
- queue depth: 779 pending candidate(s) in 775 cluster(s)
- processed this run: 10 cluster(s), 14 candidate(s)
- rejected: "Watcher re-arm background task repeatedly times out or is killed, forcing continuous re-triggering all session" (cand-20260721-6) for 2 candidate(s) across 2 session(s)
- rejected: "Escalation-watcher blind-retries a wedged escalation MCP endpoint instead of backing off" (cand-20260713-5) for 2 candidate(s) across 2 session(s)
- rejected: "Escalation-watcher bounded foreground wait is repeatedly SIGKILL'd; agent retries identically instead of adapting" (cand-20260713-4) for 2 candidate(s) across 2 session(s)
- rejected: "Escalation-watcher session repeatedly hits ModuleNotFoundError across separate escalations" (cand-20260704-14) for 2 candidate(s) across 2 session(s)
- rejected: "Renamed type swapped into a prior-private helper list in a PRD brief, carrying a stale visibility claim forward" (cand-20261108-1) for 1 candidate(s) across 1 session(s)
- rejected: "Shared tracked-corpus listing assumed to have a single consumer, so a fix narrowed to one reader leaves a second repo-wide sweep ungated" (cand-20261107-2) for 1 candidate(s) across 1 session(s)
- rejected: "Task-completion claim reported without attribution to the declared task ID, blocking the task" (cand-20261107-1) for 1 candidate(s) across 1 session(s)
- verified: "Ratchet failure points to a --help diagnostic the agent cannot run under don't-ask permission mode" (cand-20261106-1) for 1 candidate(s) across 1 session(s)
- rejected: "Task completion claimed via precondition commit with no declared attribution basis; operational step run out-of-band by unsandboxed architect" (cand-20261105-1) for 1 candidate(s) across 1 session(s)
- verified: "Post-merge gui-script verification fails on a cold cargo registry, and the remediation is blocked by Bash permission denial" (cand-20261102-1) for 1 candidate(s) across 1 session(s)

## Verification

- handed all 2 novel cluster(s) to the verifier; operator verify cap: 150 (not reached).

## Origin x Manifestation Matrix

| origin \ manifested | implement | verify |
| --- | --- | --- |
| implement | 1 | 1 |
| unknown | 0 | 1 |

## Synthesis

- applied: 1 phase refinement(s) to 1 cluster(s) and 1 new entr(ies); 0 correction(s) applied, 0 skipped
- standing, no new evidence: 122
- invariant violations this run: none
- unkeyed entries re-observed: 3

### New

- fk-8439f1f175a4 "git grep invoked with context flag after pathspec and a pathspec placed before `--`, failing with exit 128": In one session the agent wrote `git grep -n PATTERN crates/ -- ':!*/tests/*'` with `-A12` appended after the pattern and path. Git read the path before `--` as a revision and exited 128 with 'unable to resolve revision: crates/', which aborted the compound command, so the intended code read did not happen. Under heuristic 10 (clear invariants, informatively, redundantly, uniformly enforced), the breach was loud and named the offending value, but the message does not state the argument order git expects: options, pattern, revisions, `--`, pathspecs. The verdict found no file in this tree that teaches the wrong form; the only hits were the census plans and codebook that record the mistake. The cost seen was one wasted call before the agent retried with the right order.
- fk-c87a6b0141c7 "Ratchet failure points to a --help diagnostic the agent cannot run under don't-ask permission mode": The assertion message in `workspace_module_sccs_match_the_committed_baseline` carries no inline explanation of GREW or STALE. It sends the reader to `python3 scripts/assert-module-scc.py --help` for their meaning and for how to lower a baseline row. Under heuristic 11 (SPOT — single point of truth), that explanation has one home and both the message and CLAUDE.md point at it; the verdict confirmed the pointer is accurate. Under heuristic 10 (clear invariants, informatively, redundantly, uniformly enforced), the red ratchet names the invariant, but its informative half is reachable only by executing the script. In the one session, Bash calls to the script were reported denied under don't-ask mode, leaving the agent without the documented fix path. That denial comes from the agent harness and could not be observed from the tree.
- fk-b86df78cf29f "Post-merge gui-script verification fails on a cold cargo registry, and the remediation is blocked by Bash permission denial": `_rgs_tauri_features_under` in `tests/infra/test_run_gui_scripts.sh` runs `cargo metadata --locked --offline` with stderr sent to /dev/null and returns a bare 1 on any failure. A cold registry is therefore indistinguishable from a real resolve regression such as Cargo.lock drift or a feature typo. Under heuristic 10 (clear invariants, informatively, redundantly, uniformly enforced), the FAIL line from `_t36_resolve` prescribes a remedy (`cargo fetch --locked` or `scripts/setup-dev.sh`) but drops the offending values that would say which cause occurred. Under heuristic 1 (informative names), the environmental failure surfaces as a `test_failure`, a label that reads as a code regression. In the one session this happened during post-merge verification, and the prescribed warm-up was reported blocked by a Bash permission denial. That denial comes from the session's permission tooling and was not verifiable from the tree.

### Re-observed

- fk-e536a9dd5f09 "Plan.json or task-meta iteration artifact missing at bootstrap" (gained daed6905-70d6-48c2-9110-a426f3975484): no change stated

### Phase Refinements

- fk-b86df78cf29f: origin implement, manifested verify (The verified part of the finding is in how the infra test helper is written: it discards cargo's stderr and collapses every failure to a bare return code. That places the origin in implementation of the test, where the stamp says unknown. The failure surfaced during post-merge verification, so the manifested phase is unchanged.)

### Notes

- unverified: Two of this run's new findings (fk-c87a6b0141c7 and fk-b86df78cf29f) may share one harness-side condition: a don't-ask permission mode denying Bash, so that a remedy prescribed by a failure message cannot be run. (evidence: Both causes describe a Bash denial in a don't-ask-mode session blocking the documented fix path. Both verdicts state the denial comes from the harness or permission tooling and could not be observed from the tree.)
- unverified: The git grep argument-order finding (fk-8439f1f175a4) may duplicate sightings the codebook already holds rather than being a distinct new confusion. (evidence: The verdict says the codebook already records many near-identical sightings, citing around lines 30907, 31350, 31406 and 32331 of docs/legibility/confusion-codebook.yaml, and calls this one a repeat.)
- unverified: The session gained by fk-e536a9dd5f09 adds a sighting but nothing in the brief that distinguishes between the two alternatives in its standing cause (artifacts absent at lane initialisation, or the agent searching the wrong path structure). (evidence: The brief gives only the gained session id for this finding, with a null verdict and no per-session detail.)

## Findings

- area map: briefing review/briefing.yaml; areas analysis, assemblies, constraints-and-geometry, factory-infra, gui, kinematics-and-motion, language-pipeline, reify-audit, repo, tooling; overlay .claude/skills/review-all/project.md; rules `crates/reify-eval/src/{compute_persist,shell_extract_compute,morph_stage_*,engine_compute,modal_ops,dynamics_ops}.rs` → `analysis`, `crates/reify-eval/src/trajectory_ops.rs` → `kinematics-and-motion`, `crates/reify-eval/src/{geometry_ops*,topology_*,primitive_attribute_seed,selector_vocabulary_v2,sweep_classifier,structural_classifier,appearance,kernel_registry,dispatcher,warm_pool,realization_*,engine_constraints,relate_solve,tolerance_combine}` → `constraints-and-geometry`, `crates/reify-eval/` → `language-pipeline`, `crates/reify-stdlib/src/{trajectory/,joints,mechanism,loop_closure*,orientation,snapshot,flexures/}` → `kinematics-and-motion`, `crates/reify-stdlib/src/{dynamics/,modal/,fea,analysis,loads,supports,tensegrity}` → `analysis`, `crates/reify-stdlib/` → `constraints-and-geometry`, `crates/reify-compiler/src/connect.rs` → `assemblies`, `tree-sitter-reify/` → `language-pipeline`, `crates/{reify-constraints,reify-kernel-*,reify-geometry}/` → `constraints-and-geometry`, `crates/{reify-solver-elastic,reify-mesh-morph,reify-shell-extract,reify-fdm,reify-eval-fea-tests}/` → `analysis`, `crates/reify-gcode/` → `kinematics-and-motion`, `crates/{reify-cli,reify-lsp,reify-mcp,reify-config,reify-doc,reify-doc-build}/` → `tooling`, `crates/reify-audit/` → `reify-audit`, `dark-factory-orchestrator.yaml` → `factory-infra`, `gui/**` → `gui`
- 1 re-observed finding(s) are unverified: the census does not re-verify a catalogued finding, so a sighting this run only appends to its last_seen.

| id | key | area | sub_area | anchor | tags | severity | evidence_source | statement | proposal | verdict | disposition | first_seen | last_seen | supersedes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| entry-cand-20261017-4 | fk-8439f1f175a4 | repo |  | slug:git-grep-argument-order | h10, kind:confusion | low | agent-transcripts, present-tree | The agent appends `-A12` after the pattern and path, and writes `git grep -n PATTERN crates/ -- ':!*/tests/*'`. Git treats the path before `--` as a revision, so it reports 'unable to resolve revision: crates/'. Git also requires options to come before non-option arguments. The failure aborts the compound command, so the intended code read does not happen. Rewriting would need git grep's argument grammar, which is: options, then the pattern, then optional revisions, then `--`, then pathspecs. Verifier: The failure is real git behavior: `git grep` takes options first, then the pattern, then revisions, then `--`, then pathspecs. The codebook already records many near-identical sightings (for example around lines 30907, 31350, 31406 and 32331 of docs/legibility/confusion-codebook.yaml), so this one is a repeat. But the cause is the agent's habit with git/GNU grep argument order, which is tooling this tree does not contain. No existing tree file teaches the wrong form. My search for `git grep` with `-A` or `crates/ --` found hits only in the confusion-census plans and the codebook, which are records of the mistake, not instructions that teach it. Mentioning that the tree's CLAUDE.md already recommends the Grep tool or `git grep` is not a cause of the confusion either. |  | confirmed | open | census-reify-20261112 | census-reify-20261112 |  |
| entry-cand-20261106-1 | fk-c87a6b0141c7 | repo |  | crates/reify-build-utils/tests/module_scc_ratchet.rs::workspace_module_sccs_match_the_committed_baseline | h11, h10, kind:confusion | low | agent-transcripts, present-tree | The module-SCC ratchet assertion message directs the reader to run `scripts/assert-module-scc.py --help` for GREW/STALE explanations and baseline-lowering instructions, but Bash calls to that script were denied in don't-ask mode, leaving the agent without the documented fix path. Verifier: The assertion message in crates/reify-build-utils/tests/module_scc_ratchet.rs::workspace_module_sccs_match_the_committed_baseline sends the reader to `python3 scripts/assert-module-scc.py --help` for what GREW and STALE mean and how to lower a baseline row. The message carries no inline explanation. CLAUDE.md names the same --help as the canonical source. The script does contain GREW/STALE text, so the pointer is accurate. I could not observe the don't-ask permission denial from this tree. That denial comes from the agent harness, not from anything in the repo. |  | confirmed | open | census-reify-20261112 | census-reify-20261112 |  |
| entry-cand-20261102-1 | fk-b86df78cf29f | repo |  | tests/infra/test_run_gui_scripts.sh::_rgs_tauri_features_under | h10, h1, kind:confusion | low | agent-transcripts, present-tree | test_run_gui_scripts.sh fails during post-merge verification because cargo cannot resolve reify-gui and tauri on a cold registry. The failure message prescribes warming the registry (cargo fetch --locked or scripts/setup-dev.sh), but the autonomous don't-ask-mode session denies Bash, so the agent cannot run the prescribed fix or inspect the registry. The failure is environmental yet is reported as a test_failure, which makes it hard to distinguish from a code regression. Verifier: The quoted FAIL line is still emitted by `_t36_resolve` in tests/infra/test_run_gui_scripts.sh. Its label prescribes `cargo fetch --locked` or scripts/setup-dev.sh. `_rgs_tauri_features_under` runs `cargo metadata --locked --offline` with stderr sent to /dev/null and returns a bare 1 on any failure. A cold registry therefore looks the same as a real resolve regression, such as a Cargo.lock drift or a feature typo. The assertion gives no way to tell an environmental cause from a code one. The Bash-denial half of the cluster comes from the session's permission tooling, which this tree does not contain, so I could not verify it. | In `_rgs_tauri_features_under`, capture cargo metadata's stderr and use distinct return codes, or print the stderr tail, so a cold-registry or offline failure is distinguishable from a lock or feature resolve error, and let `_t36_resolve` report which one occurred. | confirmed | filed:9993 | census-reify-20261112 | census-reify-20261112 |  |
| cand-20261023-3 | fk-1504ef4fe29b | repo |  | slug:gnu-tail-obsolete-numeric-multifile | kind:confusion | low | agent-transcripts | The agent chained several check scripts with their rc values echoed, then ran `tail -3` over more than one log file in a single call. GNU tail rejects the obsolete `-N` form when several files are given (`option used in invalid context -- 3`) and the Bash call exits 1. Every check had already reported rc=0, so the exit status contradicts the results. The agent must notice that the failure comes only from the trailing inspection step and then re-run it as `tail -n 3`. Verifier: The confusion comes from GNU tail's behavior (it rejects the obsolete `-N` form when given several files) in the agent's own ad-hoc Bash call. I searched /home/leo/src/reify/scripts for `tail -N` applied to several log files and found no match. I found no script, doc, or file in this tree that teaches or produces the pattern, so this is a tooling quirk that this tree does not cause. |  | refuted | refuted | census-reify-20261112 | census-reify-20261112 |  |
| cand-20260721-6 | fk-146071429797 | repo |  | slug:watcher-rearm-background-timeout-churn | h10, kind:confusion | low | agent-transcripts | The 'Re-arm L2 watcher (reify) in background' task is dispatched as a bounded background command that frequently exceeds its wait window (exit code 124) or gets killed outright; rather than the underlying timeout/lifetime mismatch being fixed, the agent's only recourse is to keep re-issuing the same re-arm command dozens of times across the whole session, producing a chronic churn loop instead of a stable watcher. Verifier: The re-arm mechanism is not in this tree. A glob for `watcher-rearm*` under /home/leo/src/reify finds no file. The only matches for 'Re-arm L2 watcher' are in docs/legibility/confusion-codebook.yaml, the plans/confusion-census-* files, and a few notes and PRDs that quote the phrase. The exit-124 timeouts and killed background tasks come from dark-factory's watcher and background-task tooling. Nothing in reify's current files produces or teaches the churn or fails to guard against it, so no observable fact about this tree is confirmed. The codebook only records past sightings of the same pattern. |  | refuted | refuted | census-reify-20261112 | census-reify-20261112 |  |
| cand-20260713-5 | fk-93a3d212abd3 | repo |  | slug:escalation-watcher-blind-retry-wedged-mcp | h10, kind:confusion | low | agent-transcripts | Once mcp__escalation__get_pending_escalations/get_escalation calls begin returning 'The operation timed out', the autonomous watcher reissues the identical call with identical params on later turns rather than recognizing the endpoint as wedged, backing off, or surfacing the failure — burning turns/escalation budget until the supervisor's rotation limit (ROTATION_ESCALATIONS/ROTATION_HOURS) forces an exit with no escalations actually handled. Verifier: The reify tree contains no escalation-watcher prompt, MCP escalation server, or retry logic. The only mentions of get_pending_escalations and ROTATION_ESCALATIONS are in confusion-codebook and census records (docs/legibility/confusion-codebook.yaml, plans/confusion-census-*), which log earlier sightings of this behaviour. They do not show it in the current code. The watcher's retry behaviour and the escalation server's timeouts are in dark-factory tooling that this tree does not contain, so the claim can't be confirmed as a current fact about this tree. |  | refuted | refuted | census-reify-20261112 | census-reify-20261112 |  |
| cand-20260713-4 | fk-467d5d695470 | repo |  | slug:escalation-watcher-foreground-wait-killed | h10, kind:confusion | low | agent-transcripts | The watcher's 'bounded foreground wait for new L1' subprocess call (uv run escalation.watcher --timeout 540, wrapped in a Bash tool call with timeout=560000) exits 137 (killed) rather than completing normally or timing out cleanly. The agent has no signal to distinguish 'subprocess was killed' from 'no new escalation arrived within the wait window', so it re-issues the exact same command with the same --exclude-id 25 times in a row rather than switching to a backgrounded run, shortening the wait slice, or escalating the repeated kill as a systemic problem. Verifier: The watcher (`escalation.watcher`) and the agent loop that calls it are in dark-factory, not in this tree. The only matches in /home/leo/src/reify are the codebook and census files, which record earlier sightings of the same pattern. Nothing under reify's source or docs produces, teaches or fails to guard against the repeated exit-137 retries. I also can't observe the SIGKILL cause, so no current-state fact can be confirmed here. |  | refuted | refuted | census-reify-20261112 | census-reify-20261112 |  |
| cand-20260704-14 | fk-56815c9940e8 | repo |  | slug:escalation-watcher-modulenotfounderror-env | h10, kind:confusion | low | agent-transcripts | An autonomous escalation-watcher rotation (reify, 4h/50-escalation limits) hit the same class of ModuleNotFoundError at three distinct turns (21, 27, 44), indicating a missing/broken Python dependency in the watcher's execution environment that was never resolved and instead recurred across separate escalation-handling attempts within one session. Verifier: I can't confirm this as an observable fact about the reify tree. The claim is about an escalation-watcher session's Python environment, which is dark-factory tooling outside this tree. Nothing under /home/leo/src/reify contains or configures that watcher's interpreter or venv. The only trace in reify is the codebook, which records earlier ModuleNotFoundError sightings (e.g. cand-20260724-5, cand-20260724-18) as rejected env-mismatch candidates. Those involve bare python outside dark-factory's uv venv, which is a runtime environment issue. No existing reify file produces or teaches the confusion, so I can't name a remediation. |  | refuted | refuted | census-reify-20261112 | census-reify-20261112 |  |
| cand-20261108-1 | fk-0b8c5256d43b | repo |  | docs/prds/v0_6/gui-frontend-wire-bindings-bridge-port.md | h11, kind:confusion | low | agent-transcripts | When a type was renamed, the editing agent substituted the new name into an existing list of main.rs-private helpers without checking its actual visibility. The type is public in the library module, so the brief would have told the downstream implementer to move a library type out of main.rs. Verifier: The claim no longer holds in the current tree. `DebugServerSlot` is public in the library module `gui/src-tauri/src/debug_server_slot.rs`, and `main.rs` imports it from `reify_gui::debug_server_slot`. The PRD brief `docs/prds/v0_6/gui-frontend-wire-bindings-bridge-port.md` (lines 134-137 and 292) no longer lists it among the main.rs-private items. It says the type is the library's managed `debug_server_slot::DebugServerSlot`, which "needs no move", and that it "stays in the library's debug_server_slot module, not moved". The stale visibility claim was corrected, so nothing in the tree still teaches the confusion. |  | refuted | refuted | census-reify-20261112 | census-reify-20261112 |  |
| cand-20261107-2 | fk-6136db709f59 | repo |  | crates/reify-syntax/tests/common/tracked_corpus.rs | h11, kind:confusion | low | agent-transcripts | The plan assumed the parse guard was the only test that reads every tracked .ri file. The shared corpus listing in tracked_corpus.rs is used by every repo-wide .ri sweep in the test binary, and the agent did not check its other call sites before narrowing the change. Verifier: The confusion was a past planning error, not a current defect in the tree. `crates/reify-syntax/tests/common/tracked_corpus.rs` opens by saying every repo-wide `.ri` sweep in the binary reads `swept_ri_paths`. Both known consumers, `member_continuation_ambiguity_tests.rs` and `tracked_ri_parse_guard.rs`, import from it. The omission list `VERIFY_INERT_RI_TREES` is held equal to the scope classifier by `is_verify_inert_agrees_with_the_scope_classifier`. The warning already sits where an agent would read it, and the tree has no misleading single-consumer claim. |  | refuted | refuted | census-reify-20261112 | census-reify-20261112 |  |
| cand-20261107-1 | fk-b9bb1cc68822 | repo |  | slug:task-attribution-no-attribution-block | h10, kind:confusion | low | agent-transcripts | The architect reported the task already done by citing a commit from a different task branch (task/9099) as the completion evidence for task 9828. The claim had no declared attribution basis, so the orchestrator blocked the task with reason no_attribution. Verifier: The only evidence is a single orchestrator log line: the claim was blocked with reason no_attribution. The no_attribution gate is dark-factory orchestrator behaviour. The only `no_attribution` hits in this tree are the confusion codebook (which already holds this candidate and the near-identical cand-20261105-1) and two unrelated reify-eval Rust files. No file in this tree produces, teaches or fails to guard against the confusion. The gate worked as designed by blocking the unattributed claim, so there is no current observable defect to remediate here. |  | refuted | refuted | census-reify-20261112 | census-reify-20261112 |  |
| cand-20261105-1 | fk-ba88d33a9bb6 | repo |  | slug:task-attribution-role-boundary | kind:confusion | low | agent-transcripts | A task was marked done by pointing at a precondition commit from another task, but the attribution basis was not declared (attribution_basis: nothing_declared), so the block gate returned no_attribution. The architect then performed the required generator step itself, outside the implementer's landlock-confined sandbox, and recorded it only in a memory and a follow-up ticket. The next change has no declared record tying the task to the generated-artifact state. Verifier: The cluster describes dark-factory orchestrator behaviour: the block gate's no_attribution verdict, the attribution_basis field, and the unsandboxed architect role versus the landlock-confined implementer. None of that is in the reify tree. A grep for attribution_basis, no_attribution and nothing_declared finds only the confusion codebook YAML and an unrelated diagnostic comment in crates/reify-eval. The only evidence is one session quote, and I can't check it against any file in this tree. Whether the block gate returned no_attribution, or what the architect did out of band, cannot be observed here. |  | refuted | refuted | census-reify-20261112 | census-reify-20261112 |  |
| entry-cand-20260924-8 | fk-e536a9dd5f09 | repo |  | docs/notes/warm-lane-audit-runbook.md | h11, h13, kind:confusion | low | agent-transcripts | Warm-lane task worktree initialized without expected plan.json or iterations.jsonl artifacts, or agent searching wrong path structure (sibling vs nested under worktrees/) |  | unverified | filed:9430 | census-reify-20261022 | census-reify-20261022, census-reify-20261106, census-reify-20261112 |  |

## Filed Tasks

| finding_key | title | step | resolution | task_id | ticket | disposition | project_root | note |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| fk-b86df78cf29f | Post-merge gui-script verification fails on a cold cargo registry, and the remediation is blocked by Bash permission denial | semantic | existing | 9993 |  | filed:9993 |  |  |
| fk-bd1a1a099642 | run_all.sh hangs on one unfinished member until the outer timeout, and the summary names no failing member | curator | combined | 9414 | tkt_0RWWQC64CMQGZA5AAEYHH9QJ7R | filed:9414 |  |  |

## Structural

_no structural findings this run._

## Dispositions

- Re-verify at as_of_sha (D8): 4 due, 4 offered, 3 gone, 1 present, 0 inconclusive, 0 deferred by the verify cap.

| entry_id | finding_key | ticket | task_id | task_status | outcome | disposition | project_root |
| --- | --- | --- | --- | --- | --- | --- | --- |
| entry-cand-20260807-1 |  |  | 7894 | cancelled | cancelled (task 5910 decides the entry) | filed:5910 |  |
| entry-cand-20260807-1 |  |  | 5910 | pending | live | filed:5910 | /home/leo/src/dark-factory |
| entry-cand-20260921-14 |  |  | 7895 | cancelled | cancelled (task 5913 decides the entry) | filed:5913 |  |
| entry-cand-20260921-14 |  |  | 5913 | pending | live | filed:5913 | /home/leo/src/dark-factory |
| entry-cand-20260823-13 |  |  | 7896 | done | done -> entry already fixed | fixed:ad0ab1acf3b7b3bb5220e2e9c36e4ac3f5e43f67 |  |
| entry-cand-20260912-9 |  |  | 7897 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260805-1 |  |  | 7898 | pending | live | filed:7898 |  |
| entry-cand-20260818-22 |  |  | 7899 | done | done -> entry already fixed | fixed:ad0ab1acf3b7b3bb5220e2e9c36e4ac3f5e43f67 |  |
| entry-cand-20260831-14 |  |  | 7902 | cancelled | cancelled -> re-filing tkt_0RW309C15X3Y9GB5DQKBWFKM0H from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260813-9 |  |  | 7903 | cancelled | cancelled -> re-filing tkt_0RW309CEWK4ZMJAYFZGKBXJP7M from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260821-14 |  |  | 7905 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260901-16 |  |  | 7907 | cancelled | cancelled (task 7960 decides the entry) | fixed:77df97265253c514ebd4fa0ef77361aafebeb929 |  |
| entry-cand-20260901-16 |  | tkt_0RW309CQ46Y4XMV82SNCC58XRQ | 7960 | done | done -> entry already fixed | fixed:77df97265253c514ebd4fa0ef77361aafebeb929 |  |
| entry-cand-20260803-1 |  |  | 7908 | done | done -> entry already fixed | fixed:ad0ab1acf3b7b3bb5220e2e9c36e4ac3f5e43f67 |  |
| entry-cand-20260901-17 |  |  | 7909 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260914-17 |  |  | 7910 | cancelled | cancelled -> re-filing tkt_0RWNEF5J8S2DVXAZ6GN8H5H4T6 from run census-reify-20261106 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260914-18 |  |  | 7912 | done | done (task 9431 decides the entry) | filed:9431 |  |
| entry-cand-20260914-18 |  | tkt_0RW309D3JN4WBQX52BWG3KESDE | 9431 | pending | live | filed:9431 |  |
| entry-cand-20260921-16 |  |  | 7913 | cancelled | cancelled (task 5979 decides the entry) | fixed:2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4 |  |
| entry-cand-20260921-16 |  | tkt_0RWNEF66XM5Y4NPV4MASWWG64S | 5979 | done | done -> re-verified gone (fixed at 2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4) | fixed:2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4 | /home/leo/src/dark-factory |
| entry-cand-20260911-20 |  |  | 7914 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260819-19 |  |  | 7915 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260908-10 |  |  | 7916 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260908-11 |  |  | 7917 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260903-25 |  |  | 7918 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260828-16 |  |  | 7919 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260915-20 |  |  | 7920 | cancelled | cancelled -> re-filing tkt_0RW309D9HGC305Y7YDSB0YJ465 from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260807-2 |  |  | 7921 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260910-18 |  |  | 7922 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260813-12 |  |  | 7923 | done | done -> entry already fixed | fixed:ad0ab1acf3b7b3bb5220e2e9c36e4ac3f5e43f67 |  |
| entry-cand-20260818-23 |  |  | 7924 | cancelled | cancelled -> re-filing tkt_0RW309DGXQA61N6SEEVYYAP8VT from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260923-10 |  |  | 7925 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260918-15 |  |  | 7926 | cancelled | cancelled -> re-filing tkt_0RW309VX04WN8MQPM15XB3DN7K from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260916-19 |  |  | 7927 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260916-20 |  |  | 7928 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260920-8 |  |  | 7929 | cancelled | cancelled -> re-filing tkt_0RW309W5XY2B5MPD2ZC79XVZMA from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260826-33 |  |  | 7930 | done | done -> re-filing tkt_0RW309WDNRGXQ0E7BSG19XRWBA from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260904-15 |  |  | 7931 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260912-11 |  |  | 7932 | cancelled | cancelled -> re-filing tkt_0RW309WPD3FDBWHRHPVRN24S90 from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260830-14 |  |  | 7933 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260926-5 |  |  | 8477 | cancelled | cancelled -> re-filing tkt_0RWNEF6HGX3V9AC38Y9Y63PC52 from run census-reify-20261106 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20261006-7 |  |  | 8476 | done | done -> re-filing tkt_0RW309WXG8BWE3XX2DBAEEGMN6 from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260924-8 | fk-e536a9dd5f09 |  | 9430 | pending | live | filed:9430 |  |
| entry-cand-20261010-3 | fk-9c36e1e93a30 | tkt_0RWCTNTDD53A2NJ735C8EB501K | 9587 | pending | live | filed:9587 |  |
| entry-cand-20260726-13 | fk-eaa27e3e892f |  | 9432 | pending | live | filed:9432 |  |
| entry-cand-20260705-4 | fk-7fb018091f5a | tkt_0RWCTNTZB5BJ3A0K7BEGWKXJB4 | 9588 | pending | live | filed:9588 |  |
| entry-cand-20261013-2 | fk-dcb1ccb996cf | tkt_0RWNEF3A2DDCR4SRK1MPTJGA16 | 9855 | done | done -> re-verified gone (fixed at 2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4) | fixed:2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4 |  |
| entry-cand-20261023-2 | fk-4c97f2d190b8 | tkt_0RWNEF3PYXEMGF42PEXTKV33W2 | 9856 | done | done -> re-verified gone (fixed at 2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4) | fixed:2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4 |  |
| entry-cand-20261021-5 | fk-bd1a1a099642 | tkt_0RWNEF458E1SXAV3MF7GFF7MTQ | 9441 | done | done -> re-verified present (re-filed, supersedes 9441) | filed:9414 |  |

- entry-cand-20260921-16: open -> fixed, fixed at 2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4
- entry-cand-20261013-2: open -> fixed, fixed at 2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4
- entry-cand-20261023-2: open -> fixed, fixed at 2eebd70872c1dce4c957f7ffcc5fb544f21fd1f4

## Recorded, Not Filed

- 2 verified cluster(s) were promoted into the codebook and marked withheld, not filed: each has fewer than 2 sightings and no in-tree remediation from the verifier. Each files automatically once a later census counts 2 sightings of it.
  - git grep invoked with context flag after pathspec and a pathspec placed before `--`, failing with exit 128 (sightings: 1)
  - Ratchet failure points to a --help diagnostic the agent cannot run under don't-ask permission mode (sightings: 1)

## Cost

invoke calls: sonnet miner=40, sonnet verify=16, fable synthesis=1, haiku headroom-probe=3
