# T8 Warm-Lane Test Trio: Wall-Clock De-Flake Audit Findings

**Task:** #4847
**Parent PRD:** `docs/prds/infra-test-wallclock-deflake.md` (T8 entry)
**Date:** 2026-06-25
**Status:** FIXED (in-class items resolved); out-of-class residue handed off below.

---

## 1. Per-File Classification

| File | Escalations | S/R/T in-class? | Finding |
|------|------------|-----------------|---------|
| `test_warm_lane_preflight.sh` | 61 esc / 36 task | **NO** — zero timing assertions | OUT-OF-CLASS |
| `test_warm_lane_pool.sh` | 27 esc / 17 task | **YES** — reader-readiness races + B3 wall-direction | IN-CLASS (fixed #4847) + out-of-class residue |
| `test_refresh_warm_base.sh` | 5 esc / 3 task | **NO** — only mtime EQUALITY checks | OUT-OF-CLASS |

### 1.1 `test_warm_lane_preflight.sh` — OUT-OF-CLASS

Audit grepped for: `date +%s`, `+%s%3N`, numeric wall comparisons (`-lt/-le/-gt/-ge` against any time/wall variable), and `sleep 0.[0-9]` immediately before a `flock`-dependent assertion. **Result: zero matches.**

Every assertion in this file is structural:
- Exit code checks (`test "$RC" -ne 0`, `test "$RC" -eq 0`)
- Stderr-marker greps (`grep -qi "RUSTFLAGS"`, `grep -qiE "mount|provision"`)
- CALLS_FILE argv inspections (`grep -q "^cp"`)

These are load-independent — they do not inflate under CPU pressure.

**Root cause of escalations:** most plausibly one or more of:
- tmp/disk pressure under K concurrent merge-gate lanes (Block B/C fixtures create heavy mktemp trees + real `cp` walks via the passthrough cp stub)
- Block D fixtures create git repos in `/tmp` — under high concurrency, disk I/O can slow these to the point where the test itself exceeds the merge gate's wall-time budget
- Coarse merge-gate attribution (the gate times the whole `tests/infra/run_all.sh` invocation; a slow sibling test can attribute to preflight in the failure log)

**Hand-off:** see §3.1 and §3.2 below.

### 1.2 `test_warm_lane_pool.sh` — IN-CLASS items (fixed) + OUT-OF-CLASS residue

Two in-class categories found:

#### IN-CLASS (1): Reader-readiness fixed-sleep races (ALWAYS-RUN)

Three always-run sites + one substrate-gated site used a fixed sub-second `sleep` to assume a backgrounded `flock -s` reader had acquired its lock before the foreground refresh ran:

| Site | File line (pre-fix) | Sleep | Risk |
|------|---------------------|-------|------|
| SGSWAP3 | L1202 | `sleep 0.1` | Reader scheduled late → GC reaps gen.1 before assertion |
| SGSWAP4 | L1244 | `sleep 0.1` | Same |
| GC fixture | L1304 | `sleep 0.2` | Same (most impactful: holds `sleep 30` → 30s block per run) |
| B11 (substrate-gated) | L455 | `sleep 0.1` | Same; inside `_b11_concurrent_clone_during_flip()` |

Under merge-queue load the reader's `flock -s` acquisition is scheduled late; the foreground refresh's GC (`flock -n -x "$lock" sh -c 'rm -rf'` — `scripts/refresh-warm-base.sh:381`) then succeeds and reaps the gen the test expects to be retained → `SGSWAP3: retired gen dir still exists`, `SGSWAP4: prior gen dir still exists`, `GC1: retired gen NOT removed` assertions RED-flake. **Prime suspect for the 27 pool ambushes.**

**Fix applied (#4847):** introduced `_wait_for_reader_lock <ready-marker> <deadline-s>` (technique R causal-ordering + T anti-hang): reader touches a READY marker after acquiring `flock -s`; foreground polls for the marker before calling refresh. Generous deadline (30s) is an anti-hang guard only, never a timing discriminator. All four sites rewired. Reader `sleep 5`/`sleep 30` hold times replaced with `sleep 120` (readers are now killed explicitly via `kill+wait`, de-slowing the always-run section by ~40s — 40s → ~8s wall).

#### IN-CLASS (2): B3 warm-vs-cold wall-time direction (SUBSTRATE-GATED)

`assert "B3: warm lane build wall-time < cold-control build wall-time (direction)" test "$_B3_WARM_WALL" -lt "$_B3_COLD_WALL"` at L1441-1442. Both terms are cargo-build wall-times; under scheduling jitter the direction can invert → RED-flake.

**Fix applied (#4847 technique C):** assertion dropped. Warm-skip is proven structurally and load-independently by the three adjacent assertions:
- `_B3_DEP_FRESH = "true"` — heavy dep CoW-reused, not recompiled
- `_B3_LEAF_FRESH = "false"` — leaf delta rebuilt as expected
- B4: fresh-unit count in warm lane == in-place control (path-independence)

Wall-time echo retained as non-discriminating diagnostic, aligning B3 with B13 (which already only logs wall-ms, never asserts it).

#### OUT-OF-CLASS (disk-space/CoW): B11(d) df-flatness and B7 du bound

Two substrate-gated assertions bound disk bytes, not latency:

- `B11(d): df-flatness $(( $1 - $2 )) -le 50` MiB — available-space delta before/after a generation flip on the XFS volume
- `B7[cycle]: lane/target du stays bounded (≤ 2× baseline)` — target/ size over K reset-in-place cycles

These do **not** inflate under CPU load and are not scheduling-latency flakes. Their realistic flake vector is cross-lane disk activity on the shared XFS volume (concurrent tasks writing large artifacts cause the available-space delta to exceed 50 MiB). The S/R/T toolkit (structural marker / causal ordering / anti-hang timeout) does not apply.

**Hand-off:** see §3.2 below.

### 1.3 `test_refresh_warm_base.sh` — OUT-OF-CLASS

Audit grepped for the same patterns as §1.1. **Result: zero matches.**

`stat -c %Y` and `find -printf %T@` are used only for mtime/snapshot **EQUALITY** invariants (D3 clone-untouched, F3 read-only) — these compare a before/after snapshot and pass iff the mtime is unchanged, which is load-independent.

**Root cause of escalations:** most plausibly real-cp disk pressure. The passthrough cp stub (`REIFY_TEST_REFLINK_OK=1`) performs a real recursive copy (`cp -a` without `--reflink=always`) into a temp dir. Under high concurrent lane disk usage this copy can be slow, possibly exceeding the merge gate's per-test timeout.

**Hand-off:** see §3.2 below.

---

## 2. What Was Fixed and Non-Vacuous Proof

### 2.1 Fixes in this task (#4847)

1. **New primitive `_wait_for_reader_lock <ready-marker> <deadline-s>`** — polls for a READY marker (touched by reader after `flock -s` acquired) in 0.05s ticks; generous deadline is anti-hang only. Defined in `tests/infra/test_warm_lane_pool.sh` helper section.

2. **Rewired SGSWAP3, SGSWAP4, GC fixture, B11 readers** — each reader now touches `$READY` after acquiring `flock -s`; foreground calls `_wait_for_reader_lock` instead of `sleep 0.1`/`sleep 0.2`. Readers killed explicitly (`kill+wait`) instead of timing out the `sleep 5`/`sleep 30`.

3. **Dropped B3 wall-direction assert** — wall-time echo retained as non-discriminating diagnostic.

4. **Audit-provenance breadcrumbs** added to `test_warm_lane_preflight.sh` and `test_refresh_warm_base.sh` recording this verdict in-place.

### 2.2 Non-vacuous proof (PRD mandate H)

**Reader genuinely holds flock -s:** Block RH `RH-POS` assertion proves this causally: after `_wait_for_reader_lock` returns, a foreground `flock -n -x "$_RH_LOCK" true` probe **must fail** (exit non-zero). This exactly mirrors the real GC mechanism at `scripts/refresh-warm-base.sh:381`. If the handshake returned before `flock -s` was acquired (i.e., the READY marker was touched before the lock), the probe would succeed and the assertion would go RED.

The retained-gen assertions (SGSWAP3/SGSWAP4/GC1) now go RED iff the GC-defer invariant actually breaks (reader holds lock → GC `flock -n -x` fails → retained), not as a false-RED scheduling race.

**B3 warmth still proven:** `_B3_DEP_FRESH = "true"` goes RED if CoW warmth breaks and the heavy dep is recompiled.

---

## 3. Out-of-Class Residue: Hand-Off

### 3.1 `test_warm_lane_preflight.sh` escalations (61/36) — hand-off to warm-lane-pool owners

**Owner:** `docs/prds/warm-lane-pool-cow-seeding.md` (concurrent-lane isolation / pool sizing)

The preflight test has zero timing assertions. Escalations are most plausibly:
- **tmp/disk pressure under K concurrent lanes.** Preflight's Block B/C/D create multiple git fixtures and real-cp walks under `/tmp`. With K=4 concurrent merge-lane verifies, each spawning a full test suite, /tmp I/O contention can slow fixture setup past the gate's wall-time budget.
- **Coarse attribution** from `run_all.sh` timing the whole invocation.

**Recommended investigation:** (a) measure `/tmp` disk headroom on the verify host under peak concurrency; (b) check if the Block D git fixture creation can be moved to a per-lane tmpfs isolated from the shared `/tmp`; (c) check if the merge-gate timeout budget accounts for K-lane parallelism.

### 3.2 `test_warm_lane_pool.sh` disk-space assertions and `test_refresh_warm_base.sh` escalations — hand-off to space-safety owners

**Owner:** `docs/prds/warm-lane-pool-space-safety.md`

**B11(d) df-flatness (`-le 50 MiB`) and B7 du (`-le 2× baseline`):**
These bound CoW extent sharing / no-leak, not process-spawn latency. Their flake vector is cross-lane disk activity on the shared XFS volume, not CPU load. Specifically:
- Under K concurrent task lanes each running a B7 reset-in-place cycle, the XFS volume's available space may fluctuate by >50 MiB between the B11 before/after snapshots.
- du `≤ 2×` can drift if sccache or other per-lane artifacts grow the target/ beyond the 2× bound.

S/R/T techniques (structural marker, causal ordering, anti-hang timeout) do not apply to byte-count assertions. The appropriate fix is either to make the bound relative to actual concurrent disk use, to isolate the test substrate, or to mark these as advisory logs.

**`test_refresh_warm_base.sh` escalations (5/3):**
The passthrough cp stub performs a real recursive copy. Under merge-queue disk pressure this copy may time out the gate. Consider:
- Switching to a smaller or pre-warmed synthetic tree for the copy fixture
- Bounding the cp fixture size to prevent O(minutes) copies under disk stress
- Checking if the escalations correlate with large artifact trees in the test workspace
