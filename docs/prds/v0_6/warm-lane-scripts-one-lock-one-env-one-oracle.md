# Warm-lane scripts — one lane-lock owner, one worktrees env name, one status oracle

**Milestone:** v0.6 · **Status:** active (decomposed 2026-10-12; pass-1 open questions ruled by the lead, §10) · **Authored:** 2026-10-12
**Program:** `plans/review-all-program-reify-20261012.md`, stream **S20** (wave 1, agent, M/low).
**Code anchors** verified against main `8e6ffcac9d` (2026-10-12). Main moves fast — cite-by-symbol; re-locate lines at implementation time. Finding anchors were verified at `a030ee7528`; `git diff a030ee7528..8e6ffcac9d` over every path this PRD names touches only comment prose in `scripts/seed-warm-lane.sh` (tasks 7410, the ACQUIRE MODES rename) and `scripts/warm-lane-audit.sh` (task 7525, timer wording). No symbol moved; the seed lock-path derivation now sits at about line 928, not 930.

---

## 1. Goal and consumers

The warm-lane pool scripts state one protocol each in several places. This PRD gives each protocol one owner:

1. **The per-lane mutex.** `scripts/lib_lane_lock.sh` owns the lock path, the exclusive acquire/release on one fixed FD, the shared probe it already has, and a new "is it held?" assertion. `seed-warm-lane.sh --assume-lane-lock-held` calls that assertion, so the cross-repo handshake with dark-factory is checked when it is used.
2. **The worktrees directory name.** `REIFY_WARM_LANE_MOUNT` keeps one meaning, the XFS mount point. The new `REIFY_WARM_LANE_WORKTREES` names the worktrees dir. Its default is `$REIFY_WARM_LANE_MOUNT/worktrees`.
3. **The backing-task status oracle.** `scripts/lib_task_status.sh` owns the invocation contract, the terminal predicate and the task-branch-to-id parse.
4. **The dark-factory records the audit reads.** `warm-lane-audit.sh` reads `plan.json` and `.lane-state/<lane>.json` through `python3 -I` stdlib `json`. It is tested against golden records produced by dark-factory's own serialisers.
5. **The main-gate sentinel.** The sentinel carries the main tip it sanctions. `hooks/reference-transaction` consumes it only when that tip equals the transaction's old sha.

The PRD also covers three smaller items. `warm-lane-gc.sh::_do_reclaim` is factored into per-entry functions. `seed-warm-lane.sh --capabilities` replaces dark-factory's source-text grep. Greek-letter aliases leave the gc and sweep help text.

**Consumers (G1).**

| Mechanism | Consumer |
|---|---|
| `lane_lock_path` / `lane_lock_acquire` / `lane_lock_release` / `lane_lock_assert_held` | `seed-warm-lane.sh`, `thin-warm-lane.sh`, `warm-lane-gc.sh` (acquirers); `warm-lane-lock-guard.sh`, `warm-lane-audit.sh` (path only); dark-factory `GitOps._seed_warm_lane` (indirectly: its `--assume-lane-lock-held` is now verified) |
| `REIFY_WARM_LANE_WORKTREES` | `warm-lane-audit.sh`, `warm-lane-lock-guard.sh`, `warm-lane-gc.sh`, `warm-lane-gc-sweep.sh`, `warm-lane-preflight.sh` Check 6; the operator recipe in `docs/notes/warm-lane-audit-runbook.md` |
| `lib_task_status.sh` (`task_status_raw`, `task_status_bucket`, `task_branch_id`) and `lib_lane_entry.sh` (`lane_entry_matches_glob`, `lane_entry_is_git_worktree`) | `warm-lane-audit.sh`, `warm-lane-preflight.sh` Check 6, `warm-lane-degenerate-ref-check.sh`; `warm-lane-gc.sh` (entry helpers only) |
| `scripts/warm_lane_records.py` | `warm-lane-audit.sh` (`_read_lane_assignment`, `_read_plan_sync`) |
| golden records `tests/infra/fixtures/warm_lane_golden/` | `tests/infra/test_warm_lane_audit.sh`; refreshed from dark-factory `orchestrator/lane_lifecycle.py::LaneRecord.to_json` and `orchestrator/mcp/plan_tools.py`'s writer |
| `seed-warm-lane.sh --capabilities` | dark-factory `git_ops.py::_seed_script_supports` |
| tip-bound sentinel + `scripts/main-gate-mark.sh` | `hooks/reference-transaction`; marked by `hooks/pre-commit`, `hooks/pre-merge-commit`, `scripts/land.sh`, and dark-factory `advance_main` / `recover_red_main` via `dark-factory-orchestrator.yaml` `git.main_gate_mark_command` |

What a user observes: an operator, a dark-factory agent, or the orchestrator itself gets a loud, specific outcome where today the scripts report a silently wrong answer. Each leaf in §8 names its signal.

## 2. Findings closed

| key | where | sev | verdict | leaf |
|---|---|---|---|---|
| fk-60aa699ea8ce | `scripts/seed-warm-lane.sh` lane-mutex protocol | medium | weakened | L1 |
| fk-05e33a09a266 | `scripts/warm-lane-lock-guard.sh` `REIFY_WARM_LANE_MOUNT` two meanings | medium | confirmed | L2 |
| fk-6fab601d98c2 | `scripts/warm-lane-audit.sh::_task_status_raw` three oracle copies | medium | confirmed | L3 |
| fk-26e1e3d2eb67 | `scripts/warm-lane-audit.sh::_plan_anchor` hand-rolled JSON readers | medium | confirmed | L4 |
| fk-4b80fe8f7de1 | `tests/infra/test_warm_lane_audit.sh::make_plan` producer-byte fixtures | medium | confirmed | L5 |
| fk-cee9ca6b041a | `scripts/warm-lane-gc.sh::_do_reclaim` 337-line function | low | confirmed | L6 |
| fk-7dba1d7f3f47 | `scripts/warm-lane-gc.sh::_usage` Greek aliases | low | confirmed | L7 (adopts filed #8541) |
| fk-d30433df0ed6 | dark-factory flag-capability grep | low | confirmed | L8 |
| fk-638f0a64b948 | `hooks/main-gate-lib.sh::main_gate_mark` unbound sentinel | medium | confirmed | L9 (L10 completes it) |

**Weakened-finding re-read (fk-60aa699ea8ce).** The skeptic struck one clause: `lib_lane_lock.sh` *does* exist, as "the SINGLE tri-state warm-lane lock probe". It defines only `lane_lock_probe` and is sourced only by `warm-lane-audit.sh` and `warm-lane-lock-guard.sh`. The rest of the finding re-verifies at `8e6ffcac9d`:

- There are six independent lock-path derivations: seed `LANE_LOCK="${LANE_DIR}.lock"`, thin `LANE_LOCK="${LANE_DIR}.lock"`, gc Pass 1 `lane_lock` and Pass 2 `orphan_lock` (`${WORKTREES_DIR}/${name}.lock`), lock-guard `LOCK="${LOCK_PATH:-$MOUNT/$LANE.lock}"`, and audit `_probe_live "$MOUNT/$name.lock"`.
- Two FD numbers are used for the same lock: 9 in seed and thin, 8 in gc.
- Under `--assume-lane-lock-held`, seed only clears `_should_acquire_lane_lock` and runs zero checks.

This PRD therefore *extends* the existing library, per the ruling. It does not create a new one.

## 3. Pre-conditions and substrate (G3), measured at `8e6ffcac9d`

- `scripts/lib_lane_lock.sh::lane_lock_probe` takes a shared, non-blocking, read-only flock. It returns IDLE for an absent lock file, BUSY only on observed exclusive contention, and UNMEASURABLE otherwise. Hermetic coverage: `tests/infra/test_lane_lock_probe.py` (wrapper `.sh`, manifest row `pool`).
- **The dark-factory lock holders all use flock(2) on `<lane_dir>.lock`.** `GitOps._seed_warm_lane` with `SeedLaneLock.TAKE` wraps seed in `flock -x -w 30 -E 124 <lane_dir>.lock`. `HELD_BY_CALLER` callers hold `fcntl.flock(LOCK_EX)`. `verify_cancel.py::lane_lock_path` is `lane_dir.with_name(lane_dir.name + '.lock')`. Both flags are passed only when `_seed_script_supports` finds the literal flag text anywhere in the lane's own copy of the script (`flag in _seed_script_text(...)`). A flock(2) probe opened on a *fresh* open-file description therefore observes every one of these holders as BUSY. That is what makes `lane_lock_assert_held` sound. In-repo holders behave the same way: thin holds FD 9 and gc holds FD 8 across their seed calls.
- **`_seed_rc_to_unavailable` maps return codes as follows:** 75→DISK_PRESSURE, 76→BASE_ABSENT, 77→LANE_LOCK_CONTENDED, 124→LANE_LOCK_TIMEOUT, 127→absent, everything else→FAULT. The seed script does not use exit 70 today. A not-held refusal on 70 therefore lands in FAULT (blocked + L1) with no dark-factory change. That is the correct disposition for a broken handshake.
- **The production setters of the worktrees-dir env names are tests only.** `reify-warm-lane-gc.service` runs `warm-lane-gc-sweep.sh --mount /home/leo/src/warm-lanes/worktrees`, an explicit flag. `orchestrator-reify.service.d/warm-lane.conf` sets only `REIFY_LANE_LEAK_STATUS_CMD`. Dark-factory source sets none of `REIFY_WARM_LANE_MOUNT`, `REIFY_WARM_LANE_GC_MOUNT`, `REIFY_WARM_LANE_GC_SWEEP_MOUNT` or `REIFY_LANE_LEAK_WORKTREES` (`git grep` over `orchestrator/src` and `shared`: 0 hits). Dark-factory passes the worktrees dir to audit and gc as the CLI flag `--mount <worktree_base>`, and that CLI contract is kept (D5). The lock-guard comment saying dark-factory "exports REIFY_WARM_LANE_MOUNT AMBIENTLY" has no producer at main and is corrected in L2. A **fourth** worktrees-dir env name exists that the finding did not count: `warm-lane-preflight.sh` Check 6 reads `REIFY_LANE_LEAK_WORKTREES` with default `$MOUNT/worktrees`.
- **The status oracle has three consumer copies and one producer.** The copies are the audit's `_task_status_raw`/`_status_bucket`, preflight Check 6 inline, and degenerate-ref-check's `_ref_status` plus an inline `done|cancelled` arm. The producer is `scripts/lane-task-status.sh`, whose header also claims "byte-for-byte identical" consumers. `warm-lane-gc.sh` has zero `STATUS_CMD` references, so the audit header and the `_task_status_raw` comment naming gc as a consumer are stale.
- `/usr/bin/python3` is 3.12.3 on this host. That is the interpreter the gate reaches (reify CLAUDE.md "Infra tests"). The audit's no-python3 stance is a comment, not a test. No test asserts its absence (`git grep python3 -- scripts/warm-lane-*.sh` hits only the two explanatory comments).
- **Dark-factory serialisers.** `LaneRecord.to_json` is `json.dumps(self.to_dict(), indent=2)`, and `LaneState` has six values. `plan.json` is written by `orchestrator/mcp/plan_tools.py`.
- **There are five main-gate mark sites, not three (G6 correction).** The finding counted pre-commit, pre-merge-commit and land.sh. At main, dark-factory `GitOps.advance_main` and `GitOps.recover_red_main` also mark, by running `dark-factory-orchestrator.yaml` `git.main_gate_mark_command` (`touch "$(git rev-parse --git-common-dir)/reify-main-gate-ok"`). They clear with `main_gate_unmark_command` (`rm -f …`). These two sites carry roughly 28 landings a day. **`reify.mainGate.enforce` is `true`** (`git config --bool --get`), so a format change that left those two sites writing an empty file would hard-abort every merge-queue landing. L9/L10 are sequenced around this (D9).
- **Only 3 "must stay a literal" notes exist at main**, not the brief's 11: `seed-warm-lane.sh` `_usage` (one line), the parser note above `--distinct-lock-refusal-rc)`, and the `Q11_FLAG` note in `tests/infra/test_seed_warm_lane.sh`.
- **Dark-factory ships its own copies** of gc, gc-sweep, thin, disk-guard, audit, degenerate-ref-check, provision, `lib_live_refs.sh`, `lib_portable.sh` and `lib_task_citation.sh` under `orchestrator/scripts/warm-lane/`. Resolution is project-first: `<project_root>/scripts/<name>` wins (`GitOps._resolve_warm_lane_script`). Reify's copies are therefore the ones that run for reify. The dark-factory copies are dark-factory's to re-sync (§7).

## 4. Resolved design decisions

**D1 — The lock API lives in `lib_lane_lock.sh`, beside `lane_lock_probe`.**

| Function | Behaviour |
|---|---|
| `lane_lock_path <lane_dir>` | Prints `${lane_dir%/}.lock`. The only reify-side derivation. It matches dark-factory `lane_lock_path` byte for byte. |
| `lane_lock_acquire <lane_dir> <wait>` | `<wait>` is `0` (non-blocking refuse), `N` (`flock -w N`) or `unlimited` (blocking). This covers seed's three wait modes. It opens `LANE_LOCK_FD` with `>` (creating the file, as every acquirer does today) and takes `flock -x`. Return values: **0** acquired; **75** refused (contended or deadline); **64** bad wait value; **1** open failure or missing flock. On a refusal it writes the existing `LANE_LOCK_CONTENDED: …` stderr line once, so the dark-factory-grepped marker has one home. Each caller maps the return code onto its own exit contract: seed keeps 75/77, thin 75, gc "preserve". |
| `lane_lock_release` | **[Amended 2026-10-20, retro-critic R7]** Keys **only** on a lib-private flag meaning "acquired by `lane_lock_acquire` in this shell", never on the FD or on lock state. With the flag set it runs an explicit `flock -u` on `LANE_LOCK_FD`, closes it and clears the flag; without it, it is a no-op. Reason: under `--assume-lane-lock-held` seed inherits the caller's lock open-file description, which after D2 is FD 8 for thin and gc alike, so a release decided from the FD or the lock state would `flock -u 8` the caller's lock mid-reset (inv.2). Seed's EXIT trap is installed **only on the acquire path**, never under `--assume-lane-lock-held`, and the lib header says so. The trap preserves the task #5705 LANE-LOCK RELEASE CONTRACT. That block moves into the lib unchanged in substance, and seed keeps a one-line pointer. |
| `lane_lock_assert_held <lane_dir>` | Runs `lane_lock_probe` on the derived path and returns 0 **only for BUSY**. IDLE (including an absent lock file) and UNMEASURABLE both fail closed, with `LANE_LOCK_ASSERT_DETAIL` set. It never creates the file (A1 carries over). |

"Mode" in the ruling's `lane_lock_acquire(mode, wait)` is read as the wait mode. The only lock mode any caller takes is exclusive, and the shared mode is `lane_lock_probe`. A shared-acquire entry point with no caller is not built (h6). Ruled (Q1 a): mode = wait mode.

**D2 — One FD: `LANE_LOCK_FD=8`.** Seed and thin move from FD 9 to FD 8. gc already uses FD 8 for the lane lock and FD 9 for the shared gen lock in `_reseed_lane_pinned`. That is why seed's own FD-9 acquire would "clobber gc's FD-9 gen lock", per gc's own comment. A single constant ends the FD 8/9 split. The detached-children `_close_inherited_fds` mechanism closes FDs generically and is unaffected.

**D3 — `--assume-lane-lock-held` is verified at use.** When the flag is passed, seed calls `lane_lock_assert_held "$LANE_DIR"` before any target mutation. On failure it prints `LANE_LOCK_NOT_HELD: <detail>` to stderr and **exits 70**. Dark-factory maps 70 to FAULT, which escalates instead of silently reseeding an unlocked lane. Accepted gap: a fresh-description probe proves that *some* process holds the lock exclusively, not that the caller does. A stranger holding it while dark-factory does not is not caught. The assertion closes the finding's named scenario, where dark-factory stops holding the lock but keeps passing the flag. Its next-change sentence, "seed reseeds unlocked", becomes a FAULT. Companion dark-factory task D1 adds the other face of the boundary test.

**D4 — The worktrees dir has its own env name.** `REIFY_WARM_LANE_WORKTREES` defaults to `$REIFY_WARM_LANE_MOUNT/worktrees` when the mount is set. Five scripts read it: audit, lock-guard, gc (default for `--mount`, which already means the worktrees dir there and derives `--worktrees-dir` and `--base-target`), sweep (default `--mount`), and preflight Check 6. Four names retire: `REIFY_WARM_LANE_GC_MOUNT`, `REIFY_WARM_LANE_GC_WORKTREES_DIR`, `REIFY_WARM_LANE_GC_SWEEP_MOUNT` and `REIFY_LANE_LEAK_WORKTREES`. A script started with a retired name set to a non-empty value exits **2** and names the replacement. A silent ignore would be the same silent-wrong-answer class as the finding (h10). The sweep's hard-coded `/home/leo/src/warm-lanes/worktrees` default goes; the timer already passes `--mount`. `REIFY_WARM_LANE_MOUNT` keeps the XFS-mount meaning in provision, preflight, seed, thin, disk-guard, ensure-warm-base, relocate and seed-warm-base-initial. `dark-factory-orchestrator.yaml` `warm_lane_pool.substrate.mount_env` therefore stays correct as written.

**D5 — CLI flags are a dark-factory contract and do not change.** Dark-factory passes `--mount <worktree_base>` to audit and gc (`_run_warm_lane_audit`, `_run_warm_lane_gc_reclaim`), so `--mount` keeps its per-script meaning. Lock-guard and audit gain one guard on their worktrees-dir input, whether from the flag or the env var. When the directory has a `worktrees/` child and no `*.lock` siblings, they print a warning that the value "looks like the XFS mount; pass the worktrees dir". This closes the silent-IDLE path through the CLI as well as through the env var. Ruled (Q2 a): the flags keep their meaning; no dark-factory flag-day.

**D6 — Status-oracle contract.** `scripts/lib_task_status.sh` is pure, in the `lib_task_citation.sh` purity shape: every input is a parameter, it reads no globals and runs no git. It defines:

| Function | Behaviour |
|---|---|
| `task_status_raw <cmd> <id>` | Runs `"$cmd" "$id" 2>/dev/null \| tr -d '[:space:]' \|\| true`. An empty cmd or id yields empty output. |
| `task_status_bucket <raw>` | Maps to `terminal` (done, cancelled), `unknown` (empty) or `non-terminal` (anything else). |
| `task_branch_id <branch>` | Prints NNNN for `task/NNNN` with NNNN purely numeric, otherwise nothing. |

The audit's detached-HEAD `--contains` resolution stays in the audit, because it needs git. `_matches_glob` and `_is_git_worktree` move to `scripts/lib_lane_entry.sh` as `lane_entry_matches_glob` and `lane_entry_is_git_worktree`, sourced by gc and the audit. Every "mirrors … byte-for-byte" and "lifted verbatim" comment is deleted rather than reworded. `lane-task-status.sh`'s header names `lib_task_status.sh` as the consumer contract.

**D7 — Dark-factory records are read as JSON, through one reader with explicit outcomes.** `scripts/warm_lane_records.py` uses stdlib only and is run as `python3 -I <script> <kind> <file>`. It has two kinds:

| Kind | Output on success |
|---|---|
| `lane-state` | field 1 `state`, field 2 `task_id` |
| `plan` | field 1 `task_id`, field 2 `anchor_id`, field 3 `anchor_commit` |

**[Amended 2026-10-20, retro-critic R7]** The reader emits **one field per line** (read in bash with `mapfile -t`), or one record joined by the non-whitespace separator `\x1f`. The contract is stated by field position, and an empty field is an empty line (or an empty `\x1f`-delimited slot), never a dropped one. ~~one TAB-separated line~~: TAB is IFS-whitespace in bash, so `IFS=$'\t' read` collapses runs and an empty field anywhere but last shifts the others (a plan with no `task_id` but a done anchor would read `task_id=<anchor_id>`). The TAB line was declined by the ruling.

The anchor is the last entry, iterating `prerequisites` then `steps` *by key*, whose `status == "done"` and whose `commit` is a non-empty string. It is independent of key order, indent and array order.

| Exit | Meaning | Audit maps it to |
|---|---|---|
| 0 | parsed | the parsed fields |
| 3 | unparseable: invalid JSON, a top level that is not an object, or a field of the wrong type. **[Amended 2026-10-20, retro-critic R7]** `null` is permitted for `task_id` (read as empty) and for a non-done entry's `commit`; "wrong type" means a non-null non-string in those fields, or a non-string `state`. Every RELEASED `LaneRecord` carries `task_id: null`, and the golden plan's pending step carries `commit: null`. | existing `unparseable-record` |
| 4 | unreadable | existing `no-readable-record` / `-` paths |
| 127, or python3 missing | reader unavailable | new UNKNOWN cause `reader-unavailable` |

The audit never aborts (A3), and a missing interpreter is an attributed UNKNOWN, not `-`. Deleted: `_record_scalar`, `_plan_record_parses`, `_plan_anchor` (69-line awk) and both "no jq/python3 by design" notes. The raw→column `LaneState` mapping `case` stays in bash; it is the audit's own table.

**D8 — HEADROOM has one ordered field list.** A single bash array of `name:VAR` pairs drives both the table `HEADROOM …` line and the JSON `headroom` object. The output is byte-identical to today for both formats. The existing append-never-interpose rule then means appending one array element.

**D9 — The main-gate sentinel is bound to the tip it sanctions, rolled out in two phases.**

- **Phase A (L9).** `main_gate_mark` writes `git rev-parse refs/heads/main`. New `scripts/main-gate-mark.sh mark|unmark` is the CLI for out-of-process markers. `unmark` deletes only a sentinel whose content equals the current tip it would have written. `reference-transaction` handles four cases:
  - content == `_old`: sanctioned, consumed.
  - non-empty and ≠ `_old`: stale. Discarded, logged `stale sentinel <sha> discarded`, then handled as no sentinel (UNSANCTIONED; aborts under ENFORCE).
  - empty file: sanctioned but logged distinctly as `sanctioned (legacy unbound sentinel)`. This is the transition arm.
  - absent: as today.

  `land.sh` installs an EXIT trap that removes its own token by compare-and-delete. An unconditional `rm` could delete a dark-factory mark minted during land's hours-long gate. `dark-factory-orchestrator.yaml` `git.main_gate_mark_command` / `main_gate_unmark_command` become `scripts/main-gate-mark.sh mark` / `unmark`. The edit is accepted into the file at landing and takes effect at the next orchestrator restart, through `scripts/orchestrator-redeploy-restart.sh`, never `systemctl restart`. That gap is why the legacy arm exists.
- **Phase B (L10).** The legacy-empty arm is deleted once the gate log shows the dark-factory sites writing bound tokens.

Residual, stated honestly: a leftover bound token, for example from a commit aborted at the editor, still sanctions one raw fast-forward *from the same origin* until main next moves. Binding narrows the window from "until any later move" to "until main moves". No hook fires on an editor abort, so nothing narrower is reachable from a hook. A concurrent-marker race on the single shared file predates this PRD and is out of scope (§9).

**D10 — Seed capabilities are data.** **[Amended 2026-10-20, retro-critic R7]** `seed-warm-lane.sh --capabilities` prints the **capability-gated flags**, the flags a caller must probe per lane vintage, one per line, exact spelling (`--assume-lane-lock-held`, `--distinct-lock-refusal-rc`). They are held in one `CAPABILITIES` array. The base flags the parser also accepts (`-h|--help`, `--fresh-checkout`, `--reset-in-place`, `--lane-lock`, `--base-commit`, `--touch`, `--record-base`) are not capabilities and are not printed. ~~prints the optional flags its parser accepts … from one array that the parser arms are tested against~~ (contradicted B9: the parser accepts seven more optional flags). It exits 0 and ignores other arguments. The "must stay a LITERAL" notes (3 at main) go. Companion dark-factory task D8 switches `_seed_script_supports` to exact-line membership in `--capabilities` output (membership covers the capability-gated flags only), cached by (path, mtime_ns, size) as today. It falls back to the legacy substring probe *only* when `--capabilities` exits non-zero, which happens on a lane checked out at a vintage before L8. The fallback has a retirement condition, named in D8.

**D11 — Approach: bare B with two-way boundary tests on the two cross-repo seams.** The lock handshake gets L1 plus D1. The sentinel format gets L9's test matrix plus D9's dark-factory yaml wiring. The PRD meets none of the overlay's G5 high-stakes seams (FEA, ComputeNode, persistent-naming, multi-kernel, grammar), so a full B+H contract section is not warranted.

**D12 — New tests are Python; existing bash members are extended in place.** The one new member is `tests/infra/test_lib_task_status.py` with its `.sh` wrapper. **[Amended 2026-10-20, retro-critic R7]** It registers through whichever mechanism is live at implementation: the `# run-all-bucket: pool` pragma if S19 υ #9303 has landed, else a `pool` row in `tests/infra/run-all-classification.manifest`, in the same diff. ~~and a `pool` row in `tests/infra/run-all-classification.manifest`, in the same diff (overlay drift-guard rule)~~ (superseded by R7: gap-fill D-K8-4 deletes the manifest). All other coverage extends existing members: `test_lane_lock_probe.py`, `test_seed_warm_lane.sh`, `test_thin_warm_lane.sh`, `test_warm_lane_gc.sh`, `test_warm_lane_gc_sweep.sh`, `test_warm_lane_lock_guard.sh`, `test_warm_lane_audit.sh`, `test_warm_lane_preflight.sh`, `test_warm_lane_degenerate_ref.sh`, `test_reference_transaction_gate.sh` and `test_land_script.sh`. **[Amended 2026-10-20, retro-critic R7]** Each new or changed source path is registered through whichever mechanism is live: a `# guards:` header if S18 τ #9312 has landed, else a `scripts/verify-pipeline-infra-tests.txt` row in the same diff (τ migrates rows added before it lands). ~~Each new or changed source path gets a `scripts/verify-pipeline-infra-tests.txt` row in the same diff.~~ (superseded by R7: gap-fill D-K8-5 deletes the txt). No new wall-clock upper bound is asserted in any Python member (reify CLAUDE.md, #7445).

**Invariant candidate 5 (extract-with-ratchet) applies to L1 and L3.** **[Amended 2026-10-20, retro-critic R7]** Each ratchet is a set of named rows in S19 D9's shared engine (`tests/infra/adoption_ratchet.py` over `tests/infra/adoption-ratchet.manifest`, rows `pattern-id | ERE | path | ceiling | owner leaf`), at ceiling 0. No bespoke counter lives in either leaf's Python member. ~~Each names its ratchet member below.~~

## 5. Contract and boundary-test sketch (the two cross-repo seams)

| # | Scenario | Preconditions | Postcondition |
|---|---|---|---|
| B1 | reify face: flag passed, lock held | a separate process holds `flock -x` on `lane_lock_path` | seed proceeds; exit 0 |
| B2 | reify face: flag passed, nobody holds | lock file present and unheld | seed exits 70, stderr has `LANE_LOCK_NOT_HELD:`, `target/` untouched |
| B3 | reify face: flag passed, lock file absent | no `<lane>.lock` | seed exits 70; lock file NOT created |
| B4 | dark-factory face (D1) | `_seed_warm_lane(TAKE)` and `(HELD_BY_CALLER)` run a stub seed that tries `flock -n -x` on `lane_lock_path(lane_dir)` on a fresh fd | the stub's attempt fails (lock observed held) in both modes |
| B5 | sentinel: matched | mark on main at tip X; ff move X→Y | logged `sanctioned`, sentinel gone |
| B6 | sentinel: gate passed, commit aborted | mark at X; no commit; main moves X→Y by a sanctioned path; then raw `update-ref` Y→Z | the raw move is `UNSANCTIONED` (aborts under ENFORCE); stale token discarded |
| B7 | land.sh interrupted mid-gate | land.sh marks, gate killed by SIGINT/SIGTERM | no token remains; a token minted by another marker during the gate survives |
| B8 | dark-factory face (yaml) | `scripts/main-gate-mark.sh mark` then a CAS `update-ref` from that tip | `sanctioned`; `unmark` after a failed CAS removes only its own token |
| B9 | capabilities | `--capabilities` output | **[Amended 2026-10-20, retro-critic R7]** every `CAPABILITIES` array entry parses without `Unknown flag`, and the parser's capability arms equal the array, checked against a named exclusion list of base flags (D10). ~~every optional flag the parser accepts is listed~~ |

## 6. Cross-PRD relationship and seam owners (G4)

| Shared artefact | Owner (program G4 table) | This PRD's relation |
|---|---|---|
| `scripts/lib_lane_lock.sh` (probe, path, acquire) | **S20** | edits (L1) |
| dark-factory `git_ops.py::_seed_script_supports` / `_seed_warm_lane` | **S20** | dark-factory companions D1, D8 |
| dark-factory `orchestrator/lane_lifecycle.py` and `mcp/plan_tools.py` serialisers (golden-record owner) | **S20** | dark-factory companion D5 (reads the serialisers, never edits them) |
| `tests/infra/run-all-classification.manifest` rows for a new wrapper | **S19** | **[Amended 2026-10-20, retro-critic R7]** consumes S19 υ #9303 (`# run-all-bucket:` pragma, gap-fill D-K8-4) and S18 τ #9312 (`# guards:` header, D-K8-5): a new member registers through whichever mechanism is live at implementation (D12). ~~consumes: L3 adds one `pool` row in the existing format (ruled Q3 a: S19 owns the format, not the rows)~~ (superseded by R7) |
| `scripts/verify-pipeline-paths.txt` | **S18** | not edited. None of this PRD's paths require the full gate (`verify-pipeline-guard.sh requires-full-gate` exits 1 for every one). `scripts/verify-pipeline-infra-tests.txt` rows are added, and that file is not in the G4 table. |
| `hooks/main-gate-lib.sh`, `hooks/reference-transaction`, `scripts/land.sh`, `dark-factory-orchestrator.yaml` `git.main_gate_*` | not listed (S20 by its finding) | edits (L9, L10) |
| Comment prose (Greek aliases outside `_usage`, line cites in these scripts) | **S23** (wave 4) | not touched beyond what each leaf's edit necessarily rewrites |

No leaf depends on a later-wave stream. **There are no held leaves.**

## 7. Out of scope

- **Dark-factory's vendored copies** under `orchestrator/scripts/warm-lane/`, and their re-sync. Those are dark-factory's (repatriation PRD). A cross-reference line goes to the lead: the copies will lack `lib_lane_lock.sh` acquire, `lib_task_status.sh`, `lib_lane_entry.sh` and `warm_lane_records.py`.
- **Log-helper consolidation** (`info`/`hint` redefined in 8 of 11 files, three `hint` formats). It is in fk-6fab601d98c2's proposal but not in the ruled remedy or deletable list. It is not done, and the finding's oracle clause is closed by L3.
- **Renaming CLI `--mount` flags** (ruled out, Q2 a).
- **Greek letters in comments** (S23). L7 changes operator-visible `--help` text only.
- **`lane-task-status.sh`'s direct read-only `sqlite3` of `tasks.db`.** reify CLAUDE.md forbids reads while fused-memory is up. Not a finding of this run. Recorded as a cross-reference for the lead.
- **The concurrent-marker race on the single sentinel file.** Two markers in flight, the first consume deletes the second's token. It predates this PRD and the ruling does not cover it.

## 8. Decomposition plan

Labels are PRD-local; task ids were backfilled at decompose (2026-10-12). Every leaf carries `metadata.x_finding_key` (list), `x_finding_run = "review-all-reify-20261010"`, `source = "review-all"`, and `priority` = severity. Dark-factory companions (D*) are filed in project `dark_factory` via the shared fused-memory MCP and wired as `metadata.external_deps` from or to the reify leaf. They carry no `x_finding_key`, because the key lives on the reify leaf.

**L1 — Lane-lock API in lib_lane_lock.sh; seed/thin/gc acquire through it; `--assume-lane-lock-held` asserts** · medium · keys [fk-60aa699ea8ce] · **#8624**

- **Modules:** `scripts/lib_lane_lock.sh`, `scripts/seed-warm-lane.sh`, `scripts/thin-warm-lane.sh`, `scripts/warm-lane-gc.sh`, `scripts/warm-lane-lock-guard.sh` (path only), `scripts/warm-lane-audit.sh` (path only), tests above, `scripts/verify-pipeline-infra-tests.txt`.
- **Work:**
  - D1–D3. Seed and thin source the lib from `$SCRIPT_DIR`. A missing lib exits 2 with a directed message, mirroring gc's `lib_live_refs.sh` check.
  - Fixtures that pass `--assume-lane-lock-held` without holding the lock (32 in `test_seed_warm_lane.sh`, 3 each in thin and gc) now hold one, or become the B2/B3 negatives.
  - `test_seed_lane_lock_release_soak.sh`'s release-contract pointer follows the moved block. **[Amended 2026-10-20, retro-critic R7]** The moved block cites `_close_inherited_fds` by symbol: #9320 (gap-fill D-K8-10(6)) moves it into `scripts/lib_fd_hygiene.sh` after S19 μ₂ #8719.
  - **[Amended 2026-10-20, retro-critic R7]** Release keys on the lib-private acquired flag (D1), and seed's EXIT trap is installed only on the acquire path. Q7b and the Block-V FD checks in `test_seed_warm_lane.sh` are retargeted from FD 9 to `LANE_LOCK_FD=8`, with a mutation note proving Q7b reds when release is unconditional. This lands in L1, before μ₂ #8719 moves Block V.
  - **[Amended 2026-10-20, retro-critic R7]** Seed rc 70 is the **one exception** to thin's best-effort exit-0 rule for `--reseed`. Thin maps no other seed failure to a fault code: a failed re-seed is logged, the clone discarded, and thin exits 0. On seed rc 70 thin exits 70 after the (no-op) discard, with empty stdout. Thin's header and its `--help` exit-code table change. The test uses a stub seed that exits 70, because the real path is unreachable while thin holds the lock. ~~Ruled (Q6): `thin-warm-lane.sh --reseed` propagates seed's exit 70 unchanged. A not-held refusal is a distinct outcome and is never mapped to thin's generic-fault code; thin's `--help` exit-code list gains 70.~~ (superseded by R7; the premise that thin maps seed failures to a fault code was false.)
  - **[Amended 2026-10-20, retro-critic R7]** The new members register through whichever mechanism is live at implementation (D12).
- **Signal:** B1–B3 pass in `test_seed_warm_lane.sh`. Seed run by hand with the flag on an unheld lane prints `LANE_LOCK_NOT_HELD:` and exits 70 with `target/` untouched. `test_lane_lock_probe.py` gains acquire/release/assert cases and stays green. All five scripts' suites stay green.
- **Ratchet (INV candidate 5):** **[Amended 2026-10-20, retro-critic R7]** rows in `tests/infra/adoption-ratchet.manifest` at ceiling **0**: `lane-lock-path-construction` and `lane-lock-exec-open`, over seed/thin/gc/lock-guard/audit. Their EREs exclude the `.gen` and `.seed-stamp.json` siblings and comment lines. The done-criteria name both rows. They are what makes the one-spelling `seed-acquires-through-the-lib` check acceptable. ~~`test_lane_lock_probe.py` counts, over seed/thin/gc/lock-guard/audit, (a) lane-lock path constructions outside the lib (`\.lock"` built from a lane dir or name, excluding the `.gen.N.lock` sites in refresh and gc) and (b) `exec <n>>…lane…lock` opens outside the lib. Both must be **0**.~~
- **Prereqs:** none.

**L2 — `REIFY_WARM_LANE_WORKTREES` names the worktrees dir; retire four aliases** · medium · keys [fk-05e33a09a266] · **#8635**

- **Modules:** audit, lock-guard, gc, gc-sweep, preflight scripts and their tests; `docs/notes/warm-lane-audit-runbook.md` (knob table and strandedness recipe); `docs/design/merge-verify-lane-dispatch-seam.md` (knob table row).
- **Work:**
  - D4 and the D5 looks-like-mount warning.
  - Correct the lock-guard "dark-factory exports REIFY_WARM_LANE_MOUNT ambiently" comment.
  - lock-guard and audit derive the path via `lane_lock_path "$WORKTREES/$LANE"`.
  - **[Amended 2026-10-20, retro-critic R7]** gc reclaim, which is destructive, gains an ambient default derived from `REIFY_WARM_LANE_MOUNT`: in an operator shell with the mount exported, `warm-lane-gc.sh reclaim` without `--mount` reclaims the live pool instead of exiting 2. gc's header and `--help` state this.
- **Signal:** with only `REIFY_WARM_LANE_MOUNT=<xfs mount>` exported, as provision instructs, `warm-lane-lock-guard.sh check` on a held `_merge-verify` reports **BUSY (exit 3)**. Today it reports IDLE. `warm-lane-audit.sh` reports the real resident count, not 0. `warm-lane-gc.sh` started with `REIFY_WARM_LANE_GC_MOUNT=x` exits 2 naming `REIFY_WARM_LANE_WORKTREES`. These are new cases in `test_warm_lane_lock_guard.sh` / `test_warm_lane_audit.sh` / `test_warm_lane_gc.sh`.
- **Prereqs:** L1 (shared files; lock-guard and audit call `lane_lock_path`).

**L3 — lib_task_status.sh owns the oracle contract; lib_lane_entry.sh owns the entry helpers** · medium · keys [fk-6fab601d98c2] · **#8641**

- **Modules:** new `scripts/lib_task_status.sh` and `scripts/lib_lane_entry.sh`; audit, preflight, degenerate-ref-check, gc, `lane-task-status.sh` (header only); new `tests/infra/test_lib_task_status.py` and `.sh`; one manifest row; infra-tests rows.
- **Work:** D6. Delete the three inline copies, the two duplicated helper pairs, and the stale gc-consumer citations.
  - **[Amended 2026-10-20, retro-critic R7]** Drift: task 9181 added a new `_is_git_worktree "$_stash_repo"` call site in the audit; L3 migrates it too.
  - **[Amended 2026-10-20, retro-critic R7]** Pending #8780 rewrites `scripts/lane-task-status.sh` off direct `tasks.db` reads (possibly to a `.py`). There is no dependency edge, only file-lock serialisation; L3's header note follows wherever #8780 puts the contract.
  - **[Amended 2026-10-20, retro-critic R7]** The new member registers through whichever mechanism is live at implementation (D12).
- **Signal:** an oracle stub that prints `superseded` is classified identically by audit (`pin`/bucket), preflight Check 6 and degenerate-ref-check `--audit`. A three-consumer agreement case lives in `test_lib_task_status.py`, which drives all three CLIs. Each script's own suite stays green.
- **Ratchet (INV candidate 5):** **[Amended 2026-10-20, retro-critic R7]** rows in `tests/infra/adoption-ratchet.manifest` at ceiling **0** over `scripts/warm-lane-*.sh`: `status-oracle-invocation`, `done-cancelled-arm` and `entry-helper-def`, with EREs that exclude comment lines. The done-criteria name the rows. ~~`test_lib_task_status.py` counts, over `scripts/warm-lane-*.sh`, (a) status-oracle invocations `"$<var>" "$<id>" 2>/dev/null | tr -d '[:space:]'` outside the lib, (b) `done|cancelled)` case arms outside the lib, and (c) definitions of `_matches_glob() {` / `_is_git_worktree() {`. All must be **0**.~~
- **Prereqs:** L2 (both edit preflight Check 6 and the audit's arg block).

**L4 — Audit reads dark-factory records with `python3 -I` json; HEADROOM from one field list** · medium · keys [fk-26e1e3d2eb67] · **#8647**

- **Modules:** new `scripts/warm_lane_records.py`; `scripts/warm-lane-audit.sh`; `test_warm_lane_audit.sh`; the runbook's UNKNOWN-cause table (`reader-unavailable`); infra-tests row for the `.py` → `test_warm_lane_audit.sh`.
- **Work:** D7 and D8. **[Amended 2026-10-20, retro-critic R7]** D8's single field list now has 19 entries: task 9181 added `stash_entries`, `stash_unknown` and the JSON `stash_stack`, and the parity case covers them. A new member registers through whichever mechanism is live at implementation (D12).
- **Signal:**
  - **[Amended 2026-10-20, retro-critic R7]** A plan with no `task_id` and a done anchor gives `plan_task` `-` and the correct anchor (the field-position contract, D7).
  - A `plan.json` whose done step has `"commit"` *before* `"status"` reports its real `plan_sync` (OK/STRANDED). Today it reports `-` and drops out of `plan_stranded`.
  - With `python3` removed from the audit's PATH, a lane with a plan reports `UNKNOWN` with cause `reader-unavailable` and exit 0.
  - `--format json` `headroom` keys equal the table `HEADROOM` keys in order. That is a parity case. The existing suite stays green byte for byte.
- **Prereqs:** L3 (same file, sequential edits).

**L5 — Golden dark-factory records drive the audit; compact and reordered plan cases** · medium · keys [fk-4b80fe8f7de1] · **#8709**

- **Modules:** **[Amended 2026-10-20, retro-critic R7]** new `tests/infra/fixtures/warm_lane_golden/{lane-state.json,lane-state-released.json,lane-states.json,plan.json,PROVENANCE.md}`, all five files D5 shipped; `test_warm_lane_audit.sh` (`make_plan`, `make_lane_state`, the `L_CASES` enum literals).
- **Work:**
  - Fixtures are derived from the golden records: field values substituted, not byte layout hand-copied. **[Amended 2026-10-20, retro-critic R7]** `L_CASES` (the `LaneState` literal list) is read from the golden `lane-states.json`, a separate D5 file. ~~The `LaneState` literal list is read from the golden `lane-state.json` companion list that D5 emits.~~
  - **[Amended 2026-10-20, retro-critic R7]** Add the released case: the unmodified `lane-state-released.json` (null optional fields) reports RELEASED with an empty task.
  - Add plan cases: compact (no indent), key-reordered entries, and `steps` emitted before `prerequisites`.
- **Signal:** **[Amended 2026-10-20, retro-critic R7]** `test_warm_lane_audit.sh` runs the audit over the unmodified golden records and asserts: `plan.json` gives `plan_sync=UNKNOWN` with cause `anchor-object-absent:b2…` and anchor `step-1`, which proves the reader skipped the pending step (`commit: null`) and took the last done entry; `lane-state.json` gives `ASSIGNED`/`4242`; `lane-state-released.json` gives `RELEASED` with an empty task. OK/STRANDED verdicts come only from golden-derived fixtures whose commit values are substituted with real fixture shas (the golden records synthetic shas `a1`×20 / `b2`×20 that no fixture repo contains, and `PROVENANCE.md` forbids hand-editing them). ~~`test_warm_lane_audit.sh` runs the audit over the unmodified golden records and asserts the stranded/OK verdicts.~~ The three new shape cases pass. A grep finds no comment in the suite stating a producer byte-layout fact (indent, key order, `prerequisites` precedes `steps`).
- **Prereqs:** L4 (the layout-independent reader is what makes the shape cases pass), D5 (golden content).
- **Companion D5 (dark_factory:6853):** a dark-factory test serialises a fixed `LaneRecord` per `LaneState` member, and a fixed plan through `plan_tools`' writer. It asserts byte equality with dark-factory's canonical copy at `orchestrator/tests/fixtures/warm_lane_golden/`, and emits the `LaneState` value list. Its docstring names reify's vendored path as the refresh target. Signal: changing `LaneRecord.to_json`'s indent turns that dark-factory test red. Ruled (Q4 a): the canonical copy lives in dark-factory; reify vendors a byte copy with `PROVENANCE.md`.

**L6 — `_do_reclaim` as per-entry functions behind one gate** · low · keys [fk-cee9ca6b041a] · **#8668**

- **Modules:** `scripts/warm-lane-gc.sh`, `test_warm_lane_gc.sh`.
- **Work:**
  - `_reset_lane <lane>` and `_remove_orphan <entry>` each print exactly one outcome token: `reset`, `reset_cold`, `removed`, `preserved`, `preserved_live_ref` or `skipped`.
  - **[Amended 2026-10-20, retro-critic R7]** `_with_entry_gate <entry> <precheck_fn> <act_fn>` runs lock → precheck → live-ref → act: `lane_lock_acquire <entry> 0`, then `<precheck_fn>`, then `live_ref_present`, then `<act_fn>`. Pass 1 passes precheck=`true`; Pass 2 passes precheck=`_is_reclaimable`, which keeps the measured cost order (~16 ms `git status` before the ~1.9 s `/proc` walk). ~~`_with_entry_gate <entry> <fn>` holds `lane_lock_acquire <entry> 0` and the `live_ref_present` check, in today's order per pass. Pass 2 keeps `_is_reclaimable` before the live-ref walk, inside `_remove_orphan`'s gate callback.~~ (self-contradictory: a callback runs after the gate.)
  - One tally `case` per loop.
  - The summary line `reclaim: reset=… removed=… preserved=… preserved_live_ref=… reset_cold=…` stays byte-identical.
- **Signal:** a fixture pool that exercises every outcome path prints the expected summary line, and the existing `test_warm_lane_gc.sh` blocks stay green unchanged. The counter is incremented at exactly one site per outcome, checked by a count case in `test_warm_lane_gc.sh`. **[Amended 2026-10-20, retro-critic R7]** A dirty+live orphan fixture asserts `preserved=1`, `preserved_live_ref=0`.
- **Prereqs:** L1 (uses `lane_lock_acquire`), L2 (same file).

**L7 — Plain names in gc and sweep `--help`** · low · keys [fk-7dba1d7f3f47] · **#8541 (adopted, not re-filed)**

- No new task is filed. Decompose adopts #8541: it adds `consumer_ref` and the PRD path to its metadata, and wires it after L2 and L6, which rewrite the same `_usage` blocks.
- **Signal:** `warm-lane-gc.sh --help` and `warm-lane-gc-sweep.sh --help` output contain no character in `[α-ω]` and name `seed-warm-lane.sh` / `warm-lane-disk-guard.sh` / `warm-lane-gc.sh reclaim` by file.

**L8 — `seed-warm-lane.sh --capabilities`** · low · keys [fk-d30433df0ed6] · **#8670**

- **Modules:** `scripts/seed-warm-lane.sh` (parser, `_usage`), `test_seed_warm_lane.sh` (`Q11_FLAG` note).
- **Work:** D10. Delete the 3 literal notes.
- **Signal:** `seed-warm-lane.sh --capabilities` prints exactly the two flag lines and exits 0. Boundary case B9 is green. **[Amended 2026-10-20, retro-critic R7]** B9 checks the `CAPABILITIES` array against the parser's capability arms, with a named base-flag exclusion (D10).
- **Prereqs:** L1 (same parser region).
- **Companion D8 (dark_factory:6855):**
  - `_seed_script_supports` uses `--capabilities` output with exact-line membership, with the legacy substring fallback only on a non-zero exit. **[Amended 2026-10-20, retro-critic R7]** Membership covers the capability-gated flags only (D10).
  - A dark-factory test proves the old failure mode is closed: a stub script whose parser lacks the flag but whose comments mention it now answers False.
  - Signal: that test is green.
  - Retirement condition for the fallback: no lane's checked-out vintage lacks `--capabilities`. It is recorded in the code's docstring with the reify commit that introduced L8.
  - Depends on L8 landing on reify main.

**L9 — Main-gate sentinel bound to the main tip (Phase A)** · medium · keys [fk-638f0a64b948] · **#8671**

- **Modules:** `hooks/main-gate-lib.sh`, `hooks/reference-transaction`, new `scripts/main-gate-mark.sh`, `scripts/land.sh`, `dark-factory-orchestrator.yaml` (`git.main_gate_mark_command`, `git.main_gate_unmark_command`), `test_reference_transaction_gate.sh`, `test_land_script.sh`, infra-tests rows.
- **Work:** D9 Phase A. Read `test_hooks_armed_guard.sh`'s behavioural probe before editing, in case it marks the sentinel.
  - Ruled (Q5 a + c-measure): first measure whether `hooks/pre-merge-commit`'s `main_gate_mark` runs after the gate passes and immediately before land.sh's `git merge --no-ff` ref move, on both the clean-merge path and any path land.sh takes. If it does, **delete land.sh's own pre-mark and its failure-branch `rm`** instead of adding the EXIT trap, so land.sh stops being a mark site. If the measurement shows a path where pre-merge-commit does not mark, keep the pre-mark and add the compare-and-delete EXIT trap. Record the measurement in the commit message.
- **Signal:** B5–B8 are green.
  - After landing, a docs commit on main logs `sanctioned main move X -> Y`.
  - A raw `git update-ref` that reuses a token minted before main last moved logs `UNSANCTIONED` and aborts, because ENFORCE is on.
  - After the next orchestrator restart, merge-queue landings log `sanctioned`, not `sanctioned (legacy unbound sentinel)`.
- **Deploy note for the decompose seat:** the yaml edit lands inert until a restart through `scripts/orchestrator-redeploy-restart.sh`. Commit first; a dirty restart crash-loops (reify CLAUDE.md "Deploying the orchestrator").
- **Prereqs:** none.

**L10 — Delete the legacy unbound-sentinel arm (Phase B)** · low · keys [] (completes fk-638f0a64b948) · **#8713**

- **Work:** the empty-sentinel case becomes "no sentinel"; the B-matrix legacy case flips to UNSANCTIONED.
- **Gate to start:** `reify-main-gate.log` shows ≥ 20 consecutive `sanctioned main move` lines *after* the orchestrator restart that loaded L9's yaml, and **zero** `legacy unbound sentinel` lines since that restart. The implementer measures this as the leaf's first step and stops with a blocker if it does not hold. **[Amended 2026-10-20, retro-critic R7]** The gate also takes the per-worktree hooks vintage: `core.hooksPath=hooks` is relative, so each worktree runs its own hooks, and a pool lane whose `hooks/main-gate-lib.sh` predates L9 (`grep -L main_gate_token <pool>/*/hooks/main-gate-lib.sh`) still writes the empty sentinel. The census is part of the measurement.
- **Signal:** the legacy case in `test_reference_transaction_gate.sh` asserts UNSANCTIONED, and live merge-queue landings keep logging `sanctioned`.
- **Prereqs:** L9.

**L11 — PRD close** · low · keys [] · **#8715**

- **Work:** the decompose-close leaf from the overlay. Depends on L1–L10 (and #8541). Stamps `SHIPPED` with the landed ids, the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, on this PRD and its capability manifest. **[Amended 2026-10-20, retro-critic R7]** The map records L9's measured Q5(c) outcome: `hooks/pre-merge-commit` marks immediately before every land.sh move, so land.sh is not a mark site and has no EXIT trap (D9's land.sh trap text and B7 are AS-AUTHORED only).
- **Signal:** the committed header.

**Companion D1 (dark_factory:6854) — handshake face of the boundary test**

- **Work:** B4. Dark-factory runs a stub seed under `_seed_warm_lane` in `TAKE` and `HELD_BY_CALLER` modes. The stub attempts `flock -n -x` on a fresh fd and exits 70 if it gets the lock.
- **Signal:** the test is green, and red if `lane_lock_flock` is emptied for `TAKE`.
- **Prereqs:** L1 (for the shared exit-70 contract).

DAG:

```
L1 → L2 → L3 → L4 → L5 ← D5
L1 → L6
L2 → L6
L2 + L6 → #8541
L1 → L8 → D8
L1 → D1
L9 → L10
all → L11
```

## 9. G7 walk (advisory, author mode)

| Invariant | Disposition |
|---|---|
| INV-SF-4 `indeterminate-attributable-transient` | Satisfied. D7 gives every reader failure an attributed UNKNOWN cause (`reader-unavailable`, `unparseable-record`), never `-`. |
| INV-SF-5 `placeholders-owned-and-loud` | Satisfied. D3 and D4 turn silent passes (unverified flag, ignored retired env, IDLE on the wrong dir) into exits 70 and 2 or a warning. |
| INV-SF-2 `error-severity-exits-nonzero` | Satisfied by D3 and D4. |
| `nothing-vacuous-and-unowned` | The L10 gate is measured, not assumed, and the D8 fallback carries a named retirement condition. |

No waivers.

## 10. Open questions — ruled by the lead 2026-10-12 (program doc §"Resolved decisions — pass 1 open questions" → S20)

1. `lane_lock_acquire(mode, wait)` → (a): mode is the wait mode (`0` / `N` / `unlimited`). The lock is always exclusive; the shared mode stays `lane_lock_probe`.
2. CLI `--mount` → (a): the flags keep their worktrees-dir meaning, plus the looks-like-mount warning. No dark-factory flag-day.
3. ~~Manifest row → (a): L3 adds its own row in today's format. S19 owns the format, not the rows.~~ (superseded by R7 M8: a new member registers through whichever mechanism is live at implementation, `# run-all-bucket:` pragma or a manifest row, `# guards:` header or a txt row; D12 and the §6 G4 row point at S19 υ #9303 / S18 τ #9312.)
4. Golden-record home → (a): the canonical copy is in dark-factory, regenerated by D5. reify vendors a byte copy with `PROVENANCE.md`.
5. f133 premise → (a): two-phase L9/L10. L9 also measures (c) and deletes land.sh's pre-mark if `pre-merge-commit` already marks immediately before the `--no-ff` move.
6. ~~thin on seed exit 70 → propagate 70 unchanged.~~ (superseded by R7 M6, restated: seed rc 70 is the one exception to thin's best-effort exit 0 for `--reseed`; thin exits 70 after the no-op discard with empty stdout; header and `--help` exit table change; the test uses a stub seed. See L1.)

## Retro-critic sitting amendments (2026-10-20)

- **B1 + M4 + M5** (#8709 L5, #8647 L4): L5's signal is rewritten (unmodified goldens give UNKNOWN `anchor-object-absent:b2…` with anchor step-1, ASSIGNED/4242, RELEASED with an empty task; OK/STRANDED only from golden-derived fixtures with real shas); L5 vendors all five D5 files and reads `L_CASES` from `lane-states.json`; its path check gains `lane-state-released.json` and `lane-states.json`. D7: `null` permitted for `task_id` and a non-done entry's `commit`; the reader emits one field per line or `\x1f`-separated, contract by field position; a no-task_id + done-anchor case. Pointer: `plans/review-all-program-reify-20261012.md` §Resolved decisions — retro-critic sitting R7.
- **M1** (#8624 L1): `lane_lock_release` keys on a lib-private acquired flag; seed's EXIT trap only on the acquire path; Q7b and Block-V FD checks retargeted to `LANE_LOCK_FD=8` with a mutation note (lands before S19 μ₂ #8719). Pointer: `plans/review-all-program-reify-20261012.md` §Resolved decisions — retro-critic sitting R7.
- **M2** (#8668 L6): `_with_entry_gate <entry> <precheck_fn> <act_fn>` (lock → precheck → live-ref → act); dirty+live orphan fixture (preserved=1, preserved_live_ref=0); helper check widened to `^_with_entry_gate\(\) [{(]`. Pointer: `plans/review-all-program-reify-20261012.md` §Resolved decisions — retro-critic sitting R7.
- **M3** (#8670 L8, dark_factory:6855 D8): `--capabilities` prints the capability-gated flags from one `CAPABILITIES` array; B9 checks the array against the parser's capability arms with a named base-flag exclusion; "exactly two lines" stands; D8 membership covers capability flags only. Pointer: `plans/review-all-program-reify-20261012.md` §Resolved decisions — retro-critic sitting R7.
- **M6** (#8624 L1): supersedes pass-1 Q6 as worded; seed rc 70 is the one exception to thin's best-effort exit 0 (exit 70 after the no-op discard, empty stdout; header and `--help` table; stub-seed test). Pointer: `plans/review-all-program-reify-20261012.md` §Resolved decisions — retro-critic sitting R7.
- **M7** (#8624 L1, #8641 L3): ratchets are rows in `tests/infra/adoption-ratchet.manifest` at ceiling 0 (`lane-lock-path-construction`, `lane-lock-exec-open`; `status-oracle-invocation`, `done-cancelled-arm`, `entry-helper-def`), EREs excluding `.gen`/`.seed-stamp.json` siblings and comment lines; no bespoke counters. Pointer: `plans/review-all-program-reify-20261012.md` §Resolved decisions — retro-critic sitting R7.
- **M8** (#8624, #8641, #8647): supersedes pass-1 Q3; new members register through whichever mechanism is live (`# run-all-bucket:` pragma or manifest row; `# guards:` header or txt row); D12 and the §6 G4 row point at S19 υ #9303 / S18 τ #9312. Cross-PRD: #9303 and #9312 carry the matching S20 notes (not edited here). Pointer: `plans/review-all-program-reify-20261012.md` §Resolved decisions — retro-critic sitting R7.
- **Polarity** (twins): L2 #8635's five-path present check split into one check per script anchored on `\$\{REIFY_WARM_LANE_WORKTREES`; L1's `probe-substrate-exists` relabelled as the deliverable; L6's helper check widened. Pointer: `plans/review-all-program-reify-20261012.md` §Resolved decisions — retro-critic sitting R7.
- **Minors**: L10 #8713's start gate adds the per-worktree hooks vintage; L2 #8635's destructive gc ambient default stated; L11 #8715's close stamp records the measured Q5(c) outcome (land.sh is not a mark site); L3 #8641's `lane-task-status.sh` overlap with #8780 noted. Drift: 19 HEADROOM fields and the new `_is_git_worktree` call site (task 9181); #9320 moves `_close_inherited_fds` (cite by symbol). Pointer: `plans/review-all-program-reify-20261012.md` §Resolved decisions — retro-critic sitting R7.
- **Released**: L1 #8624 and L10 #8713 (held under R0) are released by R7. Pointer: `plans/review-all-program-reify-20261012.md` §Resolved decisions — retro-critic sitting R7.
