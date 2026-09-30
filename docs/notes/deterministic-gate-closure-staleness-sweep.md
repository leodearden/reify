# Closure-staleness sweep — RETIRED

**Retired by #7351, 2026-09-16.** `scripts/deterministic-gate-closure-staleness-sweep.sh` and its
hermetic suite `tests/infra/test_deterministic_gate_closure_staleness_sweep.sh` are **deleted**,
along with both of their manifest registrations. **Nothing in reify runs a closure-staleness sweep
any more**, and nothing should: see **If a trigger is ever wanted again** below.

This note is kept as the tombstone. It is the only surviving written account of why seven tasks
were cancelled, and outside citers — the #5316 runbook family and prior task records — reach it by
path.

---

## What it was

A recurring, **read-only, advisory** sweep (#5321, 2026-07-26) over tasks stranded in `blocked` or
`in-progress` after their block premise had already resolved. For each confirmed hit it emitted a
machine-readable re-dispatch request file into its `--emit-requests` directory, for a separate
consumer to act on. It never gated anything, exited 0 on every valid invocation, and degraded a row
it could not adjudicate to `unknown` rather than aborting. It shipped with three trigger classes;
#7349 (2026-09-09) retired two of them at the source, leaving only `merge_verify_red`.

## Why it was retired

- **The invoker and the consumer were both already gone.** Its only invoker was dark-factory's
  `reify-closure-staleness-sweep.timer`, disabled by hand on 2026-09-09. DF 5247 (landed
  2026-09-13) then deleted that timer along with its service, installer and wrapper, and deleted
  `scripts/consume_redispatch_requests.py` — the only reader of the emitted request files. So by
  the time this sweep was deleted its surviving class had nothing running it and nothing reading
  its output. **No need to re-check dark-factory for a timer; verified 2026-09-16:** no
  closure-staleness or redispatch unit is installed in either systemd scope or present in
  `/etc/systemd/system/`, none is tracked in dark-factory's tree, and `data/redispatch-requests/`
  holds nothing but its `consumed/` archive.
- **The surviving class carried no measured coverage.** `merge_verify_red` fired **0 times** across
  the 15 retained nightly runs (2026-08-25 .. 2026-09-09). Retiring it costs nothing that was
  observed to be worth anything.
- **A task-state policy does not belong in a project repo.** This is the sharp lesson, and it is a
  genuine counter-example to the house *reify ships the primitive, dark-factory wires the
  invocation* seam: the primitive and the actuator **disagreed about policy** across the seam.
  Because the seam carried *requests* rather than *decisions*, reify's policy silently won on the
  nights the sweep ran — a primitive can override a wired invocation's deliberate refusals without
  either side noticing.

## The measured record — what the two retired classes actually did

Preserved from the #7349 retirement, because it is the evidence and it exists nowhere else.

Its predicate: a `blocked` task with `metadata.task_kind = deterministic` and
`metadata.always_escalates` truthy, whose live escalation dir held no `status=pending`
`esc-<id>-*.json`. Its action was `close`, which the consumer turned into
`set_task_status('cancelled')` — the most destructive thing this family could do.

**The premise was unsound.** The absence of a live pending escalation is simply not evidence that a
deterministic gate task is finished; an escalation gets resolved routinely while the work it was
filed against is still open. It also **contradicts dark-factory's own state rules**
(`docs/task-escalation-state-spec.md`): `blocked` implies an open record **or** a gate marker, and a
human-resolved gate deliberately **stays parked**. The sweep read that parked state as completion.

**The damage.** Seven archived `gate_closure` requests, across seven distinct tasks: **6331, 6476,
6574, 6632, 6633, 7178, 7305**. An archived request is one that was *applied*, so each is a task
this class actually drove to `cancelled`. Five (6331, 6476, 6574, 6632, 6633) are `cancelled`
today; 7178 and 7305 have since been moved to `done`.

**Caveat — count tasks, not firings.** The archive was a per-task snapshot, not a firing log: the
filename was `redispatch-<id>-<class>.json`, so a re-emission overwrote. It bounds the number of
tasks **affected**, never the number of times the consumer ran. Do not read a firing count out of
it. (The archive directory itself was untracked, so these seven ids are now the surviving record.)

The third class, **`unmet_dependency`, was retired for OWNERSHIP, not correctness** — its predicate
was right, but dark-factory's `Scheduler._phase_redispatch_stranded_blocked` already owned the same
recovery at tick cadence, *with* two deliberate refusals (a `task_kind == 'deterministic'`
carve-out, and an escalation-pinned veto) that reify's class lacked and therefore overrode.

**The override, worked once — task 5318.** This is the single concrete instance of the
policy-override failure argued above, and its archive is untracked and wipeable at any time, so it
is preserved here or nowhere. `redispatch-5318-unmet_dependency.json` is stamped **2026-09-08
03:32:41Z** (file mtime, that night's run), verdict `STALE`, evidence `all 1 dependency(ies)
terminal: 5214=done`. Task 5318 was deliberately **parked** at that moment: `esc-5318-6` (L1,
steward) and `esc-5318-7` (L2, auto-watcher), filed 2026-09-07 06:01:15Z and 06:06:24Z, were both
still open, and a human ruling dismissed them at **2026-09-08 12:49:53Z** — **9h17m after** reify's
class had already declared the task re-dispatchable. The dependency premise genuinely had resolved;
that was never the point. DF's escalation-pinned veto existed precisely to keep a parked task
parked, and reify's class, lacking it, overrode it ~9h early. (The archive held **15**
`unmet_dependency` requests in all, against the seven `gate_closure` ones above.)

## Surviving owners

Stranded-blocked recovery did not go away with this sweep; it lives where it always belonged. Note
that **both survivors re-pend or re-file — neither ever cancels**, which is precisely the property
`gate_closure` lacked.

| Concern | Owner |
|---|---|
| Stranded `blocked` recovery, tick cadence | dark-factory `Scheduler._phase_redispatch_stranded_blocked` (DF 2408) |
| Deterministic-recon re-filing | the harness deterministic-recon sweep |
| Task/escalation state rules (`blocked`, gate markers, parked gates) | dark-factory `docs/task-escalation-state-spec.md` |
| The narrow origin this over-generalised: §5's "close the gate task when the correction lands" | `docs/notes/offline-lane-red-corruption-remediation.md` |

That last row is the root of the whole episode. #5316 §5 stated a narrow, correct, **manual**
closure step for one corruption-remediation flow; #5321 generalised it into a standing automated
sweep over all deterministic gate tasks, and the generalisation is what was unsound. That closure
step is performed **by hand**, permanently.

## If a trigger is ever wanted again

A re-verify-on-main-advance trigger is a reasonable thing to want. **File it against dark_factory,
not reify.** It needs the merge lane's git view and the task store's resolution semantics, neither
of which a reify-side script can see correctly — and putting it here is what produced the
policy-override failure above. **Do not rebuild it in this repo.**

Whoever builds it should carry forward the one measurement that constrained the original design:
at first sweep (2026-07-26) **all ten** live `in-progress` tasks had every dependency `done` —
**task 5321 itself among them**. A naive "dependency premise resolved ⇒ re-dispatch" rule would
have targeted ten actively-running agents and *destroyed* work rather than recovered it. Any such
trigger needs a liveness guard before it needs anything else.

## Pointers

| Topic | Source |
|---|---|
| Corruption signature catalog + remediation recipe (#5316) | `docs/notes/offline-lane-red-corruption-remediation.md` |
| Task/escalation state rules | dark-factory `docs/task-escalation-state-spec.md` |
| Surviving stranded-blocked recovery | dark-factory `Scheduler._phase_redispatch_stranded_blocked` (DF 2408) |
| The advisory-observability script family this cloned | `scripts/warm-lane-audit.sh`, `docs/notes/warm-lane-audit-runbook.md` |
