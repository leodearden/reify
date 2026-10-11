# confusion census 2026-10-11

Project: reify

## Method

```yaml
run_id: census-reify-20261011
as_of_sha: a4f35da461bac698e6450d89a10a6212c62f3b47
since: none
evidence:
  window:
  - '2026-10-04T00:00:00+00:00'
  - '2026-10-11T00:00:00+00:00'
  sessions_enumerated: 913
  skipped_coded: 49
  skipped_zero_signal: 170
  mined: 60
  ledger_rows: 44
verification:
  confirmed: 1
  weakened: 0
  refuted: 8
  unverified: 0
cost:
  miner_calls: 60
  verify_calls: 9
  synthesis_calls: 1
  probe_calls: 3
  embedding_calls: 0
  wall_clock_secs: 4563.763
inputs_consumed: []
extra:
  inputs_consumed_note: this census reads no other instrument's report yet; matching
    sightings against /review, /hotspot-survey and /review-all findings arrives with
    the pre-screen (plans/census-incremental-prd.md leaf L6)
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
```

## Saturation

- batches: 3
- stop reason: saturated
- operator batch cap: 50 batch(es) (not reached -- mining stopped by: saturated)
  - batch 0: dup_rate=0.75 (total=20, succeeded=20, failed=0, saturated=False)
  - batch 1: dup_rate=0.90 (total=20, succeeded=20, failed=0, saturated=True)
  - batch 2: dup_rate=0.90 (total=20, succeeded=20, failed=0, saturated=True)

## Verification

- handed all 9 novel cluster(s) to the verifier; operator verify cap: 150 (not reached).

## Origin x Manifestation Matrix

| origin \ manifested | unknown |
| --- | --- |
| unknown | 1 |

## Synthesis

_The census does not parse this section: nothing proposed in it has been applied to the codebook._

## Census of verified confusion findings — 2026-10-11

One cluster was verified in this period. It rests on one sighting.

### Cluster 1 — Infra shell test helpers cannot be invoked in isolation (low severity)

**Anchor:** `tests/infra/test_verify_scope.sh`
**Area:** `tests/infra` shell test harness invocability
**Sightings:** 1 (session `bd4e18dd-55c4-449c-9512-c9259e93e13f`; origin and manifesting phase both unknown)
**Route:** mechanical
**Verdict tags:** heuristic 13 (files make internal sense in isolation), heuristic 6 (well-defined purpose for each entity), `kind:confusion`

**What was observed.** An agent wanted to run a single helper from `tests/infra/test_verify_scope.sh`. The file offers no way to do that, so the agent cut the helper's definition out with a `sed` line range and `eval`-ed it inside a nested `bash -c`. The helper body contains `rm`. Claude Code could not statically vet the generated script and held the command for manual approval:

> This command passes a shell -c script that runs rm, and Claude Code could not check the script for dangerous removals. Approve only if you have read the script.

A non-interactive agent cannot grant that approval, so the call stalled.

**What the verification established.** `tests/infra/test_verify_scope.sh` is a top-level linear script. Its helpers (`make_fixture`, `plan_for`, `plan_for_branch` and others) are interleaved with executable top-level statements, and the file installs a `cleanup()` trap that runs `rm -rf`. There is no function-selection argument and no guard that lets the file be sourced without running the test body. Consequently the only route to exercising one helper is extraction via `sed`/`eval`. The approval prompt itself originates in Claude Code's own dangerous-removal check, which is not part of this tree; the tree's contribution is the missing entry point that pushes agents toward the extraction.

**Reading against the quality definition.** The finding turns on heuristic 13, files make internal sense in isolation: the helpers cannot be understood or exercised apart from the surrounding linear script because defining them and running the test are the same act of reading the file. It also touches heuristic 6, well-defined purpose for each entity: the file is simultaneously a helper library and a test body, a purpose that cannot be stated without "and". No other heuristic was verified against this finding, and no claim is made here about other `tests/infra` members beyond the one anchored file.

**Severity.** Low. The extraction is a convenience for running one helper; the whole script runs normally through its wrapper and the gate, so the cost is contained to agents attempting partial invocation.

**Recorded remediation (mechanical).** In `tests/infra/test_verify_scope.sh`, add a guard so that sourcing the file only defines its helpers and skips the test body and the cleanup trap. An agent could then `source` the file and call one function directly, with no `sed`/`eval` extraction and no `bash -c` wrapper for the approval check to refuse.

### Observations across the census

With a single cluster and a single sighting, no cross-cluster pattern can be stated. The one recurring shape worth noting for future censuses, as an observation and not a diagnosis, is that the confusion manifested as a tooling refusal (the dangerous-removal prompt) while the verified cause lay in file structure (no sourceable entry point). Future findings whose evidence is a tool-side refusal may warrant the same check: whether the tree's structure left the agent no vetted path.

### Proposed codebook changes (not applied; act on by hand)

- Consider adding or confirming a codebook entry under the `tests/infra shell test harness invocability` area capturing the pattern "linear test script with no source-without-run guard forces `sed`/`eval` extraction of helpers", tagged `h13`, `h6`, `kind:confusion`, severity low, route mechanical, anchored at `tests/infra/test_verify_scope.sh`. This is a suggestion for a human to weigh against the existing codebook contents; nothing in this report is applied automatically.

## Filed Tasks

_2 ticket(s) filed -- the curator's create/combine/drop decision is still pending, so no task id exists yet; resolve_ticket returns the task id once it does._

- tkt_0RVNE0AE34M1XJT96K394VDHJN
- tkt_0RVNE0AF3ECSB6ZE7EWQE43VRC

## Cost

invoke calls: sonnet miner=60, sonnet verify=9, fable synthesis=1, haiku headroom-probe=3
