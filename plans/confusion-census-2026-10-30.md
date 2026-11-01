# confusion census 2026-10-30

Project: reify

## Method

```yaml
run_id: census-reify-20261030
as_of_sha: 77df97265253c514ebd4fa0ef77361aafebeb929
since: ad0ab1acf3b7b3bb5220e2e9c36e4ac3f5e43f67
evidence:
  window:
  - '2026-09-30T00:00:00+00:00'
  - '2026-10-30T00:00:00+00:00'
  sessions_enumerated: 3978
  skipped_coded: 398
  skipped_zero_signal: 1815
  mined: 60
  ledger_rows: 412
verification:
  confirmed: 5
  weakened: 0
  refuted: 13
  unverified: 1
cost:
  miner_calls: 60
  verify_calls: 19
  synthesis_calls: 1
  probe_calls: 4
  embedding_calls: 1
  wall_clock_secs: 5974.367
inputs_consumed:
- review-reify-20261015
- review-all-reify-20261010
extra:
  inputs_consumed_note: '/hotspot-survey: no record matching plans/bug-hotspot-survey-<YYYY-MM-DD>[-<n>]-full-findings.json'
  screened:
    title: 0
    session: 1
    key: 1
    cross_instrument: 0
  attached: 0
  adjudicated: 30
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
    due: 1
    offered: 1
    gone: 1
    present: 0
    inconclusive: 0
    deferred: 0
```

## Saturation

- batches: 3
- stop reason: saturated
- operator batch cap: 50 batch(es) (not reached -- mining stopped by: saturated)
  - batch 0: dup_rate=0.75 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 1: dup_rate=0.90 (total=20, succeeded=20, failed=0, saturated=True)
  - batch 2: dup_rate=0.90 (total=20, succeeded=20, failed=0, saturated=True)

## Screened

- per rule: title 0, session 1, key 1, cross_instrument 0
- inputs consumed: review-reify-20261015, review-all-reify-20261010
- inputs note: /hotspot-survey: no record matching plans/bug-hotspot-survey-<YYYY-MM-DD>[-<n>]-full-findings.json
- screened by session: "Search tool timeout returns an empty results list, indistinguishable from a genuine no-match during verification" → entry-cand-20260923-9, entry-cand-20260914-19
- attached by key: "Warm-lane task-meta path ambiguity causing repeated not-found errors" (cand-20260924-7) → entry-cand-20260924-8 under fk-e536a9dd5f09

## Adjudication

- model text-embedding-3-small; attach at cosine >= 0.9, cluster at >= 0.85; at most 10 queue cluster(s) per run
- embeddings: 1 request(s), 28 embedded, 921 cached, 181 pruned
- queue depth: 818 pending candidate(s) in 785 cluster(s)
- processed this run: 10 cluster(s), 30 candidate(s)
- verified: "Warm-lane task-meta path ambiguity causing repeated not-found errors" (cand-20260924-7) for 5 candidate(s) across 5 session(s)
- rejected: "Warm-lane worktree dispatched without materialized .task/plan.json" (cand-20260728-52) for 5 candidate(s) across 5 session(s)
- verified: "Escalation-watcher rotation script lives outside the target project root and requires an explicit env var, not cwd inference" (cand-20260726-13) for 4 candidate(s) across 4 session(s)
- verified: "Write tool unavailable in autonomous escalation-watcher session — memory-write attempt lost" (cand-20260716-5) for 3 candidate(s) across 3 session(s)
- rejected: "Escalation-watcher agent invokes filesystem Write tool for memory, unaware only MCP memory tools are enabled in this execution context" (cand-20260709-8) for 3 candidate(s) across 3 session(s)
- verified: "StructuredOutput double-wraps array under a key matching the schema's field name" (cand-20260705-4) for 2 candidate(s) across 3 session(s)
- rejected: "MCP plan-tool called with guessed argument names absent from its declared schema" (cand-20261027-1) for 2 candidate(s) across 2 session(s)
- rejected: "Info-level escalation park does not hold against the scheduler's stranded-task sweep" (cand-20261008-1) for 2 candidate(s) across 2 session(s)
- rejected: "Agent serializes MCP envelope markup into memory content payload, fails validation 3x without self-correction" (cand-20260920-6) for 2 candidate(s) across 2 session(s)
- rejected: "Python one-liner command construction missing statement separators" (cand-20260916-15) for 2 candidate(s) across 2 session(s)

## Verification

- handed all 8 novel cluster(s) to the verifier; operator verify cap: 150 (not reached).

## Origin x Manifestation Matrix

| origin \ manifested | decompose | implement | ops | unknown |
| --- | --- | --- | --- | --- |
| decompose | 3 | 0 | 0 | 0 |
| architect | 0 | 0 | 0 | 1 |
| merge | 0 | 0 | 1 | 0 |
| ops | 0 | 0 | 8 | 0 |
| unknown | 0 | 3 | 0 | 0 |

## Synthesis

- applied: 2 phase refinement(s) to 2 cluster(s) and 2 new entr(ies); 0 correction(s) applied, 0 skipped
- standing, no new evidence: 111
- invariant violations this run: none
- unkeyed entries re-observed: 10

### New

- fk-9c36e1e93a30 "Skill location guessed under user-level ~/.claude/skills when the skill is project-scoped or elsewhere": The Landing on main bullet of /home/leo/src/reify/CLAUDE.md names `/merge-queue` as a command and gives it no on-disk home, while the repo's own .claude/skills/ holds no merge-queue skill, so a reader of that file alone cannot resolve the name it is told to use — heuristic 13, files make internal sense in isolation. The skill's location is held nowhere the tree points at, so the agent reconstructed it by analogy with ~/.claude/skills/ and got exit 2 — heuristic 11, SPOT. The failure was cheap because the same bullet names scripts/land.sh as the fallback path; the verifier could not inspect ~/.claude/skills, so where the skill does live remains unestablished.
- fk-eaa27e3e892f "Escalation-watcher rotation script lives outside the target project root and requires an explicit env var, not cwd inference": The rotation prompt states the target project's root and queue dir but no location for the watcher-rearm tooling, and neither /home/leo/CLAUDE.md nor reify's CLAUDE.md points at the dark-factory checkout for it, so the one fact the agent needed had no single home it could reach — heuristic 11, SPOT; the search cost four sessions' worth of probing, including a filesystem-wide find that timed out. Once found, scripts/watcher-rearm.sh refused to run from its own directory without an exported DARK_FACTORY_ROOT: the precondition is enforced, but not informatively enough to tell a cwd-positioned caller what to export — heuristic 10, clear invariants informatively enforced. The reify tree carries only one DARK_FACTORY_ROOT reference (scripts/test_legibility_reify_config.py, defaulting to /home/leo/src/dark-factory); the script itself was not read, so its actual env-var behaviour is the report's claim, not a verified fact.
- fk-008cb070280b "Write tool unavailable in autonomous escalation-watcher session — memory-write attempt lost": Three reify sessions show the autonomous escalation-watcher class attempting a Write-tool memory file and losing the lesson when the tool is absent. The role's tool set is a precondition the agent discovers only by failing at it, with no surfaced statement of what is provisioned and no prompt-level redirect to the fallback — heuristic 10, clear invariants informatively enforced. Reify's CLAUDE.md already directs memory writes to `add_memory`, so the fallback exists in the tree but was not reached for; provisioning for the class is dark-factory's, which this tree does not contain. The verifier notes the same signature already appears in docs/legibility/confusion-codebook.yaml near lines 6478 and 6931 without checking whether this cluster duplicates it.
- fk-7fb018091f5a "StructuredOutput double-wraps array under a key matching the schema's field name": When the StructuredOutput schema's top-level array field shares its name with the collection being tagged (`tasks`), the model emits {"tasks": {"tasks": [...]}} and the validator rejects it with `/tasks: must be array`. The boundary parse into typed values does its job — heuristic 12, structured data instead of meaningful strings — and the rejection names the path and the expected type, so the agent can retry in one turn — heuristic 10, informatively enforced. No `tasks` schema or decompose prompt exists in this tree; the producing tooling is dark-factory's. The verifier found a codebook entry recording the same shape (`{"decisions": {"decisions": [...]}}` with a `tasks` quote) and did not read it in full, so this may re-sight a standing entry rather than a new pattern.

### Re-observed

- fk-e536a9dd5f09 "Plan.json or task-meta iteration artifact missing at bootstrap" (gained 17d028ae-a9be-4a51-9332-f6b32a8f6a66, 9069f552-8389-44fb-9fe4-39a9a3050691, 4e6ca5c8-6510-4bc0-9384-c5440a230ef2, 1930e554-9669-4a0f-b464-709b16bbda62): no change stated

### Phase Refinements

- fk-9c36e1e93a30: origin ops, manifested ops (The gap the verdict locates is in the operator-maintained CLAUDE.md Landing on main bullet, which names `/merge-queue` without a location; that is operational guidance authored outside any pipeline phase, so the origin is better placed at ops than left unknown.)
- fk-008cb070280b: origin ops, manifested ops (The verdict places tool provisioning for the escalation-watcher class in dark-factory's operator configuration and notes dark-factory-orchestrator.yaml mentions the class only in comments; the absent Write tool is an ops-time provisioning decision, so origin is better placed at ops than unknown.)

### Notes

- unverified: fk-e536a9dd5f09's four new sessions add weight but no new evidence about the cause: the verdict confirms only that the runbook's `<worktree_base>/.task-meta/<lane>/` wording is abstract and that .task dangles until the architect writes plan.json, while the sightings carry no target paths, so the standing cause's two alternatives (artifacts absent vs. wrong path probed) remain undistinguished. (evidence: Verdict reason: 'The sightings give no target paths for the lane-25 session and do not show what actually caused the not-found errors. The cause is not directly verifiable.')
- unverified: fk-7fb018091f5a and fk-008cb070280b may each duplicate an entry already present in docs/legibility/confusion-codebook.yaml rather than being new patterns. (evidence: Both verdicts cite existing codebook entries with the same signature (the double-wrap entry quoting `{"tasks": {"tasks": [...]}}`; the Write-tool entries near lines 6478 and 6931) and both state the verifier did not check whether the cluster duplicates them.)
- unverified: Three of this run's four new findings (fk-eaa27e3e892f, fk-008cb070280b, fk-7fb018091f5a) have their producing mechanism in dark-factory, not in the reify tree, so no reify-side proposal is actionable for them. (evidence: Each verdict states the relevant script, tool provisioning, or schema lives in dark-factory and that the reify tree contains no file that produces or guards against the confusion; none carries a proposal.)

## Findings

- area map: briefing review/briefing.yaml; areas analysis, assemblies, constraints-and-geometry, factory-infra, gui, kinematics-and-motion, language-pipeline, reify-audit, repo, tooling; overlay .claude/skills/review-all/project.md; rules `crates/reify-eval/src/{compute_persist,shell_extract_compute,morph_stage_*,engine_compute,modal_ops,dynamics_ops}.rs` → `analysis`, `crates/reify-eval/src/trajectory_ops.rs` → `kinematics-and-motion`, `crates/reify-eval/src/{geometry_ops*,topology_*,primitive_attribute_seed,selector_vocabulary_v2,sweep_classifier,structural_classifier,appearance,kernel_registry,dispatcher,warm_pool,realization_*,engine_constraints,relate_solve,tolerance_combine}` → `constraints-and-geometry`, `crates/reify-eval/` → `language-pipeline`, `crates/reify-stdlib/src/{trajectory/,joints,mechanism,loop_closure*,orientation,snapshot,flexures/}` → `kinematics-and-motion`, `crates/reify-stdlib/src/{dynamics/,modal/,fea,analysis,loads,supports,tensegrity}` → `analysis`, `crates/reify-stdlib/` → `constraints-and-geometry`, `crates/reify-compiler/src/connect.rs` → `assemblies`, `tree-sitter-reify/` → `language-pipeline`, `crates/{reify-constraints,reify-kernel-*,reify-geometry}/` → `constraints-and-geometry`, `crates/{reify-solver-elastic,reify-mesh-morph,reify-shell-extract,reify-fdm,reify-eval-fea-tests}/` → `analysis`, `crates/reify-gcode/` → `kinematics-and-motion`, `crates/{reify-cli,reify-lsp,reify-mcp,reify-config,reify-doc,reify-doc-build}/` → `tooling`, `crates/reify-audit/` → `reify-audit`, `dark-factory-orchestrator.yaml` → `factory-infra`, `gui/**` → `gui`

| id | key | area | sub_area | anchor | tags | severity | evidence_source | statement | proposal | verdict | disposition | first_seen | last_seen | supersedes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| entry-cand-20261010-3 | fk-9c36e1e93a30 | repo |  | CLAUDE.md | h11, h13, kind:confusion | low | agent-transcripts, present-tree | The agent assumed the merge-queue skill lives at /home/leo/.claude/skills/merge-queue/. The project instructions only name `/merge-queue` as a command and give no on-disk location for the skill. The guess failed with exit 2, so the agent had no discoverable pointer to where project skills are kept (for example the repo's .claude/skills/). Verifier: In /home/leo/src/reify/CLAUDE.md line 10, `/merge-queue` appears only as a command name with no on-disk location. The project's .claude/skills/ holds only hotspot-survey, prd, reify-design, review-all and audit, so there is no merge-queue skill in the tree. The skill is therefore not discoverable under the repo, and an agent that guesses ~/.claude/skills/merge-queue/ gets a missing-path error. I could not run ls on ~/.claude/skills, so I don't know where the skill actually lives. | In the Landing on main bullet, state where the `/merge-queue` skill is defined (for example the dark-factory skills directory, or that it is an orchestrator-provided command with no file in this repo) so agents do not guess ~/.claude/skills/. | confirmed | filed:9587 | census-reify-20261030 | census-reify-20261030 |  |
| entry-cand-20260924-8 | fk-e536a9dd5f09 | repo |  | docs/notes/warm-lane-audit-runbook.md | h11, h13, kind:confusion | low | agent-transcripts, present-tree | Agent probing .task-meta path with conflicting assumptions: directory structure may be under worktrees/ vs at lane root, or path resolution failing when worktree context expected vs provided Verifier: The path layout is observable and matches the confusion. The metadata directory is /home/leo/src/warm-lanes/worktrees/.task-meta, a sibling of the lane directories. /home/leo/src/warm-lanes/.task-meta does not exist. Each lane's .task entry is an absolute symlink into <worktree_base>/.task-meta/<lane>/. It dangles until the architect writes the plan, and iterations.jsonl is not among the files recorded there. The reify docs (warm-lane-audit-runbook.md A6, and scripts/warm-lane-audit.sh) mention '<worktree_base>/.task-meta/<lane>/' only abstractly. They never give the concrete path /home/leo/src/warm-lanes/worktrees/.task-meta/<lane>/ or the file list, so an agent has to guess between the lane root, worktrees/, and .task. The sightings give no target paths for the lane-25 session and do not show what actually caused the not-found errors. The cause is not directly verifiable, so this confirms only that the layout is ambiguous in the docs. | In the A6 bullet, state the concrete task-metadata location (/home/leo/src/warm-lanes/worktrees/.task-meta/<lane>/, with .task symlinking to it and dangling until the plan is written) and list the files it holds (agent_session.json, metadata.json, plan.json, plan.lock, reviews, verdicts). | confirmed | filed:9430 | census-reify-20261022 | census-reify-20261022 |  |
| entry-cand-20260726-13 | fk-eaa27e3e892f | repo |  | slug:watcher-rearm-cross-repo-invocation | h11, h10, kind:confusion | low | agent-transcripts, present-tree | The rotation prompt for an autonomous L1 escalation watcher states only the target project's root and queue dir (e.g. "Project root: /home/leo/src/reify ... Escalation queue: /home/leo/src/reify/data/escalations") with no pointer to where the watcher-rearm tooling itself lives. The agent first hit 'no such file or directory' assuming the script was relative to the stated project root, then had to brute-force search common dark-factory install locations plus a full filesystem `find /`, which timed out after 2 minutes. After finally locating scripts/watcher-rearm.sh under /home/leo/src/dark-factory, `cd`-ing into that directory before invoking it was still insufficient — the script hard-requires the DARK_FACTORY_ROOT env var to be explicitly exported rather than inferring it from cwd, producing a second failure before the agent corrected the invocation on a later retry. Verifier: The reify tree contains no scripts/watcher-rearm.sh. The only DARK_FACTORY_ROOT use is in scripts/test_legibility_reify_config.py, which defaults to /home/leo/src/dark-factory. Neither /home/leo/CLAUDE.md nor /home/leo/src/reify/CLAUDE.md points to the dark-factory checkout for watcher tooling. The rotation prompt and the script's DARK_FACTORY_ROOT requirement belong to dark-factory, which this tree does not contain. I did not read the dark-factory checkout, so I did not check the script's own env-var behaviour. |  | confirmed | filed:9432 | census-reify-20261030 | census-reify-20261030 |  |
| entry-cand-20260716-5 | fk-008cb070280b | repo |  | slug:escalation-watcher-write-tool-not-provisioned | h10, kind:confusion | low | agent-transcripts, present-tree | Agent tried to persist a distilled feedback lesson (a warm-lane merge-verify lock-divergence finding) using the Write tool, following the interactive-session auto-memory convention of writing a markdown file with frontmatter, but the autonomous escalation-watcher agent class does not have the Write tool enabled in this context. The write failed outright and the finding was not captured through any fallback (e.g. add_memory), risking silent loss of a discovery right before the session's rotation-limit exit. Verifier: The failure is a recurring, observable fact. Three sightings in reify sessions (2026-07-05, 07-12 and 07-16) show the same error: the Write tool is not enabled for the autonomous escalation-watcher agent class, so its memory-write attempts were lost. The tree already records the same signature in docs/legibility/confusion-codebook.yaml (around lines 6478 and 6931), so it is not novel. Tool provisioning for that agent class lives in dark-factory, not in this tree. dark-factory-orchestrator.yaml mentions the escalation-watcher only in comments. Reify's CLAUDE.md already tells agents to use `add_memory` for memory writes, so no existing file here teaches or fails to guard against the confusion. |  | confirmed | open | census-reify-20261030 | census-reify-20261030 |  |
| entry-cand-20260705-4 | fk-7fb018091f5a | repo |  | slug:structuredoutput-array-field-double-wrap | h10, h12, kind:confusion | low | agent-transcripts, present-tree | When asked to tag a batch of items (here, tasks) with a schema whose top-level array field is itself named "tasks", the model reifies the batch concept twice, emitting {"tasks": {"tasks": [...]}} instead of {"tasks": [...]} — the field name collides with the natural noun for the collection being processed, producing a spurious extra wrapper object that fails schema validation (`/tasks: must be array`). Verifier: The pattern is recorded in this tree's docs/legibility/confusion-codebook.yaml, which has an entry for a StructuredOutput array field double-wrapped under its own key name (`{"decisions": {"decisions": [...]}}`, plus a `{"tasks": {"tasks": [...]}}` quote). I found no `tasks` StructuredOutput schema or decompose prompt in the tree. That tooling lives in dark-factory, so no file here produces or guards against it. I did not read the codebook entry in full or check whether this cluster duplicates it. |  | confirmed | filed:9588 | census-reify-20261030 | census-reify-20261030 |  |
| cand-20261012-5 | fk-7eb97b7eaa24 | repo |  | slug:listagents-repeated-polling | kind:confusion | low | agent-transcripts | The agent repeatedly calls ListAgents with no arguments, apparently polling for sub-agent state without varying the call or switching to a completion-notification mechanism. The digest does not show what the agent expected to learn, so the root cause is only inferred. Verifier: The cluster describes a runtime tool-use pattern: ListAgents called six times with empty arguments. It is not an observable fact about the current state of /home/leo/src/reify. The digest does not show what the agent expected to learn, so the root cause is inferred. ListAgents and the sub-agent completion notifications are harness tooling, which this tree does not contain. I did not search the tree for a file that teaches this polling. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20261003-9 | fk-3a54e86a1a41 | repo |  | slug:escalation-mcp-timeout-retry | h10, kind:confusion | low | agent-transcripts | The escalation MCP call timed out with no diagnostic detail. The agent then called it three more times with near-identical arguments (level 1, compact true) instead of treating the timeout as a systemic failure or falling back to the queue directory on disk. Verifier: The cluster describes one session's runtime behavior: an escalation MCP call timed out and the agent re-issued it. That is not a state of the reify tree. The `mcp__escalation__get_pending_escalations` server is part of dark-factory tooling, which this tree does not contain. A grep for the tool name finds it only in docs (the confusion codebook, a PRD, and census notes), with no implementation or caller. I found no existing reify file that causes or teaches the retry behavior, or that should guard against it. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20261029-5 | fk-42aec6801130 | repo |  | slug:t3-code-mcp-thread-id-format | h10, kind:confusion | low | agent-transcripts | The watcher passed a raw thread UUID to mcp__t3-code__t3_thread_read. The tool only resolves ids in the 'mcp:<uuid>' form, so the call failed with thread_not_found. The id format is not documented where the watcher gets thread ids, so the next agent making this call can repeat the mistake. Fixing the id format or documenting it in the tool description would stop that. Verifier: A search of /home/leo/src/reify for 't3_thread_read' and 't3-code' returned no matches. The failure came from a call to the external t3-code MCP tool, whose thread-id format is defined in tooling outside this tree. Nothing in the tree teaches or produces the bare-UUID mistake, and no file here is the right place to guard against it. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20261004-7 | fk-71d54d882bfb | repo |  | slug:search-tool-timeout-reported-as-empty-ok-result | h10, kind:confusion | low | agent-transcripts | The search tool reports an 'error: timeout' inside an (OK) result alongside an empty results list, and empty results for patterns known to exist (finalise_uniqueness exists in solver.rs) are consistent with a failed or truncated search. The agent can mistake this for absence of citations, producing an inconclusive verdict. The tool result status is OK, so the failure is easy to overlook. Verifier: The symbol `finalise_uniqueness` does exist in the tree, including in crates/reify-constraints/src/solver.rs, so the underlying code is fine. The reported problem is the verifier's search tool returning `error: timeout` with empty results inside an OK status. That behaviour comes from the tooling, not from anything in this repository, so the repository's files can't be checked for it or changed to fix it. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20261028-1 | fk-0a000661d478 | repo |  | slug:escalation-mcp-empty-optional-param | h10, kind:confusion | low | agent-transcripts | The agent built a get_task_escalations call with `"status": ,` (a key with no value), probably from an unset optional filter, so the harness rejected the whole input as unparseable JSON. The agent has no schema-level guard that omits unset optional parameters, and the error names only generic causes (backslashes, control characters, truncation) rather than the real defect, an empty value. This costs a wasted turn and a risk of repeat for anyone calling the tool with optional filters. Verifier: The only mention of get_task_escalations in /home/leo/src/reify is the codebook file docs/legibility/confusion-codebook.yaml. No tool definition, schema or caller exists in this tree. The malformed `"status": ,` call is a one-off agent output, and the JSON-parse error comes from the harness and the escalation MCP server, which live in dark-factory. So I cannot observe the confusion as a fact about the current state of this tree. No existing file here produces, teaches, or fails to guard against it. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20261027-3 | fk-22a8cad4e2fb | repo |  | slug:git-grep-option-ordering | comments, kind:confusion | low | agent-transcripts | The agent appended a context option (-A8) after the pattern and path arguments in a `git grep` call, so git rejected it with 'option must come before non-option arguments'. git grep requires options before the pattern, unlike GNU grep, which accepts them anywhere. The same compound command also ran a `sed -n` dump, so the output was mostly source text and the real error was one trailing line, which is easy to miss. The intended struct lookup produced nothing and had to be reissued. Verifier: The cluster describes a one-off mistake in an agent's shell invocation: `-A8` placed after the pattern in a `git grep` call. That is a fact about a past tool call, not about the current state of the reify tree, and no file in the tree could be checked to confirm or refute it. The tree's own CLAUDE.md already tells agents to use the Grep tool or `git grep` for searching, and the error came from the agent's tooling rather than from tree content. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20261003-10 | fk-954b6f4ed613 | repo |  | slug:nocapture-diff-interleaving | tests, kind:confusion | low | agent-transcripts | The agent diffed the captured --nocapture output of two cargo test runs to check run-to-run determinism. Tests in the same binary run in parallel, so their stdout lines interleave in a different order each run. The diff therefore exits 1 with only line-order hunks, even though cargo test returned rc=0 and every test passed. The compound command's exit code (diff's status) cannot tell an ordering artifact from a real numeric divergence, so the agent has to read the hunks to find out the run was actually clean. The next agent may chase a phantom nondeterminism or distrust a green result. Verifier: I found no file in /home/leo/src/reify that produces the `[ISO]` or `[ANISO]` output lines. A search of the Rust sources returned nothing. The confusion comes from an agent's own ad-hoc `diff` of two `--nocapture` runs, which is a property of how the agent ran the command. It is not something a tracked file in this tree produces or teaches. The interleaving itself is ordinary behaviour of parallel cargo tests. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20260728-52 | fk-15bcb91dea2a | repo |  | slug:warm-lane-missing-task-plan-json | h10, kind:confusion | low | agent-transcripts | Orchestrator's warm-lane worktree assignment/handoff does not guarantee .task/plan.json is present in the claimed worktree before the task agent starts, so the agent's own read of its task metadata (task_id, title, confirmation criteria) fails with FileNotFoundError, and the agent then thrashes rereading a task output file trying to recover its context. Verifier: I can't confirm from this tree that a dispatched warm-lane worktree lacks .task/plan.json. The evidence is agent session transcripts with FileNotFoundError on `_lane-N/.task/plan.json`, and those lanes are under /home/leo/src/warm-lanes, not in this repo. Reify's scripts only mention `.task/plan.json` in a comment in refresh-warm-base.sh, and in warm-lane-audit.sh, which only reports lane state. Reify's CLAUDE.md says the lane ACQUIRE step (`git clean -xfd -e target`, which would remove untracked files like .task) and the dispatch handoff belong to dark-factory's orchestrator, and `seed-warm-lane.sh` doesn't touch the source tree. Reify has no file that creates, teaches or fails to guard this, so the cause lies in tooling this tree doesn't contain. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20260709-8 | fk-28a44f8e0ef2 | repo |  | slug:escalation-watcher-write-tool-disabled | h10, kind:confusion | low | agent-transcripts | Agent conflated the interactive Claude Code auto-memory system (file-based Write to a memory directory) with the fused-memory MCP tool surface that autonomous/dispatched escalation-watcher agents actually have. Nothing in the supervisor-injected role prompt ('You are running as an autonomous escalation watcher...') flagged that Write is disabled in this context, so the agent reached for the wrong persistence mechanism when trying to record a note about a pending L2 escalation. Verifier: The error text itself is real: 'Write exists but is not enabled in this context' appears in the sightings. But the cause is a supervisor-injected role prompt and a tool allowlist, and both live in dark-factory tooling. I searched this tree for the phrase 'autonomous escalation watcher'. The only hit is docs/legibility/confusion-codebook.yaml, which records past confusions and is not a prompt. No file in /home/leo/src/reify produces, teaches or fails to guard against this confusion. The tool harness already returned a clear error, so the cost to the next change is small. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20261027-1 | fk-ff4d341071ac | repo |  | slug:plan-tools-guessed-argument-names | h1, kind:confusion | low | agent-transcripts | The agent called mcp__plan-tools__add_plan_step with keyword arguments task_id and type, which the schema does not declare, and omitted the required step_id and step_type. It then called mark_step_committed with commit_sha instead of the declared sha. Each call was rejected at argument validation before running, so the agent had to re-issue it with corrected arguments. Verifier: The plan-tools MCP server, with its add_plan_step and mark_step_committed schemas, is not in this tree. The only matches in /home/leo/src/reify are the confusion codebook and census files, plus a comment in tests/infra/test_warm_lane_audit.sh that mentions mark_step_committed's SHA width. No file here defines, documents or teaches the plan-tool argument names. The calls were rejected at argument validation, so the rejection came from tooling outside this tree. I found no file in this tree that causes or fails to guard against the confusion. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20261008-1 | fk-d8684a864f9a | repo |  | slug:info-escalation-park-vs-stranded-sweep | h11, kind:confusion | low | agent-transcripts | Parking an info-level escalation is assumed to keep its task out of the scheduler's stranded-task sweep. The sweep re-pends the task regardless, so the park silently fails to hold and the task can dispatch or be re-pended unexpectedly until it is set to deferred. Verifier: The scheduler's stranded-task sweep and the escalation park semantics are dark-factory tooling. Nothing in /home/leo/src/reify implements or documents them, so the claim can't be checked against this tree. The only matches for 'stranded task' here are census and codebook records of the confusion. docs/legibility/confusion-codebook.yaml already has an entry with this exact title at line 29938, so it isn't novel. I saw no file in this tree that teaches the wrong assumption or fails to guard against it. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20260920-6 | fk-015c0a59e6d9 | repo |  | slug:add-memory-mcp-envelope-markup-in-content | h10, kind:confusion | low | agent-transcripts | Agent's string/template construction for add_memory content field includes raw XML wrapper tags (specifically `</content>`) that belong only in the MCP envelope layer, not the serialized content value Verifier: The only reify files that mention `mcp_markup_detected` or the 'MCP envelope markup' rejection are confusion-codebook and census records (docs/legibility/confusion-codebook.yaml, plans/confusion-census-*). Nothing in the tree builds, templates or validates add_memory payloads. The guard that rejects the payload sits in the MCP/fused-memory tooling, which this tree doesn't contain. The cluster's transcript evidence (turns 206, 212, 217 and 335) can't be checked from the tree. What the tree does show is that the confusion is already recorded: the codebook has an equivalent open entry, entry-cand-20260910-15, for envelope tags leaking into a tool-call field, plus a 2026-09-01 sighting of the same `</content>` pattern. I can't confirm this as a current observable fact about the tree, and no existing file produces or fails to guard against it. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |
| cand-20260916-15 | fk-a5c82316f84b | repo |  | slug:python-oneliner-missing-statement-separators | h12, kind:confusion | low | agent-transcripts | Agent constructs inline Python with multiple statements on one line but omits semicolons between completed expressions (after json.load() call), producing Python IndentationError during shell execution Verifier: The confusion is an agent writing a malformed `python3 -c` one-liner, with no semicolon between statements. That is agent behaviour in tooling this tree does not contain. The only place the malformed pattern appears in the tree is `docs/legibility/confusion-codebook.yaml`, which records the confusion itself. No tracked source, script or doc teaches or produces the malformed command, so there is no current observable fact about the tree to confirm. |  | refuted | refuted | census-reify-20261030 | census-reify-20261030 |  |

## Filed Tasks

| finding_key | title | step | resolution | task_id | ticket | disposition | project_root | note |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| fk-9c36e1e93a30 | Skill location guessed under user-level ~/.claude/skills when the skill is project-scoped or elsewhere | curator | created | 9587 | tkt_0RWCTNTDD53A2NJ735C8EB501K | filed:9587 |  |  |
| fk-e536a9dd5f09 | Warm-lane task-meta path ambiguity causing repeated not-found errors | semantic | existing | 9430 |  | filed:9430 |  |  |
| fk-eaa27e3e892f | Escalation-watcher rotation script lives outside the target project root and requires an explicit env var, not cwd inference | semantic | existing | 9432 |  | filed:9432 |  |  |
| fk-7fb018091f5a | StructuredOutput double-wraps array under a key matching the schema's field name | curator | created | 9588 | tkt_0RWCTNTZB5BJ3A0K7BEGWKXJB4 | filed:9588 |  |  |

## Structural

These verified findings are structural (contract §6, D6): recorded in the codebook and NOT filed; each goes to deliberation with the human, then a program doc and /prd.

| finding_key | entry_id | title | anchor |
| --- | --- | --- | --- |
| fk-008cb070280b | entry-cand-20260716-5 | Write tool unavailable in autonomous escalation-watcher session — memory-write attempt lost | slug:escalation-watcher-write-tool-not-provisioned |

## Dispositions

- Re-verify at as_of_sha (D8): 1 due, 1 offered, 1 gone, 0 present, 0 inconclusive, 0 deferred by the verify cap.

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
| entry-cand-20260901-16 |  | tkt_0RW309CQ46Y4XMV82SNCC58XRQ | 7960 | done | done -> re-verified gone (fixed at 77df97265253c514ebd4fa0ef77361aafebeb929) | fixed:77df97265253c514ebd4fa0ef77361aafebeb929 |  |
| entry-cand-20260803-1 |  |  | 7908 | done | done -> entry already fixed | fixed:ad0ab1acf3b7b3bb5220e2e9c36e4ac3f5e43f67 |  |
| entry-cand-20260901-17 |  |  | 7909 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260914-17 |  |  | 7910 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20260914-18 |  |  | 7912 | done | done (task 9431 decides the entry) | filed:9431 |  |
| entry-cand-20260914-18 |  | tkt_0RW309D3JN4WBQX52BWG3KESDE | 9431 | pending | live | filed:9431 |  |
| entry-cand-20260921-16 |  |  | 7913 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
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
| entry-cand-20260926-5 |  |  | 8477 | cancelled | abandoned (eligible to re-file; not sighted this run) | open |  |
| entry-cand-20261006-7 |  |  | 8476 | done | done -> re-filing tkt_0RW309WXG8BWE3XX2DBAEEGMN6 from run census-reify-20261022 combined into this task by the curator; not re-filed while this task decides the entry | open |  |
| entry-cand-20260924-8 | fk-e536a9dd5f09 |  | 9430 | pending | live | filed:9430 |  |

- entry-cand-20260901-16: open -> fixed, fixed at 77df97265253c514ebd4fa0ef77361aafebeb929

## Cost

invoke calls: sonnet miner=60, sonnet verify=19, fable synthesis=1, haiku headroom-probe=4
