# Eval slow-tail profile — 2026-09 (task #7431)

Measurements taken on the task/7431 worktree (`_lane-7`), branch tip at the time
of each run. Every number below is MEASURED on this host, not projected. Where a
measurement contradicts the figure this task was scoped from, the measurement
wins and the discrepancy is stated rather than smoothed over.

**Scope note.** Sections 2 and 3 are a PROFILE ONLY. No file under
`crates/reify-cli/` or `gui/` was edited by this task.

---

## 1. Corpus-gate before/after

Task #7431 replaced two independently-sharded 24-way corpus sweeps, each doing
its own full compile+eval of an overlapping corpus, with ONE hash-keyed 24-way
sweep that does a single compile+eval per file and asserts both invariants off
it.

### CPU and wall, shards only

Each row is `cargo nextest run` filtered to that gate's 24 shard tests, so the
comparison is like-for-like and excludes each binary's unrelated tests.

| | Summary wall | user CPU | sys | straggler |
|---|---|---|---|---|
| BEFORE — `no_stale_undef_invariant_gate`, 24 INV-EVAL-5 shards | 160.217s | 352.1s | 11.8s | `broad_corpus_sweep_shard_15` @ 160.2s |
| BEFORE — `harness_cache`, 24 INV-EVAL-4 shards | 159.900s | 350.8s | 15.4s | `snapshot_cache_sweep_shard_15` @ 159.9s |
| **BEFORE total** — 48 test processes | — | **702.9s** | 27.2s | — |
| AFTER — `harness_corpus_gates`, 24 unified shards | 184.957s | 352.1s | 17.9s | `corpus_sweep_shard_20` @ 184.9s |
| **AFTER total** — 24 test processes | — | **352.1s** | 17.9s | — |

**Saving: −350.8 CPU-seconds (−50%), and 24 fewer test-process spawns.**

The absolute CPU numbers are lower than the 787s + 907s = 1694s this task was
scoped from — those were presumably captured under verify-pipeline contention,
not standalone. The RATIO is what transfers, and it agrees closely: the task
projected 1694s → 866s (a 49% cut); this measures 702.9s → 352.1s (a 50% cut).

Note that the unified sweep's CPU (352.1s) is essentially identical to the
INV-EVAL-5 sweep's alone (352.1s). That is the result the whole restructuring
predicted: the divergence checker's MARGINAL cost on an already-evaluated engine
is near zero, so asserting the second invariant is effectively free once the
evaluation is shared. The plan modelled that marginal at δ ≈ 0.3s/file (≈ 79s
over 264 files); measured, it is below the run-to-run noise floor.

### Whole-binary effects

- `harness_cache`: 120 tests → 96 tests; the full binary now runs in **5.523s
  wall / 35.2s user CPU**, against 159.900s / 350.8s for its shards alone before.
  The file also shrank 458 → 139 lines, widening that 10 333-line unit's margin
  against `test_harness_kloc_cap.sh`'s 20 000-line cap.
- `no_stale_undef_invariant_gate`: 39 tests + 1 skipped → 14 tests; full binary
  19.390s wall.

### Shard partition

Hash keying (`xxh3_128(repo_relative_path) % 24`) over the live 299-file corpus:

```
299 files over 24 shards — min 6, max 23, mean 12.46, assertion bound 39
shard 00: 6   shard 06: 7   shard 12: 9   shard 18: 7
shard 01: 14  shard 07: 9   shard 13: 13  shard 19: 18
shard 02: 23  shard 08: 15  shard 14: 12  shard 20: 15
shard 03: 15  shard 09: 18  shard 15: 11  shard 21: 10
shard 04: 13  shard 10: 12  shard 16: 14  shard 22: 8
shard 05: 6   shard 11: 9   shard 17: 16  shard 23: 19
```

Against the plan's balance basis — three independent 128-bit hashes over these
same 299 paths gave max 19 / 21 / 20 and min 7 / 6 / 8 — xxh3's max of 23 is
slightly worse than all three point samples but sits between the Monte-Carlo p90
(22) and p99 (25) for multinomial(299, 24), i.e. ordinary variance, not a
degenerate key. Min 6 matches the sampled range. The assertion bound of
`3 × ceil(299/24) = 39` clears the observed max by 1.7×.

### ⚠ Correction: shard size does NOT predict shard wall time

The plan projected a worst shard of ~21 files ≈ 61s by scaling a uniform
per-file cost. **That model is wrong and the measurement refutes it.** The
straggler is `corpus_sweep_shard_20` at 184.9s while holding 15 files, and
`corpus_sweep_shard_02` holds 23 files yet completes in ~62s. Per-file cost is
strongly non-uniform (a handful of FEA/OCCT-bearing examples dominate), so a
shard's wall time is set by WHICH expensive files it drew, not how many files it
holds.

Consequences, stated plainly:

- The straggler moved 160.2s → 184.9s, i.e. **+24.7s (1.15×)** — far better than
  the 1.85× a file-count model would have feared, but not the ~61s it projected.
- A second full run of the unified shards, under different host contention,
  measured a Summary wall of **238.393s**. Treat 185–240s as the observed band,
  not 185s as a point value.
- This materially weakens the plan's stated basis for leaving
  `.config/nextest.toml` alone — see §4.

### This is ONE run, not Trial E10

`data/verify-logs/` is EMPTY in a task worktree (the ledger lives in the main
checkout), so the 20-gate before/after ledger comparison cannot be produced from
a task leg. Recipe for the operator, to be run from `/home/leo/src/reify`:

1. On a pre-#7431 commit, over 20 gate runs, record CPU-seconds for the
   `no_stale_undef_invariant_gate` and `harness_cache` binaries plus the nextest
   Summary wall.
2. On a post-#7431 commit, same 20-gate window, record the same for
   `harness_corpus_gates`.
3. Compare distributions, not single runs — the 185s vs 238s spread above is
   exactly the contention variance a single run cannot separate from signal.

---

## 2. `harness_cli` (reify-cli)

One `cargo nextest run -p reify-cli --test harness_cli`.

| metric | value |
|---|---|
| tests run | 296 (of 299 `#[test]` fns across 76 modules; the rest are cfg-gated) |
| Summary wall | 58.648s |
| Σ per-test wall | 1112.5s |
| mean / p50 / p90 / p99 / max | 3.759s / 2.815s / 6.739s / 28.83s / 37.07s |
| top 10 tests' share | 231.7s = 21% of total |

Slowest: `cli_check::check_constraint_results_come_from_authoritative_check_not_build`
(37.07s), `cli_dfm_overhang::check_dfm_plus_repr_within_combined_arm` (30.01s),
`cli_determinacy_gate::check_representation_within_satisfied_exits_zero` (28.83s).

### ⚠ Correction: spawns are NOT funnelled through one site

This task was scoped on the premise that "every spawn goes through the ONE
`env!("CARGO_BIN_EXE_reify")` `Command::new` site in
`crates/reify-cli/tests/common/mod.rs:20-21`". **That is false.**
`common/mod.rs` does hold one `Command::new`, behind 13 `pub fn` run helpers —
but `harness_cli`'s own modules carry **11 further `Command::new` sites** of
their own (`cli_doc.rs` ×3, `cli_build_3mf.rs` ×3, `cli_test.rs` ×2, plus
`cli_cache_concurrent_writers.rs`, `cli_build_voxel_to_mesh.rs`,
`cli_build_3mf_coating.rs`). Any future spawn-batching work must account for
those, not assume a single chokepoint.

### Spawn census, and where the cost actually is

- **509 spawn call-sites** across the 76 modules (run-helper calls +
  direct `Command::new`), ≈ **1.7 spawns per test**.
- Top modules by spawn call-sites: `cli_check.rs` (63), `cli_doc.rs` (20),
  `cli_purpose_stdlib.rs` (20), `cli_integration_smoke.rs` (18),
  `cli_objective_inheritance_golden.rs` (18), `cli_purpose.rs` (18).
- **Bare spawn floor, measured**: 20 × `reify --version` = 3.870s real ⇒
  **0.194s per bare spawn** (dominated by dynamic linking of OCCT/OpenVDB).

**Verdict — the cost is per-spawn WORK, not process spawn.** 509 spawns ×
0.194s ≈ 99s, i.e. **≈ 9%** of the 1112.5s total; the median test spends 2.815s
against a 0.194s spawn floor, so ~93% of a typical test is the `reify` binary
compiling and evaluating its fixture. A future fix should therefore target
**fixture trimming / cheaper per-invocation work** (or in-process invocation),
**not spawn batching** — batching could recover at most ~9%, and only by
sacrificing the process isolation the spawns buy.

---

## 3. `reify-gui` lib tests

One `cargo nextest run -p reify-gui --lib`.

| metric | value |
|---|---|
| tests run | 991 (1 leaky), 0 skipped |
| census | 21 modules under `gui/src-tauri/src/tests/` — 901 `#[test]` + 91 `#[tokio::test]`; whole crate 970 + 124 = 1094 |
| Summary wall | 29.119s |
| Σ per-test wall | 906.0s |
| mean / p50 / p90 / p99 / max | 0.915s / 0.706s / 1.944s / 3.56s / 5.85s |

Ranked by summed wall:

| module | Σ wall | tests | mean | share |
|---|---|---|---|---|
| `tests::engine_tests` | 534.6s | 434 | 1.23s | **59%** |
| `tests::commands_tests` | 98.4s | 77 | 1.28s | 11% |
| `tests::types_tests` | 54.5s | 121 | 0.45s | 6% |
| `tests::mcp_context_tests` | 40.7s | 34 | 1.20s | 4% |
| `tests::diff_tests` | 34.9s | 52 | 0.67s | 4% |
| `tests::claude_bridge_tests` | 24.8s | 35 | 0.71s | 3% |
| `tests::large_stack_tests` | 17.6s | 29 | 0.61s | 2% |
| `tests::lsp_bridge_tests` | 16.5s | 14 | 1.18s | 2% |
| `tests::mcp_dispatch_tests` | 14.2s | 13 | 1.09s | 2% |
| `tests::watcher_tests` | 12.6s | 32 | 0.39s | 1% |

**Verdict — there is no straggler here to fix.** The slowest single test is
5.85s and p99 is 3.56s, yet `engine_tests` alone accounts for 59% of the total
across **434 tests averaging 1.23s each**. The cost is uniform and structural,
almost certainly a per-test engine/compile setup each of those 434 tests pays
independently. The lever is therefore **shared or cached setup** (a fixture
built once per module rather than per test), not trimming slow outliers — there
are none. Any such change belongs to the reify-gui owners; nothing here was
touched.

---

## 4. Findings and deliberate non-actions

**Residual reconciliation (task #7431 S19).** All SIX residual exemptions still
produce findings under the unified sweep's production (superset) registration,
so no entry was deleted: `integration_corner_cases.ri` (2),
`match_block_decls_bolt.ri` (1), `multi_load_bracket.ri` (1),
`surface_finish_functional.ri` (1) for INV-EVAL-5; `fdm_bracket.ri` (1),
`fea_shell_too_thick_annotated.ri` (1) for INV-EVAL-4. In particular the
superset registration did **not** resolve `fea_shell_too_thick_annotated.ri` —
its divergence is a genuine compute-dispatch eval-surface limitation, not an
artefact of the degraded `register_compute_fns` dispatch it was root-caused
under. Recorded so it is not re-litigated.

**`.config/nextest.toml` was NOT edited** — it is outside this task's scope
list, and neither gate has an entry there today. But the justification recorded
in the plan does **not** survive measurement and should not be relied on:

- The plan argued an LPT `priority` buys nothing because "the unified shards'
  ~61s worst-case straggler cannot move a makespan floor set by the >180s
  `tensegrity_t0a` LPT tier-1 straggler". **The measured straggler is 184.9s,
  with a second run at 238.4s** — i.e. AT or ABOVE that floor, not far below it.
  On this evidence a `priority` entry for `harness_corpus_gates` is a live
  lever, not a dead one, and should be evaluated against a real Summary-wall
  measurement.
- The slow-timeout argument is weaker than stated but still holds: 185–240s
  against the inherited `[profile.default]` 1200s ceiling is ~5–6.5× headroom.
  It is worth noting that at the 8× contention of esc-5097-3 a 238s nominal
  would reach ~1900s and breach that ceiling, so this is not unlimited margin.

These are recorded as follow-ups, not acted on here — editing that file is
outside the task's scope and would additionally trip
`test_nextest_slow_priority.sh` / `test_occt_gated_scope.sh` expectations that
were verified untouched.

**`scripts/heavy-test-filter-lib.sh` and the `heavy` set were NOT touched**, per
R13-D14: `REIFY_GATE_EXCLUDE_HEAVY=1` excludes heavy at `role=merge` too, and
the offline lane exits 64 until task 7423 lands, so moving anything to the heavy
tier is loss, not delay.

**Other deliberate non-actions, each named in code where a future reader will
meet it:**

- INV-EVAL-4's corpus scope stays `examples/`-only. Widening it to the 34
  reify-eval fixtures is a coverage CHANGE that could surface fresh residuals —
  outside a zero-loss restructuring.
- The two failure policies stay un-harmonised: INV-EVAL-4 keeps its
  `REIFY_SNAPSHOT_CACHE_AUDIT_BYPASS` break-glass and its stale-residual FAIL;
  INV-EVAL-5 keeps neither. Harmonising either direction is a semantic change.
- `NON_PRISMATIC_MULTI_CASE_BODY_SOURCE` is built by two different tests in
  `solve_elastic_static_body_e2e.rs`. Merging them would undo the deliberate
  #4152 split AND serialise two OCCT+gmsh builds into one longer straggler — a
  CPU-second win paid for in makespan.
- The one two-build FEA test, `non_prismatic_two_case_build_realizes_body_exactly_once`,
  is irreducible: its second build IS the assertion (the one-case control the
  two-case delta is compared against), which is the shape-robust half of PRD B9.
