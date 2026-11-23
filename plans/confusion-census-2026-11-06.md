# confusion census 2026-11-06

Project: reify

## Method

```yaml
run_id: census-reify-20261106
as_of_sha: 71ac9dfa85006956733a95bbeb6967e28075ee54
since: 77df97265253c514ebd4fa0ef77361aafebeb929
evidence:
  window:
  - '2026-10-07T00:00:00+00:00'
  - '2026-11-06T00:00:00+00:00'
  sessions_enumerated: 3843
  skipped_coded: 432
  skipped_zero_signal: 1793
  mined: 100
  ledger_rows: 532
verification:
  confirmed: 3
  weakened: 0
  refuted: 19
  unverified: 0
cost:
  miner_calls: 100
  verify_calls: 22
  synthesis_calls: 1
  probe_calls: 5
  embedding_calls: 1
  wall_clock_secs: 7652.391
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
  adjudicated: 19
  ledger_created_this_run: false
  ledger_state: ok
  ledger_pruned: 0
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
    due: 0
    offered: 0
    gone: 0
    present: 0
    inconclusive: 0
    deferred: 0
```

## Saturation

- batches: 5
- stop reason: saturated
- operator batch cap: 50 batch(es) (not reached -- mining stopped by: saturated)
  - batch 0: dup_rate=0.80 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 1: dup_rate=0.90 (total=20, succeeded=20, failed=0, saturated=True)
  - batch 2: dup_rate=0.75 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 3: dup_rate=1.00 (total=20, succeeded=20, failed=0, saturated=True)
  - batch 4: dup_rate=0.95 (total=20, succeeded=20, failed=0, saturated=True)

## Screened

- per rule: title 0, session 0, key 0, cross_instrument 0
- inputs consumed: review-reify-20261015, review-all-reify-20261010
- inputs note: /hotspot-survey: no record matching plans/bug-hotspot-survey-<YYYY-MM-DD>[-<n>]-full-findings.json

## Adjudication

- model text-embedding-3-small; attach at cosine >= 0.9, cluster at >= 0.85; at most 10 queue cluster(s) per run
- embeddings: 1 request(s), 22 embedded, 910 cached, 31 pruned
- **ALL 10 queue cluster(s) were REJECTED and none survived**, and every member was rejected with its representative. Suspect a SYSTEMIC verifier failure (model unreachable, tool access denied, or unparseable verdicts) rather than unfounded claims; check the run's per-cluster 'verify failed' warnings.
- queue depth: 794 pending candidate(s) in 781 cluster(s)
- processed this run: 10 cluster(s), 19 candidate(s)
- rejected: "Cross-project interactive session experiencing resolve_ticket timeout with dark-factory MCP tools" (cand-20260911-10) for 2 candidate(s) across 2 session(s)
- rejected: "Warm-lane agent assumes standard project paths exist relative to worktree cwd" (cand-20260909-4) for 2 candidate(s) across 2 session(s)
- rejected: "Session digest references non-existent turns" (cand-20260908-4) for 1 candidate(s) across 2 session(s)
- rejected: "Warm-lane worktree missing expected project file structure" (cand-20260820-16) for 2 candidate(s) across 2 session(s)
- rejected: "Blocking TaskOutput re-polled identically on a stalled background task" (cand-20260801-22) for 2 candidate(s) across 2 session(s)
- rejected: "Agent polls background subprocess output via chained sleep+tail instead of Monitor/run_in_background" (cand-20260801-20) for 2 candidate(s) across 2 session(s)
- rejected: "L2 escalation-watcher re-arm loop churns through dozens of bounded-wait timeouts" (cand-20260731-13) for 2 candidate(s) across 2 session(s)
- rejected: "Multi-line python3 -c heredoc flattened to one line by bash execution layer, causing IndentationError" (cand-20260731-8) for 2 candidate(s) across 2 session(s)
- rejected: "rg (ripgrep) assumed available in task worktree sandbox" (cand-20260730-26) for 2 candidate(s) across 2 session(s)
- rejected: "Multi-line Bash tool commands flattened into one line, misdirecting leading commands like cd" (cand-20260723-17) for 2 candidate(s) across 2 session(s)

## Verification

- handed all 12 novel cluster(s) to the verifier; operator verify cap: 150 (not reached).

## Origin x Manifestation Matrix

| origin \ manifested | prd | architect | verify |
| --- | --- | --- | --- |
| prd | 1 | 0 | 0 |
| verify | 0 | 0 | 1 |
| unknown | 0 | 1 | 0 |

## Synthesis

- applied: 0 phase refinement(s) to 0 cluster(s) and 0 new entr(ies); 0 correction(s) applied, 0 skipped
- standing, no new evidence: 105
- invariant violations this run: none
- unkeyed entries re-observed: 21

### New

- fk-dcb1ccb996cf "Capability manifest entry authored with an extra 'note' field the strict schema forbids": Heuristic 10, clear invariants informatively, redundantly, uniformly enforced: the manifest schema enforces its key set loudly (extra_forbidden names the offending key), but enforcement is single-point and late. Nothing checks at authoring time, so the invariant reaches the author only when a validator is run by hand, after the manifest is written. Heuristic 11, SPOT: the allowed key set for a capability entry lives only in CapabilityManifestDoc, and the authoring guidance in .claude/skills/prd/project.md neither restates it nor points at it, so the author has two sources that disagree by omission. Heuristic 13, files make internal sense in isolation: a reader of the authoring doc cannot tell from that file alone that a task entry allows `note` while a capability entry does not, and the 18 live manifests carrying task-level `note:` make the wrong generalisation the natural one. The fix is in the proposal: name both key sets where the author reads, and say unknown keys are rejected.
- fk-4c97f2d190b8 "npm install in a scratch gui copy fails with an arborist 'edgesOut' null-dereference after node_modules is deleted by hand": Heuristic 11, SPOT: the project has exactly one install recipe, `npm ci --prefer-offline` in scripts/gui-test.sh, but that recipe is reachable only by running the script from the repo root, and its header describes only the lane/worktree case. An agent working in a scratch copy of gui has no stated path and improvises, which is how a hand-deleted node_modules followed by `npm install` reaches npm's arborist crash. Heuristic 13, files make internal sense in isolation: gui/package.json depends on the local workspace package @reify/shared-utils at gui/packages/shared-utils, so a copy of gui is not self-describing as an installable unit; the dependency on a sibling path is what a hand-cleaned `npm install` trips over. The crash itself is npm-internal and was not reproduced; the observable defect is the missing recipe for the off-path case, and the proposed header note closes it.
- fk-bd1a1a099642 "run_all.sh hangs on one unfinished member until the outer timeout, and the summary names no failing member": Heuristic 10, clear invariants informatively enforced: on SIGTERM, `_ra_on_term` builds its failed-name list only from members whose `.rc` file records a nonzero exit, so a member still in flight has no `.rc` and is never named. The run is correctly marked failed, but the message `=== FAILED: (partial) ===` names no offending value, and the heartbeat prints only `X/Y complete` with no pending list. Heuristic 12, structured data instead of meaningful strings: the outcome reaches the next agent as an empty name slot inside a summary line and a block reason of `tests failed [category: test_failure]`, so a hang and a genuine failure collapse into the same string and the distinction has to be recovered by reading a very large log. The header comment labels the empty-names case deliberate; that choice over-classifies an interrupt rather than reporting what is known at interrupt time, which the proposal corrects by printing the in-flight members by name.

### Re-observed

- fk-e536a9dd5f09 "Plan.json or task-meta iteration artifact missing at bootstrap" (gained 7b20331c-e408-4112-9af4-3638a69c51e2, 81512306-467c-41e8-8bdb-6696008135e3): no change stated

### Notes

- unverified: For fk-4c97f2d190b8 the origin phase stays unknown because the brief does not say what task the agent was performing when it made the scratch copy of gui; the manifested phase of architect is the brief's stamp and nothing in the evidence places it elsewhere. (evidence: The brief's cause and verdict describe only the install attempts in a scratch copy and the missing recipe, not the surrounding task or role.)
- unverified: For fk-bd1a1a099642 the block-reason string `tests failed [category: test_failure]` is attributed to a classifier outside tests/infra/run_all.sh that the verdict did not read, so the hang-versus-failure conflation is observed in run_all.sh's own output only. (evidence: The verdict states that verify.sh and dark-factory's classifier were not read and that the cause observed lies in no other file in the tree.)
- unverified: For fk-dcb1ccb996cf the extra_forbidden rejection is taken from the session evidence, not from reading CapabilityManifestDoc, and the failing manifest was not located. (evidence: The verdict reason says it could not read CapabilityManifestDoc in this tree and did not find the failing manifest from the session.)
- unverified: The two sessions gained by fk-e536a9dd5f09 arrive with no evidence detail in the brief, so nothing about its title, cause, severity or phases can be judged changed on this run. (evidence: The brief lists gained_sessions only, with verdict null and adopted_from null.)

## Findings

- area map: briefing review/briefing.yaml; areas analysis, assemblies, constraints-and-geometry, factory-infra, gui, kinematics-and-motion, language-pipeline, reify-audit, repo, tooling; overlay .claude/skills/review-all/project.md; rules `crates/reify-eval/src/{compute_persist,shell_extract_compute,morph_stage_*,engine_compute,modal_ops,dynamics_ops}.rs` → `analysis`, `crates/reify-eval/src/trajectory_ops.rs` → `kinematics-and-motion`, `crates/reify-eval/src/{geometry_ops*,topology_*,primitive_attribute_seed,selector_vocabulary_v2,sweep_classifier,structural_classifier,appearance,kernel_registry,dispatcher,warm_pool,realization_*,engine_constraints,relate_solve,tolerance_combine}` → `constraints-and-geometry`, `crates/reify-eval/` → `language-pipeline`, `crates/reify-stdlib/src/{trajectory/,joints,mechanism,loop_closure*,orientation,snapshot,flexures/}` → `kinematics-and-motion`, `crates/reify-stdlib/src/{dynamics/,modal/,fea,analysis,loads,supports,tensegrity}` → `analysis`, `crates/reify-stdlib/` → `constraints-and-geometry`, `crates/reify-compiler/src/connect.rs` → `assemblies`, `tree-sitter-reify/` → `language-pipeline`, `crates/{reify-constraints,reify-kernel-*,reify-geometry}/` → `constraints-and-geometry`, `crates/{reify-solver-elastic,reify-mesh-morph,reify-shell-extract,reify-fdm,reify-eval-fea-tests}/` → `analysis`, `crates/reify-gcode/` → `kinematics-and-motion`, `crates/{reify-cli,reify-lsp,reify-mcp,reify-config,reify-doc,reify-doc-build}/` → `tooling`, `crates/reify-audit/` → `reify-audit`, `dark-factory-orchestrator.yaml` → `factory-infra`, `gui/**` → `gui`
- 1 re-observed finding(s) are unverified: the census does not re-verify a catalogued finding, so a sighting this run only appends to its last_seen.

| id | key | area | sub_area | anchor | tags | severity | evidence_source | statement | proposal | verdict | disposition | first_seen | last_seen | supersedes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| entry-cand-20261013-2 | fk-dcb1ccb996cf | repo |  | .claude/skills/prd/project.md | h10, h11, h13, kind:confusion | low | agent-transcripts, present-tree | The agent wrote a free-text 'note' field on a capability entry in a PRD capability manifest. CapabilityManifestDoc rejects unknown keys (extra_forbidden), so the manifest failed to load. The schema's allowed fields were not checked before authoring, and the failure only surfaced when a validation script was run ad hoc. A wrong manifest would block the next change that consumes it. Verifier: In the live reify manifests, `note:` is a task-level key. For example, `docs/prds/alpha-spine.capability-manifest.yaml` has `note:` beside `label`, `task_id` and `title` on lines 5-8. A capability entry carries `name`, `binding`, `verdict` and `delivered_check`, and no `note` (lines 9-17). `.claude/skills/prd/project.md` documents manifest landing and evidence forms, but a search of it found no `capabilities:`, `delivered_checks:` or schema text, so it does not say which keys a capability entry allows. I could not read `CapabilityManifestDoc` in this tree, so the `extra_forbidden` rejection is taken from the evidence. I did not find the failing manifest from the session. The 18 yaml manifests with task-level `note:` make an agent likely to copy it onto a capability. | In the capability-manifest authoring section, list the exact allowed keys for a task entry (which includes `note`) and for a capability entry (`name`, `binding`, `verdict`, `delivered_check`), say that unknown keys are rejected as extra_forbidden, and tell the author to put free-text caveats in the task-level `note` or in `binding`. | confirmed | filed:9855 | census-reify-20261106 | census-reify-20261106 |  |
| entry-cand-20261023-2 | fk-4c97f2d190b8 | repo |  | scripts/gui-test.sh | h13, h11, kind:confusion | low | agent-transcripts, present-tree | The agent ran `rm -rf node_modules packages/shared-utils/node_modules` and then `npm install` in a scratch copy of gui. npm crashed with an internal arborist error, `Cannot read properties of null (reading 'edgesOut')`, and no node_modules/.bin/vitest was produced. The reify project's documented self-provisioning path is `scripts/gui-test.sh`, which runs `npm ci --prefer-offline`. The scratch-copy workflow had no known-good install recipe, so the agent kept retrying install variants (the log name 'npm-install-v4b' suggests several attempts) and could not run vitest. Verifier: The documented install path exists and works: /home/leo/src/reify/scripts/gui-test.sh runs `npm ci --prefer-offline` in the repo's own gui/. gui/package.json depends on the local workspace package `@reify/shared-utils` at gui/packages/shared-utils, so deleting both node_modules directories by hand and running `npm install` in a copy is a plausible trigger for the arborist `edgesOut` crash. Nothing in the tree gives a recipe for running vitest from a scratch copy of gui. The script header only covers the lane/worktree case, where it uses the repo-root path. The crash itself is npm-internal, and I could not reproduce it here, so I did not confirm the root cause. The observable part is that a recipe is missing. | Add a short usage note to the header saying scratch copies of gui should be provisioned with `npm ci --prefer-offline` (never hand-deleted node_modules plus `npm install`) or by running this script from the repo root. | confirmed | filed:9856 | census-reify-20261106 | census-reify-20261106 |  |
| entry-cand-20261021-5 | fk-bd1a1a099642 | repo |  | tests/infra/run_all.sh::_ra_on_term | h10, h12, kind:confusion | medium | agent-transcripts, present-tree | The infra pool stalled at 195/196 complete for hundreds of seconds, and the 30m outer timeout then killed the run. The resulting summary reads 'FAILED: (partial)' with an empty failing-name field, and the block reason is only 'tests failed [category: test_failure]'. The stuck member is not identified in the failure text, so the next agent has to dig through a roughly 1.17M-character log to find which member hung. A timeout caused by a hung member is also categorized the same way as a real test failure. Verifier: The current tests/infra/run_all.sh still behaves as the cluster describes. The SIGTERM handler `_ra_on_term` builds its failed-name list only from members whose `.rc` file records a nonzero exit, plus `failed_names`. A member that is still running and has no `.rc` file is therefore never named, so a hung member gives `=== FAILED: (partial) ===` and a bare `FAILED (partial)` line. The header comment calls the empty-names case DELIBERATE and justifies it by over-classifying an interrupt as a failure. The pool progress heartbeat prints only `X/Y complete, elapsed Ns` and does not list which members are pending. Nothing in this file separates a hang from a real test failure, and the cause lies in no other file in this tree. I did not read verify.sh or dark-factory's classifier, so I did not check the 'tests failed [category: test_failure]' block reason or the 1.17M-character log. | In `_ra_on_term` and the progress heartbeat, list the pool members that have no `.rc` file (still in flight) and print them, for example as `=== IN-FLIGHT at interrupt: <names> ===`, so a hung member is named in the output. | confirmed | filed:9441 | census-reify-20261106 | census-reify-20261106 |  |
| cand-20261016-3 | fk-998fe6ef1e97 | repo |  | scripts/main-gate-mark.sh | h1, kind:confusion | low | agent-transcripts | The agent ran `git log` and `ls` on `scripts/main-gate-mark.sh`, a filename it inferred from the identifier `main_gate_mark` in a commit subject. The file does not exist, so `ls` exited 2. The commit subject names a function, not a file path. The implementation lives elsewhere, and the sentinel is marked by the hook-gated commit path. A bare `ls` of a guessed path gives no pointer to the real location, so the agent has to search again. Verifier: scripts/main-gate-mark.sh exists in the current tree. It is an executable file, and `git log` shows commits touching it, including 883d277c68 'main_gate_mark removes its temp file when the publish fails'. The `ls` failure in the claim (exit 2, no such file) does not reproduce here. The agent most likely ran it in a worktree or lane checkout that lacked the file. That cause is not something I can observe from this tree. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20261017-3 | fk-2779d8eb0865 | repo |  | slug:git-rev-parse-short-two-revisions | kind:confusion | low | agent-transcripts | The agent passed `--short` with both HEAD and main to a single `git rev-parse`. With `--short` the command takes exactly one revision, so it died with 'Needed a single revision'. Because the call was the head of an && chain, the later `wc -l` and `tail` steps never ran. Revealing the end of the deliberation plan file therefore needed another round trip. Verifier: The cluster describes a one-off agent mistake: `git rev-parse --short HEAD main` takes exactly one revision with `--short`, so the call exits 128 and the rest of the && chain is skipped. This is git's own behavior and a transient shell-command error. It is not a fact about the current state of /home/leo/src/reify. I did not read any file in the tree to check this. Nothing in the tree produces, teaches, or fails to guard against it. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20261022-4 | fk-c64ce6080c21 | repo |  | slug:empty-optional-param-tool-call-json | h10, kind:confusion | low | agent-transcripts | The agent built a get_task_escalations call with an empty value for the optional 'status' field, so the JSON was invalid. It sent the same malformed payload again at turn 63 instead of omitting the field or supplying a value, so the error text did not change its behavior. Verifier: The malformed call (`"status": ,`) came from the agent while it was building an `mcp__escalation__get_task_escalations` call. That tool is defined and served by dark-factory, not by this tree. A grep for `get_task_escalations` finds it only in confusion-codebook and census artifacts (docs/legibility/confusion-codebook.yaml and two plans/confusion-census-2026-10-30 files). No source, schema or prompt in /home/leo/src/reify defines the tool or teaches this call shape, so there is no current fact here to verify and no existing file to fix. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20261020-1 | fk-a207875312e1 | repo |  | slug:foreground-poll-loop-on-test-log | h2, kind:confusion | low | agent-transcripts | Agent waits for a long-running cargo test suite and GUI run by re-issuing near-identical grep/tail poll commands on the log file (47 times for one log, 5 for another), with the session ending before the final poll returned. There is no completion-marker wait primitive in use, so each poll burns a turn and context, and the session ended with step-4 still pending. Verifier: The cluster describes one agent session's behaviour: 47 near-identical grep/tail polls on /tmp/t6690-step4-full3.log. The session transcript is not part of the reify tree, so I can't confirm it as a fact about the current tree. The cause is how the agent runs its Bash tool (foreground polling instead of a background run or Monitor wait), and that tooling is not in this tree. The tree already points agents at purpose-built waits (scripts/await-merge-landing.py for merges, scripts/cargo-test-tally.py for counting a run once from a captured file). No existing file in the tree produces or teaches this polling pattern, and I found no specific file to name for a remediation. The one check I ran was a grep of cargo-test-tally.py for polling or background-wait guidance, which found nothing. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20261101-4 | fk-f13bf1ffbcaf | repo |  | slug:plan-tools-add-plan-step-schema-guessing | h1, kind:confusion | low | agent-transcripts | The agent guessed the add_plan_step signature. It passed task_id, which the tool does not accept, and omitted the required step_id. Validation rejected the call before anything ran, so a retry was safe, but the agent had no schema in view and wasted a turn. Verifier: The only mentions of add_plan_step in /home/leo/src/reify are the census records: docs/legibility/confusion-codebook.yaml and the plans/confusion-census-*.md and .json files. Nothing in the tree defines the tool's schema or teaches agents to call it. The tool belongs to the dark-factory plan-tools MCP server, which this tree does not contain. The codebook already lists about ten near-identical candidates under the anchor slug:plan-tools-add-plan-step-schema-guessing, and most are marked rejected. The one fact the tree does show is that agents have called the tool with guessed argument names. I can't tie that to any file here that produces or fails to guard against it. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20261012-6 | fk-4b27e069a055 | repo |  | docs/prds/v0_6/compiler-declaration-passes.md | h11, kind:confusion | low | agent-transcripts | A held leaf contract (S9 lambda) says RelationInstance is 'parsed once by the compiler'. In the code it lives in reify-constraints and is built in reify-eval's relate_solve.rs::relation_instance. The PRD wording was never checked against the code's actual ownership before decomposition. An agent implementing the leaf from that contract would look in the compiler and put the work in the wrong crate or stage. The mismatch only surfaced when a recon seat read the code. Verifier: The code facts are right: `RelationInstance` is defined in crates/reify-constraints/src/relate_solve.rs and has one construction site, `relation_instance` in crates/reify-eval/src/relate_solve.rs. But the PRD no longer holds the claimed wrong wording. The only 'parsed once by the compiler' left in docs/ is docs/prds/v0_6/compiler-declaration-passes.md line 200, struck through and marked 'superseded by R6'. The amendment on that row binds `reify_builtins::RelationKind` instead. The kinematics PRD (line 159) and its capability manifest name the same ownership: the type is in reify-constraints, and the only construction site is `reify_eval::relate_solve::relation_instance`. The remaining 'parsed once by the compiler' occurrences are in review proposal JSON and plan files, not in contracts. I did not find the S9 λ contract text. The stale wording was corrected in the docs, so it is no longer an observable fact about the current tree. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20261020-2 | fk-78a25d20442f | repo |  | slug:tail-legacy-numeric-option-invalid-context | kind:confusion | low | agent-transcripts | The agent wrote `tail -3` in a compound final-verification command (likely with multiple files or after other arguments). GNU tail only accepts the obsolete `-N` form when it is the sole or first option, so it failed with 'option used in invalid context'. The whole command exited 1 even though the ratchet and guard checks both reported rc=0. This makes a successful verification look like a failure and forces a re-run. Verifier: The cluster describes a one-off ad-hoc shell mistake: an agent ran `tail -3` in an invalid position in a command it composed itself. The GNU tail error appears only in a session log, not in any file in /home/leo/src/reify. I grepped /home/leo/src/reify/scripts for `tail -N` used with extra arguments or multiple files and found nothing. No tracked file in this tree produces, teaches or fails to guard against it. The cause lies in agent-side command construction, which this tree does not contain. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20261010-4 | fk-231b7a67d068 | repo |  | slug:escalation-mcp-empty-optional-param | h1, kind:confusion | low | agent-transcripts | The agent built a get_task_escalations call and left the optional `status` filter blank (`"status": ,`) instead of omitting the key or supplying a value. The call failed JSON parsing and was rejected with InputValidationError before reaching the tool. Verifier: The claim is not an observable fact about the current reify tree. The malformed call (`"status": ,`) is a model-side JSON generation slip that was rejected by the tool-call validator before it reached any tool. `get_task_escalations` is not defined anywhere in /home/leo/src/reify. It appears only in `docs/legibility/confusion-codebook.yaml` and two `plans/confusion-census-2026-10-30` files, which are census records and not a tool schema or instructions that teach the call. The escalation MCP server belongs to dark-factory, so no file in this tree produces or fails to guard against the confusion. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20261028-2 | fk-89a07918fc06 | repo |  | slug:agent-legacy-tail-n-invalid-context | h1, kind:confusion | low | agent-transcripts | The agent ran a `cat ... \| tail -5` style command on the output of a background task, using the obsolete `-N` form with multiple file operands or after other options. GNU tail rejects this with 'option used in invalid context'. The command exited 1 even though every awaited step had reported rc=0, so the outcome looks like a failure and invites needless retries. Using `tail -n 5` would avoid it. Verifier: The cluster describes an ad-hoc agent-issued shell command (`cat ... \| tail -5` on background task output) that GNU tail rejected. That is agent tooling behaviour. No file in /home/leo/src/reify produces or teaches the failure. The only grep hits for `tail -N` in this tree are single-input pipelines in tests/infra scripts, where the legacy form is valid, plus codebook entries. I did not inspect those hits individually, and I did not find the original multi-file invocation, so I could not confirm the cluster against the current tree. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20260911-10 | fk-ea945e1f3f8d | repo |  | slug:resolve-ticket-mcp-timeout | h10, kind:confusion | low | agent-transcripts | Interactive session spawned in non-dark-factory project (/home/leo/src/reify) invoking dark-factory MCP operations (resolve_ticket) may lack proper environment context or project-scoped configuration, causing timeout on ticket resolution Verifier: The reify tree has no code that implements or configures the fused-memory `resolve_ticket` tool. It only contains docs and skill text that mention it, plus the confusion codebook and census files. The reported timeout comes from the MCP server in tooling outside this tree, so I can't observe or attribute it to anything in the current state of this tree. The cause the cluster gives ('may lack proper environment context') is a guess, and no reify file produces or teaches it. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20260909-4 | fk-f1e042d2b4e1 | repo |  | slug:warm-lane-relative-path-resolution | h13, kind:confusion | low | agent-transcripts | Agent running in worktrees/<id> context attempts to resolve project files (scripts/, docs/, etc.) as relative paths from its cwd, but the directory structure or layout differs in the warm-lanes worktree environment Verifier: The cluster's cause, that the warm-lane worktree layout differs from the main tree, is not observable in /home/leo/src/reify. `scripts/README.md` does not exist in the main tree either, so the `sed` failure comes from a guessed filename and not from a layout difference. `gui/src-tauri/src/main.rs` does exist in the tree, so that path is valid at the repo root. The `cd gui/src-tauri/src` failure could be a wrong cwd or a missing checkout in the lane. I can't see the lane from here, and the tree holds nothing that produces or teaches that confusion. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20260908-4 | fk-513d23aefd1e | repo |  | slug:session-digest-stale-turn-references | h10, kind:confusion | low | agent-transcripts | Orchestrated task session digest records turn references (44, 180) that do not exist in the session transcript, indicating data corruption, truncation, or stale cross-references in the digest index Verifier: The claim concerns session transcripts and a digest index (turn references, n_user_turns, size_bytes). None of that is in the reify tree. A grep for 'session digest' and 'session_digest' finds only confusion-codebook and census documents. Those are records of the confusion itself, not code that builds or checks a digest. The stray turn references (44, 180, 25, 111) in 485-512 byte sessions with zero user turns point to the dark-factory session-recording and mining tooling. No file in reify produces, teaches or fails to guard against them, so I could not observe the claim in the current reify tree. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20260820-16 | fk-92fdd5dab793 | repo |  | slug:warm-lane-missing-paths | h13, kind:confusion | low | agent-transcripts | Agent task briefing or context references file paths (e.g., gui/test/visual) that do not exist in the warm-lane worktree, causing navigation and file-read failures. May indicate incomplete worktree bootstrap, stale task metadata, or path resolution misconfiguration specific to the warm-lane environment. Verifier: In the current tree gui/test/visual/ exists and holds many tracked files, including diff.ts, paths.ts, scenarios.ts, orchestrate.ts and the run_*_smoke.sh scripts. The other paths in the cluster, such as tests/infra/ and crates/reify-compiler/tests/, are normal tracked locations. I did not check those two directly. The cluster has no observable defect in this tree. The not-found errors are more likely from a cwd that was not the lane root, from a lane checked out at an older commit, or from the dark-factory acquire step. All of those lie in tooling this tree does not contain. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20260801-22 | fk-4294ca125e1b | repo |  | slug:taskoutput-blocking-repoll | h10, kind:confusion | low | agent-transcripts | Agent reissues an identical block=true/timeout=600000 TaskOutput call against the same task_id seven times in a row rather than diagnosing why the background task hasn't completed or escalating, burning turns/wait-time on a repeated no-op action. Verifier: The evidence is repeated TaskOutput calls recorded in session transcripts (task_ids bpp23bx9d and bdnxsdd6d). That is agent runtime behaviour, not a fact about the files in /home/leo/src/reify. The only markdown file in the tree that mentions TaskOutput is plans/confusion-census-2026-08-02.md, a census record. No file in the tree produces, teaches or fails to guard against re-polling a stalled background task. The tool and its blocking and timeout semantics belong to the Claude Code harness and dark-factory, which this tree does not contain. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20260801-20 | fk-f21ff8a92acb | repo |  | slug:agent-sleep-tail-background-polling | comments, kind:confusion | low | agent-transcripts | Orchestrated task agent, waiting on a background test run it had spawned, defaulted to the generic shell polling idiom (`sleep N; tail file`) rather than the harness's required background-wait primitives (Monitor with an until-loop, or run_in_background), triggering a hard guard block and wasting a turn. Verifier: The cluster describes agent behaviour seen in session transcripts: an agent chained `sleep N; tail file`, and the Claude Code harness guard blocked it. The guard that blocked it lives in the harness, which this tree does not contain. I made no tool calls, so I did not search the tree for scripts or docs that teach the sleep+tail idiom. The CLAUDE.md I was given already forbids hand-rolled sleep loops, but only for waiting on merges, where it points to scripts/await-merge-landing.py. The guard also worked as intended, so the block cost only one wasted turn. I cannot name a specific existing file in this tree that produces or fails to guard against the confusion. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20260731-13 | fk-a0b4e67d18ac | repo |  | slug:l2-watcher-rearm-retry-churn | h6, kind:confusion | low | agent-transcripts | The 'Re-arm L2 watcher' background task is relaunched cycle after cycle (at least 29 cycles across ~800 turns), alternating between exit-code-124 timeouts and manual kills, without the agent pausing to diagnose why the watcher won't stay armed or escalating the systemic issue instead of mechanically retrying. Verifier: The claim describes agent behavior in past session transcripts (repeated 'Re-arm L2 watcher' background tasks ending in exit 124 or being killed). It is not a state of the reify tree that can be read now. The tree has no watcher re-arm script or prompt. The only tracked mentions are the codebook entries in docs/legibility/confusion-codebook.yaml, which already record this pattern many times, and one comment in dark-factory-orchestrator.yaml saying attended_b3_enabled is read by the interactive L2 watcher session. The watcher skill and its bounded-wait behavior belong to dark-factory tooling outside this tree, so no file here produces, teaches or fails to guard against the churn. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20260731-8 | fk-d70cf33ce0ca | repo |  | slug:bash-wrapper-flattens-multiline-python-c | h1, kind:confusion | low | agent-transcripts | The agent wrote a multi-line `python3 -c "..."` script (with newline-delimited `import`, `p=json.load(...)`, and a `for` loop body) to inspect plan.json steps. The execution layer collapsed the newlines into a single line before running it, turning valid multi-line Python into a syntactically invalid one-liner (`import json,sys p=json.load(sys.stdin) for s in p.get('steps',[]): ...`), which failed with IndentationError instead of running the intended inspection. Verifier: The cluster describes a bash execution layer that flattens newlines in a multi-line `python3 -c` command. That tooling is not part of the reify tree, so I could not observe the behaviour in the tree's current state. I did not read any file in the tree to check this. The only artifacts involved, `.task/plan.json` and the warm-lane worktrees, are runtime state and not tracked source. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20260730-26 | fk-c9571afcd919 | repo |  | slug:rg-missing-in-sandbox | h10, kind:confusion | low | agent-transcripts | Agent invoked `rg` directly assuming ripgrep is installed in the worktree/sandbox environment; it wasn't present, producing an immediate exit-1 tool error and forcing a fallback to the Grep tool. Verifier: The two sightings (2026-07-24 and 2026-07-30) show an agent running `rg` and getting "'rg' not found". The cause is the sandbox's tooling: `rg` isn't installed there. Nothing in this tree produces or teaches it. A grep of /home/leo/src/reify/scripts for `rg -n`, `rg --` or `ripgrep` found no matches. The tree's CLAUDE.md already tells agents to search with the Grep tool or `git grep`, so the guidance is present. The error was also cheap, an immediate failure followed by a fallback to the Grep tool. I did not search outside scripts/, so other directories are unchecked. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| cand-20260723-17 | fk-a719b906e9ff | repo |  | slug:bash-tool-multiline-flattening | h10, kind:confusion | low | agent-transcripts | A Bash tool call containing an embedded multi-line script (cd ...\necho ...\ngit add ...) executed with its newlines collapsed/lost, so tokens from subsequent lines were fed as positional arguments to the first command, producing 'cd: too many arguments' instead of running the intended sequence Verifier: The cluster describes a transient behaviour of the Bash tool harness (newlines in a multi-line command being lost, producing 'cd: too many arguments'). That is tooling outside /home/leo/src/reify, and no file in the tree produces, teaches, or fails to guard against it. I did not search the tree and found no tree-side cause, so I cannot confirm it as an observable fact about the current state of the tree. |  | refuted | refuted | census-reify-20261106 | census-reify-20261106 |  |
| entry-cand-20260924-8 | fk-e536a9dd5f09 | repo |  | docs/notes/warm-lane-audit-runbook.md | h11, h13, kind:confusion | low | agent-transcripts | Warm-lane task worktree initialized without expected plan.json or iterations.jsonl artifacts, or agent searching wrong path structure (sibling vs nested under worktrees/) |  | unverified | filed:9430 | census-reify-20261022 | census-reify-20261022, census-reify-20261106 |  |

## Filed Tasks

| finding_key | title | step | resolution | task_id | ticket | disposition | project_root | note |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| fk-dcb1ccb996cf | Capability manifest entry authored with an extra 'note' field the strict schema forbids | curator | created | 9855 | tkt_0RWNEF3A2DDCR4SRK1MPTJGA16 | filed:9855 |  |  |
| fk-4c97f2d190b8 | npm install in a scratch gui copy fails with an arborist 'edgesOut' null-dereference after node_modules is deleted by hand | curator | created | 9856 | tkt_0RWNEF3PYXEMGF42PEXTKV33W2 | filed:9856 |  |  |
| fk-bd1a1a099642 | run_all.sh hangs on one unfinished member until the outer timeout, and the summary names no failing member | curator | combined | 9441 | tkt_0RWNEF458E1SXAV3MF7GFF7MTQ | filed:9441 |  |  |
|  | Python heredoc embedding raw Rust source text triggers unicodeescape SyntaxError | curator | combined | 7910 | tkt_0RWNEF5J8S2DVXAZ6GN8H5H4T6 | filed:7910 |  |  |
|  | merge_request called without required `worktree` argument, surfacing only as a raw Pydantic error after an identical call already timed out | curator | combined | 5979 | tkt_0RWNEF66XM5Y4NPV4MASWWG64S | filed:5979 | /home/leo/src/dark-factory |  |
|  | Guessed scripts subdirectory probed with stderr suppressed, so a nonexistent path surfaces only as a bare exit 2 | curator | combined | 8477 | tkt_0RWNEF6HGX3V9AC38Y9Y63PC52 | filed:8477 |  |  |

## Structural

_no structural findings this run._

## Dispositions

- Re-verify at as_of_sha (D8): 0 due, 0 offered, 0 gone, 0 present, 0 inconclusive, 0 deferred by the verify cap.

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
| entry-cand-20260914-17 |  |  | 7910 | cancelled | abandoned (eligible to re-file; re-filed this run) | filed:7910 |  |
| entry-cand-20260914-18 |  |  | 7912 | done | done (task 9431 decides the entry) | filed:9431 |  |
| entry-cand-20260914-18 |  | tkt_0RW309D3JN4WBQX52BWG3KESDE | 9431 | pending | live | filed:9431 |  |
| entry-cand-20260921-16 |  |  | 7913 | cancelled | abandoned (eligible to re-file; re-filed this run) | filed:5979 |  |
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
| entry-cand-20260926-5 |  |  | 8477 | cancelled | abandoned (eligible to re-file; re-filed this run) | filed:8477 |  |
| entry-cand-20261006-7 |  |  | 8476 | done | done -> re-filing tkt_0RW309WXG8BWE3XX2DBAEEGMN6 from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260924-8 | fk-e536a9dd5f09 |  | 9430 | pending | live | filed:9430 |  |
| entry-cand-20261010-3 | fk-9c36e1e93a30 | tkt_0RWCTNTDD53A2NJ735C8EB501K | 9587 | pending | live | filed:9587 |  |
| entry-cand-20260726-13 | fk-eaa27e3e892f |  | 9432 | pending | live | filed:9432 |  |
| entry-cand-20260705-4 | fk-7fb018091f5a | tkt_0RWCTNTZB5BJ3A0K7BEGWKXJB4 | 9588 | pending | live | filed:9588 |  |

_no outcome changed this run._

## Cost

invoke calls: sonnet miner=100, sonnet verify=22, fable synthesis=1, haiku headroom-probe=5
