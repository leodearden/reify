# Phase 6 XFS-reflink CoW Spike — Results Memo

**Spike date:** 2026-06-17  
**Task:** #4641  
**Status:** IN PROGRESS — measurements being collected  
**Spec reference:** `docs/design/warmer-builds-merge-verify.md` §10 (§10.2 path-sensitivity vectors a/b/c; §10.3 XFS-reflink mechanics; §10.7 value-gating Q1/Q2)

---

## 1. Host Context

| Item | Value |
|---|---|
| Date (UTC) | 2026-06-17T14:09:37Z |
| nproc | 32 |
| CPU pressure avg10 (at spike start) | 42.86 % |
| XFS mount | `/media/leo/data_lv_1/leo/reify-build/xfs-spike` |
| XFS device | `/dev/loop30` (loopback-mounted file) |
| XFS features | `reflink=1`, `bigtime=1`, `rmapbt=1` |
| XFS capacity | 400 GiB total, ~393 GiB free |
| sccache version | 0.14.0 |
| sccache cache dir | `/home/leo/.cache/sccache` (100 GiB) |
| sccache hit rate (pre-seed) | 64.36 % (25 930 Rust hits / 15 716 Rust misses) |
| Rust toolchain | stable cargo 1.96 (mtime-based freshness; `-Z checksum-freshness` is nightly-only, §10.2.3) |
| main HEAD | `139a1522f1` — Merge task/4318 into main |

### Spike paths

| Symbol | Path |
|---|---|
| P_seed | `/media/leo/data_lv_1/leo/reify-build/xfs-spike/seed` |
| P_lane | `/media/leo/data_lv_1/leo/reify-build/xfs-spike/lane` |
| Scratch log dir | `/media/leo/data_lv_1/leo/reify-build/xfs-spike/logs/` |

P_seed and P_lane are DISTINCT paths (design decision: load-bearing for a valid Q1 answer — see §10.2 vector-b rationale).

---

## 2. Method

### Representative delta

- **File:** `crates/reify-gcode/src/lib.rs` (line 1 — first `//!` doc-comment line)
- **Change:** Replace the opening line with an updated doc comment (adds `// spike-delta` marker)
- **Reverse-dependency closure:** `reify-gcode` → `reify-stdlib` only (1 reverse dep; low fanout gives sharp warmth/no-warmth signal)
- **Rebuild closure expected if warm:** 2 crates (`reify-gcode` + `reify-stdlib`) + linker steps only

### mtime normalization timestamp

`2020-01-01T00:00:00` — stamped on all checked-out sources in P_seed BEFORE build (so sources are older than seeded artifacts), and re-applied in P_lane after CoW clone (so seeded artifacts are newer than sources in the lane).

### Path stabilization (§10.2)

- **Vector-a fix:** `RUSTFLAGS=--remap-path-prefix=${P_lane}=/spike-lane` (normalizes embedded debuginfo paths)
- **Vector-b (tested):** fixed lane path `P_lane` (structural fix; this is exactly what Q1 tests — see §4)
- **Vector-c fix:** `find <src> -type f -exec touch -d 2020-01-01 {} +` before each build

### Build command

```bash
/usr/bin/time -v env \
  RUSTC_WRAPPER=sccache \
  CARGO_INCREMENTAL=0 \
  REIFY_TEST_SEMAPHORE_DISABLE=1 \
  REIFY_COMPILE_GATE_DISABLE=1 \
  scripts/verify.sh all --profile both --scope all
```

Exact commands with paths are in §3–§5 below.

---

## 3. Step-1: Seed Build (Cold-from-Empty Reference)

**Seed path:** P_seed = `/media/leo/data_lv_1/leo/reify-build/xfs-spike/seed`  
**Purpose:** Build a complete warm `target/` at P_seed (exact merge-gate build, §1.4). Serves as the cold-from-empty full-build reference datum and the source for the CoW clone.

```bash
cd /media/leo/data_lv_1/leo/reify-build/xfs-spike/seed
# mtime-normalize sources OLDER than build start to avoid spurious rebuilds
find . -path ./target -prune -o -type f -print -exec touch -d 2020-01-01 {} +
# Run full build
/usr/bin/time -v env \
  RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 \
  REIFY_TEST_SEMAPHORE_DISABLE=1 REIFY_COMPILE_GATE_DISABLE=1 \
  scripts/verify.sh all --profile both --scope all \
  2>&1 | tee /media/leo/data_lv_1/leo/reify-build/xfs-spike/logs/seed-build.log
```

### Results

| Metric | Value |
|---|---|
| Cold-from-empty wall-clock | **PENDING** |
| verify.sh exit code | PENDING |
| `du -sh target/` post-build | PENDING |
| `du -sh target/debug` | PENDING |
| `du -sh target/release` | PENDING |
| sccache Rust hits delta | PENDING |
| sccache Rust misses delta | PENDING |
| Sample artifact filefrag extents (baseline) | PENDING |

---

## 4. Step-2: CoW Clone P_seed → P_lane

**Purpose:** Clone the seed at a DISTINCT path (P_lane) using XFS reflink CoW. Proves shared-extent deltas-only disk claim (§10.3).

```bash
time cp -a --reflink=always \
  /media/leo/data_lv_1/leo/reify-build/xfs-spike/seed \
  /media/leo/data_lv_1/leo/reify-build/xfs-spike/lane
```

### Verification commands

```bash
# Allocated (CoW sees only the P_lane header, not the shared blocks)
du -sh /media/leo/data_lv_1/leo/reify-build/xfs-spike/lane
# Logical (full apparent size — same as P_seed)
du -sh --apparent-size /media/leo/data_lv_1/leo/reify-build/xfs-spike/lane
# Shared-extent proof on a sample artifact
filefrag -v /media/leo/data_lv_1/leo/reify-build/xfs-spike/seed/target/release/reify \
  /media/leo/data_lv_1/leo/reify-build/xfs-spike/lane/target/release/reify
```

### Results

| Metric | Value |
|---|---|
| Clone wall-clock | **PENDING** |
| P_lane allocated (du -sh) | PENDING |
| P_lane apparent size (du -sh --apparent-size) | PENDING |
| P_seed target/ du -sh | PENDING |
| Shared-extent flags in filefrag output? | PENDING |

---

## 5. Step-3: Path Stabilization in P_lane

**Purpose:** Apply §10.2 path-stabilization so the seeded lane build only recompiles the delta + its reverse-dep closure (not everything due to stale path embedding).

```bash
# (a) Vector-a: RUSTFLAGS remap — set as env for all lane builds
export RUSTFLAGS="--remap-path-prefix=/media/leo/data_lv_1/leo/reify-build/xfs-spike/lane=/spike-lane"

# (c) Vector-c: mtime-normalize P_lane sources OLDER than seeded artifacts
find /media/leo/data_lv_1/leo/reify-build/xfs-spike/lane \
  -path '*/target*' -prune -o \
  -type f -print \
  -exec touch -d '2020-01-01' {} +
```

### Notes

- **Vector-b** is addressed structurally by P_lane being a different path from P_seed; step-4 tests whether the fixed-path approach succeeded (Q1 core question).
- P_seed seed build was built WITHOUT `--remap-path-prefix`, so there is an asymmetry between seed and lane remap; this is DOCUMENTED and acceptable — the lane remap only affects newly compiled artifacts in the lane, and the fingerprints/dep-info with embedded P_seed paths are exactly what Q1 tests.

### Applied

| Item | Value |
|---|---|
| RUSTFLAGS (lane) | `--remap-path-prefix=/media/leo/data_lv_1/leo/reify-build/xfs-spike/lane=/spike-lane` |
| mtime stamp | `2020-01-01T00:00:00` |
| Source dirs touched | `P_lane/**` (excluding `target/`) |

---

## 6. Step-4: Q1 Decisive Measurement — Seeded Build

**Purpose:** Apply the representative delta in P_lane, run the verify build with path-stabilization, measure seeded wall-clock, and determine whether path-vector (b) forced a broad rebuild.

```bash
# Apply delta
cd /media/leo/data_lv_1/leo/reify-build/xfs-spike/lane
# One-line edit to crates/reify-gcode/src/lib.rs (line 1)
# [see representative delta §2]
touch -d 'now' crates/reify-gcode/src/lib.rs  # mark as newer than seeded artifacts

# Run seeded build
/usr/bin/time -v env \
  RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 \
  RUSTFLAGS="--remap-path-prefix=/media/leo/data_lv_1/leo/reify-build/xfs-spike/lane=/spike-lane" \
  REIFY_TEST_SEMAPHORE_DISABLE=1 REIFY_COMPILE_GATE_DISABLE=1 \
  scripts/verify.sh all --profile both --scope all \
  2>&1 | tee /media/leo/data_lv_1/leo/reify-build/xfs-spike/logs/seeded-build.log

# Inspect dep-info after seeded build
find /media/leo/data_lv_1/leo/reify-build/xfs-spike/lane/target \
  -name 'dep-*.d' -path '*/reify-gcode*' | head -5 | xargs -I{} sh -c 'echo "=== {} ==="; cat "{}"'
```

### Results

| Metric | Value |
|---|---|
| Seeded wall-clock | **PENDING** |
| verify.sh exit code | PENDING |
| Crates recompiled (from build log) | PENDING |
| Path-vector (b) finding: did P_seed paths appear in dep-info? | PENDING |
| Warmth assessment | PENDING (HELD / PARTIALLY HELD / LOST) |

---

## 7. Step-5: Q1 Cold Control

**Purpose:** Establish the cold reference for the SAME delta from an empty target/ at a fixed path on the mount. Apples-to-apples comparison.

```bash
# Cold path
P_COLD="/media/leo/data_lv_1/leo/reify-build/xfs-spike/cold"
git -C /home/leo/src/reify worktree add --detach "$P_COLD" 139a1522f1
find "$P_COLD" -path '*/target*' -prune -o -type f -print -exec touch -d '2020-01-01' {} +
# Apply same delta
# [same one-line edit to crates/reify-gcode/src/lib.rs]
touch -d 'now' "$P_COLD/crates/reify-gcode/src/lib.rs"

cd "$P_COLD"
/usr/bin/time -v env \
  RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 \
  REIFY_TEST_SEMAPHORE_DISABLE=1 REIFY_COMPILE_GATE_DISABLE=1 \
  scripts/verify.sh all --profile both --scope all \
  2>&1 | tee /media/leo/data_lv_1/leo/reify-build/xfs-spike/logs/cold-build.log
```

**Note:** The seed build (step-1/§3) already provides the cold-from-empty datum for the full build. This cold+delta control isolates the per-delta cold cost.

### Results

| Metric | Value |
|---|---|
| Cold+delta wall-clock | **PENDING** |
| Crates compiled (count) | PENDING |
| verify.sh exit code | PENDING |

### Q1 Comparison

| Run | Wall-clock | Crates recompiled | delta vs cold |
|---|---|---|---|
| Cold-from-empty (step-1) | PENDING | all | — |
| Cold+delta (step-5) | PENDING | PENDING | n/a |
| Seeded+delta (step-4) | PENDING | PENDING | **PENDING (the number)** |
| **Seeded saving** | — | — | **PENDING (%)** |

---

## 8. Step-6: Q2 XFS-reflink Fragmentation + Per-Cycle Performance

**Purpose:** Characterize fragmentation and wall-clock drift over N≈5–10 reset-in-place cycles in P_lane (path stays stable, mtimes move only on changed files).

### Method (per cycle)

```bash
cd /media/leo/data_lv_1/leo/reify-build/xfs-spike/lane
# 1. Advance delta (rotate through 5 commits or re-apply same delta)
git reset --hard <next_delta_commit>
# 2. Re-stamp changed sources to mtime=old
touch -d '2020-01-01' <changed files>
# 3. Mark changed file as newer than artifacts
touch -d 'now' crates/reify-gcode/src/lib.rs
# 4. Build
/usr/bin/time -v env RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 \
  RUSTFLAGS="--remap-path-prefix=..." \
  REIFY_TEST_SEMAPHORE_DISABLE=1 REIFY_COMPILE_GATE_DISABLE=1 \
  scripts/verify.sh all --profile both --scope all
# 5. Measure fragmentation
filefrag /media/leo/data_lv_1/leo/reify-build/xfs-spike/lane/target/release/reify 2>&1 | tail -1
```

### Per-cycle results

| Cycle | Delta | Wall-clock | reify binary extents | libstd.rlib extents |
|---|---|---|---|---|
| 0 (seed clone, no build) | — | — | PENDING | PENDING |
| 1 | gcode delta v1 | PENDING | PENDING | PENDING |
| 2 | gcode delta v2 | PENDING | PENDING | PENDING |
| 3 | gcode delta v3 | PENDING | PENDING | PENDING |
| 4 | gcode delta v4 | PENDING | PENDING | PENDING |
| 5 | gcode delta v5 | PENDING | PENDING | PENDING |

### Q2 XFS-reflink fragmentation verdict

PENDING

---

## 9. Verdict

### Q1: Does a seeded+remapped+mtime-normalized lane SKIP the rebuild?

**Answer:** PENDING

**Warmth finding:**
- PENDING

**§10.7 gate recommendation:**
- PENDING

### Q2: XFS-reflink fragmentation/perf over reset-in-place cycles

**Answer:** PENDING

---

## 10. Appendix: Raw Timing Extracts

Timing data from `/usr/bin/time -v` outputs — to be filled after each measurement step.

### Seed build (step-1)

```
PENDING
```

### Seeded build (step-4)

```
PENDING
```

### Cold+delta build (step-5)

```
PENDING
```
