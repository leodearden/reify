# Agent cargo builds under the verify's settings: bench and recommendation

Task 10169. Measured 2026-11-19 on `leo-MS-7C35` (AMD Ryzen 9 3950X, 16 cores / 32 threads, 125 GiB RAM,
kernel 7.0.0-31-generic; rustc and cargo 1.96.0, sccache 0.14.0).

| | |
|---|---|
| Warm base (`BASE_GEN`) | `/home/leo/src/warm-lanes/base/target.gen.643`, basecommit `cb24012c60a6b800f763acc65943ca0d21fc163e` |
| Bench head (`BENCH_HEAD`) | `f743808f664fbf782a2867019102ae1c97c86705`, main's tip at bench start, 341 commits ahead of the basecommit |
| Raw per-step data | `docs/notes/agent-build-settings-bench-raw.jsonl` |

Status: this revision carries the census. The bench results, pricing, recommendation and caveats are added
by the later steps of the task.

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
