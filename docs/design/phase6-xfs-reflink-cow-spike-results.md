# Phase 6 XFS-reflink CoW Spike — Results Memo

**Spike date:** 2026-06-17  
**Task:** #4641  
**Status:** DONE — Q1 + Q2 measured on a quiesced box; verdict below.  
**Spec reference:** `docs/design/warmer-builds-merge-verify.md` §10 (§10.2 path-sensitivity vectors a/b/c; §10.3 XFS-reflink mechanics; §10.7 value-gating Q1/Q2)

---

## 0. TL;DR

- **Q1 — does a seeded CoW lane skip the rebuild? YES, and the win is large.** A merge-gate `verify.sh all --profile both --scope all` on a CoW-seeded lane ran in **9 min 31 s** vs **22 min 29 s** cold (sccache-warm, no CoW) for the identical delta and identical test work — **~58 % off total wall-clock, ~70 % (~12.6 min) off the compile/link phase**. CoW let cargo skip ~900 of ~940 unit compiles entirely.
- **Mechanism is sound and proven path-independent.** A reflink clone faithfully preserves cargo's whole `.fingerprint/` + artifact cache, and cargo's freshness/metadata hash does **not** embed the absolute build path — a bare build produces a byte-identical fresh/miss profile whether run in the original seed path or in a renamed CoW clone. Warmth genuinely transfers across the lane-path boundary.
- **Q2 — XFS-reflink reset-in-place is safe.** Over 5 reset-in-place cycles, fragmentation did not accumulate (relinked binary plateaued at 2 extents; rlibs stayed at 1; untouched files stayed CoW-shared), there was no space leak (allocated size + free space flat), and per-cycle wall-clock did not drift.
- **§10.7 recommendation: PROMISING — Phase 6 is worth pursuing.** The win clears the "beat sccache-alone" bar by a wide margin and the CoW substrate is mechanically stable.

---

## 1. Host Context

| Item | Value |
|---|---|
| Date (UTC) | 2026-06-17 |
| nproc | 32 |
| Q1 measurement environment | **quiesced** — reify + my-solar-challenge orchestrators + all watchdog timers stopped; CPU pressure avg10 ≈ 0.2–8.5 % during the runs (vs the 42.86 % contamination that spoiled the first attempt) |
| Q2 measurement environment | busy box (orchestrators restarted) — valid because fragmentation is load-independent; per-cycle wall-clock is load-contaminated and reported as secondary |
| XFS mount | `/media/leo/data_lv_1/leo/reify-build/xfs-spike` |
| XFS device | loopback over `/var/lib/reify-xfs-spike.img` (loop device id varies across remounts: `/dev/loop13` this session) |
| XFS features | `reflink=1`, `bigtime=1` |
| XFS capacity | 400 GiB total |
| sccache version | 0.14.0 (cache dir `/home/leo/.cache/sccache`, 100 GiB) |
| sccache hit rate (during the Q1 builds) | 64.31 % Rust |
| Rust toolchain | stable cargo 1.96 (mtime-based freshness; `-Z checksum-freshness` is nightly-only, §10.2.3) |
| Seed main HEAD | `139a1522f1` — Merge task/4318 into main |

### Spike paths

| Symbol | Path | Role |
|---|---|---|
| P_seed | `…/xfs-spike/seed` | warm `target/` reference (built by the merge-gate `verify.sh` command) |
| P_q1seeded | `…/xfs-spike/q1-seeded` | CoW clone of seed + delta → Q1 seeded measurement |
| P_q1cold | `…/xfs-spike/q1-cold` | CoW clone of seed with `target/` removed + delta → Q1 cold control |
| P_q2lane | `…/xfs-spike/q2-lane` | CoW clone of seed, reset-in-place over cycles → Q2 |
| Scratch log dir | `…/xfs-spike/logs/` | raw logs + timing + results |

P_seed and the lanes are **distinct paths** (load-bearing for a valid Q1 answer — see §10.2 vector-b rationale; confirmed harmless in §6.1).

---

## 2. Method

### Representative delta

- **File:** `crates/reify-gcode/src/lib.rs` — appended a trailing `// spike-delta` line comment (valid anywhere; changes the source so cargo treats reify-gcode as dirty).
- **Reverse-dependency closure under `--all-targets --profile both`:** reify-gcode's dependents *including dev-dependency edges from test targets* (reify-stdlib + the test targets of crates that dev-depend on gcode). This is a deliberately representative, mid-size closure — not a single crate — so the warmth signal reflects a realistic merge-lane delta.

### mtime normalization

`2020-01-01T00:00:00` stamped on all checked-out sources (excluding `target/` and `.git`) so sources are older than the seeded artifacts; the delta file is then `touch`-ed to now so cargo recompiles exactly its closure. `git-restore-mtime` is not installed; a plain `find … -exec touch` is sufficient and was verified (newest source = 2020, oldest artifact = 2026).

### Path stabilization (§10.2)

- **Vector-a (RUSTFLAGS `--remap-path-prefix`) NOT applied — by design.** The seed used `RUSTFLAGS=""`. Adding a path-specific remap in a lane build changes the RUSTFLAGS string, which invalidates all cargo fingerprints and misses every sccache entry → a full cold recompile regardless of CoW. So all builds here use consistent `RUSTFLAGS=""`. This turned out to be **unnecessary anyway**: §6.1 proves cargo's freshness hash is already path-independent, so vector-a is not needed for warmth to transfer.
- **Vector-b (fixed lane path ≠ seed path):** addressed structurally and **proven benign** in §6.1.
- **Vector-c (mtime):** applied as above.

### Build command (identical for seed, Q1 seeded, Q1 cold — the real merge gate)

```bash
env -u RUSTFLAGS RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 DF_VERIFY_ROLE=task \
  REIFY_TEST_SEMAPHORE_DISABLE=1 REIFY_COMPILE_GATE_DISABLE=1 \
  /usr/bin/time -v scripts/verify.sh all --profile both --scope all
```

(Admission gates disabled because the Q1 box is quiesced and we are the only consumer — they would only add wall-clock noise.)

---

## 3. CoW Clone Mechanics (§10.3)

`cp -a --reflink=always seed <lane>` completed in **4–5 s** for the full ~72 GB tree. `filefrag -v` confirms seed and clone share the same physical extents (`shared` flag) immediately after clone — deltas-only on disk, near-instant.

| Metric | Value |
|---|---|
| Clone wall-clock (72 GB tree) | 4.35–5.37 s |
| Post-clone allocated (du -sh) | 72 GB (shared extents) |
| Shared-extent flags in filefrag | present (`shared`) on every sampled artifact |

---

## 4. Step-4 / §6.1: Warmth-Transfer Mechanism — Does CoW + path-change defeat cargo freshness?

**Decisive control (load-independent, so valid even before quiescing):** run a bare `cargo build --workspace` (debug, `RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0`, `RUSTFLAGS=""`) **(a)** in the original `seed` path and **(b)** in a renamed CoW clone, with cargo's fingerprint trace on (`CARGO_LOG=cargo::core::compiler::fingerprint=info`), and compare.

| | bare build in `seed` (original path) | bare build in CoW clone (different path) |
|---|---|---|
| Units judged **Fresh** | 383 | 383 |
| reify-audit lib unit hash cargo sought | `reify-audit-498551377d43003e` | `reify-audit-498551377d43003e` (**identical**) |
| reify-eval lib unit hash cargo sought | `reify-eval-9a5a57053f0f6ed6` | `reify-eval-9a5a57053f0f6ed6` (**identical**) |

**Findings:**
1. **Cargo's metadata/freshness hash is path-independent.** Building in-place vs. in a renamed clone yields a byte-identical fresh/miss profile and the *same* requested fingerprint hashes. The renamed lane path is invisible to cargo's freshness decision. → **vector-b is benign**; warmth transfers across the path boundary.
2. **CoW is a faithful cache copy.** 383 units came up Fresh in the clone purely from the reflinked `.fingerprint/` + artifacts — cargo reused them with zero recompiles.
3. **The misses under the *bare* build were invocation mismatch, not CoW/path/mtime.** The dirty reason was uniformly `fingerprint error … failed to read .fingerprint/<hash>/<unit>` — i.e. the seed never materialized that exact unit-config (a bare `cargo build` is a different invocation than the seed's `verify.sh` clippy/test passes, which build different feature/target/profile permutations). Proven by the control: the *same* misses occur in-place with no clone at all. Under the matching `verify.sh` invocation (§6), the seeded lane recompiles only the delta closure.

> Corrects an earlier in-flight read ("22/30 crates rebuilt → warmth lost"): that was a contaminated run counting crate *names* across the debug + release + clippy + test passes (one crate legitimately appears as several distinct units). The path-independence control supersedes it.

---

## 5. Step-3: Path Stabilization Applied

| Item | Value |
|---|---|
| RUSTFLAGS (all builds) | `""` (no remap; vector-a deliberately omitted, shown unnecessary in §4) |
| mtime stamp | `2020-01-01T00:00:00` on sources; delta file `touch`-ed to now |
| Source dirs touched | lane `**` excluding `target/` and `.git` (verified: newest source 2020, oldest artifact 2026) |

---

## 6. Step-4/5: Q1 Decisive Measurement — Seeded vs Cold (quiesced box)

Both runs used the **identical** merge-gate command (§2) and produced the **identical test work** — 17 317 + 8 907 tests, all passing, exit 0 — so the wall-clock difference is purely compile/link savings.

| Run | Wall-clock | `Compiling` units (incl. deps) | reify crates compiled | Test execution | exit |
|---|---|---|---|---|---|
| **Cold + delta** (sccache-warm, empty `target/`) | **22:28.88** (1 348.9 s) | 940 | 31 (all) | 139.9 s + 120.4 s = 260 s | 0 ✓ |
| **Seeded + delta** (CoW clone of warm `target/`) | **9:30.62** (570.6 s) | 36 | 22 (delta `--all-targets` closure) | 117.8 s + 121.8 s = 240 s | 0 ✓ |

### Q1 derived numbers

| Quantity | Value |
|---|---|
| Total wall-clock saved | 1 348.9 − 570.6 = **778.3 s ≈ 13.0 min** |
| Total reduction | **57.7 %** |
| Non-test (compile + clippy + link) — cold | 1 348.9 − 260 = 1 088.5 s (≈ 18.1 min) |
| Non-test (compile + clippy + link) — seeded | 570.6 − 240 = 330.6 s (≈ 5.5 min) |
| **Compile/link phase saved** | **757.9 s ≈ 12.6 min (69.6 %)** |
| Units cargo skipped entirely via CoW | ~904 (940 → 36) |

**Interpretation.** sccache was warm (64 % Rust hit rate) for *both* runs, yet the cold gate still took 22.5 min: even with sccache hits, cargo must still spawn rustc, do an sccache lookup/decompress, run build scripts, and **link** for every one of ~940 units. CoW-seeding eliminates ~904 of those entirely (cargo judges them Fresh and skips them), which is the ~12.6 min compile/link win. **This is the unique value over the sccache-only baseline we already have**, and it is large.

### 6.1 vector-b confirmation

Confirmed benign — see §4 (path-independence control). cargo's `.fingerprint/**/dep-*` did **not** force a broad rebuild due to the fixed/renamed lane path.

---

## 7. Step-6: Q2 — XFS-reflink Fragmentation + Per-Cycle Performance (busy box)

5 reset-in-place cycles in a single CoW lane (path stable; each cycle re-applies a rotating reify-gcode delta → rebuild → measure). Fragmentation (`filefrag` extent count) is load-independent; the `wall_s` column is load-contaminated (orchestrators running, by design) and secondary.

| Cycle | wall (s) | reify bin extents | gcode rlib | stdlib rlib | control rlib (untouched) | lane du | df avail |
|---|---|---|---|---|---|---|---|
| 0 (clone, pre-build) | — | 1 | 1 | 1 | 1 | 72 G | 117 G |
| 1 | 86 | **2** | 1 | 1 | 1 | 73 G | 116 G |
| 2 | 12 | 2 | 1 | 1 | 1 | 73 G | 116 G |
| 3 | 11 | 2 | 1 | 1 | 1 | 73 G | 116 G |
| 4 | 9 | 2 | 1 | 1 | 1 | 73 G | 116 G |
| 5 | 10 | 2 | 1 | 1 | 1 | 73 G | 116 G |

### Q2 verdict — SAFE

- **No fragmentation accumulation.** The relinked binary jumps 1→2 extents on the first rebuild then **plateaus at 2** through cycle 5. Recompiled rlibs stay at **1 extent** (cargo writes each fresh artifact contiguously). The untouched control rlib stays at **1 extent and CoW-shared** the whole time.
- **No space leak.** Allocated size (72→73 G) and free space (117→116 G) are **flat** after the first divergence — reset-in-place frees and reuses old extents rather than leaking.
- **No per-cycle perf drift.** Cycle 1 (86 s) is the cold-ish first rebuild on a busy box; cycles 2–5 settle to ~10 s and do **not** climb. XFS reflink reset-in-place imposes no measurable degradation over cycles.

---

## 8. Verdict

### Q1: Does a seeded (mtime-normalized, fixed-path) CoW lane SKIP the rebuild?

**YES — and the win clears the §10.7 value bar by a wide margin.**

- Warmth transfers through CoW: a reflink clone faithfully preserves cargo's `.fingerprint/` + artifacts, and cargo's freshness hash is **path-independent** (§4 control — identical fresh/miss in-place vs. renamed clone). No vector-a remap needed; vector-b is benign; vector-c handled by mtime normalization.
- Magnitude: **22:29 → 9:31** for the full merge gate on an identical delta with identical passing test work — **~58 % total**, **~70 % (~12.6 min) on compile/link**, ~904 unit-compiles skipped.
- This is a genuine win **over sccache-alone** (sccache was warm in both runs): sccache shortcuts each rustc, but CoW lets cargo skip the rustc/link/build-script/fingerprint work for the unchanged ~96 % of units entirely.

### Q2: XFS-reflink fragmentation/perf over reset-in-place cycles

**SAFE.** Bounded fragmentation (binary ≤2 extents, rlibs 1, untouched files stay shared), no space leak, no wall-clock drift over 5 cycles.

### §10.7 gate recommendation

**PROMISING — Phase 6 (CoW-seeded warm merge-verify lanes) is worth pursuing.** Both gating questions resolve positively: the mechanism works and is path-robust, the wall-clock win is large and is *additive* to the sccache baseline, and the CoW substrate is operationally stable under repeated reset-in-place. Phase 6 design notes:
- Keep `RUSTFLAGS` **consistent** between seed and lane (here: `""`). Path-prefix remap (vector-a) is **not** required and should be avoided (it would invalidate the very fingerprints CoW preserves).
- The seed must be built with the **same** `verify.sh` invocation the lane will run, so the materialized unit-configs match (a different invocation re-materializes its own units — see §4 invocation-mismatch).
- Reset-in-place lane reuse is viable without periodic defrag/reclone within at least 5 cycles; revisit at higher cycle counts if a lane is reused heavily.

---

## 9. Appendix: Raw Timing Extracts

### Seeded build (`logs/q1-seeded-time.txt`)
```
Command: scripts/verify.sh all --profile both --scope all
Elapsed (wall clock): 9:30.62      Percent of CPU: 1760%      Exit: 0
Tests: 17317 passed (117.821s) + 8907 passed (121.770s)
Units compiled: 36   reify crates compiled: 22
```

### Cold + delta build (`logs/q1-cold-time.txt`)
```
Command: scripts/verify.sh all --profile both --scope all  (empty target/, sccache warm)
Elapsed (wall clock): 22:28.88     Percent of CPU: 781%      Exit: 0
Tests: 17317 passed (139.918s) + 8907 passed (120.440s)
Units compiled: 940  reify crates compiled: 31
```

### Path-independence control (`logs/diag-fingerprint.log`, `logs/seed-bare-control.log`)
```
bare `cargo build --workspace` — Fresh units: 383 (in-place) == 383 (CoW clone)
requested unit hashes identical across paths (e.g. reify-audit-498551377d43003e)
=> cargo freshness hash is path-independent; misses are invocation-mismatch, not CoW.
```

### Q2 (`logs/q2-results.txt`)
```
cycle wall_s reify_bin_ext gcode_rlib_ext stdlib_rlib_ext ctrl_ext lane_du df_avail
0     -      1             1               1               1        72G     117G
1     86     2             1               1               1        73G     116G
2     12     2             1               1               1        73G     116G
3     11     2             1               1               1        73G     116G
4     9      2             1               1               1        73G     116G
5     10     2             1               1               1        73G     116G
```
