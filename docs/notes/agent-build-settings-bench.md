# Agent cargo builds under the verify's settings: bench and recommendation

Task 10169. Measured 2026-11-19 on `leo-MS-7C35` (AMD Ryzen 9 3950X, 16 cores / 32 threads, 125 GiB RAM,
kernel 7.0.0-31-generic; rustc and cargo 1.96.0, sccache 0.14.0).

| | |
|---|---|
| Warm base (`BASE_GEN`) | `/home/leo/src/warm-lanes/base/target.gen.643`, basecommit `cb24012c60a6b800f763acc65943ca0d21fc163e` |
| Bench head (`BENCH_HEAD`) | `f743808f664fbf782a2867019102ae1c97c86705`, main's tip at bench start, 341 commits ahead of the basecommit |
| Raw per-step data | `docs/notes/agent-build-settings-bench-raw.jsonl` |

Status: this revision carries the census, the bench method and the reify-compiler and reify-eval results. The
remaining crate, the pricing, the recommendation and the caveats are added by the later steps of the task.

The question, from Leo's ruling of 2026-11-17: would agents' own cargo builds reuse the warm base of a
freshly seeded lane if they ran under the verify's settings (`CARGO_INCREMENTAL=0` plus
`RUSTC_WRAPPER=sccache`), and what does that cost them on the edit loop? Measure, then recommend. The setting
is not flipped here, and `dark-factory-orchestrator.yaml` is not edited.

## Observed agent build mix

### Method: the census rule

- **Source.** `data/orchestrator/agent-transcripts/<task>/<lane-slug>/**/*.jsonl` in the main checkout,
  globbed recursively so subagent transcripts are included. Files are read for every task that has at least
  one file touched since the window start, so a task that began before the window is classified from its
  whole history rather than mistaken for a fresh one.
- **Window.** The 14 full UTC days 2026-11-05 to 2026-11-18, chosen by each record's own `timestamp`, never
  by file mtime. Four of the fourteen days (2026-11-10 to 2026-11-13) were idle: main took no merge commits
  and the archive holds no transcript file on them. Rates below are per calendar day, with the
  per-active-day figure (10 active days) beside them.
- **Dedupe.** Each tool_use appears twice in the archive, once timestamped and once with timestamp `None`.
  Tool_use ids are deduplicated across the whole archive and the timestamped copy is kept. A call's wall
  is its tool_result timestamp minus its tool_use timestamp, taken only for calls that did not background.
- **Heavy cargo invocation.** A command-position `cargo test`, `build`, `check`, `clippy`, `bench` or
  `nextest run`, found by tokenising each Bash command with `shlex` after dropping heredoc bodies and joining
  backslash continuations. Leading env assignments and wrappers (`nice`, `timeout`, `env`, ...) are skipped,
  `bash -c '...'` strings are scanned recursively, and mentions inside `echo`, `grep` or commit messages are
  not counted. `cargo run`, `doc`, `tree`, `metadata` and the like (184 calls) are not heavy and are left
  out. Nine `scripts/verify.sh` runs are counted separately as `via-script` and are not in the tables.
- **Per invocation:** subcommand; crate (the first `-p X`, else `--workspace`); selection (`--test T`,
  `--lib`, `--bins`, `--all-targets`, `--tests`, `--doc`, else bare); a `--no-run` flag; a `--release` flag;
  setting; class; role.
- **Setting.** By the command text: `verify-env` when it contains both `CARGO_INCREMENTAL=0` and
  `RUSTC_WRAPPER=sccache`, `incr0-only` when only the first, otherwise `default` (incremental on, no
  wrapper). Shell state does not persist between Bash calls in the agent harness, so the command text is the
  whole environment.
- **Class.** `first` is the first invocation in the task with the same (setting, mode, crate, release flag),
  where mode is `build` for test, build, nextest and bench and `check` for check and clippy. Otherwise
  `edit-rebuild` if an Edit, Write or MultiEdit on a `.rs` path, or a Bash `sed -i` on one, occurred since
  the task's previous cargo invocation, otherwise `rerun`. The release flag in the key is a refinement of
  the plan's key: a release build is a different unit set (8 calls).
- **Role.** From the session's first prompt: `submit_review_verdict` means reviewer, `confirm_plan` or
  `create_plan` means architect, "Execute the next pending steps in TDD order" (or an amendment or
  escalation-resume prompt) means implementer, anything else is `other`. Subagent transcripts are in the
  glob, but none of the 2,114 heavy invocations was attributed to one.

The miner is a throwaway stdlib script (`python3 -I`), deliberately not tracked: this note states the rule,
and the archive is the source.

### Volume and shape

2,114 heavy invocations in 140 tasks: 151 per calendar day, 211 per active day.

| day | 11-05 | 11-06 | 11-07 | 11-08 | 11-09 | 11-10..13 | 11-14 | 11-15 | 11-16 | 11-17 | 11-18 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| invocations | 126 | 84 | 257 | 131 | 14 | 0 | 115 | 236 | 292 | 495 | 364 |

By subcommand: test 1,636 (77.4%), clippy 253 (12.0%), nextest 128 (6.1%), build 55 (2.6%), check 42 (2.0%).
Of the 2,114, 61 (2.9%) carry `--no-run` and 8 are release builds. By role: implementer 1,609 (76.1%),
reviewer 364 (17.2%), other 84 (4.0%), architect 57 (2.7%).

By subcommand and selection (rows under 25 calls pooled), split by setting and class. "incr0" is `verify-env`
plus `incr0-only`:

| subcommand | selection | n | default | incr0 | first | edit-rebuild | rerun |
|---|---|---:|---:|---:|---:|---:|---:|
| test | `--test T` | 885 | 535 | 350 | 135 | 162 | 588 |
| test | `--lib` | 556 | 337 | 219 | 86 | 157 | 313 |
| clippy | `--all-targets` | 199 | 123 | 76 | 79 | 12 | 108 |
| test | bare | 123 | 81 | 42 | 27 | 11 | 85 |
| nextest | `--test T` | 87 | 61 | 26 | 17 | 10 | 60 |
| test | `--bins` | 65 | 38 | 27 | 5 | 27 | 33 |
| nextest | bare | 34 | 24 | 10 | 6 | 3 | 25 |
| build | bare | 30 | 14 | 16 | 12 | 3 | 15 |
| clippy | `--tests` | 27 | 18 | 9 | 15 | 1 | 11 |
| (other) | | 108 | 75 | 33 | 37 | 7 | 64 |
| **all** | | **2,114** | **1,306** | **808** | **419** | **393** | **1,302** |

By crate (top 8 of 29 keys; the keys are `-p` crate names, plus `(none)` for a call naming neither a package
nor `--workspace`):

| crate | n | share | default | incr0 | first | edit-rebuild | rerun |
|---|---:|---:|---:|---:|---:|---:|---:|
| reify-compiler | 653 | 30.9% | 399 | 254 | 103 | 126 | 424 |
| reify-eval | 507 | 24.0% | 326 | 181 | 86 | 95 | 326 |
| reify-cli | 293 | 13.9% | 174 | 119 | 54 | 57 | 182 |
| reify-gui | 146 | 6.9% | 89 | 57 | 33 | 27 | 86 |
| reify-build-utils | 85 | 4.0% | 45 | 40 | 17 | 12 | 56 |
| reify-ir | 62 | 2.9% | 54 | 8 | 8 | 11 | 43 |
| reify-lsp | 53 | 2.5% | 22 | 31 | 10 | 10 | 33 |
| reify-syntax | 53 | 2.5% | 36 | 17 | 13 | 14 | 26 |
| (21 other keys) | 262 | 12.4% | 161 | 101 | 95 | 41 | 126 |

Per task (140 tasks with at least one heavy call in the window): median 13 invocations, p90 29, mean 15.1,
max 83.

### Settings: a mix, drifting toward the default

- Whole window: default 1,306 (61.8%), verify-env 797 (37.7%), incr0-only 11 (0.5%).
- By role, incr0 share: implementer 40.3%, architect 38.6%, reviewer 31.9%, other 26.2%.
- Trend, incr0 share by week: 2026-11-05 to 11-11 was 280 of 612 (45.8%); 2026-11-12 to 11-18 was 528 of
  1,502 (35.2%). By day: 61, 63, 32, 50, 21, then 30, 51, 57, 25, 22 percent. The prefix is not spreading.
- **Lanes mix settings.** 54 of 148 (task, lane) pairs (36.5%) ran both an incr0 and a default call, and 53
  of 140 tasks (37.9%) had such a lane. Counting `first` builds beyond one per (mode, crate, release) in
  those lanes: 73 extra first builds in total, median 1, worst lane 6. A lane that mixes builds both unit
  sets, which is the warning in memory bfd48b27. In 39 of the 54 mixed lanes at least one role itself ran
  both settings; in the other 15 each role was consistent and the mix is between roles, most often (7 of 15)
  an implementer on the verify env and a reviewer on the default, and in 3 of 15 the reverse.
- **"Today's agent settings" is therefore a mix, not the plain default.** The observed scenario in the
  pricing prices each invocation under its own recorded setting.

### The `_lane-15` counterexample, explained

The lane that looked like a counterexample, an agent lane whose artifacts matched the verify env although
the agent env sets neither variable, is the task 10058 implementer's own doing. Transcript
`10058/-home-leo-src-warm-lanes-worktrees--lane-15/83a9214f-7560-4e38-b3cd-09105e8a4968.jsonl`, at
2026-11-17T12:16:20Z, runs `RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo test -p reify-compiler --lib
name_env::tests` (output redirected to a log), and its later clippy carries the same prefix. The census
reproduces the call as a `verify-env` `first` build (196.9 s foreground). The agent typed the verify env
itself, consistent with the memories: ab2b34af says to always prefix manual cargo in a warm lane with both
variables, and the 2026-08-21 amendment to bfd48b27 says the same for a pool-seeded lane. A reviewer in that same lane later ran the default env (a `first` build
of 142 s), the mixed-lane pattern above. So the lane says nothing against the premise that the agent env
differs from the verify env; it says that some agents already paste the verify env by hand.

### Observed foreground wall, context only

Median foreground wall of single-invocation commands, in seconds (count in brackets). These include test
execution, the crate and selection mix differs between columns, and the host was loaded, so they are a hint
for the bench, not an A/B:

| subcommand, class | default | incr0 |
|---|---:|---:|
| test/build, first | 210.6 [137] | 168.6 [76] |
| test/build, edit-rebuild | 49.3 [185] | 77.9 [154] |
| test/build, rerun | 46.8 [488] | 61.5 [231] |
| clippy, first | 210.8 [54] | 123.1 [35] |
| clippy, rerun | 54.2 [62] | 109.6 [32] |

The pattern the bench tests: the verify env looks cheaper on a first build, dearer on an edit-rebuild.

### Limits of the class rule

`rerun` is not free: 222 of the 1,302 reruns are the first time that selection (a new `--test T`, say) was
built in the task, which compiles and links a new test binary even though the crate is warm. The pricing
treats a rerun with no source change as costing no build CPU and reports this share as a sensitivity.
Reruns also include test execution, which this bench excludes (`--no-run`).

### Output for the bench steps

| rank | crate | calls (share) | most-selected `--test T` | most-edited lib source file (edits in window) |
|---|---|---|---|---|
| 1 | reify-compiler | 653 (30.9%) | `harness_modules_ports` (107 of 338 `--test` calls) | `compile_builder/functions_phase.rs` (53) |
| 2 | reify-eval | 507 (24.0%) | `harness_engine` (135 of 231) | `relate_solve.rs` (68) |
| 3 | reify-cli | 293 (13.9%) | `harness_cli_surface` (103 of 159) | `help.rs` (19) |

Together the three crates are 68.7% of heavy invocations. The expected crates were right. `reify-cli` is
bin-only (no lib target), so its S1 selection is `--bins` (76 census calls) rather than `--lib`. The fixed
function that takes the edit in each file is a private, non-generic, single-line-signature function:
`describe_slot` in the compiler's `functions_phase.rs`, `realize_structures` in the evaluator's
`relate_solve.rs`, `write_flags` in the CLI's `help.rs`.

## Bench method

### Arms

All arms inherit the implementer's own ambient agent env, which is itself a `role_env_overrides`-merged agent
env: `CARGO_HOME=/tmp/reify-agent-cargo-home`, neither `CARGO_INCREMENTAL` nor `RUSTC_WRAPPER` set,
`LD_LIBRARY_PATH` unset. Changes common to every arm: `CARGO_MAKEFLAGS`, `MAKEFLAGS` and `MFLAGS` unset and
`-j 8` on every cargo command (a niced bench must not hold shared task-jobserver tokens, and parallelism is
equal across arms), `nice -n 19 ionice -c3` on every process, and
`CARGO_LOG=cargo::core::compiler::fingerprint=info` with stderr kept per step, from which the dirty and
"failed to read" unit counts are taken.

| arm | env delta | meaning |
|---|---|---|
| (a) | none | today's agent default: incremental on, no wrapper |
| (b) | `CARGO_INCREMENTAL=0`, `RUSTC_WRAPPER=sccache` | the verify's pair, as `role_env_overrides` would merge it |
| (b0) | `CARGO_INCREMENTAL=0` | diagnostic: the verify's incremental setting without the wrapper |

Arm (b) points sccache at a PRIVATE server: its own port, its own `SCCACHE_DIR` under the bench directory,
`SCCACHE_CACHE_SIZE=20G`, started in the foreground with `SCCACHE_START_SERVER=1 SCCACHE_NO_DAEMON=1`. A fresh
server is started inside each measured step and stopped, waited for, before the step ends, so the rustc
processes it spawns on a cache miss are inside the measured tree; the cache directory persists across one
sequence's steps and is emptied between sequences. The shared server is never touched.

### Lane and seed

The lane is a standalone `git clone --shared --no-checkout` of the main checkout at
`<worktree>/.task/bench-10169/lane` (gitignored), detached at `BENCH_HEAD`. It is a clone and not a
`git worktree add`, so the seed's rerere and hooks arming lands in the clone's own `.git` and not in the
shared store. It sits inside the task worktree because the implementer is sandboxed (write set: the worktree
and `/tmp`) and `/tmp` is ext4, while the base and the worktree are on the same XFS loop, where a reflink
seed works.

Before every sequence (step S0, untimed): DF's acquire steps (`git checkout -f --detach <head>`,
`git clean -xfd -e target -e 'target.reseed-trash.*'`), then the real
`scripts/seed-warm-lane.sh <BASE_GEN> <lane> --fresh-checkout` under `flock -s <BASE_GEN>.lock`. That is the
state a freshly acquired production lane starts from: a reflinked `target/`, the 2020 mtime stamp, the delta
touch, the `env!()` relink and the tauri build-dir deletion. The bench parks the previous `target/` by
renaming it into `.task/bench-10169/junk/` instead of letting the seed delete ~170 GB at idle IO priority;
the seed then finds no `target/`, which is the state it creates anyway.

The base was pinned once, `target.gen.643`, and stayed current throughout (each S0 record carries
`current_base_gen`). Its `.basecommit` is `cb24012c60`, written 2026-11-17 13:04; the previous generation's
dates from 2026-11-09. No newer generation existed at the bench start on 2026-11-19, 36 hours and 341
commits later, while main took 64 merges on 11-18 alone. That staleness is the bench's default condition:
every sequence runs at `BENCH_HEAD`, so the stale-delta rebuild a production lane pays is in every number
below, unless the row says zero delta.

### One sequence

S0 reseed. Then, with `X` the crate, `T` its census `--test` target and `F` its most-edited lib file:

| step | command |
|---|---|
| S1 | `cargo test -p X --lib --no-run` (`--bins` for the bin-only reify-cli) |
| S2 | `cargo test -p X --test T --no-run` |
| E1, E2, E3 | edit k, then S2's command, then S1's command, recorded as `Ek-S2` and `Ek-S1` |
| S6 | `cargo clippy -p X --all-targets -- -D warnings` |
| S7 | verify pre-pay probe, fingerprint-only, no compile, always last |

Edit k inserts `let _ = std::hint::black_box(<k>u32);` as the first statement of one fixed private function
in `F`; edit k replaces edit k-1, so every edit is new content. Same function, same file, in every arm.

S7 is the study's `failrustc.sh` pattern: `CARGO_INCREMENTAL=0`, `RUSTC_WRAPPER` set to a script that passes
rustc's `-vV` and `--print` probes through and refuses every compile, `CARGO_HOME=/tmp/reify-agent-cargo-home`,
and `cargo test -p <X plus its workspace reverse-dependency closure> --no-run`. It reports how many of the units
the verify would build are dirty or unreadable after the agent's work, without building any. The difference
between arms is the number of verify units the arm-(b) agent pre-paid. The probe deletes the hash files of
the units it finds dirty, which is why it ends the sequence.

### What is measured per step

- `cpu_s`: user plus sys of the step's whole waited process tree (`getrusage(RUSAGE_CHILDREN)` delta). Under
  (b) the private server is a child of the step, so the compiles it runs are included. The primary metric.
- `cgroup_cpu_s`: the same step's `usage_usec` from the dedicated `systemd-run --user --scope` unit each
  driver runs in, a cross-check: the largest relative difference from `cpu_s` is 0.14% over the 41 crate-1
  bench steps.
- `sccache_server_cpu_s`: the server's own CPU, from `/proc/<pid>/stat` after the step.
- `wall_s`: monotonic. Secondary, because the host is shared (below).
- `units_written`: fingerprint directories whose newest file mtime is after the step start, the rule of
  `scripts/verify-ledger.py::scan_fingerprints`. `units_new_vs_base`: of those, the names absent from
  `BASE_GEN/debug/.fingerprint`.
- `dirty`, `failed_to_read`: distinct units the fingerprint log reports as "fingerprint dirty for" and
  "err: failed to read". A unit that cargo cannot read is one the base never had under that hash: new.
  `dirty_root` counts dirty units whose reason is not a propagated dependency change.
- PSI `some avg10` at start and end and `loadavg` at start. The host ran other agents and verifies throughout.
  Over the 41 crate-1 bench steps PSI `some avg10` was 14 to 66 (median 36) and the 1-minute load average
  52 to 146 (median 86). CPU time is robust to that load, wall time is not, and the arms are interleaved so
  both see the same weather.

### Calibration

The private server must not hide its compile CPU. One cacheable lib miss (`cargo build -p
reify-shell-extract --lib`, 4 units) was run under (b) and (b0), with the gate that (b)'s `cpu_s` is within
25% of (b0)'s. Round 1 measured `getrusage` only and failed: 13.0 s against 9.3 s, ratio 1.39. Per the plan's
contingency the step wrapper then moved into a `systemd-run --user --scope` unit with the cgroup cross-check,
and round 2 passed: 9.9 s against 9.0 s, ratio 1.10, with the cgroup within 0.1% of `getrusage` and the
server accounting for 9.3 of the 9.9 CPU-s. Both rounds are in the raw data (`phase: calibration`). On a
4-unit, 10-second build, (b)'s own two readings differ by 31% between rounds, which is host-load noise; the
bench steps that matter run for minutes.

### Bookkeeping and the zero-delta control

Cumulative `cpu_s` over all bench steps is capped at 450 CPU-minutes with a stop rule (a `truncated` flag on
the record that crosses it). Crate 1's five planned sequences used 53.3 CPU-minutes, far below the 200
CPU-minute line that would have reduced steps 3 and 4, so those run in full.

One addition to the plan's sequences: a **zero-delta control** (`d0a` under (a), `d0b` under (b)). Same crate,
reseeded at the basecommit itself (`behind_by` 0, so the seed's delta touch is empty), steps S1, S2, S6, S7 and
no edits. At 341 commits of staleness the delta dirties 19 units of reify-compiler's closure under (b), which
confounds the effect of the settings with the effect of the base's age; the control isolates the second. It
brackets the production case, since the age of a production lane's base varies.

## Bench results

Tables give the median over the repetitions of a sequence: two for (a) and (b) on reify-compiler
(`a1`/`a2`, `b1`/`b2`), one for (b0). `cpu_s` and `wall_s` are seconds. "Units written (new vs base)" counts
the fingerprint units the step wrote and, in brackets, those the base never had under that hash. Every row
below runs at `BENCH_HEAD`, 341 commits ahead of the base, unless it says zero delta.

### reify-compiler (census rank 1)

`T` = `harness_modules_ports`, `F` = `compile_builder/functions_phase.rs` (function `describe_slot`), S7 closure
= 25 packages. Source: sequences `a1 b1 a2 b2 b0`, 41 measured steps.

| step | arm | reps | cpu_s | wall_s | units written (new vs base) | dirty | failed to read |
|---|---|---:|---:|---:|---:|---:|---:|
| S1 `--lib` | a | 2 | 311.0 | 145.9 | 39 (39) | 0 | 39 |
| S1 `--lib` | b | 2 | 334.8 | 184.3 | 32 (13) | 19 | 13 |
| S1 `--lib` | b0 | 1 | 280.4 | 143.3 | 32 (13) | 19 | 13 |
| S2 `--test T` | a | 2 | 13.6 | 8.7 | 1 (1) | 0 | 1 |
| S2 `--test T` | b | 2 | 11.7 | 10.4 | 1 (1) | 0 | 1 |
| E1-S2 | a | 2 | 10.3 | 8.3 | 4 (4) | 4 | 0 |
| E1-S2 | b | 2 | 88.6 | 65.4 | 4 (3) | 4 | 0 |
| E1-S1 | a | 2 | 7.7 | 7.0 | 1 (1) | 1 | 0 |
| E1-S1 | b | 2 | 37.8 | 28.1 | 1 (1) | 1 | 0 |
| E2-S2 | a | 2 | 9.6 | 7.7 | 4 (4) | 4 | 0 |
| E2-S2 | b | 2 | 85.4 | 62.0 | 4 (3) | 4 | 0 |
| E2-S1 | a | 2 | 5.7 | 5.0 | 1 (1) | 1 | 0 |
| E2-S1 | b | 2 | 36.1 | 25.7 | 1 (1) | 1 | 0 |
| E3-S2 | a | 2 | 10.1 | 7.9 | 4 (4) | 4 | 0 |
| E3-S2 | b | 2 | 77.5 | 53.8 | 4 (3) | 4 | 0 |
| E3-S1 | a | 2 | 6.4 | 6.1 | 1 (1) | 1 | 0 |
| E3-S1 | b | 2 | 34.1 | 24.2 | 1 (1) | 1 | 0 |
| S6 clippy | a | 2 | 186.7 | 128.1 | 62 (62) | 0 | 62 |
| S6 clippy | b | 2 | 180.2 | 168.2 | 55 (42) | 13 | 42 |
| S7 probe | a | 2 | 8.1 | 14.7 | – | 620 | 34 |
| S7 probe | b | 2 | 2.6 | 6.5 | – | 607 | 33 |

Pooled over the three edits (six readings per arm), median `cpu_s` / `wall_s`:

| edit-rebuild | (a) | (b) | (b) over (a) |
|---|---:|---:|---:|
| `Ek-S2` (`--test T` after an edit) | 10.0 / 8.1 | 83.3 / 60.3 | 8.3x CPU, 7.4x wall |
| `Ek-S1` (`--lib` after an edit) | 6.4 / 6.1 | 34.9 / 25.4 | 5.5x CPU, 4.2x wall |

Rep spread, `|rep1 - rep2|` over the mean of `cpu_s`: first build 5.2% (a) and 11.7% (b); S2 10% (a) and 13%
(b); clippy 1.4% (a) and 4.3% (b); edit-rebuild steps 0.2% to 9% (a) and 6% to 18% (b).

What the rows say:

- **The base is reused only under (b) and (b0); under (a) nothing of it is.** Every unit (a) writes is new:
  `-C incremental` changes the hash of every workspace unit, so the base holds none of them (39 of 39 in S1,
  62 of 62 in S6, zero dirty). Under (b) and (b0) the units the base has are found. No external crate is
  rebuilt or unreadable in any S1, S2, Ek or S6 step of any arm.
- **At today's staleness that reuse buys no first-build CPU.** (b)'s S1 reuses all but 32 units and still costs
  335 CPU-s against (a)'s 311, a difference inside (b)'s own 11.7% rep spread, and 26% more wall (184 s
  against 146 s). The 32 units are 19 the delta dirtied and 13 the base never had. The 19 are all
  `FsStatusOutdated`, source files the seed's delta touch marked newer than the fingerprint: the libraries
  of reify-ast, -core, -builtins, -compiler, -config, -build-utils, -ir, -syntax and -shell-extract, the
  library and build-script run of tree-sitter-reify, the library, build script and build-script run of
  reify-config, and the build script and its run for the gmsh, solver-elastic and openvdb kernels. The 13 are
  absent from the base under the hash this selection asks for: the `reify-compiler` test unit, the libraries
  of reify-eval (with its build script and its run), -stdlib, -constraints, -expr, -compute-contract,
  -solver-elastic, -kernel-gmsh, -kernel-openvdb, -mcp and -test-support. That is the feature-split of the
  study's M1 (dark-factory `plans/seeded-lane-rebuild-study-2026-11-17/S-seed-mechanics.md` section 4),
  consistent with a base built under the verify's workspace-wide feature set; this bench does not test the
  cause. The 13 are rebuilt whatever the delta.
- **The wrapper is not what makes the base reusable.** (b0) has no wrapper and shows exactly (b)'s unit
  counts (32 written, 13 new, 19 dirty, 13 unreadable). Had `RUSTC_WRAPPER` been part of the unit hash or
  fingerprint, (b0) would have found every unit of the closure unreadable or dirty. It agrees with the
  study's experiment iii, which reused the base under a different wrapper. The `CARGO_INCREMENTAL=0` half is
  the only one that matters for reuse; what this means for the existing memories is in the recommendation.
- **sccache itself costs CPU on a cold cache.** (b) 335 against (b0) 280 CPU-s on S1 is 19% more, one (b0)
  repetition, so indicative. The server's own share of (b)'s S1 was 274 of 315 CPU-s in `b1`.
- **The edit loop is where (b) loses, by a wide margin.** Each edit-rebuild costs 5 to 8 times the CPU and 4
  to 7 times the wall under (b). Under (a) incremental compilation keeps an edit-rebuild to 10 CPU-s for
  the test target and 6 for the lib test; under (b) the edited crate is recompiled from scratch, 63 to 79
  server CPU-s per `Ek-S2`, and sccache cannot help because the content is new (3 cache misses of 3 executed
  requests in `E1-S2`).
- **Clippy does not benefit.** (b)'s S6 is 180 CPU-s against (a)'s 187, equal within the spread, and 31%
  more wall. sccache cannot cache check-mode compiles: 52 of 56 requests were not cacheable in `b1`'s S6.
  Of the 55 units (b)'s S6 wrote, 13 were in the base and dirtied by the delta, and 42 were not in it.
- **A (b) agent pre-pays almost nothing the verify would build, at this staleness.** S7: after (b)'s work the
  verify's closure still has 607 dirty units against 620 after (a)'s, so (b) pre-paid 13 units (2.1%).
  The delta dirties about 600 units of the verify's graph, all but 5 of them workspace units, whichever arm
  the agent ran.
- **S2 builds one test unit:** 13.6 against 11.7 CPU-s, the same.

#### Zero-delta control, reify-compiler (`d0a`, `d0b`)

One repetition each, reseeded at the basecommit itself (`behind_by` 0: the seed's log reports "Touched 0 git
delta path(s)"), S1, S2, S6 and S7 only, no edits.

| step | arm | cpu_s | wall_s | units written (new vs base) | dirty | failed to read |
|---|---|---:|---:|---:|---:|---:|
| S1 `--lib` | a | 306.1 | 215.6 | 39 (39) | 0 | 39 |
| S1 `--lib` | b | 174.2 | 79.0 | 13 (13) | 0 | 13 |
| S2 `--test T` | a | 12.2 | 8.6 | 1 (1) | 0 | 1 |
| S2 `--test T` | b | 12.0 | 9.4 | 1 (1) | 0 | 1 |
| S6 clippy | a | 190.1 | 173.5 | 62 (62) | 0 | 62 |
| S6 clippy | b | 166.3 | 139.8 | 42 (42) | 0 | 42 |
| S7 probe | a | 2.3 | 5.1 | – | 299 | 29 |
| S7 probe | b | 2.6 | 7.7 | – | 299 | 28 |

- **The delta, not the setting, ate (b)'s first-build advantage.** On a base with no delta, (b)'s first build
  costs 174 CPU-s against (a)'s 306, 43% less, and 79 s of wall against 216 s. The wall gap is inflated by
  the host (the two sequences ran minutes apart at PSI `some avg10` 56 and 39); the CPU gap is not. At 341
  commits the same build costs (b) 335 CPU-s: the 19 delta-dirtied units cost it another 160. Arm (a) does not
  care, 306 at zero delta against 311 at 341 commits, because it reuses nothing either way.
- **The 174 CPU-s that remain are the 13 feature-split units, a floor a fresher base does not remove.** They
  are the same 13 as at 341 commits: the `reify-compiler` test unit and the libraries of reify-eval (with its
  build script and its run), -stdlib, -constraints, -expr, -compute-contract, -solver-elastic, -kernel-gmsh,
  -kernel-openvdb, -mcp and -test-support. The server did 141 of the 174 CPU-s (10 cache misses, 2 requests
  not cacheable).
- **Clippy gains a little:** 166 against 190 CPU-s (12% less), 42 units written against 62. The base holds
  check-mode units for 13 workspace libraries and the build scripts of 3 packages (7 units), and (b) reuses
  those 20 where (a) rebuilds them; none of the 24 test-mode check units is in the base under either arm.
  sccache executed 2 of 43 requests (both misses); the other 41 were check-mode compiles it does not cache.
- **A zero-delta lane is not clean for the verify either.** S7 finds 299 dirty units in both arms. They are
  the seed's own doing, by design: its log reports 235 `env!()`-baked-path test sources relinked and 84
  build-script output directories invalidated. The roots are 26 `FsStatusOutdated` test units (14 of them
  reify-audit) and 2 re-run build scripts, and the other 271 are units that depend on them. The (b) agent
  pre-paid one unit of the verify's graph (29 unreadable against 28), because the 13 units (b)'s S1 builds
  are the feature-split ones the verify never builds under that hash.

### reify-eval (census rank 2)

`T` = `harness_engine`, `F` = `relate_solve.rs` (function `realize_structures`), S7 closure = 25 packages. One
sequence per arm (`a1`, `b1`) and no second repetition, as planned: the CPU of a fixed unit set is
near-deterministic, and crate 1's pairs bound the noise (first build 5% and 12%, clippy 1% and 4%, edit steps up
to 9% and 18%). Test targets were built, never run.

| step | arm | reps | cpu_s | wall_s | units written (new vs base) | dirty | failed to read |
|---|---|---:|---:|---:|---:|---:|---:|
| S1 `--lib` | a | 1 | 412.7 | 306.2 | 54 (49) | 5 | 49 |
| S1 `--lib` | b | 1 | 340.9 | 176.8 | 47 (20) | 27 | 20 |
| S2 `--test T` | a | 1 | 9.4 | 8.6 | 1 (1) | 0 | 1 |
| S2 `--test T` | b | 1 | 6.3 | 5.3 | 1 (1) | 0 | 1 |
| E1-S2 | a | 1 | 10.1 | 12.2 | 5 (5) | 5 | 0 |
| E1-S2 | b | 1 | 51.1 | 35.4 | 5 (5) | 5 | 0 |
| E1-S1 | a | 1 | 13.7 | 16.1 | 1 (1) | 1 | 0 |
| E1-S1 | b | 1 | 84.2 | 60.3 | 1 (1) | 1 | 0 |
| E2-S2 | a | 1 | 9.8 | 12.1 | 5 (5) | 5 | 0 |
| E2-S2 | b | 1 | 59.1 | 42.6 | 5 (5) | 5 | 0 |
| E2-S1 | a | 1 | 13.2 | 16.5 | 1 (1) | 1 | 0 |
| E2-S1 | b | 1 | 79.1 | 51.6 | 1 (1) | 1 | 0 |
| E3-S2 | a | 1 | 10.6 | 31.1 | 5 (5) | 5 | 0 |
| E3-S2 | b | 1 | 45.1 | 29.4 | 5 (5) | 5 | 0 |
| E3-S1 | a | 1 | 15.3 | 73.9 | 1 (1) | 1 | 0 |
| E3-S1 | b | 1 | 61.1 | 42.3 | 1 (1) | 1 | 0 |
| S6 clippy | a | 1 | 309.9 | 215.8 | 288 (285) | 3 | 285 |
| S6 clippy | b | 1 | 209.6 | 110.9 | 281 (260) | 21 | 260 |
| S7 probe | a | 1 | 8.3 | 32.5 | – | 615 | 34 |
| S7 probe | b | 1 | 3.0 | 7.2 | – | 597 | 34 |

Pooled over the three edits (three readings per arm), median `cpu_s` / `wall_s`:

| edit-rebuild | (a) | (b) | (b) over (a) |
|---|---:|---:|---:|
| `Ek-S2` (`--test T` after an edit) | 10.1 / 12.2 | 51.1 / 35.4 | 5.1x CPU, 2.9x wall |
| `Ek-S1` (`--lib` after an edit) | 13.7 / 16.5 | 79.1 / 51.6 | 5.8x CPU, 3.1x wall |

- **Here the base does help the first build at today's staleness, modestly.** (b)'s S1 costs 341 CPU-s against
  (a)'s 413 (17% less) and 177 s of wall against 306 s. One repetition each, and 17% is only a little above
  crate 1's spreads, so it is a modest saving and not a precise figure. (b) writes 47 units: 27 dirty, which
  is the delta (21 `FsStatusOutdated` library and build-script units and the `manifold-csg-sys` build-script
  run that the seed invalidates, plus their dependents), and 20 absent from the base, the feature-split of M1
  again: the libraries of reify-compiler, -eval (and its test unit), -stdlib, -constraints, -expr,
  -compute-contract, -doc-build, -git-env, -runtime, -test-support, -mesh-morph, -solver-elastic and
  -kernel-occt, and the library, build script and build-script run of the gmsh and openvdb kernels. (a)
  writes 54: 49 absent from the base and 5 the base has but the seed invalidated. The private server did 258
  of (b)'s 341 CPU-s (35 cache misses in 35 executed requests).
- **The edit loop costs (b) 5 to 6 times the CPU,** in line with crate 1 (8.3x and 5.5x): 45 to 59 CPU-s per
  `Ek-S2` against (a)'s 10, 61 to 84 per `Ek-S1` against 13 to 15. The server did 39 to 51 of each `Ek-S2`,
  4 cache misses in 4 executed requests. The wall ratios (2.9x and 3.1x) are lower than crate 1's because
  (a)'s E3 readings are load-inflated: 31 s and 74 s of wall for 10.6 and 15.3 CPU-s.
- **Clippy is 32% cheaper under (b):** 210 against 310 CPU-s, 111 s against 216 s of wall. Both arms write about
  285 units, since reify-eval's `--all-targets` graph has one check unit per integration-test target (237
  test-mode check units). The 7 units (b) does not write are the build scripts of reify-constraints (compile
  and run) and tree-sitter-reify, and the check-mode libraries of reify-doc, -fdm, -gcode and
  workspace-feature-union, all found in the base. One repetition; I do not attribute the 100 CPU-s gap further.
  sccache executed 9 of 282 requests, all misses; 273 were check-mode and not cacheable.
- **S7:** the verify's closure has 597 dirty units after (b)'s work against 615 after (a)'s, so (b) pre-paid 18
  units (2.9%), none of them external.

#### Zero-delta control, reify-eval (`d0a`, `d0b`)

One repetition each, reseeded at the basecommit, S1, S2, S6 and S7 only.

| step | arm | cpu_s | wall_s | units written (new vs base) | dirty | failed to read |
|---|---|---:|---:|---:|---:|---:|
| S1 `--lib` | a | 440.4 | 205.5 | 54 (49) | 5 | 49 |
| S1 `--lib` | b | 339.2 | 239.4 | 26 (20) | 6 | 20 |
| S2 `--test T` | a | 11.1 | 9.6 | 1 (1) | 0 | 1 |
| S2 `--test T` | b | 7.3 | 13.9 | 1 (1) | 0 | 1 |
| S6 clippy | a | 297.2 | 144.6 | 288 (285) | 3 | 285 |
| S6 clippy | b | 235.6 | 141.5 | 264 (260) | 4 | 260 |
| S7 probe | a | 1.5 | 2.2 | – | 294 | 29 |
| S7 probe | b | 2.4 | 5.7 | – | 293 | 29 |

- **For reify-eval the base's age barely matters; the feature-split units set the floor.** (b)'s first build
  costs 339 CPU-s at zero delta and 341 at 341 commits. The 21 units the delta dirties are small crates and
  cost almost nothing next to the 20 absent units, which include reify-eval's own library and test unit. At
  zero delta (b) saves 101 CPU-s (23%) on the first build against (a)'s 440; at 341 commits it saved 72 (17%)
  against 413. (a) itself reads 7% higher here than at stale, one repetition each.
- **The remaining dirty units are the seed's own.** (b)'s 6 and (a)'s 5 dirty units are the `manifold-csg-sys`
  build-script run, which the seed always invalidates, and the units that depend on it (`manifold-csg-sys`,
  `manifold-csg`, `manifold3d`, and for (b) `reify-kernel-manifold`).
- **CPU and wall disagree on the first build, and the wall readings are not comparable.** (b)'s S1 took 239 s
  of wall for 339 CPU-s at PSI `some avg10` 4, against (a)'s 206 s for 440 CPU-s at PSI 63.
  Hypothesis, not tested: the 20 absent units include one very large library (reify-eval's own) compiled in a
  single rustc process, so (b)'s wall is bound by that critical path, while (a)'s incremental build splits it
  into many codegen units, and a quiet host would shorten (a)'s wall more than (b)'s. The first-build wall
  difference between arms on this crate is therefore not resolved by this bench; the CPU difference is.
- **Clippy:** 236 against 297 CPU-s (21% less under (b)), 260 absent units as at stale and 4 dirty. sccache
  executed 7 of 267 requests, all misses.
- **S7:** 293 against 294 dirty units, so (b) pre-paid none of the verify's units at zero delta. The 294 are,
  as for reify-compiler, the seed's own invalidation.

### reify-cli (census rank 3)

`T` = `harness_cli_surface`, `F` = `help.rs` (function `write_flags`). reify-cli is bin-only, so S1 is
`cargo test -p reify-cli --bins --no-run`, and S7's closure is the crate alone (nothing depends on it). One
sequence per arm (`a1`, `b1`), as for reify-eval.

| step | arm | reps | cpu_s | wall_s | units written (new vs base) | dirty | failed to read |
|---|---|---:|---:|---:|---:|---:|---:|
| S1 `--bins` | a | 1 | 413.9 | 146.8 | 61 (56) | 5 | 56 |
| S1 `--bins` | b | 1 | 323.1 | 136.7 | 54 (26) | 28 | 26 |
| S2 `--test T` | a | 1 | 16.5 | 10.8 | 2 (2) | 0 | 2 |
| S2 `--test T` | b | 1 | 12.0 | 7.3 | 2 (2) | 0 | 2 |
| E1-S2 | a | 1 | 3.5 | 3.7 | 1 (1) | 1 | 0 |
| E1-S2 | b | 1 | 6.4 | 5.7 | 1 (1) | 1 | 0 |
| E1-S1 | a | 1 | 3.5 | 5.0 | 1 (1) | 1 | 0 |
| E1-S1 | b | 1 | 6.4 | 6.0 | 1 (1) | 1 | 0 |
| E2-S2 | a | 1 | 3.1 | 3.3 | 1 (1) | 1 | 0 |
| E2-S2 | b | 1 | 6.3 | 5.7 | 1 (1) | 1 | 0 |
| E2-S1 | a | 1 | 3.0 | 3.3 | 1 (1) | 1 | 0 |
| E2-S1 | b | 1 | 6.2 | 5.4 | 1 (1) | 1 | 0 |
| E3-S2 | a | 1 | 3.3 | 3.5 | 1 (1) | 1 | 0 |
| E3-S2 | b | 1 | 6.5 | 5.5 | 1 (1) | 1 | 0 |
| E3-S1 | a | 1 | 3.5 | 3.6 | 1 (1) | 1 | 0 |
| E3-S1 | b | 1 | 6.1 | 4.9 | 1 (1) | 1 | 0 |
| S6 clippy | a | 1 | 151.8 | 95.3 | 62 (59) | 3 | 59 |
| S6 clippy | b | 1 | 160.5 | 132.4 | 55 (35) | 20 | 35 |
| S7 probe | a | 1 | 5.4 | 11.1 | – | 23 | 29 |
| S7 probe | b | 1 | 0.8 | 21.5 | – | 0 | 1 |

Pooled over the three edits (three readings per arm), median `cpu_s` / `wall_s`:

| edit-rebuild | (a) | (b) | (b) over (a) |
|---|---:|---:|---:|
| `Ek-S2` (`--test T` after an edit) | 3.3 / 3.5 | 6.4 / 5.7 | 2.0x CPU, 1.6x wall |
| `Ek-S1` (`--bins` after an edit) | 3.5 / 3.6 | 6.2 / 5.4 | 1.7x CPU, 1.5x wall |

- **(b)'s first build is 22% cheaper at today's staleness:** 323 against 414 CPU-s, 137 s against 147 s of
  wall. (b) writes 54 units: 28 dirty (24 roots and 4 dependents; 22 of the roots are
  `FsStatusOutdated`, the delta's doing, among them the build scripts of reify-cli, reify-gui, reify-config and
  the gmsh, occt and solver-elastic kernels, and the other 2 are the build-script runs of reify-gui and
  `manifold-csg-sys` that the seed invalidates) and 26 absent from the base, the feature-split once more (reify-cli's own test unit, the libraries
  of reify-compiler, -eval with its build script and run, -stdlib, -constraints, -expr, -runtime, -lsp, -gui,
  -mcp and others, and the libraries and build-script runs of the openvdb and mesh-morph kernels). (a) writes
  61: 56 absent and 5 invalidated by the seed. The private server did 308 of (b)'s 323 CPU-s.
- **The edit loop is cheap under both arms and (b) loses little in absolute terms.** The edited crate is the
  binary, small next to the libraries it links, so an edit-rebuild costs 3.3 CPU-s under (a) and 6.4 under (b):
  2.0x and 1.7x, three seconds more per edit. The one compile request is not cacheable and passes through the
  server (0.24 s of server CPU).
- **Clippy is a wash:** 160 against 152 CPU-s at stale (b 6% dearer), 152 against 158 at zero delta.
- **S7 is degenerate for this crate.** The probe's closure is `-p reify-cli`, the same selection the agent
  built, so (b) pre-pays all of it: 0 dirty and 1 unreadable unit against (a)'s 23 and 29. It bounds the
  pre-pay from above; the verify's real build is workspace-wide, and for reify-compiler and reify-eval, whose
  probes span 25 packages, (b) pre-paid 13 to 18 units at stale and 0 or 1 at zero delta.

#### Zero-delta control, reify-cli (`d0a`, `d0b`)

One repetition each, reseeded at the basecommit, S1, S2, S6 and S7 only.

| step | arm | cpu_s | wall_s | units written (new vs base) | dirty | failed to read |
|---|---|---:|---:|---:|---:|---:|
| S1 `--bins` | a | 341.7 | 158.3 | 57 (52) | 5 | 52 |
| S1 `--bins` | b | 275.5 | 172.2 | 29 (24) | 5 | 24 |
| S2 `--test T` | a | 14.8 | 8.1 | 2 (2) | 0 | 2 |
| S2 `--test T` | b | 9.9 | 4.2 | 2 (2) | 0 | 2 |
| S6 clippy | a | 157.8 | 112.2 | 58 (55) | 3 | 55 |
| S6 clippy | b | 151.7 | 301.3 | 36 (33) | 3 | 33 |
| S7 probe | a | 0.7 | 0.8 | – | 0 | 27 |
| S7 probe | b | 1.1 | 26.7 | – | 0 | 1 |

- **At zero delta (b)'s first build is 19% cheaper:** 276 against 342 CPU-s. Against its own stale reading
  (b) saves 47 CPU-s (323 to 276), the 28 dirtied units less the 5 the seed always invalidates.
- **(a) is not delta-insensitive on this crate:** 414 CPU-s at stale against 342 at zero delta, 61 units
  written against 57. The delta changed reify-cli's unit graph: at stale (b) lists the reify-gui and
  reify-mcp libraries among its absent units and at zero delta it does not. The pricing therefore does not
  pool (a)'s two delta settings for this crate, only for those where the written-unit counts match
  (reify-compiler 39 and 39, reify-eval 54 and 54).
- **Wall is unusable on the clippy row:** (b)'s S6 took 301 s of wall for 152 CPU-s, a load artefact (PSI is
  recorded per step in the raw data).
- **S7:** (a) leaves 27 of the probe's units unreadable and (b) leaves 1, the same degenerate selection as above.
