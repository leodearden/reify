# The docs-truth sweep: one recurring adjudication sitting for drifted docs

**Task #6347 | 2026-10-04**

`scripts/docs-truth-sweep.py`, run weekly by a systemd `--user` timer, checks
the repo's prose docs with every docs-truth detector. When it finds something
new, it books **one** human adjudication sitting at L2. This note is the single
home for its scope decisions. The code headers point here and do not restate
them.

## 1. Purpose and the ruling it implements

Docs drift out from under their own text: a PRD reads as live work after its
last leaf landed, or a capability manifest cites a symbol that has since been
deleted. Leo's 2026-08-19 ruling: **adjudication is a per-doc human sitting,
not auto-filed fix tasks.** Each doc needs its own judgement, and the right
answer for one doc (stamp it terminal) is the wrong answer for another (a dated
snapshot must not be edited at all).

`scripts/pprdstatus-escalate.py` (#6932) is the one-shot primitive: it raises
one PPRDSTATUS run, when someone runs it. The sweep adds what that primitive
deliberately left out:

- a recurring cadence;
- aggregation across the whole family of docs-truth detectors;
- set-level silence, so a doc the human already ruled on does not page again;
- a deterministic raise to L2, where the sitting is booked.

## 2. The family, and what qualifies a member

**Membership criterion:** a detector over prose docs whose findings arise from
changes *outside the doc's own diff*: task-state changes or code changes. No
merge gate can hold that line, because the commit that makes the doc false
never touches the doc. Only a sweep of the whole corpus sees it.

| Member | reify-audit token | Finding pattern | Fires when |
|---|---|---|---|
| PRD status-prose drift | `PPRDSTATUS` | `PPrdStatus` | every leaf of a PRD is terminal but its Status header reads live or is absent; or, in a live PRD, a `#NNNN (status)` cite's status word contradicts the cited task's status |
| Capability-manifest cites | `PCITE` | `PManifestCite` | a manifest row's `grep:` evidence names a symbol that no longer exists |

PCITE is what #6233 eventually landed as (via #6931).

**Excluded:**

- **PDOCCOVER.** It is a hard merge gate (#6931), so its corpus stays clean on
  `main` by construction, and sweeping it would be vacuous.
- **PLINECITE** (#6935). It has not landed and has no token. Per its own spec,
  its corpus is source comments with `docs/` excluded. Revisit it when it lands.

**Adding a member is one `FamilyMember` row** in the sweep's `FAMILY`: the token,
the sitting command, and the per-doc rulings that sitting chooses between. One
precondition: when the new token cannot actually check, its single-token run
must print **no** findings array. An empty array is read as a clean run.

## 3. Cadence: weekly, Monday 06:00, `Persistent=true`

Because of the novelty rule (§5), cadence does **not** set how often the queue
is paged: an unchanged or shrinking set is silent at any cadence. Cadence only
bounds how late a *new* finding is raised.

Doc drift accrues as PRD leaves land and symbols are deleted, spread over days.

- **Daily** would raise a fresh L2 member on most days that a PRD freezes, which
  is the ignored-alert failure this design exists to avoid.
- **Weekly** batches a week of freezes into one sitting-sized escalation.
- **Fortnightly** doubles the latency and buys no noise reduction, because
  weekly batching plus dedupe already silences repeats.

Each run costs one single-token detector run per member, plus a cargo no-op
when the release binary is already fresh. Monday 06:00 puts the L2 at the
start of a working week.

`Persistent=true` catches up a tick missed while the host was down.
systemd.timer(5) applies it only to `OnCalendar=`, which is why this timer is
not the `OnUnitActiveSec=` shape.

## 4. Trigger shape

**(a) A pure timer is the chosen shape.**

**(b) An event-driven trigger was costed and deferred.** It would hook
fused-memory's reconciliation path (in dark-factory) on each task status
transition. It would read the task's `metadata.prd`, scan every task sharing
that PRD, and on all-terminal invoke reify's binary across the repo boundary.
No `metadata.prd` index exists, so that is a full task scan per transition,
or a new index. It needs a new dark-factory seam and task.

Worse, (b) is precise for only **one** of the three finding kinds:

| Finding kind | Caused by |
|---|---|
| PPRDSTATUS stale status header | a PRD freeze |
| PPRDSTATUS cite-status contradiction | **any** cited task changing status |
| PCITE fabricated cite | a code deletion |

Latency buys nothing here either: the response is a human sitting, booked at
human cadence.

**(c) Both triggers stay reachable later with zero reify change.** The sweep
is idempotent, and its novelty state does not depend on what triggered it. A
future dark-factory hook can invoke `scripts/docs-truth-sweep.sh` on a freeze
event.

## 5. Silence discipline

**Identity.** A finding's identity is `FindingIdentity(pattern, path)`: the
detector's pattern name and the doc's repo path. Both are structured fields of
the finding. The identity is never derived from summary text, because
summaries carry volatile line numbers and leaf counts, and keying on them would
re-raise on cosmetic edits. The doc is also the unit of the human's judgement.

**Novelty rule.** Raise iff the current set holds an identity absent from the
last **observed** set. Every fully-checked run records the set it observed:

| Run | Outcome |
|---|---|
| unchanged set | silent |
| a shrink (sitting progress) | silent; the smaller set is recorded |
| an empty set | silent; recorded, so a later reappearance is news |
| a doc fixed, then drifting again | news |
| the same doc under a second detector | news |

A raise carries the **whole** current set, plus how many identities are new.

**State file.** `<project_root>/data/audit-runs/docs-truth-sweep.json` holds
`{"version": 1, "seen": [{"pattern", "path"}, ...]}`, written atomically.
`data/audit-runs/` is already gitignored and is the `/audit` family's artifact
dir.

- A missing file is a first run. The first run is the survey, and raises
  everything.
- An unreadable or wrongly-shaped file refuses (exit 125) rather than guessing.
- **To reset, delete the file:** the next run re-raises every current finding.

**Why a local state file.** The `escalate_info` MCP tool takes no dedupe
fingerprint: that is stamped only orchestrator-internally, and the MCP dedupe
gate folds only `infra_issue` within 600 s. So the sweep has to own set-level
dedupe. Querying the queue for a pending record would not suffice either. Once
the human *resolves* the L2 while deliberately leaving a doc as is, an
unchanged set would otherwise re-raise every week.

**Second layer: the server-side fold.** `promote_to_l2` folds a call into an
existing *pending* L2 that has the same canonical `root_cause` (it appends
members and amendments) instead of minting a duplicate. The sweep's
`ROOT_CAUSE` is a constant, so even lost local state cannot create a second
pending sitting.

## 6. What the escalation carries

One MCP session makes two calls.

**1. `escalate_info`, the L0 member:**

- subject `"audit"`, because a repo path cannot mint an escalation id
  (`.claude/skills/audit/references/severity-routing.md` §1);
- role `docs-truth-sweep`, category `risk_identified`, severity `info`;
- `terminal_state_is_the_bug=True`;
- **summary:** the finding count, the doc count, the new count, and the
  sitting commands (`/audit --pattern PPRDSTATUS`, `/audit --pattern PCITE`, one
  for each member that has findings);
- **detail:** the complete machine-readable list
  `[{pattern, path, summary}, ...]`;
- **suggested_action:** each flagged member's rulings, and an explicit "do not
  auto-file fix tasks";
- **evidence:** one entry measured at `HEAD=<sha>`.

**2. `promote_to_l2` of that member, the decision point:**

- the constant `root_cause`;
- **options:** the rulings of only those members that have findings:

  | Member | Rulings |
  |---|---|
  | PPRDSTATUS | still-active PRD: correct its prose · completed plan: stamp a terminal Status (SHIPPED / SUPERSEDED naming the successor / WITHDRAWN) · dated snapshot: leave the body as authored |
  | PCITE | rotted cite: correct the manifest row · legitimately external cite: mark it `pcite:allow` with a reason |

- **evidence:** the doc list;
- **summary:** the same counts and sitting commands, so the L2 consumer can
  triage without re-deriving anything.

## 7. The seam, and why `promote_to_l2`

**Reify raises it.** The sweep files over the escalation MCP HTTP endpoint that
the tracked `.mcp.json` declares (`mcpServers.escalation.url`). The detector
family, its binary, its freshness guard and its corpus are all reify's.
Wiring the raise in dark-factory would make dark-factory learn reify's detector
registry and binary path. An escalation write is not a task write, so reify
can make it. Reify ships the sweep, the units and the installer; nothing is
wired on the dark-factory side. The URL is read from `.mcp.json` rather than
hard-coded, so there is no fourth copy of the port. The Python takes the
endpoint as a required argument with no default, so no test can file into the
live queue by omission.

**Why `promote_to_l2`, not a born-at-L2 filing or level 1:**

- **Born at L2** would need severity `critical`/`urgent` *and* a
  `harness-*`/`orchestrator-*` role. Any other role is downgraded to `blocking`
  at L0. The sweep would have to masquerade as a harness sentinel and claim an
  urgency it does not have, and born-L2 records also bypass dedupe.
- **`escalate_blocker(level=1)`** hands the decision to the auto-watcher, which
  is not deterministic.
- **`promote_to_l2`** accepts header-less callers, does not check the member's
  level, and folds same-cause calls. It inherits the member's `info` severity,
  which is deliberately non-pinning: there is no real task to hold open.

## 8. Failure semantics and exit codes

The sweep is **all-or-nothing**. Each member runs as its own single-token
detector run, so each has a definite checked or unchecked verdict.

A mixed `--pattern PPRDSTATUS,PCITE` run is not used. Over an empty task
corpus it prints only a "skipped" breadcrumb, and still emits an array that
cannot be told apart from a clean run. A partial set (say PPRDSTATUS refused
because fused-memory was down) would read as a shrink one week and as growth
the next, re-raising docs already seen.

| Exit | Meaning | State file |
|---|---|---|
| 0 | every member checked; raised, correctly silent, or printed under `--dry-run` | recorded (except under `--dry-run`) |
| 1 | a raise was due but `escalate_info` or `promote_to_l2` could not be filed (an `accepted_unpersisted` member counts as not filed, and is never promoted), or the observed set could not be written | untouched after a failed raise, so the next run retries |
| 125 | nothing was checked or raised: a member's run printed no parseable findings array (detector failure, or a refusal of an empty task corpus), the state file is unreadable, `.mcp.json` declares no endpoint, or no fresh runnable `reify-audit` could be had | untouched |

The endpoint is resolved **before** the detectors run, so a misconfigured
deployment fails on every run, not only on the first run that has news.

The entrypoint `scripts/docs-truth-sweep.sh` guards
`target/release/reify-audit` in `rebuild` mode, the `/audit` skill's policy,
so the unattended sweep sees the same detector as the sitting it books.
`warn-open` exists for the synchronous pre-done path's 30 s budget, which a
weekly oneshot does not have.

The sweep writes no tracked file: its state, the cargo build output and Python
bytecode are all gitignored. So it is **not** an unattended writer of `main`
(`docs/notes/unattended-writers-of-main.md`).

## 9. Operations

**One-time activation, after merge, on the orchestrator host.** Run, from the
main checkout:

```bash
scripts/install-docs-truth-sweep-units.sh
```

- The unit's `ExecStart` is the main checkout's
  `/home/leo/src/reify/scripts/docs-truth-sweep.sh`, so the installer cannot
  usefully run from a task lane.
- It is deliberately **not** wired into `scripts/setup-dev.sh`. The timer
  files into the live human escalation queue, so installing it must be a
  deliberate act on that host. A non-orchestrator host would just accumulate
  a failing weekly unit.
- The installer warns when user lingering is off. Without
  `loginctl enable-linger`, the timer fires only while the user is logged in.

**Check the schedule:**

```bash
systemctl --user list-timers reify-docs-truth-sweep.timer
```

**Run it by hand:**

- `scripts/docs-truth-sweep.sh` runs the sweep exactly as the unit does;
- `scripts/docs-truth-sweep.sh --dry-run` prints the planned `escalate_info`
  and `promote_to_l2` arguments, files nothing and records nothing.

**Read the logs:**

```bash
journalctl --user -u reify-docs-truth-sweep.service
```

**Re-raise everything:** delete `data/audit-runs/docs-truth-sweep.json`. A
still-pending L2 absorbs the raise through the `root_cause` fold.

**Stop it:**

```bash
systemctl --user disable --now reify-docs-truth-sweep.timer
```

**Installer library.** The installer is built on
`scripts/lib_systemd_user_install.sh`, the consolidation that
`scripts/install-jcodemunch-index-units.sh`'s header names. Migrating that
installer and `scripts/install-warm-lane-units.sh` onto the library is a filed
follow-up, not part of #6347.
