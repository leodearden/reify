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
- **Run-to-run spread is large and is the headline caveat.** Three runs of
  `corpus_sweep_shard_20` measured **184.9s, 238.4s and 158.2s**. Treat
  **158–240s** as the observed band; no single run is a point value. Notably the
  FASTEST of the three was the run with the MOST concurrent load (all three
  affected binaries together, 151 tests), so the spread is not a simple function
  of host contention — scheduling order matters at least as much.
- This materially weakens the plan's stated basis for leaving
  `.config/nextest.toml` alone — see §4.
  (Settled by task #7564 with later measurement — see §5.)

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
  `tensegrity_t0a` LPT tier-1 straggler". **The measured straggler STRADDLES
  that floor rather than sitting far below it** — 158.2s / 184.9s / 238.4s across
  three runs, i.e. below it once and above it twice. The plan's premise is not
  simply wrong, but it is not established either, and ~61s is certainly not the
  right input. On this evidence a `priority` entry for `harness_corpus_gates` is
  a lever that must be settled by a real Summary-wall measurement rather than
  ruled out a priori.
- The slow-timeout argument is weaker than stated but still holds: 158–240s
  against the inherited `[profile.default]` 1200s ceiling is ~5–7.6× headroom.
  It is worth noting that at the 8× contention of esc-5097-3 a 238s nominal
  would reach ~1900s and breach that ceiling, so this is not unlimited margin.

These are recorded as follow-ups, not acted on here (settled by task #7564 —
see §5) — editing that file is
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

  Provenance: task #7560 widened INV-EVAL-4 to the full union and made a stale
  residual fatal for both invariants, so the code no longer names those two
  non-actions; the bypass asymmetry (INV-EVAL-4 only) was kept, and
  `InvariantGate::bypass_env` still names it.
- `NON_PRISMATIC_MULTI_CASE_BODY_SOURCE` is built by two different tests in
  `solve_elastic_static_body_e2e.rs`. Merging them would undo the deliberate
  #4152 split AND serialise two OCCT+gmsh builds into one longer straggler — a
  CPU-second win paid for in makespan.
- The one two-build FEA test, `non_prismatic_two_case_build_realizes_body_exactly_once`,
  is irreducible: its second build IS the assertion (the one-case control the
  two-case delta is compared against), which is the shape-robust half of PRD B9.

---

## 5. Settlement of the nextest re-tune (task #7564, 2026-11)

§4 left two levers open: an LPT `priority` entry for `harness_corpus_gates`,
and re-tuning `CORPUS_SHARD_COUNT` from measurement. **Both settle as NO
behaviour change.** There is no priority entry, and the count stays 24, now
re-derived from measured per-file cost rather than inherited. §1–§4 above are
#7431's dated record and are left as written. Where they disagree with this
section, this section is the later measurement.

### 5.1 Gate-run evidence

Two real verify-gate runs, read from nextest's JUnit report. Per-test start =
the testcase `timestamp` minus the testsuites `timestamp`; end = start + `time`.
Slack = makespan − straggler end, computed from the unrounded figures.

| run | role / scope | tests | exec | sweep start window | straggler: duration, end | makespan-setter: start, duration, end | slack |
|---|---|---|---|---|---|---|---|
| 2026-10-20T03:14:36+01:00, `_mainsweep-cdbb14ff` | background, full scope | 26716 | 1240.3s | 500.7–501.7s | `corpus_sweep_shard_05`: 123.2s, ends 624.0s | `solve_elastic_static_body_e2e::non_prismatic_two_case_build_realizes_body_exactly_once`: 653.4s, 586.9s, ends 1240.3s | **616.3s** |
| 2026-11-02T10:05:57Z, `_lane-12` | task, branch scope | 7480 | 609.3s | 175.6–177.6s | `corpus_sweep_shard_05`: 136.7s, ends 312.6s | the same test: 336.8s, 272.5s, ends 609.3s | **296.6s** |

Both reports were read from
`/home/leo/src/warm-lanes/worktrees/<lane>/target/nextest/default/reify-confirm.xml`.
Lane targets are ephemeral, so this is not an archive: a 1304-test run
overwrote `_lane-12`'s report on 2026-11-03.

The other heavy shards in the same two runs (full / branch): `shard_02` 55.6s /
66.0s, `shard_09` 44.9s / 49.2s, `shard_14` 25.8s / 34.4s, `shard_03` 10.5s /
16.3s. Every other shard took ≤ 8.7s. #7431's straggler `shard_20` took 1.6s /
2.2s.

### 5.2 Per-file cost

`diag_per_file_timing` evaluates every corpus file once, sequentially, in one
process. It ran twice over the same 293 evaluated files:

- **Run A** (planning): `_lane-19` at b31c164e63, with a throwaway patch
  that printed each file's shard (since reverted). Host loadavg ~130–216 on 32
  cores.
- **Run B**: `_lane-19` at b31c164e63 plus #7564's change that makes the
  diagnostic print shards itself, 2026-11-02T16:11Z. Loadavg 87.2 at start,
  60.8 at end.

Both runs count evaluated files only. A file that fails the sweep's compile
check is skipped, and shard 20 holds one such file that is likely heavy once it
compiles again (§5.5). The heavy set and the N table below are for the corpus
as evaluated today.

Heavy files (≥ 5s in either run):

| file | shard | run A | run B |
|---|---|---|---|
| `examples/buckling_multi_case_smoke.ri` | 05 | 120.86s | 69.11s |
| `examples/buckling_column_smoke.ri` | 02 | 51.76s | 38.19s |
| `examples/modal/cantilever_beam_modes.ri` | 14 | 38.17s | 12.90s |
| `examples/modal/simply_supported_beam_modes.ri` | 09 | 32.92s | 12.94s |
| `examples/trajectory/tots_optimal_ptp.ri` | 09 | 10.83s | 6.03s |
| `examples/buckling_column_p2.ri` | 03 | 5.38s | 6.74s |
| **total, all 293 evaluated files** | | **281.9s** | **160.5s** |

The next file in run B is `examples/anisotropic_bar.ri` at 2.49s (shard 21).

Shard sums at N = 24:

| | run A | run B |
|---|---|---|
| straggler | shard 05: 121.27s over 6 files | shard 05: 69.33s |
| floor = costliest single file | 120.86s | 69.11s |
| straggler − floor | 0.41s | 0.23s |
| next shards | not recorded | 02: 38.53s; 09: 19.44s; 14: 13.10s; 03: 8.29s; the rest ≤ 2.65s |
| shard 20 | 0.43s over 13 files | 0.18s |

What differs between the runs: absolute cost was ~43% lower in run B, which
ran at lower load, and not uniformly. The two modal files fell ~3× while `buckling_multi_case_smoke.ri`
fell 1.75×, so the order inside the modal pair flipped, and
`buckling_column_p2.ri` overtook `tots_optimal_ptp.ri`. What does not differ:
the same six files form the heavy set, shard 05 is the straggler, and that
straggler is bound by one file. Both runs are recorded; neither replaces the
other.

Max shard sum by N. This was computed during PLANNING, from run A's per-file
figures and each file's printed xxh3-128 key, so it uses the same key the sweep
ships. The step-1 diagnostic reproduces only the N = 24 column.

| N | 4 | 6 | 8 | 12 | 16 | 20 | 24 | 32 | 48 |
|---|---|---|---|---|---|---|---|---|---|
| max shard sum (run A) | 170.2s | 123.6s | 124.1s | 121.8s | 121.2s | 132.1s | 121.3s | 121.1s | 120.9s |

The gate-run walls (§5.1) and run B agree on WHICH shards are heavy, in the
order 05 > 02 > 09 > 14 > 03. Run A's per-file figures imply the same order. A
standalone `cargo nextest run -p reify-eval --test harness_corpus_gates` on
run B's tree (46 tests; loadavg 295.5 shortly before it started) finished in a
70.2s Summary wall, set by `corpus_sweep_shard_05` at 70.199s.

### 5.3 Part 1 — no LPT `priority` entry for `harness_corpus_gates`

An LPT `priority` changes only when a test STARTS. It can shorten a run only
for a test whose end sets, or nearly sets, the makespan. In both gate runs the
sweep's straggler ended long before the makespan: 616.3s early in the
full-scope run and 296.6s early in the branch-scope run. Starting it earlier
cannot move the end of the run.

This holds even at #7431's worst isolated straggler of 240s. Taking the latest
shard start in each run, the straggler would end at 177.6 + 240 = 417.6s <
609.3s (branch) and 501.7 + 240 = 741.7s < 1240.3s (full scope). That bound
also covers the skipped `shard_20` file of §5.5 rejoining the sweep, if it
costs what #7431's `shard_20` did.

Both candidate entries are worse than none:

- **Whole-binary priority** would start all 24 shards, plus the binary's other
  tests, ahead of other `occt`-group work. Twenty of the shards take ≤ 8.7s, so
  for most of what it moves this is anti-LPT, and it buys no makespan.
- **A priority on the straggler shard alone** is brittle. The straggler moved
  from `shard_20` to `shard_05` within six weeks without any file changing shard
  (§5.5), so a pinned shard index goes stale without any signal.

§4 asked for this lever to be "settled by a real Summary-wall measurement
rather than ruled out a priori". The two gate reports in §5.1 are that
measurement, taken under real gate contention. They are stronger evidence than
a synthetic A/B run of a 20–40 minute suite would be, given the run-to-run
spread §1 documents. `.config/nextest.toml`'s LPT comment block points here.

### 5.4 Part 2 — `CORPUS_SHARD_COUNT` stays 24

**The rule.** Per-file cost is heavy-tailed: in both diagnostic runs six files
carry most of the cost. A shard's wall time is therefore set by the costliest
file it drew, and no shard count can push the straggler below the single
costliest file. The count only has to keep the heavy files in separate shards.

At N = 24 the straggler sits within 0.41s (run A) and 0.23s (run B) of that
floor. In the N table, N = 16 and N ≥ 32 are lower, each by ≤ 0.4s, and
N = 20 is worse (132.1s).

Why not change it anyway:

- **Lowering N** saves only per-shard fixed cost: process spawn plus native
  dynamic link. §2 measured a bare `reify` spawn at 0.194s, dominated by
  OCCT/OpenVDB linking. In exchange it raises the chance (1/N per new file) that
  a future heavy file lands beside an existing one.
- **Any change to N** reassigns every file's shard. That invalidates every
  recorded "shard NN" reproduction, which is the insert-stability #7431
  shipped, and buys nothing measurable.
- **Raising N** buys ≤ 0.4s on a test that ends 297–616s ahead of the makespan.
- **The hard-kill margin is not a constraint today.** The measured stragglers
  of 121–137s sit against the inherited `[profile.default]` slow-timeout of
  1200s (period 120s × terminate-after 10), about 8.8× headroom. That is for the
  corpus as evaluated today. If #7431's 158–240s `shard_20` returns with the
  file §5.5 names, headroom falls back to §4's 5–7.6×, which §4 notes the
  esc-5097-3 8× contention case would breach.

**Revisit when** any of these happens:

- A gate JUnit report shows the sweep's straggler ending near the makespan.
- A shard's sum exceeds the single-file floor because a second heavy file
  landed in it. The §5.7 diagnostic shows this directly: compare the top
  `DIAG-SHARD` sum with the costliest `DIAG` line.
- `examples/trajectory/printer_print_envelope.ri` compiles clean in reify-eval's
  sweep again, which is expected when #5847 dimensions its two TOTSShaper limit
  sites (§5.5). It then drops out of the `DIAG-SKIP` lines. Re-run the
  §5.7 diagnostic, re-check the floor and which shard is the straggler, and
  re-check the hard-kill headroom against §4's esc-5097-3 8× contention case.

### 5.5 Observation: the straggler moved

#7431 measured `corpus_sweep_shard_20` as the straggler at 158–240s (§1). Now
`shard_20` takes 1.6s / 2.2s in the gate runs and sums to 0.43s / 0.18s in the
diagnostic runs, and `shard_05` is the straggler.

Hash keying never moves an unchanged file between shards
(`shard_of_is_independent_of_corpus_membership`). No `.ri` file under the
corpus roots was deleted or renamed between #7431's first commit (a756bd32f8,
2026-09-17) and run B (`git log --diff-filter=DR`), and
`buckling_multi_case_smoke.ri` has existed since 2026-06-16. So no file left
`shard_20`'s partition. What changed is which of its files the sweep EVALUATES.

Observed in `_lane-19` on 2026-11-04, on #7564's branch, which changes no sweep
behaviour over b31c164e63:

- `corpus_sweep_shard_20` prints `13 of 15 file(s) evaluated, 2 skipped
  (compile errors)`. §1's histogram gives shard 20 15 files, and §5.2's
  diagnostic runs counted 13. The extended §5.7 diagnostic lists the same two
  as `DIAG-SKIP ... shard 20`.
- One skip is `examples/auto/bearing_unsat.ri`. It fails to compile by design
  (no candidate satisfies its constraint) and has been in `examples_smoke`'s
  `SKIP_SET` since it was added on 2026-06-16, so #7431's runs skipped it too.
- The other is `examples/trajectory/printer_print_envelope.ri`, in the corpus
  since 2026-06-06. It runs a gantry modal analysis, TOTS shaping and
  `simulate_trajectory` for three variants: the cost classes of the modal and
  TOTS files in §5.2's heavy set.
- `reify check` on that file reports exactly two errors: its TOTSShaper
  `velocity_limit` and `acceleration_limit` arguments are bare `Real` where the
  params require velocity and acceleration. Those sites became errors with
  #5306's δ step 5 (9b179b3f3d, `feat(5306)!: flip
  CTOR_FIELD_CONFORMANCE_SEVERITY Warning -> Error`). That commit reached main
  through e90a25ef5c (`Merge task/5306 into main`, 2026-09-25), and
  `git merge-base --is-ancestor 9b179b3f3d 5e822de99a` is false, so #7431's
  merge (2026-09-17 09:31) predates it.
- `examples_smoke` waives exactly those two sites (`is_migration_debt_diagnostic`,
  a debt owned by #5847). The reify-eval sweep has no such waiver, so it
  skips the whole file.

**Hypothesis:** #7431's runs evaluated `printer_print_envelope.ri`, and its
evaluation was most of `shard_20`'s 158–240s. If so, the straggler moved
because the sweep stopped evaluating that file. That is a coverage change, not
a per-file cost change. The file will then make `shard_20` heavy again when
#5847 lands. At N = 24 it would be the only heavy file in `shard_20`, so it
would not stack on `shard_05`'s cost. §5.4 carries this as a revisit trigger.

Not measured: the file's evaluation cost, since the sweep cannot evaluate it
today, and whether `buckling_multi_case_smoke.ri` got dearer.

### 5.6 Out of scope, observed

`tensegrity_t0a` is LPT tier 1 on the premise that it is a ">180s straggler".
Its 13 tests all started at t = 0.16–0.17s in the full-scope run, and the
longest took 1.96s. The makespan-setter in both gate runs is
`solve_elastic_static_body_e2e::non_prismatic_two_case_build_realizes_body_exactly_once`,
which carries no priority. Re-deriving the LPT tiers means re-pointing
`tests/infra/test_nextest_slow_priority.sh` Assertions A/C/D/D2/E/I, which is
outside #7564's file set. It was filed as a follow-up from task #7564 (ticket
`tkt_0RWH2BTJTPKJ6JK84C00MHN0NR`). No task number is cited here because the
curator assigns it asynchronously.

### 5.7 Re-measure recipe

Per-file and per-shard cost, sequential in one process (160–282s of test time
in the two §5.2 runs, on top of the build; redirect to a file and read it,
never pipe):

```
cargo test -p reify-eval --test harness_corpus_gates -- --ignored --exact \
  eval_invariant_corpus_sweep::diag_per_file_timing --nocapture > /tmp/diag.log 2>&1
```

It prints the 40 costliest evaluated files as
`DIAG <duration> shard <NN> <file>`, then every file the sweep skips for a
compile error as `DIAG-SKIP <duration> shard <NN> <file>`, then a `DIAG-TOTAL`
line, then every shard ranked by summed cost as `DIAG-SHARD <NN> <sum>`. A
skip's duration is its compile time, which a gate shard pays too, so the shard
sums include it. On 2026-11-04 there were 10 skips, each ≤ 0.11s. A `DIAG-SKIP`
line is a file the sweep does not cover yet; §5.4 names the one to watch.
Record `/proc/loadavg` alongside: absolute figures
differed ~1.8× between the two §5.2 runs, which ran at different host load, so
compare rankings and the straggler − floor gap, not raw seconds.

Gate-run walls come from nextest's JUnit report at
`<lane>/target/nextest/default/reify-confirm.xml`, which
`scripts/gen-nextest-config.sh` configures. A run's last nextest step (usually
the GUI pass) overwrites that file, so workspace-scope reports survive mostly in
the `_mainsweep-*` lanes. Derive each test's start as the testcase `timestamp`
minus the testsuites `timestamp`, as in §5.1.
