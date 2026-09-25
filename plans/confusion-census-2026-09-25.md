# confusion census 2026-09-25

Project: reify

## Saturation

- batches: 30
- stop reason: saturated
  - batch 0: dup_rate=0.73 (total=20, succeeded=15, failed=5, saturated=False)
  - batch 1: dup_rate=0.79 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 2: dup_rate=0.83 (total=20, succeeded=18, failed=2, saturated=False)
  - batch 3: dup_rate=0.79 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 4: dup_rate=0.84 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 5: dup_rate=0.74 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 6: dup_rate=0.75 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 7: dup_rate=0.89 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 8: dup_rate=0.95 (total=20, succeeded=20, failed=0, saturated=True)
  - batch 9: dup_rate=0.88 (total=20, succeeded=17, failed=3, saturated=False)
  - batch 10: dup_rate=0.74 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 11: dup_rate=0.89 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 12: dup_rate=0.68 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 13: dup_rate=0.84 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 14: dup_rate=0.74 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 15: dup_rate=0.95 (total=20, succeeded=20, failed=0, saturated=True)
  - batch 16: dup_rate=0.72 (total=20, succeeded=18, failed=2, saturated=False)
  - batch 17: dup_rate=0.74 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 18: dup_rate=0.79 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 19: dup_rate=0.89 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 20: dup_rate=0.79 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 21: dup_rate=0.79 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 22: dup_rate=0.78 (total=20, succeeded=18, failed=2, saturated=False)
  - batch 23: dup_rate=0.74 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 24: dup_rate=0.89 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 25: dup_rate=0.85 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 26: dup_rate=0.84 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 27: dup_rate=0.79 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 28: dup_rate=0.95 (total=20, succeeded=20, failed=0, saturated=True)
  - batch 29: dup_rate=0.90 (total=20, succeeded=20, failed=0, saturated=True)

## Origin x Manifestation Matrix

| origin \ manifested | prd | architect | implement | verify | review | merge | recon | ops | unknown |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| architect | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| implement | 0 | 0 | 21 | 1 | 1 | 0 | 0 | 0 | 0 |
| verify | 0 | 0 | 0 | 3 | 0 | 0 | 0 | 0 | 0 |
| review | 0 | 0 | 0 | 0 | 2 | 0 | 0 | 0 | 0 |
| merge | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 0 |
| recon | 0 | 0 | 0 | 0 | 0 | 0 | 5 | 0 | 0 |
| ops | 0 | 0 | 0 | 1 | 0 | 1 | 1 | 4 | 0 |
| unknown | 1 | 0 | 6 | 4 | 1 | 2 | 0 | 2 | 8 |

## Synthesis

Evidence gathering is complete. Writing the synthesis now.

**Date:** 2026-09-25
**Project:** reify
**Method:** periodic census per `plans/confusion-reduction-prd.md` §5 (η): stratified-random saturation mining (Sonnet) over session digests, per-finding verification against current main (Sonnet), then this synthesis (Fable). Sixty-six findings reached synthesis. This document adds a read of the reify codebook by session id and title, the task record for task 6084, the verify.sh source on main, the census runner's coder index builder and the digest's re-ingested-content classifier in dark-factory, and the loaded schema of the Monitor tool. Every mechanism claim below names the evidence it rests on; where the verifier's framing is contradicted by that evidence, the correction is stated.
**Companion artifact:** `docs/legibility/confusion-codebook.yaml`. Dispositions in §5 are inputs to the merger.
**Run notes:** second periodic census for reify. `census-state.json` reads `last_census_at: 2026-08-02`, 54 days before this run, against a 10-day calendar trigger. The codebook at synthesis time: 56 entries, 1,054 candidates, 2,161 sightings, 0 retired, 1.4 MB on disk (PRD §12 item 1 suggested deciding on compaction at ~100 KB). The input findings carry no dates; the nine sessions the codebook already dates fall between 2026-08-13 and 2026-09-21. Saturation statistics and filed-task ids are appended by the runner outside this synthesis. The synthesis sandbox refused scratch-file writes, `python3`, `awk`, `sort` and `uniq`, and the escalation and reify-debug MCP servers did not connect, so evidence is limited to greps, reads, one fused-memory `get_task`, and one tool-schema load.

### Corpus

- **66 verified findings, 61 sessions, 66 sightings, 11 clusters.** Five sessions carry two findings each (`f3200dc8`, `ff09b74c`, `ab4ab6df`, `b79c8b74`, `35d521ee`).
- **Sixteen of the 61 sessions are already in the codebook**, across 20 entries or candidates. Eight findings re-observe a record that already names the same session AND the same shape: `dd945f01` (cand-20260813-7), `d975796a` (cand-20260818-3), `b79c8b74` twice (cand-20260826-28's 09-10 sighting; cand-20260910-5), `46e186b5` (cand-20260901-10), `76421ac7` (cand-20260921-6), `ab4ab6df` (cand-20260916-4, the same retrieval event framed as a cross-project failure), and `bc6c828f` (entry-cand-20260725-2, heredoc with parentheses under eval). The verifier presented all eight as novel. The other eight recorded sessions are on record for a different shape than the one found here.
- **The candidates block has become the codebook's bulk.** By title, roughly 50 candidates describe MCP envelope markup leaking into a tool argument, roughly 35 describe a multi-line `python3 -c` script flattened to one line, roughly 33 describe review adjudication conflating "raised before" with "resolved", and several dozen describe warm-lane path or bootstrap assumptions. None of these four families has an entry. The reason is visible in the code: `scripts/legibility/coder.py::build_codebook_index` reads `codebook["entries"]` only, so the trickle coder is never shown the candidates block and re-files each recurring shape as a new candidate every night. This is a read of the index builder, not a reproduction of a coding run.
- **Phase stamps.** The verifier's stamps are carried through; this synthesis had no transcript access to refine them. Where the codebook already stamps the same session differently, it is noted in §1.

### Executive summary (observations)

1. **Shell exit-status semantics account for the largest cluster, and every instance is one command wide.** Eleven findings are a pipeline's `$?` read as the upstream command's status, an `&&` chain cut short by grep's no-match exit 1, a `;` chain reported by its last member, or a legitimate predicate exit 1 logged as tool error. In two of them the masked output contained real compiler errors or a rejected argument, printed immediately above the false `EXIT=0`. On main, `scripts/verify.sh` line 697 rejects an unknown argument with exit 64; the `head -4` pipe in session `d0b6781a` turned that into 0.
2. **The MCP envelope-markup leak is the most-repeated single shape and recurs within sessions after repair.** Seven sightings, all rejected by the middleware with `matched_pattern` `</invoke>`, `</rationale>` or `</content>`; three sessions leak again on a later call in the same session after receiving a `repaired_call`. One variant (session `84797189`) was refused with no repair and the text "NOTHING WAS PRESERVED". The middleware carries an `allow_mcp_markup` override (dark-factory `shared/src/shared/mcp_markup_middleware.py`); no sighting used it.
3. **Four sessions called `read_file` with an identical, non-harness argument shape, and all four manifest in the verify phase.** Each call is `read_file({"max_lines": "<n>", "path": "..."})`, with `max_lines` as a string, followed in two sessions by `grep_search` or `git_log`. Which role or model emitted these was not determinable from the digests; the uniformity is the observation.
4. **The self-blocking dependency finding rests on a premise the task record itself retracts.** Task 6084's record (read via `get_task` during this synthesis) carries `self_blocking_warning: RETRACTED 2026-08-07`: task 5261 has no `metadata.delivered_checks`, so the edge could not have blocked 6084. The edge removal stands as Leo's ruling; the verifier's cause text ("self-blocking by construction") describes the reasoning at removal time, not a verified mechanism.
5. **Two findings against the digest instrument name injection sources the classifier does not list.** `is_reingested_content` in `scripts/legibility/digest.py` unions a coder-judgment check with `is_harness_injected_turn`, whose marker tuples (`HARNESS_PROMPT_MARKERS`, `HARNESS_CONTEXT_BLOCK_MARKERS`) contain neither `<task-notification>` nor "New Escalation for Task". The heading-set inventory was not read in full, so whether the escalation heading matches by that route is unverified.

### Origin × manifestation matrix

The verifier's stamps, unrefined.

| origin \ manifested | prd | architect | implement | verify | review | merge | recon | ops | unknown | **total** |
|---|---|---|---|---|---|---|---|---|---|---|
| architect | · | 1 | · | · | · | · | · | · | · | **1** |
| implement | · | · | 21 | 1 | 1 | · | · | · | · | **23** |
| verify | · | · | · | 3 | · | · | · | · | · | **3** |
| review | · | · | · | · | 2 | · | · | · | · | **2** |
| merge | · | · | · | · | · | 1 | · | · | · | **1** |
| recon | · | · | · | · | · | · | 5 | · | · | **5** |
| ops | · | · | · | 1 | · | 1 | 1 | 4 | · | **7** |
| unknown | 1 | · | 6 | 4 | 1 | 2 | · | 2 | 8 | **24** |
| **total** | **1** | **1** | **27** | **9** | **4** | **4** | **6** | **6** | **8** | **66** |

Readings, observational. Twenty-seven of 66 sightings manifest in implement sessions; 24 origins are `unknown`, and eight sightings are unknown on both axes, all of them shell-construction or tool-limit findings where the digest carries no role briefing. Unlike dark-factory's recent censuses, the verify and merge columns are non-zero here: nine and four. Off-diagonal cells are few: implement→review (`0ca774a9`, a safety claim caught by the reviewer), implement→verify (`45f3034b`), ops→verify (`efeb7a65`), ops→merge (`1eb96f46`), ops→recon (`35d521ee`). The verifier stamped `d975796a` implement×implement; the codebook's cand-20260818-3 stamps the same session architect×verify.

### 1. Verified clusters

#### 1.1 Pipeline, chain and predicate exit statuses read as the command's verdict (11 sightings, 11 sessions)

- **`$?` after a pipe is the last stage's.** `d0b6781a`: `verify.sh test --confirm-failed --print-plan 2>&1 | head -4; echo EXIT=$?` printed the script's "unknown argument" error and `EXIT=0`. `bf9c927b`: `reify build … | tail -15; echo exit=$?` printed three compiler errors including `E_MODULE_PATH_MISMATCH` above `exit=0`; the Bash tool's exit 2 came from an unrelated trailing `ls`.
- **`&&` chains cut short by grep's exit 1.** `ccbd37e9` and `955656cb`: `echo header && grep … && echo header && grep …` printed the first header only. `01423347`: an optional `ls <glob>` existence check exited 2 and dropped the `git diff` the command existed to run.
- **Chain reported by its last member.** `8bdb45d3`: a `;` chain of lookups found the gmsh binary in its `find` stage and was logged exit 2 from a trailing `ls -d`. `ae297a2a`: `git merge-base --is-ancestor main HEAD` returned its meaningful 1 as the whole call's status. `9f2d300d`: `wc -l` over several files printed counts and a total, exited nonzero for one missing fixture, and that fixture's absence recurred as not-found at five later turns.
- **Pipe closure.** `a120ddca`: under `set -euo pipefail` a downstream consumer closed early and the helper died 141, indistinguishable from no-match until the agent added rc prints. `b7e3fb64`: `head --short` exited on the bad flag; the Rust producer then panicked on SIGPIPE, and the panic trace dominated the output above the one-line real cause.
- **eval parses the whole string first.** `bc6c828f`: a heredoc syntax error voided a `git add && git status && git commit` chain; after switching to `git commit -F`, the file was not re-staged. The codebook holds this session on entry-cand-20260725-2 ("Heredoc commit message with literal parentheses breaks under eval-wrapped bash execution").

**Codebook state.** No entry carries the pipeline-exit shape. Candidates "Grep no-match exit code surfaces as a false tool_error" and "Multi-file wc command reports failure exit code while still printing valid partial output" cover two facets.

#### 1.2 Tool-call envelope markup serialized into a free-text argument (7 sightings, 7 sessions)

Rejections by `mcp_markup_detected` in `add_design_decision.rationale` (`0187efa5`, `f3200dc8`, `b79c8b74`) and `add_memory.content` (`46e186b5`, `84797189`, `e9740693`, `cdcd4dea`). Three sessions record the leak recurring on a later call after a `repaired_call` was offered: turns 262 and 281 (`0187efa5`), 279 and 288 (`b79c8b74`), two unrelated decisions (`f3200dc8`). Session `84797189` received the no-repair variant: "boundary cannot be determined, so no repair was attempted … NOTHING WAS PRESERVED". Phases: five implement, one architect, one ops.

**Codebook state.** No entry. About 50 candidates by title since cand-20260826-28 (first seen 08-26, which already holds `b79c8b74`'s 09-10 sighting); cand-20260901-10 holds `46e186b5`. Candidate cand-20260910-4 records six rejections in one session (`a3609b7b`). The within-session recurrence is therefore already on record twice.

#### 1.3 Command text constructed wrongly before the shell sees it (6 sightings, 6 sessions)

`50d2e5f7` and `ea12b484`: multi-statement Python in `python3 -c` reached the interpreter on one line and raised `IndentationError` at line 1 (`50d2e5f7` inside an `&&` chain; `ea12b484` written with spaces as separators). `a6e3e1d9`: Rust source containing `\u` inside a non-raw Python triple-quoted string; `unicodeescape` SyntaxError before any edit ran. `b3f4d5ae`: a grep pattern with three literal backticks; bash's eval reported "unexpected EOF while looking for matching ``'" before grep ran. `e76ce2d8`: an alternation built by concatenating literal `(` from function signatures; ripgrep "unclosed group". `68c34428`: `sed -n 900,945 file` with no `p`; "missing command".

**Codebook state.** No entry for flattening; about 35 candidate titles, the earliest read here first seen 08-13. Candidates exist for backticks under eval ("Bash regex pattern with backtick loses escaping in JSON→shell→eval chain", "Unmatched backtick in grep pattern"). Nothing for the `sed` or ripgrep-group shapes.

#### 1.4 Unbounded or self-targeting process actions (5 sightings, 5 sessions)

- `5a6f083d` and `71d9409c`: `pkill -f "<pattern>"` where the pattern is a literal inside the invoking `bash -c` line; the call ended with exit 144 and the agent could not tell a self-kill from a failed reap. Candidate "pkill -f with broad pattern matching invoking shell causes exit 144" holds this shape with another session.
- `3b6e2087`: a background grep for `grammar_substrate_usable` with no scope or timeout, ended by a manual `pkill` (itself exit 144).
- `3abbf824`: two `timeout 2400 cargo test -p reify-eval` runs chained in one call to derive two numbers from one output. The codebook holds this session on entry-cand-20260724-6 (foreground build killed by harness timeout).
- `efeb7a65`: `find . -name <file>` from the dark-factory root, excluding only `node_modules`, matched the same file across `.worktrees-orphaned/*` and `.eval-worktrees/*` and hit the 2-minute cap. Today those directories hold 1 and 11 children respectively; the evidence quote's timestamps are from 2026-08-26.

#### 1.5 Tool names, schemas and limits guessed rather than read (9 sightings, 8 sessions)

- **Hallucinated tool names**, four sessions (`23b98b66`, `d421d2a8`, `066b59dc`, `64f3d599`): `read_file` with `max_lines` as a string and `path`, repeated after the first "No such tool available", then `grep_search` or `git_log`. All four manifest in verify. Candidate "Cross-project tool-name hallucination: generic Claude Code tools invoked outside dark-factory context" holds the shape.
- **Namespace crossed**: `151229c9` called `mcp__escalation__get_task` (fused-memory's name) on the escalation server. The escalation server did not connect during this synthesis, so its actual tool list is unverified here.
- **Schema not loaded**: `f425da46` called Monitor with `condition` and `timeoutSeconds`; the loaded schema has `description`, `timeout_ms`, `command`, `ws`. Candidate "Deferred tool invoked before loading its schema via ToolSearch" and entry-adjacent candidate "Guard advises Monitor for polling, but Monitor schema not preloaded" hold the shape.
- **Malformed call**: `7e5a4cb9` emitted `{"file_path": …, "offset": 1, 200}`, a bare positional value.
- **Required argument missing after a timeout**: `76421ac7`'s first `merge_request` timed out; the retry surfaced Pydantic "worktree: Missing required argument". Already cand-20260921-6, same session.
- **Tool limit**: `a516dde2` read `crates/reify-core/src/diagnostics.rs` whole and hit the 256 KB cap at 345.5 KB; the file is 436,203 bytes on main today. The digest shows follow-on not-found misses and no self-correction marker.

#### 1.6 fused-memory calls that time out or disconnect, retried with the argument varied (6 sightings, 6 sessions)

`c455234a` (`get_task` → "The operation timed out", no detail); `24154e2c` (`add_memory` timed out at turn 276, resent identical at 281, timed out again); `da478aea` (`add_memory` of a session summary timed out at session end, no retry); `f17f35db` (`resolve_ticket` timed out five times with five ticket ids, then once against dark-factory); `08bbe7f6` (one `-32603` then three "not connected" for three task ids); `ab4ab6df` (`get_memories_by_metadata` with the same topic filter three times). Whether any write landed server-side before the client timeout was not checked by any session and could not be checked here.

**Codebook state.** No entry. Candidates cover the ticket-timeout shape at least seven times ("resolve_ticket MCP call times out regardless of extended client timeout", "MCP resolve_ticket timeout retry chain without recovery strategy", "MCP get_task timeout on cross-project project_root contamination", and others), the duplicate-write risk once ("MCP write-call client timeout leaves duplicate-submission risk on submit_task"), and identical retry once ("Identical retry on empty/not-found memory lookup instead of changing strategy"). `f17f35db` is on record on three entries (20260726-1, 20260722-4, 20260729-6), none of which is this shape. entry-cand-20260727-2 is the nearest entry-level shape: an error retried by varying only the argument.

#### 1.7 Worktree layout and plan.json assumptions (6 sightings, 6 sessions)

- **Paths**: `d975796a` issued repo-root-relative paths for 160 turns from a cwd inside `crates/reify-compiler/tests` (seven not-founds); already cand-20260818-3, same session. `4154b8d0` assumed `harness_cli/` through two explicit not-founds and corrected at turn 90.
- **plan.json step schema**: `94df703e` and `b79c8b74` keyed `s['step_type']`; `f3200dc8` sliced `s['description'][:60]` on a step whose fields are still `None`. `step_type` appears nowhere in `orchestrator/src/orchestrator/*.py` on dark-factory main. cand-20260910-5 holds `b79c8b74`; three further candidate titles name `step_type` and five name the null `commit` field.
- **Concurrent actor**: `1eb96f46` parked a worktree READY; its HEAD reflog then shows a checkout to `task/5689` and back to main by another process, and the shared cargo-check log was overwritten with `/home/leo/src/reify-fix2` paths. Nothing surfaced this until the agent's own final review.

#### 1.8 Waiting on a merge outcome (1 sighting, session `041926f2`)

Two background Bash waits were stopped; Monitor then needed three re-arms for one merge outcome and two for a queue drain. The loaded Monitor schema caps `timeout_ms` at 600,000 (10 minutes) with a 5-minute default, so a wait longer than that is re-armed by construction. The session is on record on entry-cand-20260722-2 (foreground polling loop hits hard timeout); candidates "Merge-queue landing required a long chain of manual backoff polls and Monitor re-arms" and "Merge-status Monitor repeatedly times out across a multi-resubmission merge" hold this shape.

#### 1.9 The instrument: injected notifications filed under User Corrections (2 sightings, 2 sessions)

`50e12d17`: a `<task-notification>` reporting a sub-agent's weekly-limit failure. `77fb6617`: a "# New Escalation for Task 5099" payload ending "Handle this escalation, then call `resolve_issue`". entry-cand-20260730-10 records the memory-hint variant. §Executive summary item 5 states what the classifier's marker tuples contain.

#### 1.10 Premises checked in the wrong context, or not checked (9 sightings, 8 sessions)

- `0a364f16`: a memory-prescribed probe, `readlink /proc/self/fd/1` inside command substitution, reads its own substitution pipe and returns `pipe:*` unconditionally; the session's own Block V proved it.
- `0ca774a9`: a 12× heavy-test ceiling raise justified by "REIFY_GATE_EXCLUDE_HEAVY=1 is set for every role". On main, `scripts/verify.sh` lines 849 to 852 apply the exclusion only when `DF_VERIFY_ROLE` is `task` or `merge`; the `background` arm (line 817) is commented "merge-level completeness". The reviewer's correction stands.
- `8da8494d`: `prefer_local` treated as a live orchestrator key across turns 7 to 300; no such key exists in `orchestrator/src` or `dark-factory-orchestrator.yaml`. Self-corrected at turn 392. The session is on record on entry-cand-20260728-4, entry-cand-20260730-6 and cand-20260904-7, none of which is this shape.
- `36ec86dc`: a cap-kill and a verdict-discard narrated as one invocation; runs.db showed four `error_max_budget_usd` events and a fifth, different invocation.
- `35d521ee`: `/proc/<pid>/environ` of the user manager denied (same UID); and the prior triage's refutation of the PATH premise was made in an initialised session rather than the timer's environment. Already cand-20260819-5 by session, different framing.
- `dd945f01`: guard tests grepping comment prose, with split string literals to avoid self-matching. Already cand-20260813-7, same session, same shape.
- `6defcc8d`: the prior-round suggestion list conflates deferred with resolved. About 33 candidate titles carry this shape since 08-13; no entry.
- `45f3034b`: `module <name>` must equal the file basename; `E_MODULE_PATH_MISMATCH` is emitted from `crates/reify-compiler/src/compile_builder/pre_pass.rs`. One wasted eval, then a written warning for later runs.

#### 1.11 Task-graph and curation semantics (4 sightings, 3 sessions)

- `e7bb4b33`: the 6084→5261 edge, added "for bookkeeping", was removed by Leo's ruling. The task record's `self_blocking_warning` is marked RETRACTED because 5261 carries no `delivered_checks`; the record's `dep_on_5261_dropped_2026_08_07` block preserves both the ruling and the retraction. The codebook's entry-cand-20260728-4 ("Cross-PRD dependency fields conflated: enforced edges vs informational prose") is the same underlying shape: one enforced edge type carrying informational intent.
- `ff09b74c` twice: 8-hex UUID prefixes cited in memory text with no resolver or write-time check; and one entity listed in two consolidation gates' member lists, halting a fold. Neither shape is in the codebook by title.
- `ab4ab6df`: the retain-and-tag procedure lacks rules for a pending dependent (widen vs file) and for re-checking peer staleness; Leo supplied both mid-session. Same session as the retrieval retry in §1.6.

### 2. Overlap with the codebook, in one table

| finding | session | codebook state before this census |
|---|---|---|
| meta-tests on comment prose | `dd945f01` | cand-20260813-7, same session, same shape |
| repo-root paths from nested cwd | `d975796a` | cand-20260818-3, same session, same shape (stamped architect×verify there) |
| rationale markup leak | `b79c8b74` | cand-20260826-28, same session, same shape |
| `step_type` KeyError | `b79c8b74` | cand-20260910-5, same session, same shape |
| content markup leak | `46e186b5` | cand-20260901-10, same session, same shape |
| `merge_request` missing `worktree` | `76421ac7` | cand-20260921-6, same session, same shape |
| metadata query ×3 | `ab4ab6df` | cand-20260916-4, same session, same event |
| eval voids `git add` chain | `bc6c828f` | entry-cand-20260725-2, same session, same shape |
| `prefer_local` premise | `8da8494d` | session on three records, none this shape |
| `/proc` environ; wrong-context check | `35d521ee` | cand-20260819-5, same session, different framing |
| `wc` partial output | `9f2d300d` | session on entry-20260801-2 and cand-20260915-8, neither this shape |
| `resolve_ticket` ×5 | `f17f35db` | session on three entries, none this shape |
| Monitor re-arms | `041926f2` | entry-cand-20260722-2, same session, adjacent shape |
| `head` pipe masks exit | `d0b6781a` | session on two warm-lane entries, not this shape |
| task-notification as correction | `50e12d17` | session on entry-20260727-6, not this shape |
| duplicate full-suite run | `3abbf824` | entry-cand-20260724-6, same session, adjacent shape |
| remaining 50 findings, 45 sessions | | no session match; 4 shape families each present as 30 to 50 candidates |

Eight of 66 findings re-observe a same-session record; a further eight sessions are on record for another shape. The verifier's novelty screen did not consult `candidates:` by session id, the same gap dark-factory's 09-20 census recorded.

### 3. Observations the runner may act on

These are observations, not rulings.

- **The four largest families have no entry and cannot acquire one through the trickle.** With `build_codebook_index` reading `entries` only, each night's coder re-files markup leaks, `python3 -c` flattening, resolved-vs-deferred conflation and warm-lane path assumptions as fresh candidates. Promotion is the census's job by design (PRD §6 item 3); this census is the first opportunity since 08-02 to do it, and §5 proposes the four promotions.
- **The codebook is 14× the size at which the PRD asked for a compaction decision.** 2,161 sightings across 1,110 records; the trickle commits it nightly.
- **Exit-status masking is checkable on every future sighting from the command text alone**: a `| head`/`| tail` followed by `$?`, a `&&` chain whose members include a bare `grep`, or a `;` chain reported by its last member. All eleven sightings in §1.1 would match such a rule; whether any command text is a false positive was not measured.
- **The self-blocking finding should carry the retraction**, not the removal rationale, if it is recorded (§1.11).
- **Two injection sources are absent from the classifier's marker inventory** (§1.9). The PRD's own note calls a newly-sighted shape "a one-line addition"; whether the heading-set rule already catches "# New Escalation for Task" was not read.

### 4. What this census did not verify

The escalation MCP server's tool list (server unreachable); which role or model produced the four `read_file` calls; whether any timed-out fused-memory write in §1.6 landed server-side; the trickle-coder behaviour implied by the index-builder read (a code read, not a run); the digest heading-set rule for the escalation notification; every phase stamp, which is the verifier's; and the dates of the 52 sessions the codebook does not already hold.

### 5. Codebook dispositions (inputs to the merger)

- **Promote cand-20260826-28** to an entry, "Tool-call envelope markup serialized into a free-text argument, recurring within a session after repair"; fold the other markup candidates as sightings; add `0187efa5`, `f3200dc8`, `84797189`, `e9740693`, `cdcd4dea` (with the no-repair variant noted on `84797189`); `46e186b5` and `b79c8b74` are already recorded.
- **Promote cand-20260813-6** ("deferred concerns conflated with resolved") to an entry; fold the ~33 same-shape candidates; add `6defcc8d`.
- **Promote the earliest `python3 -c` flattening candidate** to an entry; fold the ~35 same-shape candidates; add `50d2e5f7`, `ea12b484`. Keep `a6e3e1d9` (unicodeescape), `b3f4d5ae` (backticks under eval, fold to the existing backtick candidates), `e76ce2d8` and `68c34428` as separate candidates.
- **New entry: pipeline, chain and predicate exit status read as the command's verdict**; sightings `d0b6781a`, `bf9c927b`, `8bdb45d3`, `ae297a2a`, `9f2d300d`, `ccbd37e9`, `955656cb`, `01423347`; fold the two grep/wc candidates. Add `a120ddca` and `b7e3fb64` as a linked pipe-closure candidate.
- **Promote cand-20260910-5** (`step_type`) to an entry titled for the plan.json step schema; fold the other `step_type` and null-`commit` candidates; add `94df703e`, `f3200dc8`.
- **Promote the tool-name hallucination candidate**; add `23b98b66`, `d421d2a8`, `066b59dc`, `64f3d599`, noting the shared argument shape and verify-phase manifestation. Add `151229c9` (namespace), `f425da46` (fold to the ToolSearch candidate), `7e5a4cb9`, `a516dde2` as candidates.
- **New entry: fused-memory timeout or disconnect retried with the argument varied**; fold the ticket-timeout and duplicate-write candidates; add `c455234a`, `24154e2c`, `da478aea`, `f17f35db`, `08bbe7f6`; `ab4ab6df` is recorded.
- **pkill self-match candidate**: add `5a6f083d`, `71d9409c`. Add `3b6e2087`, `efeb7a65` as candidates; `3abbf824` as a sighting on entry-cand-20260724-6 with the duplicate-invocation note.
- **entry-cand-20260728-4**: add `e7bb4b33` (ops×ops) with the retraction from task 6084's record.
- **entry-cand-20260730-10**: no new sighting; add two linked candidates for the task-notification (`50e12d17`) and escalation-notification (`77fb6617`) sources.
- **entry-cand-20260722-2**: add `041926f2` (unknown×merge) noting the three and two re-arm rounds.
- **New candidates**: `0a364f16`, `0ca774a9`, `8da8494d` (prefer_local), `36ec86dc`, `35d521ee` (two, linked to cand-20260819-5), `45f3034b`, `ff09b74c` (two), `ab4ab6df` (procedure gaps), `1eb96f46`, `4154b8d0`.
- **No entry retires.** `dd945f01`, `d975796a`, `76421ac7`, `bc6c828f`: already recorded, no action.


## Filed Tasks

_59 ticket(s) filed -- the curator's create/combine/drop decision is still pending, so no task id exists yet; resolve_ticket returns the task id once it does._

- tkt_0RV2QFBHZX8X86V8ZNX1GK5QTT
- tkt_0RV2QFBKNRDQX8VV386GZP9AMW
- tkt_0RV2QFBMWGK169X5Y0P5DVXYHJ
- tkt_0RV2QFBNM2GYGRJRH8F0V1VTJ1
- tkt_0RV2QFBPB6J5Y5M8F0QSM7R802
- tkt_0RV2QFBRY8R29WPGYK94PSWJ2Y
- tkt_0RV2QFBSX5JFAEQ5G4A9X9RSKS
- tkt_0RV2QFBTJ9HG6DYRBEFZN9VQ8P
- tkt_0RV2QFC59GVD26VVCAASRDKB0M
- tkt_0RV2QFC6EAZ1Q1Q503EZ96Q12S
- tkt_0RV2QFC7RSPCWRH1X520B9MSQ3
- tkt_0RV2QFC88J749XDGVFA0V15D0S
- tkt_0RV2QFC8KVDPWX5VDE67PMFE1Q
- tkt_0RV2QFC91GYA8DEP8ZHY5P8M2A
- tkt_0RV2QFC9HKTCM76VE9TQ5QHGVP
- tkt_0RV2QFCA48RWWCGS3VNNT26HDD
- tkt_0RV2QFCB2NMZCTN7A6AP8TPFHY
- tkt_0RV2QFCCMXW73YYCR21PJPFSXT
- tkt_0RV2QFCE8RP4QBH475KMBDJFER
- tkt_0RV2QFCF52XBRFSEB2T372E18D
- tkt_0RV2QFCG14M3MYE2PP1S5HZXVY
- tkt_0RV2QFCGJMMKRQCZTFX7YDV3C7
- tkt_0RV2QFCJ7QZAHFAD338H9H80P1
- tkt_0RV2QFCJQRA9JMS4M7TP4D3NCC
- tkt_0RV2QFCKB0YX7B9BXS5PRDD72Z
- tkt_0RV2QFCKS6MKRQDT25BFAZWZVD
- tkt_0RV2QFCMBECJXFWJEF1TPYWQ25
- tkt_0RV2QFCMZVDCRHMJW32F5QBWP4
- tkt_0RV2QFCNAA3YFRQJE1F3VMM4YA
- tkt_0RV2QFCNNXNYCABA5MZ82HNH5F
- tkt_0RV2QFCP0CRR8N57S9K2C5EAEC
- tkt_0RV2QFCPAQJJ4YTTHKSTQ0NAEQ
- tkt_0RV2QFCQDRJ0EXTMZM10SPAYPQ
- tkt_0RV2QFCR4X2W849NX2HY53CBNA
- tkt_0RV2QFCRR385MVF0MA5P2YD84T
- tkt_0RV2QFCSBJH0YWMY191N58K0QV
- tkt_0RV2QFCT5NTNXN7RY94FKM323P
- tkt_0RV2QFCTN9J56Y3WY387J3GFS1
- tkt_0RV2QFCV1NGKPF48QYGN1S84QN
- tkt_0RV2QFCVA0QEY21W3T15CR6CA8
- tkt_0RV2QFCVKJ2JH5SC9H3J1YH3GE
- tkt_0RV2QFCVY0ZJ4S0QW31SDR801X
- tkt_0RV2QFCW5TZJERDD3490EGB8H1
- tkt_0RV2QFCWVY7WEBX7DDG1SZQ44P
- tkt_0RV2QFCXAY5JT750QWHW5DS5AS
- tkt_0RV2QFCY12HYEZ9WZAG2QTVD4K
- tkt_0RV2QFCYBF6M5KAEH5GRH499J0
- tkt_0RV2QFCYNC2GZEMTV5D4YRZMJN
- tkt_0RV2QFCZ29HC55BQZBQXWBZFY8
- tkt_0RV2QFCZXBFDDBVD4YMBV2JR1J
- tkt_0RV2QFD0F8JJQZZFYRNVMJ4BQP
- tkt_0RV2QFD12CBNW8J8RPQN8MTB7V
- tkt_0RV2QFD2FE8NDPR2656H13BMQ2
- tkt_0RV2QFD2YC9D0DNDBJEX5W6JG9
- tkt_0RV2QFD3H4DPGNAC3ECMJXSP62
- tkt_0RV2QFD43M6VQEG5NH2J53PX2N
- tkt_0RV2QFD68F55SEPV5X3CKJ8WRK
- tkt_0RV2QFD6VRJ15G1K89GTD7KJEY
- tkt_0RV2QFD7WAGGNSY1GNGTRPT8DV

## Cost

invoke calls: sonnet miner=600, sonnet verify=114, fable synthesis=1, haiku headroom-probe=24
