# confusion census 2026-08-02

Project: reify

_--force: operator-initiated run._

## Saturation

- batches: 50
- stop reason: capped
- coverage: mined 1000 session digest(s) across 50 batch(es); operator batch cap = 50 batch(es) -- mining was BOUNDED BY THE CAP, not run to saturation: sessions beyond the cap were NOT mined, so this census is PARTIAL coverage, not a full sweep.
- NOT PICKED UP LATER: this run still advances last_census_at, so the next census window starts here -- the capped-away sessions fall outside it and are never re-enumerated. Sweeping them means rolling last_census_at back in docs/legibility/census-state.json before the next run; a plain re-run will not reach them.
  - batch 0: dup_rate=0.55 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 1: dup_rate=0.60 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 2: dup_rate=0.65 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 3: dup_rate=0.53 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 4: dup_rate=0.35 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 5: dup_rate=0.65 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 6: dup_rate=0.60 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 7: dup_rate=0.80 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 8: dup_rate=0.37 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 9: dup_rate=0.70 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 10: dup_rate=0.40 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 11: dup_rate=0.75 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 12: dup_rate=0.50 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 13: dup_rate=0.55 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 14: dup_rate=0.55 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 15: dup_rate=0.70 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 16: dup_rate=0.70 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 17: dup_rate=0.60 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 18: dup_rate=0.47 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 19: dup_rate=0.80 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 20: dup_rate=0.68 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 21: dup_rate=0.63 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 22: dup_rate=0.65 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 23: dup_rate=0.60 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 24: dup_rate=0.50 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 25: dup_rate=0.60 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 26: dup_rate=0.60 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 27: dup_rate=0.80 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 28: dup_rate=0.70 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 29: dup_rate=0.79 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 30: dup_rate=0.44 (total=20, succeeded=18, failed=2, saturated=False)
  - batch 31: dup_rate=0.50 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 32: dup_rate=0.55 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 33: dup_rate=0.80 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 34: dup_rate=0.58 (total=20, succeeded=19, failed=1, saturated=False)
  - batch 35: dup_rate=0.65 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 36: dup_rate=0.40 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 37: dup_rate=0.60 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 38: dup_rate=0.55 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 39: dup_rate=0.70 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 40: dup_rate=0.65 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 41: dup_rate=0.60 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 42: dup_rate=0.40 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 43: dup_rate=0.45 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 44: dup_rate=0.55 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 45: dup_rate=0.55 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 46: dup_rate=0.65 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 47: dup_rate=0.30 (total=20, succeeded=10, failed=10, saturated=False)
  - batch 48: dup_rate=1.00 (total=20, succeeded=1, failed=19, saturated=False)
  - batch 49: dup_rate=0.00 (total=20, succeeded=0, failed=20, saturated=False)

## Verification

- verified 150 of 533 novel clusters (operator verify cap: 150); 383 deferred as pending candidates -- merged into the codebook by this run but NOT verified; adjudication deferred to a later census.
- a deferred candidate is re-adjudicated only if the same confusion RECURS in a later window: this run advances last_census_at, so these sightings are never re-mined. A one-off deferred by the cap stays pending until it is adjudicated by hand.

## Origin x Manifestation Matrix

| origin \ manifested | prd | decompose | architect | implement | verify | ops | unknown |
| --- | --- | --- | --- | --- | --- | --- | --- |
| prd | 2 | 0 | 0 | 0 | 0 | 0 | 0 |
| decompose | 0 | 1 | 1 | 0 | 0 | 1 | 0 |
| implement | 0 | 0 | 0 | 6 | 1 | 1 | 0 |
| recon | 0 | 0 | 0 | 2 | 0 | 0 | 0 |
| ops | 0 | 0 | 0 | 0 | 0 | 17 | 0 |
| unknown | 0 | 0 | 0 | 11 | 1 | 5 | 7 |

## Synthesis

All verification checks are done. Key code-reading results against current main: `is_harness_injected_turn` (digest.py:593) still requires all three briefing headings to co-occur — the exact gap the 07-31 census's method notes flagged; watcher-rearm.sh hard-requires `DARK_FACTORY_ROOT` (line 132) and self-declares its CEILING outcome machine-readably (line 197); the escalation-watcher SKILL.md documents both prerequisites, so the gap is in what spawned rotations can see; the sleep-chain guard has no source in `hooks/` — it is harness-level; and the codebook already holds `entry-cand-20260722-3`, `watcher-loop-harness-mismatch`, and `watcher-capability-envelope`, which several clusters recur.

Here is the synthesis for the dated census report:

---

# Confusion census — 2026-08-03

**Date:** 2026-08-03
**Method:** periodic census per `plans/confusion-reduction-prd.md` §5 (η): stratified-random saturation mining (Sonnet) over session digests → per-finding verification against current main (Sonnet) → this synthesis (Fable). Every finding restated below survived the verification stage; this synthesis adds clustering, counting, and code-reading against current main only — no diagnosis appears here that was not itself verified.
**Companion artifact:** `docs/legibility/confusion-codebook.yaml` (dispositions in §5 are inputs to the merger, which promotes/rejects in place).
**Run notes:** third completed periodic census. Previous: 2026-07-31 (`plans/confusion-census-2026-07-31.md`, 15 findings / 4 clusters) and 2026-07-24 (52 findings / 9 clusters). Saturation statistics and filed-task ids are appended by the census runner outside this synthesis.

## Corpus

- **56 verified findings across 46 unique sessions.** Ten sessions contributed two findings each (64c5541b, da9d8468, 6acb9fe4, ed41d6f5, b020e966, 7bc39ff3, 77847b75, 5d2eb75b, 74cf001b, fa5527d4); the rest one each.
- **Composition inverted since 07-31.** Instrument findings (defects in the legibility pipeline itself) fell from 12/15 (80%) to **2/56 (~4%)** — and both recur scopes the 07-31 report explicitly left open (§1.9). The batch is dominated by subject-agent confusion in **cross-project operation**: 17 findings across 15 sessions are autonomous escalation-watcher rotations against the reify target, and roughly 20 more come from dispatched sessions working inside warm-lanes task worktrees. Around two-thirds of the batch arises where dark-factory tooling operates on or inside another project.
- Phase-stamp coverage: **24/56 findings carry `origin_phase: unknown`** (43%); 7 are unknown/unknown. **Zero merge-manifested sightings for the third consecutive census.** Verify-manifested is **nonzero for the first time** (2), and six findings carry known, unequal origin/manifestation stamps — the first census with repeatedly sighted cross-phase propagation (§ matrix).

## Executive summary (observations)

1. **The instrument receded; the subjects dominated.** 54/56 findings are subject-agent confusion. The two instrument recurrences are precisely the two scopes the 07-31 census marked open: the briefing-heading filter gap and the error-channel exit-code conflation. Code-reading on main confirms both remain: `is_harness_injected_turn` (`scripts/legibility/digest.py:593`) still requires all three headings `# context` + `## agent identity` + `# task` to co-occur as stripped lines, while the sighted mislabeled turn quotes only `# Context`/`## Project Context`; and the `tool_error` extraction has no exit-code or designed-outcome awareness, so `WATCHER_REARM_OUTCOME: CEILING exit=124` remains countable as an error — 13 times in one sighted session. The 07-31 report's own "Filed Tasks" section reads "none filed", consistent with no remediation having landed.
2. **The largest single mass is the escalation-watcher's cross-repo bootstrap gap, recurring at scale.** Seven sessions — each an autonomous rotation against reify — independently rediscovered by trial and error that `watcher-rearm.sh` is a dark-factory tool requiring `DARK_FACTORY_ROOT` (`scripts/watcher-rearm.sh:132-134`), which their environment left unset and their injected briefing never mentioned. This is codebook `entry-cand-20260722-3`, open since 07-22, now with seven new sightings. The requirement *is* documented on main — `skills/escalation-watcher/SKILL.md:21` names the env var as prerequisite #1 and line 134 shows the canonical `cd $DARK_FACTORY_ROOT && scripts/watcher-rearm.sh` form — so the sighted gap is between what the skill documents and what the spawned rotation's environment and banner actually carry.
3. **Bounded-wait outcomes were read as retryable failures.** Five sessions re-issued a failing or ceiling-hitting wait essentially unchanged — 5×, 8×, 13×, 21+4×, and a second full-hour wait against a 4-hour rotation budget — and at least three ended only when the user manually rejected a tool call, in nominally unattended roles. The rearm script self-declares a machine-readable outcome (`WATCHER_REARM_OUTCOME: <FIRED|CEILING|KILLED|ERROR>`, `watcher-rearm.sh:37,197-198`); no sighted session branched on it.
4. **Waiting on background work is the top harness-level confusion.** Six sessions tripped the anti-polling guard by chaining `sleep N; tail ...` — the guard is harness-level (no matching guard text exists in this repo's `hooks/`), and every sighting discovers it reactively, at the cost of a turn. Substitutes chosen after (or instead of) the guard trip were also wrong: three consecutive manual `Read` calls as an ad-hoc wait, a hand-rolled foreground `until` loop that consumed its full 10-minute timeout, `ScheduleWakeup` invoked without its required `prompt` (it is /loop-scoped, not a general delay), and `TaskOutput` polled with ids the registry no longer knew.
5. **Warm-lane per-lane task metadata is undiscoverable or absent.** Eight sessions probed `worktrees/.task-meta/<lane>/` artifacts: three guessed the path shape wrong (sibling-of-`worktrees/` instead of nested), four found `iterations.jsonl` or `plan.json` absent at the correct path — including one lane handed to an agent with no `plan.json` at its very first tool call, and one symlink that lists healthy but resolves to an empty/missing target — and one ran a two-minute tree-wide `find` instead of `readlink -f` on the symlink.
6. **Cross-project memory content reached dispatched agents' briefings and was acted on.** Three warm-lanes worktree sessions received `# Context` blocks carrying Reify architecture/task facts (tasks 2310/3539/4543, `crates/reify-*`) and then attempted `cd` into reify paths that do not exist in their worktree, producing chained not-found failures. Two of the three are stamped recon→implement — the first census in which origin-side context assembly is repeatedly implicated in implement-phase manifestations.
7. **Two findings manifested in verify — the column's first nonzero cells** — but both are harness/continuity shapes (a 10-minute foreground build kill; a usage-limit resume re-running a completed suite), not the architect/implement→merge/verify propagation the PRD hypothesized, which remains unsighted.

## Origin × manifestation matrix

Rows = `origin_phase`, columns = `manifested_phase`. Counts are verified sightings (56 total). Zero-origin rows (architect, verify, review, merge) omitted; `merge`/`review`/`recon` columns kept to show their zeros.

| origin \ manifested | prd | decompose | architect | implement | verify | review | merge | recon | ops | unknown | **total** |
|---|---|---|---|---|---|---|---|---|---|---|---|
| prd | 2 | · | · | · | · | · | · | · | · | · | **2** |
| decompose | · | 1 | 1 | · | · | · | · | · | 1 | · | **3** |
| implement | · | · | · | 6 | 1 | · | · | · | 1 | · | **8** |
| recon | · | · | · | 2 | · | · | · | · | · | · | **2** |
| ops | · | · | · | · | · | · | · | · | 17 | · | **17** |
| unknown | · | · | · | 11 | 1 | · | · | · | 5 | 7 | **24** |
| **total** | **2** | **1** | **1** | **19** | **2** | **0** | **0** | **0** | **24** | **7** | **56** |

Readings (observational):

- Six findings carry known, unequal stamps (decompose→architect, decompose→ops, implement→verify, implement→ops, recon→implement ×2) — the first census where off-diagonal known-known cells are populated at all, and recon→implement is the first repeated cross-phase shape.
- The ops/ops mass (17) is almost entirely watcher-rotation findings; the implement column's size (19) is dominated by warm-lanes worktree sessions with unknown origins. The unknown-origin rate (43%) is concentrated in exactly those dispatched sessions, where digests do not identify where the confusion was introduced.
- Merge-manifested: zero for the third consecutive census.

## 1. Verified clusters

Ordered by sighting count. Session ids abbreviated to 8 chars.

### 1.1 Waiting on background work: guard trips and wrong wait primitives (9 sightings, 9 sessions)

**Facet (a) — sleep-chain polling blocked by the harness guard (6: d4deb99a, 1a4ea130, f55e1318, b020e966, 4b8728ed, 74cf001b).** Each session chained `sleep N; tail/echo ...` in one Bash call to poll a background task's output file and was blocked outright, with the guard message itself naming the sanctioned primitives (Monitor until-loop; `run_in_background`). The guard is harness-level — no guard text matching it exists in this repo's `hooks/` — and in every sighting it taught reactively, costing the session a turn. One sighting (d4deb99a) then fell back to re-invoking `Read` on the same output file three times in a row as a substitute wait.

**Facet (b) — wrong substitutes chosen without a guard trip (3: da9d8468, 48a00bce, 6e979b6b).** A hand-rolled foreground `until ... grep -qE ...` loop waiting on another process's log consumed its full 10-minute timeout (exit 143), losing the entire wait; `ScheduleWakeup` was called with only `delaySeconds`/`reason` as a generic delay and failed validation (`prompt` is required — the tool is /loop-scoped); `TaskOutput` was polled twice with task ids the harness reported as unknown ("No task found with ID"), consistent with polling after reap/expiry or a mistracked id.

Evidence (representative): `Blocked: sleep 240 followed by: tail -c 2000 ... Do not chain shorter sleeps to work around this block.`; `ScheduleWakeup({"delaySeconds": 240, ...}) -> 'prompt' is required when 'stop' is not true.`

### 1.2 Warm-lane per-lane task-meta artifacts undiscoverable or absent (8 sightings, 8 sessions)

**Facet (a) — path shape guessed wrong (3: 8e014b83, cfcd74c2, 234e5439).** `.task-meta/<lane>/` probed at the warm-lanes root instead of nested under `worktrees/`; in cfcd74c2 the ENOENT was invisible because stderr was piped to `/dev/null` (bare exit 2), and the correct nested path was not tried until 15 turns later.

**Facet (b) — artifacts absent at the correct path (4: f1237c59, 5d2eb75b, 74cf001b, 285cca92).** `iterations.jsonl` missing at `worktrees/.task-meta/<lane>/` (two sessions, one probing repeatedly across a 50+-turn span); a lane handed to an orchestrated-task agent whose **first tool call** read `.task/plan.json` and got "File does not exist", proceeding without plan context; and a `.task/plan.json` symlink that appears healthy under `ls -la` but whose target yields no output from `wc -c`/`head -c` (exit 1) — a missing/empty plan masked behind an intact-looking link, discovered during a usage-limit resume.

**Facet (c) — symlink indirection triggering search instead of resolution (1: da9d8468).** The agent inspected the `plan.json` symlink, then ran a tree-wide `find` across the warm-lanes root for `iterations.jsonl`, timing out at 2 minutes; a later `readlink -f` resolved it directly.

Adjacent codebook entry: `warm-lane-cache-incoherence` covers warm-lane *build-cache* coherence; this cluster is the *metadata sidecar* surface — related area, distinct mechanism.

### 1.3 Escalation-watcher rotations rediscover the dark-factory tooling root by trial and error (7 sightings, 7 sessions)

Sessions: d72166e6, c237f154, 9fff5608, 7bc39ff3, ef792b8a, 8bcac732, be5ec2d3 — every one an autonomous rotation against reify, cwd and briefing supplying only the target project's root and queue path. `watcher-rearm.sh` lives in the dark-factory repo and hard-fails without `DARK_FACTORY_ROOT` (`scripts/watcher-rearm.sh:132-134`). Sighted discovery paths: relative invocation from reify's cwd (exit 127); a guessed `reify/scripts/` absolute path; the unset variable expanding `$DARK_FACTORY_ROOT/scripts/...` to `/scripts/...`; `cd` into dark-factory without exporting the var (guard exit 2); **one session re-issued the identical failing invocation 8 times** before first trying `export`; two fell back to filesystem-wide `find` for `dark-factory*` — one surfacing seven candidates including tmp/archived decoys, one hitting the 2-minute Bash timeout mid-scan.

Verified against main: the interactive skill documents both the prerequisite (`skills/escalation-watcher/SKILL.md:21`) and the canonical invocation form (line 134: `cd $DARK_FACTORY_ROOT && scripts/watcher-rearm.sh`). The sighted sessions' environments had the variable unset and their injected banners did not restate it. This is `entry-cand-20260722-3` (open, ops/ops, first sighted 07-22) recurring seven times in one census window.

### 1.4 Bounded-wait outcomes read as retryable failures; identical re-issues until human interrupt (5 sightings, 5 sessions)

- **9b198d5c** — post-bootstrap, `watcher-rearm.sh --timeout 3600` repeatedly exited 124 (`WATCHER_REARM_OUTCOME: CEILING`); the identical long invocation was re-run **five times over ~120 turns**, each differing only by one more exclude-id, until the user manually interrupted.
- **647400e7** — CEILING exits answered with a redundant dual mechanism: 8 rearm re-invocations with shrinking timeouts *plus* 4×2 manual `get_pending_escalations` polls at both levels, instead of trusting one bounded-wait primitive.
- **24923034** — the foreground 560000ms Bash call wrapping the watcher's internal `--timeout 540` was killed at exit 137; the identical command was reissued **21 times, then 4 more**, never switching to a background pattern that would survive the kill.
- **7bc39ff3** — after one full 3600s CEILING, the same 3600s foreground wait was immediately re-issued (two such waits = half the 4.0h rotation budget); the user rejected it before it ran.
- **a823400c** — a path-scope guard trip (cross-project warm-lane pool check) was read as a wrong-path bug rather than a hard boundary: dozens of turns later the agent re-issued essentially the same check with a "corrected mount path", requiring manual rejection.

The rearm script's outcome contract is machine-readable and documented on main (`watcher-rearm.sh:31,37,197-198`); no sighted session branched on it. The exit-137 shape recurs the promoted entry `watcher-loop-harness-mismatch` (foreground Bash limits vs the skill's bounded waits). The instrument-side mirror of this cluster — designed exit-124s scored as `tool_error` — is §1.9.

### 1.5 Task-graph edge semantics vs narrative fields; lifecycle transitions leave stale state (5 sightings, 3 sessions)

- **64c5541b ×2** — a task whose sole deliverable is an edit to gitignored `tasks.db` can never produce a commit, so it structurally cannot satisfy `confirm_plan`'s non-empty-files requirement; nothing blocked filing/dispatch, and the gap surfaced only after full pipeline failure and escalation (decompose→architect). The same session's second finding: dependency *prose* in a task was conflated with enforced edges — only same-project `add_dependency` and cross-project `metadata.external_deps` are scheduler-honored — requiring a corrective memory amendment.
- **6acb9fe4 ×2** — two "same-class" reify escalations (esc-5669-5, esc-5632-15) adjudicated together: the initial write mis-attributed 5632-15's cause to the wrong dark-factory task (user-directed amendment to DF 3113 followed), and 5669-5 was worked without first wiring the cross-repo dependency edge to owning DF task 3260 — backfill, edge addition, and re-pend were all user-directed afterward.
- **77847b75** — cancelling a task whose remaining work a successor absorbed does not re-point the successor's dependency edge; the stale-but-blocking edge (5612→5607) had to be manually detected and fixed before cancellation to avoid stranding.

### 1.6 Cross-project memory context bleeds into dispatched sessions' briefings and is acted on (3 sightings, 3 sessions)

Sessions e2f648af, 198f5f89, 278bbd68 — all with cwd inside a warm-lanes task worktree, all with injected `# Context` blocks containing fused-memory/Graphiti facts about Reify's structure ("Reify v0.1 references the architecture...", tasks 2310/3539/4543, `crates/reify-eval` plan references). Each agent then probed a reify path in the warm-lanes worktree — `crates/reify-syntax` + `tests/common/mod.rs` + root `Cargo.toml` (three chained failures), `cd crates/reify-runtime`, `cd crates/reify-eval/tests` — hitting not-found errors. Two sightings are stamped recon→implement (context assembler origin); the sightings' shared hypothesis, recorded as theirs, is that the context block was not project-scoped before injection. Recurs the 07-24 census's "session-start memory injection unusable" headline in a sharper form: the leaked content is now demonstrably *acted on*, not merely unhelpful.

### 1.7 Shell diagnostics discarded before they could discriminate (3 sightings, 3 sessions; +1 facet shared with §1.2)

- **86f8dd27** — two consecutive `grep -rn ... crates/ --include=*.rs` calls both exited 2 (an I/O/argument error, distinct from exit 1 no-match); the only adjustment between attempts was loosening the pattern (`pub fn` → `fn`), not checking whether `crates/` existed relative to cwd.
- **60aff933** — `2>/dev/null` on a grep made exit 2 indistinguishable from a no-match; the retry narrowed the path set rather than diagnosing the error.
- **86a5a16f** — a `sed -n "$(grep -n ...)"` range built by unvalidated command substitution produced a bare comma, failing with ``unknown command: `,'``.
- (Shared with §1.2: cfcd74c2's `2>/dev/null` hid the ENOENT that would have revealed the wrong `.task-meta` path shape for 15 turns.)

### 1.8 Autonomous-role capability envelope and persistence surprises (3 sightings, 3 sessions)

- **3b9a61f4** — at end-of-rotation, the watcher reached for `Write` to persist its digest and got "Write exists but is not enabled in this context" — at exactly the moment output delivery matters most. Direct recurrence of promoted entry `watcher-capability-envelope`.
- **35b1e3b7** — an autonomous watcher formatted a finding as an interactive-session auto-memory file (YAML frontmatter, `memory/*.md` convention) and called `Write` — conflating the interactive file-based memory with the always-available `mcp__fused-memory__add_memory` — losing the write with no visible fallback to the correct tool.
- **b020e966** — a `Write` of an AFK handoff digest tripped the read-before-write guard ("File has not been read yet") mid-handoff.

### 1.9 The instrument: both recurrences fall in scopes the 07-31 census left open (2 sightings, 2 sessions)

- **39ce9b55** — a digest filed a tool-injected `# Context` / `## Project Context` memory-dump turn under "User Corrections" with all five `signal_counts` at zero — the 07-31 cluster 1.1(a)/1.2 shape. Verified on main: `is_harness_injected_turn` (`digest.py:593`) requires all three headings `# context` + `## agent identity` + `# task` to co-occur as stripped lines; the sighted turn's quoted content shows only `# Context`/`## Project Context`. If the turn lacked the other two headings, the filter passes it through by construction — this is precisely the discriminating check the 07-31 §6 method notes prescribed.
- **fd89f5e4** — `escalation.watcher --timeout 540`'s designed bounded-poll exit (124) was scored as `tool_error` **13 times in one session**, inflating the error count and generating retry-loop noise that is not agent confusion. This is 07-31 R3's scope verbatim; no exit-code or designed-outcome handling exists in the `tool_error` extraction on main, and the 07-31 report filed no tasks.

### 1.10 Usage-limit resume discontinuity (2 sightings, 2 sessions)

- **952e07e0** — the resume prompt ("Continue where you left off and complete your task") carries no record of completed work; the agent re-launched the full `reify-eval` test suite from scratch and had to be stopped by user rejection (implement→verify).
- **cf2e6a57** — a StructuredOutput schema failure (`{"tasks": {"tasks": [...]}}` double-wrap → "/tasks: must be array") was reproduced **identically** on the first post-resume attempt, consistent with the resume losing the immediate tool-error feedback from the failed call.

### 1.11 Harness timeouts kill legitimate long commands, returning only the kill artifact (2 sightings, 2 sessions; overlaps §1.1(b), §1.2(c), §1.3)

- **77847b75** — worktree-listing/warm-lane-audit investigation commands exceeded the default 2-minute foreground timeout (exit 143) mid-audit, forcing re-runs or partial-evidence reasoning.
- **44e125ce** — a synchronous `cargo build -p reify-eval --lib` reproduction was SIGTERM-killed at the 10-minute ceiling, so the agent never saw the compiler output it was after and could not distinguish a hang from a slow cold build.

The same shape appears inside other clusters (the 2-minute `find` kills in §1.2(c) and §1.3, the 10-minute until-loop in §1.1(b)): in each case the timeout destroyed the evidence the command was run to collect.

### 1.12 Search-tooling capability assumptions in task worktrees (2 findings, 1 session: fa5527d4)

Grouped on shape, single-session provenance: the agent shelled out to `rg` directly (binary absent from the worktree environment entirely — "error: 'rg' not found"), then switched to the Grep tool but wrote a negative look-behind pattern, which the tool's non-PCRE2 engine rejects ("look-around ... is not supported") — two consecutive failures from two different unverified capability assumptions.

### 1.13 Concurrent-actor coordination gaps (2 sightings, 2 sessions)

- **ed41d6f5** — after pivoting to direct-to-main commits for PRD artifacts, overlapping background Bash invocations (wait-for-lock, retry-commit, commit-now) collided on a live `.git/index.lock` (exit 128); one in-flight attempt had to be force-killed. Nothing serialized or cancelled the prior attempt before starting the next.
- **c048bcc4** — an interactive session had no way to see that concurrent unblock sessions already owned tasks 5548/5585/5599; the user had to manually order a stand-down to prevent duplicate remediation.

Echoes the 07-31 census's cluster 1.4 (concurrent sessions colliding on shared mutable state); adjacent to promoted entry `machine-operated-main-checkout`.

## 2. One-off sightings (3 findings, verified)

- **Leak-status oracle unset silently downgrades `leaked=0` into a false "no leaks" signal** (15f4a1b9, implement→ops). With `REIFY_LANE_LEAK_STATUS_CMD` unconfigured, orphaned lanes that cannot be confirmed LEAKED fall back to PRESERVED-OK: the audit headline read `leaked=0` while 18 lanes sat in `leak_unknown` — "confirmed clean" and "detection unavailable" are indistinguishable at the top line.
- **Heredoc commit message with literal parentheses breaks under an eval-wrapping runner** (ed41d6f5, prd→prd). A `git commit -m "$(cat <<'EOF' ...)"` passed through a background runner that re-parses via `eval` hit a bash syntax error on `(` before the command ran.
- **Read tool `offset` called with slice-range syntax** (5d2eb75b, implement→implement). `"offset": 185, 210` — a Python-slice mental model of the offset+limit schema, producing unparseable JSON. Kin to the StructuredOutput double-wrap (§1.10) and the ScheduleWakeup misuse (§1.1(b)) as tool-schema shape errors.

## 3. Cross-cutting observations

These restate patterns visible across the verified sightings; they diagnose nothing beyond what the sightings and the cited code show.

1. **The cross-project seam is where the confusion lives.** Roughly two-thirds of the batch comes from dark-factory tooling operating on or in another project, and the three biggest non-instrument masses each sit on that seam: the watcher's tooling root is in a different repo than its target (§1.3), the warm-lane metadata contract spans the pool repo and the task worktree (§1.2), and briefing context assembled in one project's memory store surfaced another project's paths (§1.6).
2. **"Documented elsewhere" is not "discoverable here."** The DARK_FACTORY_ROOT prerequisite, the canonical rearm invocation, and the rearm outcome contract are all documented on main — in the interactive skill file and the script header — yet seven rotations, five retry loops, and thirteen mis-scored exits proceeded as if they weren't. In every case the failing context (spawned environment, injected banner, digest scorer) does not carry what the documentation says.
3. **Identical-retry-until-human-interrupt is the batch's signature failure mode.** At least six sessions ended, or were rescued, only by a manual user rejection or interrupt — five of them autonomous watcher rotations. The retried operations' outcomes were terminal or designed (CEILING, exit 137, a scope guard, a completed suite), and the retries varied only cosmetically.
4. **Evidence destruction precedes misdiagnosis.** Across clusters, the discriminating datum was discarded before the agent reasoned: stderr piped to /dev/null (§1.2, §1.7), exit-code semantics collapsed (§1.7, §1.9), timeouts returning only kill artifacts (§1.11), a resume dropping the prior tool-error feedback (§1.10). The wrong retries that followed are each consistent with the missing evidence, as sighted.
5. **The matrix has off-diagonal mass for the first time**, and the repeated shape (recon→implement, ×2) implicates origin-side context assembly in downstream manifestation — the direction the PRD hypothesized for architect→merge, observed here on a different edge. Merge-manifested remains at zero for the third consecutive census.

## 4. Remediation candidates

To be filed via the curator path (plain `submit_task`; curator dedup is the protection — PRD §6.9). Known adjacent in-flight work to dedup against: the warm-lane task-meta lane-keying batch (tasks 3154–3161) for R4, and the 07-31 census's unfiled R1–R3 which are refiled here as R6.

| # | Candidate | Cluster | Size |
|---|---|---|---|
| R1 | Watcher rotation spawn/banner: inject `DARK_FACTORY_ROOT` (env var set at spawn, and the canonical `cd $DARK_FACTORY_ROOT && scripts/watcher-rearm.sh` form restated in the injected instructions) for cross-project targets — discharges `entry-cand-20260722-3` | 1.3 | S |
| R2 | Watcher loop outcome handling: instruct (skill + banner) branching on `WATCHER_REARM_OUTCOME` — CEILING → budget-checked bounded re-arm count; KILLED/137 → background wait pattern; never re-issue an identical failed invocation more than N times | 1.4 | M |
| R3 | Wait-primitive briefing for dispatched/autonomous roles: state Monitor/`run_in_background` as the sanctioned wait pattern up front in role prompts, so the harness guard stops being each session's reactive teacher | 1.1 | S |
| R4 | Warm-lane task-meta contract: verify plan.json is non-empty **through the symlink** before lane handoff; write or explicitly document absence of `iterations.jsonl`; document the `worktrees/.task-meta/<lane>/` shape where dispatched agents can see it (dedup vs 3154–3161) | 1.2 | M |
| R5 | Project-scope the briefing `# Context` assembly (recon Stage 1): filter memory-search results to the target project before injection, or tag cross-project facts as foreign | 1.6 | M |
| R6 | Digest instrument refile of 07-31 R1–R3 (never filed): relax `is_harness_injected_turn`'s all-three-headings co-occurrence to match real briefing shapes; add exit-code/designed-outcome awareness to the `tool_error` channel | 1.9 | M |
| R7 | Task filing validation: reject or flag at submit time any task whose deliverables consist entirely of gitignored paths (structurally cannot satisfy `confirm_plan`) | 1.5 | S |
| R8 | Cancellation flow: dependent-edge sweep with a warning (or auto-repoint prompt) when cancelling a task that still has open dependents | 1.5 | S |

## 5. Codebook dispositions (input to the merger; promote/reject in place, never delete)

| Cluster / finding | Suggested disposition |
|---|---|
| 1.3 DARK_FACTORY_ROOT bootstrap | Add 7 sightings to `entry-cand-20260722-3`; recurrence at this scale is promotion evidence and a severity-bump candidate |
| 1.4 bounded-wait retry loops | Add sightings to `watcher-loop-harness-mismatch` (exit-137/foreground facet); the unconsumed-CEILING-contract facet is new — candidate if the merger finds no existing coverage |
| 1.8 Write-not-enabled at digest emit | Add sightings to `watcher-capability-envelope`; the auto-memory-file vs fused-memory-MCP conflation (35b1e3b7) is a distinct new facet/candidate |
| 1.1 wait primitives | New candidate (harness-level guard + substitute-pattern errors; area tooling/waiting, explicitly not a DF hook) |
| 1.2 warm-lane task-meta | New candidate; cross-reference `warm-lane-cache-incoherence` (adjacent area, distinct surface: metadata sidecars, not build caches) |
| 1.5 task-graph edge semantics | New candidate (enforced-edge vs prose; stale edges on cancel; gitignored-deliverable filing gap); cross-reference `fused-memory-api-traps` |
| 1.6 cross-project context bleed | New candidate; cross-reference the 07-24 memory-injection cluster entries (merger locates promoted ids) |
| 1.7 diagnostics discarded | New candidate (exit-code conflation + stderr suppression + unvalidated substitution as one hygiene family) |
| 1.9 instrument recurrences | Add sightings to the 07-31 disposition targets (mislabel family; score/`signal_counts` entry; exit-124-as-tool_error candidate); record that 07-31 filed no tasks |
| 1.10 usage-limit resume | New candidate |
| 1.11 timeout evidence destruction | New candidate; cross-reference `watcher-loop-harness-mismatch`'s 120s-kill facet |
| 1.12 search-tooling capabilities | New candidate (env provisioning: no `rg`; Grep non-PCRE2) |
| 1.13 concurrent-actor coordination | Add to the 07-31 concurrent-session candidate if promoted; else new; cross-reference `machine-operated-main-checkout` |
| One-offs (leak oracle, heredoc-eval, Read offset) | Dated one-off entries per convention; heredoc-eval recurs a known shape — merger should locate any prior entry before minting |

## 6. Method notes for the next census

- **Discriminating checks this cycle set up:** zero new DARK_FACTORY_ROOT sightings after R1 lands is the fix-confirmed signal for the batch's biggest cluster; likewise zero identical-retry CEILING loops after R2. For the instrument, the 07-31 §6 marker check was *performed* this cycle (the all-three-headings requirement stands on main and the sighted turn shape defeats it); what is still missing is per-sighting digest provenance — record whether each digest predates or postdates any landed filter change.
- **Stamping:** 43% unknown origin, concentrated in dispatched warm-lanes sessions; and the biggest clusters are tooling/harness defects whose "origin" is structurally ambiguous under the current enum. The 07-31 note recurs: a stamping convention for instrument/tooling-defect findings would make the matrix reflect phenomena rather than stamping variance.
- **Composition shift:** this corpus is dominated by cross-project sessions (reify watchers, warm-lanes worktrees) where 07-31's was dominated by the instrument. Whether that reflects sampling strata or the fleet's actual activity mix is not established here; comparing sampled-session class composition across the three censuses would distinguish them.
- **Merge-manifested: zero for the third consecutive census.** Verify-manifested went nonzero for the first time, but on harness shapes, not the PRD's motivating architect/implement→merge hypothesis — which remains untested.

---

Two process notes for the operator, outside the report: the 07-31 census's remediation candidates were never filed as tasks (its "Filed Tasks" section reads "none filed"), which is why R6 refiles them — the census loop did not close last cycle. And per convention I have not written the report file or filed tasks myself: the census runner assembles `plans/confusion-census-2026-08-03.md` (saturation stats, filed-task ids, cost) around this synthesis and advances `census-state.json`.


## Filed Tasks

_dry-run: 56 payload(s) written to /home/leo/src/reify/plans/confusion-census-2026-08-02-payloads.json -- NOTHING filed; review before filing._

## Cost

invoke calls: sonnet miner=1000, sonnet verify=150, fable synthesis=1, haiku headroom-probe=1; WARNING: 2 storm batch(es) at indices [48, 49] (>50% coding failures -- degraded dup-rate signal, excluded from the saturation decision)
