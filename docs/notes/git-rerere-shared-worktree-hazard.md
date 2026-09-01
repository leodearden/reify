# Shared `.git/rr-cache` hazard runbook — `scripts/git-rerere-guard.sh`

**Task #5870 | 2026-08-13 | esc-5785-5**

Operational digest for the git rerere hazard in reify's shared-`.git` warm-lane pool: why rerere is
disabled repo-wide, how to recognise the two failure signatures it produces when it is not, and how
to recover a stuck lane without making things worse.

Every measurement below was reproduced first-hand on git 2.43.0 against throwaway repos and the live
store; the hermetic behavioural oracles live in `tests/infra/test_git_rerere_guard.sh`.

---

## 1. Mechanism

`rr-cache` is a git **COMMON** path. `git rev-parse --git-path rr-cache` resolves to the *common*
git dir from every linked worktree, while `MERGE_RR` and `index` resolve to
`.git/worktrees/<name>/`. Reify's pool is *hundreds* of worktrees of one `.git`, so **every one of
them shares one `/home/leo/src/reify/.git/rr-cache`**. The exact figure is a moving snapshot and is
never load-bearing anywhere in this hardening — dated measurement: 239 worktrees / 241 rr-cache
entries on 2026-08-13; 224 linked worktrees on 2026-08-26.

Git takes its only rerere lockfile on the **per-worktree** `MERGE_RR`. That lock therefore provides
**zero cross-worktree mutual exclusion** over the shared payload directory: two lanes writing
`rr-cache/` concurrently are not serialised by anything.

Git exposes no configuration knob to relocate `rr-cache`, so **per-lane cache isolation is
impossible by construction**, and an external lock would have to wrap every `git merge`, `rebase`,
`cherry-pick`, `revert` and `stash pop` issued by every lane plus the orchestrator and every
interactive shell across two repos — unenforceable, and it would serialise unrelated lanes. The only
sound fix is to remove the shared mutable state entirely: turn rerere off.

That costs nothing. `git grep -lI -e rerere -e rr-cache` over all tracked files returns **0 hits** —
nothing in reify references rerere.

## 2. The two hazards

### 2a. Silent cross-lane resolution bleed (the dangerous one)

Demonstrated end-to-end with two linked worktrees. Lane A (task X) resolves a conflict its way; lane
B (unrelated task Y) later merges and git prints:

```
Staged 'f' using previous resolution.
```

Lane B's working tree now holds **task X's resolution**, **already staged** (because
`rerere.autoupdate=true`), with **no conflict markers** and a **clean `git status`**. An agent
running `git commit` commits a foreign resolution having never seen a conflict.

rr-cache ids are content-addressed, so any two lanes hitting the same recurring hunk collide **by
design** — this is rerere working as intended, in an environment where "recurring" spans unrelated
tasks. It has bitten for real: task 5054 had to discard a plausible-but-wrong rerere resolution by
hand.

`rerere.autoupdate` is the half that makes this silent rather than merely wrong, which is why the
guard reports it independently of `rerere.enabled`.

### 2b. The stale `MERGE_RR.lock` signature

A failed rr-cache preimage write leaves a stale **zero-byte `MERGE_RR.lock`** in
`.git/worktrees/<lane>/` — or, for the **main checkout** (whose git dir *is* the common dir), at
`.git/MERGE_RR.lock`. Every subsequent `git commit` in that work tree then exits 128:

```
Unable to create '.../MERGE_RR.lock': File exists.
```

Two competing accounts of the *proximate* write failure exist and this note deliberately does **not**
adjudicate between them:

- **(a)** shared-cache collision/removal — another lane removing an entry mid-write (Mem0 `d4e7ad3a`);
- **(b)** `rr-cache/` sitting outside the landlock write-set in sandboxed lanes, so the `mkdir` of the
  per-conflict subdirectory is denied and the following write reports ENOENT (Mem0 `d3048711`,
  esc-5467-5).

Both are consistent with every observation in the transcripts, and distinguishing them would need a
core-dump analysis or a git bisect this work does not justify. **The fix is agnostic between them:**
with rerere explicitly off, neither path can execute at all.

## 3. The exit-128-yet-lands trap

**This is the real damage vector.** When `git commit` fails on the stale lock, it exits 128 (or
segfaults, 139) **after the commit object has been written and the ref has already moved**. At least
one observed commit segfaulted and landed intact.

A fatal-looking exit that actually succeeded invites a retry — and the retry **double-commits**.

> **ALWAYS run `git log --oneline -1` before retrying any git command that failed in a lane.**
> Confirm whether your commit is already HEAD. Do not reason from the exit code.

`scan-locks` repeats this warning inline on every STALE finding, because the operator reading it is
exactly the person about to retry.

## 4. Noise floor

A bare **`MERGE_RR`** (no `.lock` sibling) is **ORDINARY rerere state**, not the stuck condition. 41
of the live store's worktrees carry one right now, and none of them is broken.

Only **`MERGE_RR.lock`** indicates the stuck condition. Grep for the `.lock` suffix specifically, or
you will wrongly conclude the corruption is fleet-wide. `scan-locks` never matches a bare `MERGE_RR`.

## 5. Recovery — a stuck lane

1. **Check whether your commit already landed** — `git log --oneline -1` in the affected lane. See §3.
2. **Census the locks** — `scripts/git-rerere-guard.sh scan-locks` (read-only, never deletes).
3. **Read the classification.** `OPERATION-IN-PROGRESS` means a live `MERGE_HEAD` / `MERGE_MSG` /
   `CHERRY_PICK_HEAD` / `REVERT_HEAD` / `rebase-merge/` / `rebase-apply/` is present: **do not remove
   the lock** — finish or abort that operation from inside the worktree first, then re-scan.
4. **For a `STALE` finding**, run the exact command it printed, and only that:

   ```
   rm -f <common-git-dir>/worktrees/<lane>/MERGE_RR.lock
   ```

   A finding labelled `<main checkout>` sits one level up — `<common-git-dir>/MERGE_RR.lock`, with no
   `worktrees/<lane>/` component — because the main checkout's git dir *is* the common dir. Trust the
   printed path over this shape.

**NEVER delete `rr-cache/<id>/` while another lane may hold it.** That is precisely the operation
that reproduces the segfault + stale-lock signature — the "cleanup" would re-cause the disease across
every live lane of the store. It is also unnecessary: an explicit `rerere.enabled=false` neutralises the residual
cache **in place**, which is measured, not assumed (`test_git_rerere_guard.sh` (f-c)/(f-d) runs a
real conflicted merge against a populated `rr-cache/` and asserts zero new entries).

Deletion stays manual and operator-driven throughout: `scan-locks` prints the command and never runs
it, because it must not reach into another lane's git dir.

## 6. The guard — `scripts/git-rerere-guard.sh`

```
scripts/git-rerere-guard.sh <check|arm|scan-locks> [target_dir]
```

`target_dir` defaults to the repo root one level up from the script. Any worktree of the store
resolves — via `rev-parse --git-common-dir` — to the same shared config and the same `rr-cache`, so
the main checkout and any lane are interchangeable.

- **`check`** — read-only. Never writes config anywhere. Resolves `rerere.enabled` /
  `rerere.autoupdate` at every scope that can decide the answer, then sweeps every `config.worktree`
  in the store — the main checkout's own as well as every linked worktree's.
- **`arm`** — idempotently pins both keys `false` in the shared local config, then re-verifies via
  `check`. Never prunes `rr-cache`.
- **`scan-locks`** — read-only census of `MERGE_RR.lock` across the main checkout and every linked
  worktree, classified STALE vs OPERATION-IN-PROGRESS. Never deletes.

> **The exit-code contract is normative in exactly one place: the header comment block of
> `scripts/git-rerere-guard.sh`.** Read it there before writing a consumer. It is not restated here,
> because five hand-maintained copies of it is how the next behavioural change ships four stale ones.
> The sections below give the *rationale and the measured history* behind those codes — the part
> that is genuinely not in the script.

All diagnostics go to **stderr**; stdout stays empty so the exit code is the machine-readable signal.

`scan-locks` covers the main checkout deliberately: for it, git dir == common dir, so its lock lands
at `<common-git-dir>/MERGE_RR.lock` and never under `worktrees/`. The main checkout is an active
merge site — `scripts/land.sh` runs a real `git merge --no-ff` there — so a census that only globbed
`worktrees/*/` would report `clean` while every `git commit` on `main` exits 128.

**Why the header insists a consumer branch on `0 | 2 | *` rather than a closed set:** the guard runs
under its own `set -euo pipefail`, so a git invocation that aborts outside a guarded `if` propagates
git's status (4, 5, 128, 255…) instead of the documented failure code. A consumer that treated only 1
as fatal would read such a failure as success and leave the fleet armed. `setup-dev.sh` gets this
right because its `else` arm is the fatal one. Pinned by `test_git_rerere_guard.sh` (h-g), which
drops write permission on the git dir and asserts exit *exactly* 1 rather than git's status.

### `arm` exit 2 — why it is advisory, not fatal

`arm` writes `--local` only, so it can never clear **another lane's** `config.worktree` — and that is
the dominant way its post-write re-verify still reports armed. That is what makes the code advisory
rather than fatal: `setup-dev.sh` runs `arm` under `set -e` on every invocation (beside the main-gate
worktree config block), and one self-armed lane must not abort everything after that point (the
build-accelerator systemd units, npm, the smoke test) for every developer, with no remediation the
script could offer. It warns and points here instead.

The `config.worktree` sweep is itself gated on `extensions.worktreeConfig` being true, because git
does not read those files at all while the extension is off — an inert plant would otherwise produce
a false ARMED that `arm` could never clear.

The *other* out-of-reach case shares the code: a lane the guard could not verify at all (`check`
exit 3, below). The shared write still landed; nothing is known to be armed; but the store was not
fully verified. Advisory for the same reason — `arm` cannot repair a lane whose config it cannot
read. Its stderr says which of the two it is: *"rerere is STILL armed"* vs *"N worktree(s) could not
be verified"*.

### `check` exit 3 — UNVERIFIABLE is not safe

The sweep skips a lane it cannot read — an unreadable `config.worktree`, or an `include.path` chain
git cannot resolve (circular, or an unreadable target: both exit 128 with **no stdout**, which is
byte-for-byte indistinguishable from "no rerere keys here"). It prints a `WARNING: … UNKNOWN, not
verified safe` and continues, so one broken lane cannot mask the rest.

That skip used to leave the exit code at **0**, which made `check` **fail-open on its only
machine-readable channel**: a store whose one armed lane was a lane the guard could not read answered
"the fleet is clean". Exit **3** is now that third state — *nothing found armed, but not everything
was checked*.

The operational point the header's contract exists to serve: for the periodic-probe use in §8,
reading `!= 1` as clean re-opens the fail-open hole this code was added to close.

Three distinct conditions land here, each with its own test block: an unreadable `config.worktree`
(g-g), an unresolvable `include.path` chain — circular or unreadable target (g-j), and an unreadable
`gitdir` file, which leaves an entry's *liveness* unknown (g-k). That last one had the same fail-open
shape as the original bug: it was folded into the stale-entry silent skip, so a live armed lane whose
`gitdir` could not be read simply vanished from the sweep and `check` answered 0. An **absent**
`gitdir` is genuinely prunable and stays a silent skip; a **present but unreadable** one is not —
nothing about a permission bit makes an entry prunable.

### Stale worktree entries are inert, not armed

The sweep walks `<common-git-dir>/worktrees/*/config.worktree` straight off disk. When a lane's
**working tree** is deleted without `git worktree prune`, the administrative dir — `config.worktree`
included — survives, and git marks the entry `prunable gitdir file points to non-existent location`.
No git command can run in that worktree again, so git will never read that file again; reporting it
would be an inert-config false positive of the same class (and the same uniquely damaging shape) as
the `extensions.worktreeConfig` case. Reify's pool creates and destroys lanes continuously
(`warm-lane-gc.sh`), so a prunable entry is a realistic transient — measured on 2026-08-28, two
read-only passes over the live store minutes apart saw **0** and then **1** (`_merge-8af90d00`, a
merge lane whose working tree was gone but whose admin dir had not yet been pruned) out of 242
entries. Neither is a defect; the churn is the point.

Each linked entry is therefore gated on liveness first, using git's own prunability test — does the
path named by the entry's `gitdir` file still exist — read with a bash redirect rather than a
`git worktree list --porcelain` fork (whose output would then need a reverse mapping from worktree
path back to admin-dir name; they are not 1:1, since git de-duplicates names). A stale entry is
skipped **silently** and is **not** counted as UNVERIFIABLE: it is verified irrelevant, not
unverifiable, and counting it would pin a lane-churning pool at exit 3 forever. The gate is
deliberately *not* conditioned on `locked` — lockedness stops `git worktree prune`, but it does not
make an absent working tree readable; a locked worktree on a detached removable device is skipped
while absent and reported again once it is back, since `check` is re-run rather than one-shot.

### `arm` writes with `--replace-all`

`git config --add rerere.enabled true` makes the shared key **multi-valued**, and a plain
single-value write then fails: measured on git 2.43.0, `git config --local rerere.enabled false`
against a `true`/`true` shared config reports `error: cannot overwrite multiple values with a
single value` and exits 5. That was not inert: `arm` returned 1, and `setup-dev.sh`'s `*` arm turns
any non-zero into `err` + `exit 1`, killing the build-accelerator systemd block, npm and the smoke
test for every developer — while the fleet stayed **armed**, the exact outcome the guard exists to
prevent.

This defence is **prophylactic**, not a description of the live writer. The re-armer identified in
§7 uses a plain set, not `--add`, so no sighting on this store has been multi-valued — every armed
reading has been the single-valued SPLIT shape. It is kept because the failure mode it prevents is
*measured* rather than hypothetical, and one `--add` from any of the ~253 lanes — by an agent, or by
a future script — is enough to trigger it.

`--replace-all` is a strict superset of the old write: **byte-identical** output for the unset and
single-valued cases (so idempotence is unchanged), and it collapses a multi-valued key to one
`false` instead of failing. The idempotence probe reads `--get-all`, not `--get`, and deliberately
omits `--includes`; the reasoning for both sits inline at the call site in `cmd_arm`.

### Why a guard, and not a one-time `git config` write

**git's default for `rerere.enabled` is `-1` = "enabled iff `rr-cache/` exists".** Measured:

| `rerere.enabled` | `rr-cache/` on disk | entries written by a conflicted merge |
|---|---|---|
| unset | present | **1 — rerere is ON** |
| unset | absent | 0 |
| explicit `false` | present | 0 |

Because the shared store carries a residual populated `rr-cache/`, **losing the explicit `false`
silently re-arms the entire fleet**. `git config --unset rerere.enabled` is a **re-arm, not a
no-op**. The value must be present, not merely absent — so this ships as a re-runnable guard.

The value is also not durable here: Claude Code's worktree feature demonstrably clobbers shared
`.git/config` (the reason `setup-main-gate-worktree-config.sh` exists), a stray `core.hooksPath =
echo` was found in that same shared config during this task's earlier audit (esc-5870-1..3), and
**`config.worktree` was measured to BEAT shared config** — so any single lane can re-arm rerere for
itself. That last fact is why `check` **sweeps every `config.worktree` in the store** rather than
reading the shared file alone — and the sweep covers the **main checkout's own
`<common-git-dir>/config.worktree`** as well as every `worktrees/*/config.worktree`. For the main
checkout git dir == common dir, so its per-worktree config sits *beside* the shared config rather
than under `worktrees/`, and a glob over the linked worktrees alone is blind to it. That blind spot
is doubly silent: `check`'s effective-value read only ever sees the *target's own* `config.worktree`,
so from any lane a main-checkout self-arm would be invisible to both detection paths at once and
`arm` would report "disarmed and verified" while `main` stayed armed. The main checkout is also
where `scripts/land.sh` runs its sanctioned `git merge --no-ff` — an active merge site, the same
reasoning that already makes `scan-locks` cover `<common-git-dir>/MERGE_RR.lock`.

### Why `check` reads more than the effective value

The effective value alone is not enough, and the gap is the exact *mirror* of the main-checkout blind
spot above. `config.worktree` beats shared config in **both** directions, so a lane that disarms
*itself* reads clean while the shared config still arms every other lane of the store. Measured
against an earlier build of the guard: shared `rerere.enabled=true` + `rerere.autoupdate=true`, one
lane setting both false in its own `config.worktree` — `check <lane>` exited **0 printing nothing**.

So `check` also reads the shared (`--local`) value explicitly and reports it whenever the target's own
config merely masks it. It is reported **only** from the not-armed branch of the effective read, so an
outright-armed store is never listed twice for the same key.

Reporting this is safe by construction, unlike the inert-`config.worktree` case: `arm` is a `--local`
writer, so the scope reported here is exactly the scope `arm` can clear — it self-heals to exit 0
rather than the advisory 2. Pinned by `test_git_rerere_guard.sh` (g-f) and (g-f-g).

**And "unset in `.git/config`" is not "git falls back to its built-in default".** Precedence is
system < global < local < worktree, so what an unpinned lane *actually* inherits is the user's
`~/.gitconfig` or `/etc/gitconfig` value whenever either sets the key — the `-1` fallback is never
reached. Comparing only `--local` against the effective value got this wrong in both directions
(measured, git 2.43.0):

| global | shared `.git/config` | `rr-cache/` | old verdict | truth |
|---|---|---|---|---|
| `enabled = false` | unset | present | **ARMED**, blaming "$TARGET's *own config*" — a scope not involved | safe; no lane armed, because git never reaches `-1` |
| `enabled = true` | unset, target's `config.worktree` sets false | — | **clean** | armed — every *other* lane inherits the `true` |

The unset branch therefore resolves `--global` then `--system` (last-wins within a scope, `--includes`
explicitly) before falling through to the `-1` verdict. An inherited `true` is ARMED and names that
scope; an inherited `false` is exit **0** with a NOTE — safe, but *unpinned*: the disarm lives in a
file outside the store, so it does not travel with the repo and one `git config --global --unset`
re-arms every lane. Pinned by (g-l)/(g-l-b), the suite's only fixtures that point `GIT_CONFIG_GLOBAL`
at a real file rather than the hermetic `/dev/null`.

### `include.path` — why the scoped reads pass `--includes`

`git config` follows `include.path` **only for an effective read**. It turns include-following OFF
whenever a specific file or scope is named — `--file`, `--local`, `--worktree`, `--global`,
`--system`, `--blob`. Every scoped read in the guard is one of those, so each silently skipped an
indirection that git's own resolution honoured. Measured on git 2.43.0:

| read | without `--includes` | with `--includes` | git's effective answer |
|---|---|---|---|
| `--file <config.worktree> --get-regexp` | exit 1, no output | `rerere.enabled true` | `true` — the lane IS armed |
| `--local --get rerere.enabled` | exit 1 (unset) | `true` | `true` |

Both are now passed explicitly. The consequence for an operator is worth stating plainly: **a lane
can be armed with the string `rerere` appearing nowhere in its `config.worktree`** — the file need
contain nothing but `[include] path = extra.cfg`. "I grepped the configs and they're clean" is
therefore not evidence. Run `check`.

Three properties of that fix are deliberate, and a future editor should not tidy them away:

- **`arm`'s idempotence probe does NOT use `--includes`**, in intentional asymmetry with the three
  reads above. It asks a different question — "is the literal shared *file* already pinned to
  `false`, so this write would be a byte-level no-op?" — not "what does git resolve?". A shared
  config whose *included* file happened to set `false` would otherwise make `arm` skip the write
  entirely, leaving `.git/config` with no direct pin and re-armable the moment another lane edits or
  removes that included file.
- **The sweep reports git's LAST-WINS resolution, not any emitted value.** `--get-regexp` emits
  *every* value a file sets for a key while git resolves a multi-valued key to the last one, so
  flagging any emitted `true` reported a `config.worktree` whose final word is `false` as ARMED
  (measured, and pre-dating the include work). That false positive is uniquely damaging: `arm` writes
  `--local` and can never clear a per-worktree file, so the store would sit on the advisory exit 2
  permanently and `setup-dev.sh` would warn on every developer setup. The sweep accumulates the last
  value per key and decides after the loop.
- **A circular or unreadable include chain is reported UNKNOWN — never clean, and never ARMED.**
  Following an include lets the read fail on a file the guard never opens: measured, a circular chain
  and an unreadable target both exit **128 with no stdout**, which the old `2>/dev/null || true`
  laundered into "no rerere keys here, clean". The status is now captured; exit 1 is git's ordinary
  "no matching key" answer and stays clean, anything else warns (naming the worktree and the config
  path) and the sweep continues so one broken lane cannot mask the rest. It is not folded into
  `armed`, because `arm` cannot fix a lane the guard merely fails to read — the same trap the
  `extensions.worktreeConfig` gate avoids; it surfaces as `check` **exit 3** instead, so "never
  clean" holds on the exit code and not merely in the prose. A *missing* include target is benign
  (git ignores it) and stays silent.

Pinned by `test_git_rerere_guard.sh` (g-h), (g-i) and (g-j), each with its negative control asserted
before the plant and its fixture preconditions measured.

### Cost

`check` walks every `config.worktree` in the store, so it is O(lanes) by construction. It is kept
cheap enough to be a startup probe rather than a setup-only one-shot: a single
`grep -lisE 'rerere|include'` prefilter decides which files are worth parsing (values are still read
by `git config`, never by grep — comments, valueless keys and `yes`/`on`/`1` all defeat a grep;
`include` is matched, case-insensitively so it catches `includeIf` too, because the read below now
passes `--includes` and a rerere key can reach the file by indirection), the per-key reads collapse
into one `--bool --get-regexp`, and labels come from parameter expansion rather than
`basename`/`dirname`. Measured on the 224-lane store: **5.42s → 0.10s**. The fork pair per file, not
the `git config` reads, had been the dominant cost.

`arm` writes with `--local`, never `--worktree`: the point is that every lane inherits one shared
default with zero per-lane wiring.

## 7. Incident history

esc-5785-5 pairs **`_lane-22`** (triggered by a bad `git stash pop`) and **`_lane-16`** (a plain
`git merge main`), both 2026-07-30. The differing triggers are **irrelevant** — both funnel into the
same shared-rr-cache write path and produce the identical downstream signature.

Transcript evidence corrects two facts in the original escalation:

- the two incidents were **~10h50m apart, not "~55min"**;
- there were **more than two**. `_lane-39` (2026-07-29) preceded both by a day, `_lane-16`/task-5866
  recurred ~34min after the escalation was filed, and recurrences continued through 2026-08-08
  (`_lane-45`, `_lane-47`, `_lane-9` ×2, `_lane-49` ×2).

**The 2026-08-26 disarm was a point-in-time event, not a closure — the store was measured ARMED again
on 2026-08-27.** Read the history as a sequence of measurements:

| When | Measurement |
|---|---|
| 2026-08-13 | Armed: `rerere.enabled=true` / `rerere.autoupdate=true`, 241 rr-cache entries. Filed `escalate_info` on task 5870. |
| 2026-08-26 | Still armed, 266 entries (+25 in 13 days). Filed as `esc-5870-5`. |
| 2026-08-26 | Steward ran `scripts/git-rerere-guard.sh arm /home/leo/src/reify` → `SET: rerere.enabled=false (was true)`, `SET: rerere.autoupdate=false (was true)`, exit 0 (disarmed and self-verified). Confirmed from an unrelated lane: both keys `false`, `check` exits 0, all 266 entries still present (never pruned — see §5), main checkout's tracked files untouched. |
| 2026-08-27 | **Armed again** (detail below). Filed as `esc-5870-11`. |

Re-measured read-only from lane `_lane-5` on 2026-08-27 (~22:55 local), the store had re-armed within a
day of the disarm:

- `git -C /home/leo/src/reify config --get rerere.enabled` → `true`; `--get rerere.autoupdate` → `true`.
- `--show-origin --show-scope --get-regexp '^rerere\.'` reports both in scope `local`, `file:.git/config`
  — the SHARED config, the same file `arm` writes. Per-scope reads confirm `--global`, `--system` and
  `--worktree` are all UNSET, so nothing is *masking* a disarmed shared value; the shared value itself
  is `true`.
- `bash scripts/git-rerere-guard.sh check /home/leo/src/reify` → **exit 1**, two `ARMED:` lines.
- `rr-cache` holds **269** entries, up from the 266 counted at the disarm. Exactly 3 post-date it, all
  stamped 2026-08-27 05:09, each carrying a `preimage` written 04:56–04:58 and a `postimage` +
  `thisimage` written 05:09 — a conflict recorded *and* a resolution written back, ~17h after the
  disarm. Writing those requires rerere to have been armed at the time.

So the re-arm happened between the 2026-08-26 `arm` and 2026-08-27 04:56. **The writer has not been
identified**, and the available timestamps cannot narrow it:

- `.git/config`'s mtime (2026-08-27 22:59:37) post-dates the rr-cache writes it would have to explain by
  ~18h. mtime records a file's *last* write, so a later, unrelated write to the shared config has
  already overwritten the rerere write's timestamp. It dates nothing here.
- The store carries 238 linked worktrees with `extensions.worktreeConfig=true`, and per-lane config
  writes are demonstrably concurrent with this session (`_lane-1/config.worktree` rewritten
  2026-08-27 22:28). Config churn across the pool is continuous; no single mtime isolates one writer.
- No tracked reify script or hook writes `rerere.*` — a grep over `scripts/` and `hooks/`, excluding the
  guard and this runbook, returns nothing. The equivalent sweep of the dark-factory tree did **not**
  complete (killed at a 45s timeout) and is therefore UNMEASURED; the orchestrator side is not cleared.

> **Hypothesis:** an active writer outside reify's tracked scripts put the keys back. This is *not*
> established — no candidate mechanism has been excluded, and the evidence above constrains only *when*
> the write happened, never *what* made it.

One incidental finding from the same sweep, recorded because it is easy to misread as a fleet-wide fix:
exactly 3 of the 238 lanes (`_lane-1`, `_lane-2`, `_lane-3`) carry `rerere.enabled = false` in their own
`config.worktree`. That disarms **those three lanes only** — the other 235 inherit the armed shared
value. It is also the wrong scope for a fleet fix, which is exactly why `arm` writes `--local` and never
`--worktree` (§6).

None of this weakens the design; it sharpens the case for it. A one-shot `git config` write was observed
not to hold for even a day — which is the whole argument for shipping a re-runnable *guard* wired into
`setup-dev.sh` rather than a one-time fix. Note the ordering, though: nothing re-asserts the pin until
`setup-dev.sh` carries the guard, and that happens only when this task lands.

**Re-measured 2026-08-29, read-only, three days after the disarm — still armed.**
`bash scripts/git-rerere-guard.sh check /home/leo/src/reify` → **exit 1**,
`ARMED: rerere.enabled=true`. Per-scope reads: `--local` → `true`; `--global` and `--system` both
UNSET. `rerere.autoupdate` is `--local false` — so the store is in a *split* state, one key pinned and
the other not, which no single write by `arm` produces (it writes both). The store now carries **254**
linked worktrees. Three `check` runs took 1.51s / 0.63s / 0.91s wall — read-only and sub-second, which
is what makes the periodic-probe item below cheap.

> `Hypothesis:` the split state is consistent with a writer that sets `rerere.enabled` alone rather
> than re-writing the whole `[rerere]` section. That narrows the *shape* of the write, **not** its
> source; the writer is still unidentified and no candidate mechanism has been excluded.

### 2026-08-30 — the re-armer is IDENTIFIED (task 6889, open item (a))

**It is agent behaviour, not automation.** A bare `git config rerere.enabled true` run from inside a
**linked worktree** is `--local`-scoped, and for a linked worktree `--local` resolves to the **SHARED
common-dir** `.git/config` — not to worktree scope, which is what the operator intends. So a write
believed to be lane-local silently re-arms rerere for the whole store.

Proven hermetically rather than inferred, in a throwaway store built for the purpose (`git init` +
`git worktree add`): a bare `git config zzz.probe hello` issued from the LINKED worktree landed in
`main-checkout/.git/config` and **not** in `main-checkout/.git/worktrees/<name>/config`;
`git config --show-scope --show-origin` reported it as `local  file:.../main-checkout/.git/config`.
In that same lane `git rev-parse --absolute-git-dir` gives `.../worktrees/<name>` while
`--git-common-dir` gives `.../main-checkout/.git` — the two paths whose conflation is the whole bug.

The real store matches: `git -C /home/leo/src/reify-fix{12,13,14,15} rev-parse --git-common-dir` all
return `/home/leo/src/reify/.git`, so a `git config` issued in any of those recovery checkouts writes
the shared config. Full identification evidence, including the transcript counts that located the
instruction (the literal string appears 27× in one session transcript and across 37 files under
`~/.claude/projects/-home-leo-src-reify/`) and the hard negatives that cleared every mechanical
suspect: mem0 `bdae6960-5a5e-4c73-838f-7919c08cead5`.

This explains the SPLIT signature §7 hypothesised above and could not previously source: the
instruction sets `enabled` **alone**, so the guard's earlier `autoupdate = false` survives untouched.
Every armed sighting since has carried exactly that shape.

**Measurements taken 2026-08-30 (read-only, from lane `_lane-31`).** Observations, stated as fact:

| When | Measurement |
|---|---|
| 07:06:11 | `.git/config` mtime; size **2508**; SPLIT (`rerere.enabled=true` + `rerere.autoupdate=false`, both `--local`). |
| 11:44:46 | Re-armed. Same size **2508**, same SPLIT shape. `rr-cache` **355** entries, **254** linked worktrees. |
| 12:13:01 | Re-armed a **third** time the same day. Size **2508**, SPLIT. Measured first-hand at 12:29 BST: `rr-cache` **355** entries, **253** linked worktrees, 43 packs, 1638 refs, 65G `.git`. |

The **one-byte size discriminator** tells a third-party write from the guard's own: `true` → `false`
is one character longer, so an armed config is 2508 bytes and the guard's disarm write leaves 2509.
Every sighting above is 2508 — none of them is the guard.

`rr-cache` growth across the same window: **266** (2026-08-26) → **269** (2026-08-28) → **354**
(2026-08-30, earlier) → **355** (2026-08-30, 12:29). Entries are still never pruned (§5).

`check` from a lane against the live store: **exit 1** (`ARMED: rerere.enabled=true`), stdout empty,
three consecutive runs at **0.180s / 0.082s / 0.120s** wall (an earlier cold run the same day measured
0.249s). Read-only and sub-second, which is what makes lane-cadence arming free.

**The re-arm interval is minutes, not hours.** esc-5870-14 measured a **~10-minute** window between a
disarm and the next armed reading; the 11:44:46 → 12:13:01 pair above is ~28 minutes. The earlier
"~17h" figure was an artefact of infrequent sampling, not the writer's actual rate.

**Negative sweeps, re-run 2026-08-30 and still clean.** No `rerere` *writer* exists in reify's
`scripts/` or `hooks/` beyond the guard itself — the only hits are `setup-dev.sh` *calling* the guard
and a row in `scripts/verify-pipeline-infra-tests.txt`. A grep of dark-factory's code files
(`*.py`, `*.sh`, `*.toml`, `*.yaml`, `*.yml`, excluding its worktrees, `data/`, `docs/` and `plans/`)
returns **zero** matches for `rerere`; the only tree-wide hits are prose in archived escalations and
confusion-codebook entries. The orchestrator side is now cleared, closing the gap §7 left open when
the equivalent 2026-08-27 sweep was killed at a 45s timeout.

**CONSEQUENCE — do not read the lane-cadence wiring (§8 item 3) as a cure.** No `arm` cadence outruns
an agent that re-runs the shared write: an acquire pins the store, an agent re-arms it minutes later,
and any resolution recorded in between still lands in the one shared cache. Lane cadence is a
**mitigation** that narrows the exposure window from "since the last developer setup" to "since the
last acquire". The only cure is stopping the practice: never run a bare `git config rerere.enabled
true` (or any bare `git config rerere.*` write) in **any** reify worktree — use process-scoped
`git -c rerere.enabled=true <cmd>` when the behaviour is genuinely wanted for one command.

## 8. Open items

All three items below are **CLOSED** as of 2026-08-30 (task 6889). They are kept as a numbered
ledger — at their original numbers, which §7 back-references — because each records what was
measured and, more importantly, what its closure does *not* buy.

1. **CLOSED 2026-08-30 (task 6889, open item (b)) — `git fsck` is MEASURED, and the store is clean.**
   `git fsck --connectivity-only --no-progress` on `/home/leo/src/reify` completes in **1m12s**,
   **exit 0**, emitting only `dangling tree` / `dangling commit` lines. The 900s timeout that killed
   the 2026-07-30 attempt was never a wall, and the queue-idle window this item asked for turned out
   not to be needed. Store shape measured alongside the run: **39G** of objects, **43** packs,
   **1641** refs, **253** linked worktrees. Forensics record: mem0
   `d26dcfbb-c59e-4de7-b4da-fdb6570e51e9`.

   **The interpretive point outlives the result, and is the half a future reader cannot re-derive
   from a clean exit code: `.git/rr-cache` holds plain `preimage`/`postimage`/`thisimage` FILES, not
   git objects.** The concurrent rr-cache writers this runbook is about were therefore never capable
   of corrupting the object database *at all* — no amount of cross-lane rr-cache contention reaches
   the ODB, so a clean fsck here confirms a property that was structural rather than lucky. The only
   ODB-visible residue of the exit-128-yet-lands path (§3) is **dangling objects**, which is exactly
   what the run reported.

   This **retires** the old "whether the concurrent writes left any object-level damage behind is an
   open question" caveat rather than deferring it: §8's standing claim about the existing store's
   integrity is now positive — measured clean — instead of absent.

2. **CLOSED 2026-08-30 (task 6889, open item (a)) — the re-armer is IDENTIFIED.** It is **agent
   behaviour**, not automation: a bare `git config rerere.enabled true` run from inside a **linked
   worktree** is `--local`-scoped, and for a linked worktree `--local` resolves to the **SHARED**
   common-dir config. The evidence — the hermetic proof, the transcript counts that located the
   instruction, the hard negatives that cleared every mechanical suspect (including the now-complete
   dark-factory sweep), and the measured re-arm interval — is in §7's *2026-08-30 — the re-armer is
   IDENTIFIED* subsection, and is not restated here.

   **What survives the resolution.** Naming the writer sharpens this item's bound; it does not lift
   it. The guard re-asserting `false` — now at lane cadence as well as developer cadence (item 3) —
   still only **narrows the window between re-arm and disarm; it does not close it**, because no
   `arm` cadence outruns an agent re-running the shared write, and any resolution recorded inside
   that window still lands in the one shared cache. The **cure** is the practice change stated at
   the end of §7, not the cadence.

   **The periodic-`check` probe this item proposed is still the right monitoring**, and its
   normative caveat is untouched by the identification: **such a probe must treat any non-zero as
   "not clean", not just 1** — `check` exit **3** means it could not determine some lane's state at
   all (§6), and reading that as healthy would put the fail-open hole back in the one place the
   probe exists to close.

   **RETIRED hypothesis — `include.path`, ruled out.** This item previously carried a `Hypothesis:`
   that the store was being re-armed through an `include.path` chain rather than a direct key. It
   was never the diagnosis, and it is not the diagnosis now: the identified writer sets the key
   directly, and the measurements agree — both the 2026-08-27 reading and a read-only
   re-measurement on 2026-08-28 found the keys set **directly** in `/home/leo/src/reify/.git/config`
   (lines 46-48) with **no `include` directive anywhere in that file** (`check` exit 1, `rr-cache`
   269 entries, still armed). It is recorded here rather than deleted because the *detectability*
   point stands on its own: an `include.path` re-arm would leave no `rerere` string in the file an
   auditor greps, so §6's `--includes` reads keep covering that shape whichever writer is live.

3. **CLOSED 2026-08-30 (task 6889, open item (c)) — `arm` now runs at LANE cadence.** The gap this
   item recorded was real: `CLAUDE.md` states the disarm as an invariant while `setup-dev.sh` was
   the only caller, so *no frequently-executed path upheld it* — which is why the 2026-08-29
   re-measurement found the store armed with nothing having re-asserted the pin in between.
   `scripts/seed-warm-lane.sh --fresh-checkout` now delegates to `git-rerere-guard.sh arm` at the
   tail of the seed, re-pinning the shared config on **every warm-lane ACQUIRE**. The mechanism —
   mode gate, existence gate, the fail-open `0 | 2 | *` branch that guarantees an acquire never
   fails because the store could not be pinned, and the `REIFY_WARM_LANE_RERERE_ARM=0` operator
   escape hatch — is normative in that script's *git rerere disarm at LANE cadence* block and
   summarised in the guard header's CALLERS section; it is not restated here.

   **CORRECTION, in place rather than merely superseded: `setup-main-gate-worktree-config.sh` does
   NOT run per lane.** This item previously named `scripts/setup-main-gate-worktree-config.sh` as
   the obvious fix because "it already runs per lane". That is **measured false**, and it is the
   exact claim that mis-aimed this task's own original description, so it is corrected here rather
   than quietly dropped. MEASURED on base `ee5c57c8ca`: its only runtime caller is
   `scripts/setup-dev.sh:341` — the *same* developer-setup cadence this gap is about — so wiring
   `arm` there would have bought nothing. `scripts/seed-warm-lane.sh --fresh-checkout`, driven by
   dark-factory's `_seed_warm_lane()` (`git_ops.py:1227`) on every ACQUIRE, is the only genuine
   per-lane host inside reify. `scripts/warm-lane-preflight.sh` is **not** a substitute either: it
   is pool-level, takes `--mount`/`--base-dir` and never a lane dir, and is fail-closed.
   Measurement record: mem0 `329f8efa-c09f-47d7-b926-5880e11f619c`.

   **A mitigation, not a cure — do not read this closure as closing the hazard.** Lane cadence
   narrows the exposure window from "since the last developer setup" to "since the last acquire";
   it does not close it, because the re-armer is **agent behaviour** and no `arm` cadence outruns an
   agent re-running the shared write (§7's CONSEQUENCE paragraph, and item 2 above). The cost is why
   running it every acquire is free regardless: `check` is sub-second on the live 253-lane store
   (0.082–0.180s, measured §7) and `arm` is a byte-level no-op once the pin is in place.

   **Known gap, left deliberately.** The merge-spec lane is not covered: dark-factory's
   `acquire_spec_lane` (`git_ops.py:5923`) calls `_seed_warm_lane(lane, '--reset-in-place')` at
   `:6076`, at an indent common to BOTH its create and its reset branch, so a merge-spec acquire is
   **always** `--reset-in-place` and never reaches the block. That is harmless for this defence,
   which is why the gate was left as-is rather than widened: the pin is a property of the **one
   shared `.git/config`**, not of a lane, so any acquire that pins it pins it for every lane
   including the spec lane — and task-lane acquires dominate by volume. It would matter for a
   lane-scoped write; it does not for a shared-store one.

## Pointers

| Topic | Source |
|---|---|
| Guard contract, exit codes, subcommand detail (incl. CALLERS, both cadences) | `scripts/git-rerere-guard.sh` header |
| Lane-cadence `arm` wiring (mode gate, fail-open, opt-out) | `scripts/seed-warm-lane.sh` → *git rerere disarm at LANE cadence* block |
| Developer-cadence `arm` wiring (aborts setup on an unexpected non-zero) | `scripts/setup-dev.sh` |
| Behavioural oracles (real conflicted merges against a populated cache) | `tests/infra/test_git_rerere_guard.sh` |
| Sibling shared-`.git` hazard (`refs/stash` is one host-wide ref) | `CLAUDE.md` → "Warm lanes"; `hooks/reference-transaction` |
| Warm-lane pool lifecycle & invariants | `docs/prds/warm-lane-pool-cow-seeding.md` §9.3/§9.5 |
