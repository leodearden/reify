# Capability manifest — `warm-lane-scripts-one-lock-one-env-one-oracle` (program S20)

PRD: `docs/prds/v0_6/warm-lane-scripts-one-lock-one-env-one-oracle.md`. Machine-readable twin: `warm-lane-scripts-one-lock-one-env-one-oracle.capability-manifest.yaml`. It validates under dark-factory `shared.capability_manifest.load_capability_manifest` (11 tasks) at authoring.

**Code anchors** verified against main `8e6ffcac9d` (2026-10-12). Every binding below cites a symbol or a pattern, never a line. Each `absent` check's pattern was confirmed **present** at `8e6ffcac9d`, so a later `absent` result is a real change and cannot be vacuous.

Status: authored (pass 1). Task ids are stamped by `commit_planning` at decompose. The dark-factory companions D1, D5 and D8 are bound by `external_task_id` once filed.

## L1 — lane-lock API; `--assume-lane-lock-held` asserts

| Capability | Evidence | Verdict |
|---|---|---|
| The probe substrate exists | `scripts/lib_lane_lock.sh::lane_lock_probe`. It is shared, non-blocking and read-only. An absent file reports IDLE, and BUSY is reported only on rc 124. | PASS |
| Dark-factory holders use flock(2) on the sibling path | `git_ops.py::_seed_warm_lane` uses `flock -x -w 30 -E 124 <lane_dir>.lock` (TAKE). `HELD_BY_CALLER` holds `fcntl.flock(LOCK_EX)`. `verify_cancel.py::lane_lock_path` gives `<name>.lock`. | PASS |
| rc 70 lands in FAULT | `_seed_rc_to_unavailable` handles 75, 76, 77, 124 and 127 explicitly; every other code is FAULT. Seed uses no exit 70 at main. | PASS |
| Delivered: `lane_lock_assert_held` defined; seed and thin no longer compose `${LANE_DIR}.lock` | grep `^lane_lock_assert_held\(\) \{` is present in the lib. grep `LANE_LOCK="\$\{LANE_DIR\}\.lock"` is absent from seed and thin; it is present at main in both. | PASS |

## L2 — `REIFY_WARM_LANE_WORKTREES`

| Capability | Evidence | Verdict |
|---|---|---|
| Two meanings measured | `git grep REIFY_WARM_LANE_MOUNT` returns two kinds of reader. Mount-meaning readers: provision, preflight, seed, thin. Worktrees-meaning readers: audit `_probe_live "$MOUNT/$name.lock"` and lock-guard `$MOUNT/$LANE.lock`. | PASS |
| The four retired names have no production setter | `reify-warm-lane-gc.service` passes `--mount` explicitly. The orchestrator drop-in sets only `REIFY_LANE_LEAK_STATUS_CMD`. dark-factory `orchestrator/src` and `shared` have 0 hits. | PASS |
| Delivered: the five readers name the new env var | grep `REIFY_WARM_LANE_WORKTREES` is present in lock-guard, audit, gc, sweep and preflight. | PASS |

## L3 — `lib_task_status.sh` / `lib_lane_entry.sh`

| Capability | Evidence | Verdict |
|---|---|---|
| Three oracle copies measured | The copies are in `warm-lane-audit.sh::_task_status_raw`, preflight Check 6 and `warm-lane-degenerate-ref-check.sh::_ref_status`. The `done\|cancelled` arms occur in the same three files. `_matches_glob` and `_is_git_worktree` are each defined in both gc and audit. | PASS |
| Delivered: the lib defines the bucket; the audit copy is gone | grep `^task_status_bucket\(\) \{` is present in `scripts/lib_task_status.sh`. grep `^_task_status_raw\(\) \{` is absent from the audit; it is present at main. | PASS |
| Ratchet member | `tests/infra/test_lib_task_status.py` counts inline oracle invocations, `done\|cancelled)` arms and helper definitions outside the libs, and expects 0. Behavioural cover: the three-consumer `superseded` agreement case. | PASS (named; delivered by L3) |

## L4 — `python3 -I` json reader; one HEADROOM field list

| Capability | Evidence | Verdict |
|---|---|---|
| The gate's interpreter is present | `/usr/bin/python3` is 3.12.3. No test pins the audit as python-free; `python3` appears only in two comments in `warm-lane-*.sh`. | PASS |
| Delivered: reader file; awk tokenizer deleted | path `scripts/warm_lane_records.py` is present. grep `^_plan_anchor\(\) \{` is absent from the audit; it is present at main. | PASS |

## L5 — golden records

| Capability | Evidence | Verdict |
|---|---|---|
| The producer serialisers are located | dark-factory `lane_lifecycle.py::LaneRecord.to_json` is `json.dumps(…, indent=2)`. `LaneState` has six values. `plan.json` is written by `orchestrator/mcp/plan_tools.py`. | PASS |
| Delivered: golden records checked in | path `tests/infra/fixtures/warm_lane_golden/{plan.json,lane-state.json}` is present. The content comes from companion D5 (dark_factory). | PASS |

## L6 — `_do_reclaim` factoring

| Capability | Evidence | Verdict |
|---|---|---|
| Shape measured | `preserved_count` is incremented at 10 sites. There are two `flock -n 8` gates and two `live_ref_present` gates. The file is unchanged since `a030ee7528`. | PASS |
| Delivered: one gate helper | grep `^_with_entry_gate\(\) \{` is present in gc. | PASS |

## L7 — adopt #8541

| Capability | Evidence | Verdict |
|---|---|---|
| The filed task covers the finding | #8541 carries `x_finding_key=[fk-7dba1d7f3f47]` and names the files gc and sweep. | PASS (manual check: `--help` output contains no `[α-ω]`) |

## L8 — `--capabilities`

| Capability | Evidence | Verdict |
|---|---|---|
| The dark-factory probe is a substring test | `_seed_script_supports` does `flag in _seed_script_text(...)`. | PASS |
| Delivered: parser arm | grep `^ *--capabilities\)` is present in seed. | PASS |

## L9 — tip-bound sentinel (Phase A)

| Capability | Evidence | Verdict |
|---|---|---|
| Five mark sites | Three call `main_gate_mark`: pre-commit, pre-merge-commit and land.sh. Two are dark-factory's `advance_main` and `recover_red_main`, both via `dark-factory-orchestrator.yaml` `git.main_gate_mark_command`, which is `touch` of the empty file. | PASS (this corrects the finding's count of 3) |
| ENFORCE is on | `git config --bool --get reify.mainGate.enforce` returns `true`. | PASS |
| Delivered: the dark-factory mark command is repointed | grep `main_gate_mark_command: .*main-gate-mark\.sh mark` is present in `dark-factory-orchestrator.yaml`. | PASS |

## L10 — legacy arm removal (Phase B)

| Capability | Evidence | Verdict |
|---|---|---|
| Observation gate | The leaf measures `reify-main-gate.log` after the restart that loaded L9's yaml. It requires at least 20 consecutive sanctioned moves and 0 legacy-unbound lines. | OPEN (the leaf decides this) |

## L11 — PRD close

| Capability | Evidence | Verdict |
|---|---|---|
| Terminal stamp | grep `Status:\*\* SHIPPED` is present in the PRD. | PASS |
