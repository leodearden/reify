# Precision envelope — dense re-walk of torus, cone and fillet blend

Measurement record for **task #7463**. The main calibration note,
`docs/notes/precision-refine-envelope-calibration.md` ("the main note" below), labelled three
analytic classes `supremum` at the finest rung their §1.3 `d`-walks reached, 0.3 mm: torus
0.978 at `torus(1000mm, 20mm)`, cone 0.970 at `cone(500mm, 400mm, 1000mm)` and fillet blend
0.925 at `fillet(box(1000mm, 1000mm, 1000mm), 490mm)`. This record walks each of those regime
points below 0.3 mm and densely around its leaders, with the staged method of the main note's
§1.5 #7128 block, and states which label each value can carry.

**Relation to the main note.** The main note's §1.1 and §3.1 consume this record's results; its
§1.3 keeps the coarse walks as history. Three definitions live there and are cited, not
restated: the datum gate (§0 Caveat 2), the ratio convention (§4, "A dense parallel walk":
`decimal.Decimal`, `ROUND_HALF_UP`, 4 dp) and the display-precision wall (§1.5, #7128 block).
Every number below is **measured** (a run in this session's `runs.tsv`) or **derived**
(arithmetic on measured numbers), marked at each use.

**Plan-time readings are not data here.** The plan for this task quoted probes from an earlier
session — among them torus 0.9842 at 0.1 mm, cone 0.9840 at 0.02 mm and fillet blend 0.9724 at
0.1 mm. They motivated the extension spine of §2. None is cited as a result; every value below
comes from the runs this section's stamp covers.

---

## 1. Apparatus

### 1.1 Provenance

Own stamp — not the main note's §0, nor its §1.5 or §1.6 stamps:

| | |
|---|---|
| binary | `target/release/reify`, built 2026-10-09 01:50; the first-parent `main` commit before that mtime is `6e0047c757` (01:34). Not rebuilt |
| HEAD | `a030ee7528` (branch `task/7463`) at session start; last `crates/` commit `d2cf03022b` (2026-10-10 21:41). During Stage C the branch was rebased onto `main` `1a89d3d850`; that move changes none of the three fixtures, the main note, `crates/reify-kernel-occt`, `tolerance_combine.rs` or `crates/reify-cli/src`, and the binary was not rebuilt, so every datum below comes from this one binary |
| binary vs HEAD | 134 `crates/` files differ between `6e0047c757` and HEAD. On the measurement path, `crates/reify-kernel-occt` and `crates/reify-eval/src/tolerance_combine.rs` are unchanged, and `crates/reify-cli/src` differs by one comment-only commit (`14a0c4c5c2`, `dev.rs`). `reify-kernel-gmsh` changed (FEA meshing, not facet deviation). The reproduction gate (§1.2) is the empirical check |
| kernel | OCCT 7.8: 26 `libTK*.so.7.8` ldd lines, 26 distinct sonames, from `/usr/lib/x86_64-linux-gnu` (`libTKernel.so.7.8.1`); 16 are direct `NEEDED` entries of the binary. 27 `libTK*.so.7.9` lines resolve from `/opt/reify-deps/lib`; the binary needs none of them directly, while `libgmsh.so.4.15` needs 19 |
| machine | AMD Ryzen 9 3950X, 16C/32T (same box as the main note's §0), Linux 7.0.0-31 |
| load | 97.92 1-min loadavg at session start; **50 – 467** at probe start across the record. Per §0 Caveat 1 this moves wall clocks only |

### 1.2 Reproduction gate

Before any new datum was trusted, every value the main note's §1.3 publishes at these regime
points was re-measured, plus the torus determinism anchor of its §1.1:

| subject | d | published (main note) | a (m) | a/d |
|---|---|---|---|---|
| `torus(1000mm, 100mm)` | 10 mm | 5.665e-3 (§1.1) | 5.665e-3 | 0.5665 |
| `torus(1000mm, 20mm)` | 10 mm | 0.501 | 5.009e-3 | 0.5009 |
| `torus(1000mm, 20mm)` | 1 mm | 0.635 | 6.354e-4 | 0.6354 |
| `torus(1000mm, 20mm)` | 0.3 mm | 0.978 | 2.934e-4 | 0.9780 |
| `cone(500mm, 400mm, 1000mm)` | 10 mm | 0.908 | 9.082e-3 | 0.9082 |
| `cone(500mm, 400mm, 1000mm)` | 0.3 mm | 0.970 | 2.911e-4 | 0.9703 |
| `fillet(box(…), 490mm)` | 10 mm | 0.771 | 7.714e-3 | 0.7714 |
| `fillet(box(…), 490mm)` | 0.3 mm | 0.925 | 2.774e-4 | 0.9247 |

All eight match at the 3 dp §1.3 printed, and the anchor matches its string exactly. The gate is
cross-binary: §1.3 was measured on 2026-08-10's binary (main note §0), this on 2026-10-09's.

The three **committed** fixtures, run in place as
`timeout 240 ./target/release/reify check tests/prd-gate/fixtures/pnrg_envelope_<class>.ri`,
each printed `error: RepresentationWithin: sampled facet deviation <a> m exceeds bound
1.000e-6 m for <checker>` and exited 1:

| fixture | `a` (m) | checker | exit |
|---|---|---|---|
| `pnrg_envelope_torus.ri` | 5.665e-3 | `PnrgTorusCheck` | 1 |
| `pnrg_envelope_cone.ri` | 8.155e-3 | `PnrgConeCheck` | 1 |
| `pnrg_envelope_fillet_blend.ri` | 2.103e-3 | `PnrgFilletBlendCheck` | 1 |

These reproduce the main note's §1.3 default-point readings (0.567, 0.816, 0.210) and are the
byte-stability baseline for this task's fixture-header edits.

### 1.3 Sentinels

Each failure token was fired by a live failure before use. **`NO-DATUM`** twice: the cone copy
with its bound loosened to `RepresentationWithin(subject, 50mm)` printed
`OK PnrgConeCheck#constraint[0]` / `All constraints satisfied.` and exited 0 (§0 Caveat 2's
trap); a copy whose basename did not match its `module` failed `E_MODULE_PATH_MISMATCH` and
exited 1. **`SED-FAILED`**: rewriting the torus constructor text `torus(1000mm, 100mm)` instead
of the `let` line matched twice. **`TIMEOUT`**: `timeout 1` returned rc 124, and the driver
completed its other jobs. A `#precision(0.0123456mm)` cone copy printed a datum (`1.175e-5`),
so 7-decimal `d` literals survive parsing.

### 1.4 Harness

The script below is the apparatus for every stage of this record. It follows the main note's
§4 dense-walk conventions — a `git show HEAD:` snapshot, one parent dir per probe with the
basename kept, output captured before the exit code is read, distinct failure tokens, P = 4,
`T` = 240 s — with **one difference: the subject swap rewrites the whole `let` line**, as §4's
loft recipe does. The torus header comment repeats the constructor text, so an exactly-once
replace of `torus(1000mm, 100mm)` returns `SED-FAILED` (§1.3).

```python
# dense_probe.py — run from the repo root.  One job per stdin line, tab-separated:
#   <class> <rhs | -> <d_mm> <stage> <rep> [<timeout_s>]        (rhs "-" = committed subject)
# Appends one row per run to /tmp/t7463/runs.tsv, every token run included:
#   stage class subject d a|TOKEN ratio rc wall_s loadavg1 rep
import re, subprocess, sys, tempfile, time
from concurrent.futures import ThreadPoolExecutor
from decimal import Decimal, ROUND_HALF_UP
from pathlib import Path

LET = {"torus": "t", "cone": "c", "fillet_blend": "f"}
DATUM = re.compile(r"sampled facet deviation (\S+) m exceeds bound 1\.000e-6 m")
TOKENS = ("NO-DATUM", "TIMEOUT", "SED-FAILED")

def probe(cls, rhs, d, timeout_s=240):
    f = f"tests/prd-gate/fixtures/pnrg_envelope_{cls}.ri"
    src = subprocess.run(["git", "show", f"HEAD:{f}"],          # committed text, never the tree
                         capture_output=True, text=True, check=True).stdout
    if rhs != "-":                                   # the WHOLE `let` line, exactly once
        src, n = re.subn(rf"^(    let {LET[cls]} = ).*$", lambda m: m.group(1) + rhs, src, flags=re.M)
        if n != 1: return "SED-FAILED", "-", 0.0
    src, n = re.subn(r"^#precision\([^)]*\)$", f"#precision({d}mm)", src, flags=re.M)
    if n != 1: return "SED-FAILED", "-", 0.0
    path = Path(tempfile.mkdtemp(prefix=f"{cls}_{d}_", dir="/tmp/t7463/runs")) / Path(f).name
    path.write_text(src)                             # own parent dir, basename kept
    t0 = time.monotonic()
    p = subprocess.run(["timeout", str(timeout_s), "./target/release/reify", "check", str(path)],
                       capture_output=True, text=True)
    wall = time.monotonic() - t0
    path.with_name("out.txt").write_text(f"rc={p.returncode}\n{p.stdout}{p.stderr}")
    if p.returncode == 124: return "TIMEOUT", 124, wall
    m = DATUM.search(p.stdout + p.stderr)            # `OK` and E_MODULE_PATH_MISMATCH land here
    return (m.group(1) if m else "NO-DATUM"), p.returncode, wall

def ratio(a, d):
    if a in TOKENS: return "-"
    return str((Decimal(a) / (Decimal(d) / 1000)).quantize(Decimal("0.0001"), ROUND_HALF_UP))

def job(line):
    cls, rhs, d, stage, rep, *t = line.rstrip("\n").split("\t")
    load = Path("/proc/loadavg").read_text().split()[0]
    a, rc, wall = probe(cls, rhs, d, int(t[0]) if t else 240)
    return "\t".join([stage, cls, rhs, d, a, ratio(a, d), str(rc), f"{wall:.1f}", load, rep])

if __name__ == "__main__":
    jobs = [l for l in sys.stdin if l.strip() and not l.startswith("#")]
    with ThreadPoolExecutor(4) as ex, open("/tmp/t7463/runs.tsv", "a") as out:
        for row in ex.map(job, jobs):                # P = 4; do not raise it on this box
            print(row, file=out, flush=True)
            print(row, flush=True)
```

`d` is passed as canonical decimal text (`0.29`, never `0.290`), so the job's `d` is also the
ratio's divisor. Every tally in this record is computed by one script over `runs.tsv`.

*Checked.* The gate first ran through an equivalent scratch harness that adds what the
sentinels and the in-place runs need: an extra exactly-once rewrite, a basename override and a
working-tree run. The nine §1.2 subject probes (the eight table rows plus the committed torus
subject at 10 mm) then ran through this listing verbatim and returned the same `a` strings.

---

## 2. Stage A — the extension spine

**Status criterion, fixed in the task's plan before any run.** A class keeps `supremum` only
if (1) its spine shows a fall below the leader at finer `d` that exceeds the oscillation
amplitude between adjacent rungs of the same ladder — #6545's review criterion, as the main
note's §1.5 and §1.6 apply it — **and** (2) no dense bracket or regime spot-check exceeds the
leader. Otherwise it is a `lower bound`, naming the clause that failed. `K` > 1 is judged by
#7128's three-way verdict on the display-wall interval (main note §1.5).

**Method.** Each class at its regime point, walked down from 0.3 mm: 0.3, then 0.29 to 0.25 at
0.01 mm steps, then 0.2, 0.15, 0.12, 0.1, 0.08, 0.06, 0.05, 0.04, 0.03, 0.025, 0.02, 0.015 and
0.01 mm, continuing to 0.008, 0.006 and 0.005 mm while the previous rung completed in under
240 s. A class stops at its first `TIMEOUT`, its budget wall. `T` = 240 s, raised to 600 s below
0.05 mm (a deviation from §4's 240 s, as in the main note's §1.6). Bold marks the published
§1.3 rung and the spine maximum. Walls are contended (main note §0 Caveat 1).

**Torus**, `torus(1000mm, 20mm)`:

| d | a (m) | a/d | wall | note |
|---|---|---|---|---|
| **0.3 mm** | 2.934e-4 | **0.9780** | 6 s | published §1.3 rung |
| 0.29 mm | 2.782e-4 | 0.9593 | 8 s |  |
| 0.28 mm | 2.737e-4 | 0.9775 | 8 s |  |
| 0.27 mm | 2.587e-4 | 0.9581 | 7 s |  |
| 0.26 mm | 2.546e-4 | 0.9792 | 8 s | beats the published rung, 0.04 mm finer |
| 0.25 mm | 2.411e-4 | 0.9644 | 8 s |  |
| 0.2 mm | 1.953e-4 | 0.9765 | 9 s |  |
| 0.15 mm | 1.463e-4 | 0.9753 | 13 s |  |
| 0.12 mm | 1.184e-4 | 0.9867 | 15 s |  |
| 0.1 mm | 9.842e-5 | 0.9842 | 18 s |  |
| 0.08 mm | 7.938e-5 | 0.9923 | 22 s |  |
| 0.06 mm | 5.928e-5 | 0.9880 | 27 s |  |
| 0.05 mm | 4.985e-5 | 0.9970 | 30 s |  |
| 0.04 mm | 3.958e-5 | 0.9895 | 35 s |  |
| 0.03 mm | 2.969e-5 | 0.9897 | 50 s |  |
| 0.025 mm | 2.498e-5 | 0.9992 | 65 s |  |
| 0.02 mm | 1.990e-5 | 0.9950 | 78 s |  |
| 0.015 mm | 1.499e-5 | 0.9993 | 140 s |  |
| **0.01 mm** | 1.001e-5 | **1.0010** | 354 s | spine maximum; finest rung reached: 354 s ≥ 240 s ends the descent |

*Measured.* The published 0.9780 at 0.3 mm is beaten by 12 of the 18 other rungs, including
0.26 mm (0.9792), just 0.04 mm finer, so 0.3 mm was not even the maximum of [0.25, 0.3] mm. The
ratio is **still rising at the finest rung reached**: 1.0010 at 0.01 mm. No rung timed out;
the descent stopped because 0.01 mm took 354 s. Adjacent rungs swing by up to **0.0211**
(0.27 → 0.26 mm), and by up to 0.0095 at and below 0.1 mm (0.03 → 0.025 mm). *Derived:*
`a` = 1.001e-5 m prints to 1e-8 m, so the true ratio at 0.01 mm lies in [1.0005, 1.0015),
entirely above 1.

**Cone**, `cone(500mm, 400mm, 1000mm)`:

| d | a (m) | a/d | wall | note |
|---|---|---|---|---|
| **0.3 mm** | 2.911e-4 | **0.9703** | 1 s | published §1.3 rung |
| 0.29 mm | 2.787e-4 | 0.9610 | 1 s |  |
| 0.28 mm | 2.728e-4 | 0.9743 | 2 s |  |
| 0.27 mm | 2.615e-4 | 0.9685 | 2 s |  |
| 0.26 mm | 2.514e-4 | 0.9669 | 2 s |  |
| 0.25 mm | 2.414e-4 | 0.9656 | 2 s |  |
| 0.2 mm | 1.925e-4 | 0.9625 | 2 s |  |
| 0.15 mm | 1.454e-4 | 0.9693 | 2 s |  |
| 0.12 mm | 1.167e-4 | 0.9725 | 2 s |  |
| 0.1 mm | 9.705e-5 | 0.9705 | 3 s |  |
| 0.08 mm | 7.833e-5 | 0.9791 | 4 s |  |
| 0.06 mm | 5.897e-5 | 0.9828 | 4 s |  |
| 0.05 mm | 4.889e-5 | 0.9778 | 5 s |  |
| 0.04 mm | 3.925e-5 | 0.9813 | 6 s |  |
| 0.03 mm | 2.957e-5 | 0.9857 | 10 s |  |
| 0.025 mm | 2.456e-5 | 0.9824 | 8 s |  |
| 0.02 mm | 1.968e-5 | 0.9840 | 9 s |  |
| 0.015 mm | 1.481e-5 | 0.9873 | 10 s |  |
| 0.01 mm | 9.900e-6 | 0.9900 | 26 s |  |
| 0.008 mm | 7.912e-6 | 0.9890 | 25 s |  |
| 0.006 mm | 5.944e-6 | 0.9907 | 35 s |  |
| **0.005 mm** | 4.961e-6 | **0.9922** | 38 s | spine maximum; the spine's last rung |

*Measured.* The published 0.9703 at 0.3 mm is beaten by 15 of the 21 other rungs, the first at
0.28 mm (0.9743). The ratio is **still rising at 0.005 mm** (0.9922), the spine's last rung,
which cost 38 s: the descent ended where the spine ends, not at a budget limit. Adjacent rungs
swing by up to **0.0133** (0.29 → 0.28 mm), and by up to 0.0086 at and below 0.1 mm
(0.1 → 0.08 mm). Below 0.03 mm the ratio dips twice (at 0.025 and 0.008 mm) and otherwise
climbs.

**Fillet blend**, `fillet(box(1000mm, 1000mm, 1000mm), 490mm)`:

| d | a (m) | a/d | wall | note |
|---|---|---|---|---|
| **0.3 mm** | 2.774e-4 | **0.9247** | 7 s | published §1.3 rung |
| 0.29 mm | 2.774e-4 | 0.9566 | 8 s | same `a` as 0.3 mm |
| 0.28 mm | 2.653e-4 | 0.9475 | 8 s |  |
| 0.27 mm | 2.034e-4 | 0.7533 | 8 s | trough |
| 0.26 mm | 2.466e-4 | 0.9485 | 8 s |  |
| 0.25 mm | 2.365e-4 | 0.9460 | 8 s |  |
| 0.2 mm | 1.913e-4 | 0.9565 | 12 s |  |
| 0.15 mm | 1.444e-4 | 0.9627 | 15 s |  |
| 0.12 mm | 1.162e-4 | 0.9683 | 19 s |  |
| 0.1 mm | 9.724e-5 | 0.9724 | 22 s |  |
| 0.08 mm | 7.614e-5 | 0.9518 | 24 s |  |
| 0.06 mm | 5.872e-5 | 0.9787 | 30 s |  |
| 0.05 mm | 4.842e-5 | 0.9684 | 36 s |  |
| 0.04 mm | 3.925e-5 | 0.9813 | 47 s |  |
| 0.03 mm | 2.920e-5 | 0.9733 | 68 s |  |
| 0.025 mm | 2.432e-5 | 0.9728 | 74 s |  |
| 0.02 mm | 1.950e-5 | 0.9750 | 122 s |  |
| 0.015 mm | 1.468e-5 | 0.9787 | 188 s |  |
| **0.01 mm** | 9.818e-6 | **0.9818** | 299 s | spine maximum; finest rung reached: 299 s ≥ 240 s ends the descent |

*Measured.* The published 0.9247 at 0.3 mm is beaten by 17 of the 18 other rungs — every one
except 0.27 mm. 0.29 and 0.3 mm return byte-identical `a` = 2.774e-4: a plateau, on which the
ratio falls as `d` rises, so the published rung sat on the plateau's high-`d` side (0.9566 at
0.29 mm). 0.27 mm is a deep trough (0.7533), which gives the largest adjacent swing in this
record, **0.1952** (0.27 → 0.26 mm); at and below 0.1 mm swings reach 0.0269 (0.08 → 0.06 mm).
The spine maximum, 0.9818 at 0.01 mm, is the finest rung reached (299 s ended the descent),
and it leads 0.9813 at 0.04 mm by only 0.0005. At and below 0.1 mm the ratio oscillates within
[0.9518, 0.9818] rather than rising cleanly.

**The leader sits at the finest rung on every class.** Torus (0.01 mm), cone (0.005 mm) and
fillet blend (0.01 mm) each peak at the finest rung their descent reached, so no spine shows a
fall below its leader at finer `d`. Clause (1) of the status criterion therefore fails for all
three, and each class is a **`lower bound`** whatever Stages B and C find. No class met a budget
wall: no rung timed out, and the finest rungs cost 38 s (cone), 299 s (fillet blend) and 354 s
(torus).

**Regime spot-check** at each class's spine-best `d` (≤ 4 probes per class; the regime
coordinate is not walked). It asks whether §1.1's "at" coordinate, which §1.3 chose at
`d` = 10 mm, still carries the class maximum at fine `d`:

| class | coordinate | subject | d | a (m) | a/d | vs the regime point |
|---|---|---|---|---|---|---|
| torus | minor/major 0.02 | `torus(1000mm, 20mm)` | 0.01 mm | 1.001e-5 | 1.0010 | the regime point |
| torus | 0.012 | `torus(1000mm, 12mm)` | 0.01 mm | 1.003e-5 | **1.0030** | **above** |
| torus | 0.05 | `torus(1000mm, 50mm)` | 0.01 mm | `TIMEOUT` (600 s) | — | cost result |
| torus | 0.5 | `torus(1000mm, 500mm)` | 0.01 mm | `TIMEOUT` (600 s) | — | cost result |
| cone | top/bottom 0.8 | `cone(500mm, 400mm, 1000mm)` | 0.005 mm | 4.961e-6 | 0.9922 | the regime point |
| cone | 0.9 | `cone(500mm, 450mm, 1000mm)` | 0.005 mm | 4.991e-6 | **0.9982** | **above** |
| cone | 0.98 | `cone(500mm, 490mm, 1000mm)` | 0.005 mm | 5.005e-6 | **1.0010** | **above** |
| cone | 0.998 | `cone(500mm, 499mm, 1000mm)` | 0.005 mm | 5.009e-6 | **1.0018** | **above** |
| fillet blend | r/feature 0.49 | `fillet(box(…), 490mm)` | 0.01 mm | 9.818e-6 | 0.9818 | the regime point |
| fillet blend | 0.45 | `fillet(box(…), 450mm)` | 0.01 mm | 9.775e-6 | 0.9775 | below |
| fillet blend | 0.25 | `fillet(box(…), 250mm)` | 0.01 mm | 9.751e-6 | 0.9751 | below |

*Measured.* On torus and cone the published regime point does **not** carry the class
maximum at fine `d`. The cone rises monotonically toward the cylinder limit across the four
points, as §1.3's 10 mm regime table did, and crosses 1. *Derived* display-wall intervals:
torus 0.012 in [1.0025, 1.0035); cone 0.98 in [1.0009, 1.0011); cone 0.998 in [1.0017,
1.0019) — each entirely above 1. Fillet blend's regime point still leads its two spot-checks.
The torus coordinates thicker than the regime point were unaffordable at 0.01 mm. These are
spot readings: the regime coordinate is not walked here.

**Determinism.** 13 probes re-run, 13 byte-identical `a`: each class's top-3 spine rungs by
ratio (fillet blend's third place is a tie, 0.06 and 0.015 mm both reading 0.9787, so both
were re-run), every spine rung ≥ 0.999 (all three are already in torus's top 3), and every
spot-check ≥ 0.999 (torus 0.012, cone 0.98 and 0.998). Load varied from 62 to 467 1-min
loadavg across Stage A; no `a` did.

**Stage-B brackets, derived from the bracket rule.** (i) [0.290, 0.310] mm at 0.001 mm for
each class. (ii) d_c ± 5 % around each class's top-2 spine rungs, at 0.001 mm for d_c ≥ 0.2 mm
and 0.5 % · d_c below, with `d` rounded to 1e-5 mm; a bracket whose Stage-A probe cost more
than 120 s is skipped and the rung stays spine-only:

| class | top-2 rungs (Stage-A cost) | bracket (ii) |
|---|---|---|
| torus | 0.01 mm (354 s), 0.015 mm (140 s) | none — both spine-only |
| cone | 0.005 mm (38 s), 0.006 mm (35 s) | [0.00475, 0.00525] mm at 0.000025 mm; [0.0057, 0.0063] mm at 0.00003 mm |
| fillet blend | 0.01 mm (299 s), 0.04 mm (47 s) | 0.01 mm spine-only; [0.038, 0.042] mm at 0.0002 mm |

The relative step departs from the task's fixed 0.001 mm, which at `d` = 0.005 mm is 20 % of
`d`, far coarser than the ~0.1–0.4 % plateaus the main note's §1.5 and §1.6 measured. Rounding
to 1e-5 mm makes the first cone bracket's steps alternate between 2e-5 and 3e-5 mm. Torus's
1.0010 is a Stage-C target even without a bracket, because it reads ≥ 0.9995.

---

## 3. Stage B — the dense brackets

The brackets §2 derived, 126 probes. A column is added, as in the main note's §1.5: the deficit
**δ = d − a**, an integer count of `a`'s display quanta (1e-7 m at 0.3 mm, 1e-8 m at 0.04 mm,
1e-9 m at 0.005 mm). "Lowest rung of a plateau" marks the smallest sampled `d` of a run of
identical `a`; the plateau's true lower edge lies between it and the rung below.

**Torus, (i) [0.290, 0.310] mm**:

| d | a (m) | δ | a/d | note |
|---|---|---|---|---|
| 0.29 mm | 2.782e-4 | 118 | 0.9593 | spine rung |
| 0.291 mm | 2.797e-4 | 113 | 0.9612 |  |
| **0.292 mm** | 2.902e-4 | 18 | **0.9938** | lowest rung of a plateau; bracket max |
| 0.293 mm | 2.902e-4 | 28 | 0.9904 |  |
| 0.294 mm | 2.902e-4 | 38 | 0.9871 |  |
| 0.295 mm | 2.918e-4 | 32 | 0.9892 | lowest rung of a plateau |
| 0.296 mm | 2.918e-4 | 42 | 0.9858 |  |
| 0.297 mm | 2.918e-4 | 52 | 0.9825 |  |
| 0.298 mm | 2.934e-4 | 46 | 0.9846 | lowest rung of a plateau |
| 0.299 mm | 2.934e-4 | 56 | 0.9813 |  |
| 0.3 mm | 2.934e-4 | 66 | 0.9780 | published rung |
| 0.301 mm | 2.950e-4 | 60 | 0.9801 | lowest rung of a plateau |
| 0.302 mm | 2.950e-4 | 70 | 0.9768 |  |
| 0.303 mm | 2.950e-4 | 80 | 0.9736 |  |
| 0.304 mm | 2.966e-4 | 74 | 0.9757 | lowest rung of a plateau |
| 0.305 mm | 2.966e-4 | 84 | 0.9725 |  |
| 0.306 mm | 2.966e-4 | 94 | 0.9693 |  |
| 0.307 mm | 2.966e-4 | 104 | 0.9661 |  |
| 0.308 mm | 2.983e-4 | 97 | 0.9685 | lowest rung of a plateau |
| 0.309 mm | 2.983e-4 | 107 | 0.9654 |  |
| 0.31 mm | 2.983e-4 | 117 | 0.9623 |  |

*Measured.* **A regular staircase.** `a` holds constant across runs of three or four rungs and
steps up by 16–17 quanta between them, at transitions in (0.294, 0.295], (0.297, 0.298],
(0.300, 0.301], (0.303, 0.304] and (0.307, 0.308] mm — every 0.003–0.004 mm across
[0.292, 0.310] mm. The step into the 2.902e-4 plateau, between 0.291 and 0.292 mm, is larger
(105 quanta). Within every plateau the ratio falls as `d` rises, so each peaks at its lowest
rung. The published 0.3 mm is the **top** rung of the 2.934e-4 plateau, the lowest ratio on
it. The bracket maximum, 0.9938 at 0.292 mm, stays below Stage A's 1.0010.

**Cone, (i) [0.290, 0.310] mm**:

| d | a (m) | δ | a/d | note |
|---|---|---|---|---|
| 0.29 mm | 2.787e-4 | 113 | 0.9610 | lowest rung of a plateau; spine rung |
| 0.291 mm | 2.787e-4 | 123 | 0.9577 |  |
| 0.292 mm | 2.848e-4 | 72 | 0.9753 | lowest rung of a plateau |
| 0.293 mm | 2.848e-4 | 82 | 0.9720 |  |
| 0.294 mm | 2.848e-4 | 92 | 0.9687 |  |
| 0.295 mm | 2.848e-4 | 102 | 0.9654 |  |
| 0.296 mm | 2.848e-4 | 112 | 0.9622 |  |
| 0.297 mm | 2.848e-4 | 122 | 0.9589 |  |
| **0.298 mm** | 2.911e-4 | 69 | **0.9768** | lowest rung of a plateau; bracket max |
| 0.299 mm | 2.911e-4 | 79 | 0.9736 |  |
| 0.3 mm | 2.911e-4 | 89 | 0.9703 | published rung |
| 0.301 mm | 2.911e-4 | 99 | 0.9671 |  |
| 0.302 mm | 2.911e-4 | 109 | 0.9639 |  |
| 0.303 mm | 2.911e-4 | 119 | 0.9607 |  |
| 0.304 mm | 2.911e-4 | 129 | 0.9576 |  |
| 0.305 mm | 2.976e-4 | 74 | 0.9757 | lowest rung of a plateau |
| 0.306 mm | 2.976e-4 | 84 | 0.9725 |  |
| 0.307 mm | 2.976e-4 | 94 | 0.9694 |  |
| 0.308 mm | 2.976e-4 | 104 | 0.9662 |  |
| 0.309 mm | 2.976e-4 | 114 | 0.9631 |  |
| 0.31 mm | 2.976e-4 | 124 | 0.9600 |  |

*Measured.* **Wider plateaus, the same shape.** Four plateaus with steps of 61–65 quanta; the
two lying wholly inside the bracket are six and seven rungs wide. The published 0.3 mm sits
third on the 2.911e-4 plateau; that plateau's lowest rung, 0.298 mm, reads 0.9768.

**Cone, (ii) [0.00475, 0.00525] mm**:

| d | a (m) | δ | a/d | note |
|---|---|---|---|---|
| 0.00475 mm | 4.717e-6 | 33 | 0.9931 | beats Stage A |
| 0.00478 mm | 4.743e-6 | 37 | 0.9923 | beats Stage A |
| 0.0048 mm | 4.769e-6 | 31 | 0.9935 | beats Stage A |
| 0.00483 mm | 4.796e-6 | 34 | 0.9930 | beats Stage A |
| 0.00485 mm | 4.808e-6 | 42 | 0.9913 |  |
| 0.00488 mm | 4.834e-6 | 46 | 0.9906 |  |
| 0.0049 mm | 4.863e-6 | 37 | 0.9924 | beats Stage A |
| 0.00493 mm | 4.891e-6 | 39 | 0.9921 |  |
| 0.00495 mm | 4.906e-6 | 44 | 0.9911 |  |
| 0.00498 mm | 4.944e-6 | 36 | 0.9928 | beats Stage A |
| 0.005 mm | 4.961e-6 | 39 | 0.9922 | spine rung |
| 0.00503 mm | 4.988e-6 | 42 | 0.9917 |  |
| **0.00505 mm** | 5.018e-6 | 32 | **0.9937** | bracket max; beats Stage A |
| 0.00508 mm | 5.045e-6 | 35 | 0.9931 | beats Stage A |
| 0.0051 mm | 5.059e-6 | 41 | 0.9920 |  |
| 0.00513 mm | 5.090e-6 | 40 | 0.9922 |  |
| 0.00515 mm | 5.105e-6 | 45 | 0.9913 |  |
| 0.00518 mm | 5.133e-6 | 47 | 0.9909 |  |
| 0.0052 mm | 5.163e-6 | 37 | 0.9929 | beats Stage A |
| 0.00523 mm | 5.193e-6 | 37 | 0.9929 | beats Stage A |
| 0.00525 mm | 5.207e-6 | 43 | 0.9918 |  |

**Cone, (ii) [0.0057, 0.0063] mm**:

| d | a (m) | δ | a/d | note |
|---|---|---|---|---|
| **0.0057 mm** | 5.658e-6 | 42 | **0.9926** | bracket max; beats Stage A |
| 0.00573 mm | 5.676e-6 | 54 | 0.9906 |  |
| 0.00576 mm | 5.710e-6 | 50 | 0.9913 |  |
| 0.00579 mm | 5.745e-6 | 45 | 0.9922 |  |
| 0.00582 mm | 5.762e-6 | 58 | 0.9900 |  |
| 0.00585 mm | 5.799e-6 | 51 | 0.9913 |  |
| 0.00588 mm | 5.834e-6 | 46 | 0.9922 |  |
| 0.00591 mm | 5.856e-6 | 54 | 0.9909 |  |
| 0.00594 mm | 5.887e-6 | 53 | 0.9911 |  |
| 0.00597 mm | 5.924e-6 | 46 | 0.9923 | beats Stage A |
| 0.006 mm | 5.944e-6 | 56 | 0.9907 | spine rung |
| 0.00603 mm | 5.980e-6 | 50 | 0.9917 |  |
| 0.00606 mm | 5.998e-6 | 62 | 0.9898 |  |
| 0.00609 mm | 6.036e-6 | 54 | 0.9911 |  |
| 0.00612 mm | 6.075e-6 | 45 | 0.9926 | beats Stage A |
| 0.00615 mm | 6.095e-6 | 55 | 0.9911 |  |
| 0.00618 mm | 6.132e-6 | 48 | 0.9922 |  |
| 0.00621 mm | 6.152e-6 | 58 | 0.9907 |  |
| 0.00624 mm | 6.191e-6 | 49 | 0.9921 |  |
| 0.00627 mm | 6.208e-6 | 62 | 0.9901 |  |
| 0.0063 mm | 6.250e-6 | 50 | 0.9921 |  |

*Measured.* **No plateau resolved.** No two adjacent rungs share `a`: each step is 20–30
display quanta, and whatever structure `a` has here is finer than that. `a` rises with `d`
irregularly. δ wanders over 31–47 quanta in the first bracket and 42–62 in the second, with no
visible period, and is equal on one adjacent pair only (0.0052 and 0.00523 mm, 37 quanta), the
one unit-slope pair in these brackets. **Thirteen rungs beat Stage A's 0.9922** — ten in the
first bracket, three in the second — and the best, **0.9937 at 0.00505 mm**, lies 0.00005 mm
from the spine rung it brackets.

**Fillet blend, (i) [0.290, 0.310] mm**:

| d | a (m) | δ | a/d | note |
|---|---|---|---|---|
| **0.29 mm** | 2.774e-4 | 126 | **0.9566** | lowest rung of a plateau; spine rung; bracket max |
| 0.291 mm | 2.774e-4 | 136 | 0.9533 |  |
| 0.292 mm | 2.774e-4 | 146 | 0.9500 |  |
| 0.293 mm | 2.774e-4 | 156 | 0.9468 |  |
| 0.294 mm | 2.774e-4 | 166 | 0.9435 |  |
| 0.295 mm | 2.774e-4 | 176 | 0.9403 |  |
| 0.296 mm | 2.774e-4 | 186 | 0.9372 |  |
| 0.297 mm | 2.774e-4 | 196 | 0.9340 |  |
| 0.298 mm | 2.774e-4 | 206 | 0.9309 |  |
| 0.299 mm | 2.774e-4 | 216 | 0.9278 |  |
| 0.3 mm | 2.774e-4 | 226 | 0.9247 | published rung |
| 0.301 mm | 2.774e-4 | 236 | 0.9216 |  |
| 0.302 mm | 2.296e-4 | 724 | 0.7603 | lowest rung of a plateau |
| 0.303 mm | 2.296e-4 | 734 | 0.7578 |  |
| 0.304 mm | 2.296e-4 | 744 | 0.7553 |  |
| 0.305 mm | 2.296e-4 | 754 | 0.7528 |  |
| 0.306 mm | 2.296e-4 | 764 | 0.7503 |  |
| 0.307 mm | 2.296e-4 | 774 | 0.7479 |  |
| 0.308 mm | 2.296e-4 | 784 | 0.7455 |  |
| 0.309 mm | 2.296e-4 | 794 | 0.7430 |  |
| 0.31 mm | 2.296e-4 | 804 | 0.7406 |  |

*Measured.* **One plateau across most of the bracket, then a fall.** `a` = 2.774e-4 from 0.290
to 0.301 mm, then **drops** by 478 quanta to 2.296e-4 at 0.302 mm and holds through 0.310 mm:
a coarser request returns a smaller deviation, the non-monotonicity the main note's §1.5
records on nurbs_surface. The bracket maximum is its lowest rung. The spine's 0.28 mm reads
2.653e-4, so this plateau's lower edge lies in (0.28, 0.29] mm, outside the bracket; there
the ratio could reach 2.774e-4 / 0.28 mm = 0.9907 (*derived* bound), above every fillet-blend
reading so far.

**Fillet blend, (ii) [0.038, 0.042] mm**:

| d | a (m) | δ | a/d | note |
|---|---|---|---|---|
| 0.038 mm | 3.649e-5 | 151 | 0.9603 |  |
| **0.0382 mm** | 3.753e-5 | 67 | **0.9825** | lowest rung of a plateau; bracket max; beats Stage A |
| 0.0384 mm | 3.753e-5 | 87 | 0.9773 |  |
| 0.0386 mm | 3.753e-5 | 107 | 0.9723 |  |
| 0.0388 mm | 3.753e-5 | 127 | 0.9673 |  |
| 0.039 mm | 3.817e-5 | 83 | 0.9787 |  |
| 0.0392 mm | 3.816e-5 | 104 | 0.9735 | lowest rung of a plateau |
| 0.0394 mm | 3.816e-5 | 124 | 0.9685 |  |
| 0.0396 mm | 3.816e-5 | 144 | 0.9636 |  |
| 0.0398 mm | 3.816e-5 | 164 | 0.9588 |  |
| 0.04 mm | 3.925e-5 | 75 | 0.9813 | lowest rung of a plateau; spine rung |
| 0.0402 mm | 3.925e-5 | 95 | 0.9764 |  |
| 0.0404 mm | 3.925e-5 | 115 | 0.9715 |  |
| 0.0406 mm | 3.925e-5 | 135 | 0.9667 |  |
| 0.0408 mm | 3.994e-5 | 86 | 0.9789 |  |
| 0.041 mm | 3.993e-5 | 107 | 0.9739 | lowest rung of a plateau |
| 0.0412 mm | 3.993e-5 | 127 | 0.9692 |  |
| 0.0414 mm | 3.993e-5 | 147 | 0.9645 |  |
| 0.0416 mm | 3.993e-5 | 167 | 0.9599 |  |
| 0.0418 mm | 3.179e-5 | 1001 | 0.7605 |  |
| 0.042 mm | 4.110e-5 | 90 | 0.9786 |  |

*Measured.* **Plateaus four rungs wide, about 2 % of `d`**, at 3.753e-5, 3.816e-5, 3.925e-5
and 3.993e-5 m, then a trough at 0.0418 mm (3.179e-5, 0.7605). Twice the rung just below a
plateau reads one quantum **above** it (0.039 mm at 3.817e-5; 0.0408 mm at 3.994e-5), so `a`
falls by one quantum as `d` rises there. Within each plateau the ratio falls as `d` rises.
**0.0382 mm reads 0.9825, beating Stage A's 0.9818** at 0.01 mm.

**Shape across the three classes.** Wherever the step resolves it — all three (i) brackets
and fillet blend's (ii) — the structure is the one the main note's §1.5 found on
nurbs_surface: `a` piecewise-constant on plateaus, the ratio peaking at each plateau's lower
edge, and `a` not monotone in `d`. Apart from the single cone pair above, no unit-slope
segment was resolved. At 0.005 mm a 0.5 % step is too coarse to resolve plateaus at all.

**Determinism.** 18 re-runs, 18 byte-identical `a`: each class's top-5 Stage-B rungs by ratio,
plus each bracket's maximum not already among them (cone 0.298 and 0.0057 mm, fillet blend
0.29 mm). No Stage-B rung reads ≥ 0.999. Load ran from 56 to 236 1-min loadavg across Stage B.

**Skipped for cost** (§2): torus's two bracket-(ii) candidates (0.01 and 0.015 mm) and fillet
blend's 0.01 mm. All three stay spine-only rungs.

**Stage-C targets.** Per class, the top three candidates by ratio. Rungs reading ≥ 0.9995 are
mandatory. A spine-only rung has no Stage-B neighbourhood, so its regime is unknown: Stage C
first probes two neighbours on each side, at a fifth of the Stage-B step, and then searches
the edge the neighbours point to.

| class | target | reading | known structure | edge searched |
|---|---|---|---|---|
| torus | T1 0.01 mm | 1.0010 | spine-only; **mandatory** (≥ 0.9995) | classify first |
| torus | T2 0.015 mm | 0.9993 | spine-only | classify first |
| torus | T3 0.025 mm | 0.9992 | spine-only | classify first |
| cone | C1 0.00505 mm | 0.9937 | no plateau at the Stage-B step | classify first |
| cone | C2 0.0048 mm | 0.9935 | no plateau at the Stage-B step | classify first |
| cone | C3 0.00508 mm | 0.9931 | no plateau at the Stage-B step | classify first |
| fillet blend | F1 0.0382 mm | 0.9825 | plateau 3.753e-5, lower edge in (0.038, 0.0382] | lower |
| fillet blend | F2 0.01 mm | 0.9818 | spine-only | classify first |
| fillet blend | F3 0.04 mm | 0.9813 | plateau 3.925e-5, lower edge in (0.0398, 0.04] | lower |
| fillet blend | F4 0.29 mm | 0.9566 | plateau 2.774e-4, lower edge in (0.28, 0.29] | lower |

Two choices here go beyond the bare rule. **C3** ranks by the unrounded ratio: 0.00508 mm and
0.00475 mm both print 0.9931, at 0.993110 and 0.993053. **F4 is a fourth fillet-blend
target**: its derived edge bound, 0.9907, exceeds every measured fillet-blend ratio, so it is
the one place a measured bound says this class could read higher. The Stage-A spot-checks that
read ≥ 0.9995 (torus 0.012, cone 0.98 and 0.998) are not targets. Their display intervals
already lie entirely above 1, so the ambiguity the mandatory rule exists to settle does not
arise, and the regime coordinate is not walked here (§2).

---

## 4. Stage C — plateau edges

Each §3 target was scanned toward its edge and then sectioned until the edge was bracketed to
1e-4 · `d` (F4, the one target above 0.1 mm, to 1e-5 mm). Two departures from a plain bisection,
both disclosed:

* **Classification first.** A spine-only or unresolved target was first probed at two
  neighbours on each side, a fifth of its Stage-B step apart (1e-5 mm at 0.01 mm). Every
  target turned out to sit on a plateau; no unit-slope segment was found, so every edge
  searched is a **lower** edge.
* **Five-way sections.** Each round placed four probes inside the current bracket at once
  instead of one midpoint. That uses the four slots and narrows the bracket 5× per round.

Every pinned edge is **bracketed**: its outer probe returns a different `a`. A scan that met a
neighbouring plateau whose reachable ratio was higher pinned that plateau's edge too; those
edges carry a letter suffix (T1a, C1a, …). One plateau is a control, not a pinned edge (T1d).
The display interval is the main note's §1.5 wall: `a` ± half its last printed digit, over the
exact `d`.

**Torus**, `torus(1000mm, 20mm)`:

| edge | plateau `a` (m) | outer probe → `a` | `d_lo` | a/d at `d_lo` | display interval | on plateau |
|---|---|---|---|---|---|---|
| T1d | 9.981e-6 | none — unbracketed | 0.00994 mm | 1.0041 | [1.004074, 1.004176) | 7 probes, 0.00994–0.0099472 mm |
| T1c | 9.987e-6 | 0.0099472 mm → 9.981e-6 | 0.0099476 mm | 1.0040 | [1.003910, 1.004012) | 9 probes, 0.0099476–0.0099668 mm |
| T1a | 9.997e-6 | 0.0099668 mm → 9.987e-6 | 0.0099672 mm | 1.0030 | [1.002939, 1.003040) | 10 probes, 0.0099672–0.0099868 mm |
| T1 | 1.001e-5 | 0.0099868 mm → 9.997e-6 | 0.0099872 mm | 1.0023 | [1.001782, 1.002784) | 10 probes, 0.0099872–0.0100068 mm |
| T1b | 1.002e-5 | 0.0100068 mm → 1.001e-5 | 0.0100072 mm | 1.0013 | [1.000779, 1.001779) | 5 probes, 0.0100072–0.01002 mm |
| T2a | 1.498e-5 | 0.0149352 mm → 1.496e-5 | 0.0149358 mm | 1.0030 | [1.002624, 1.003295) | 11 probes, 0.0149358–0.0149712 mm |
| T2 | 1.499e-5 | 0.0149712 mm → 1.498e-5 | 0.0149718 mm | 1.0012 | [1.000881, 1.001550) | 8 probes, 0.0149718–0.015 mm |
| T3a | 2.495e-5 | 0.024917 mm → 2.467e-5 | 0.024918 mm | 1.0013 | [1.001083, 1.001485) | 9 probes, 0.024918–0.024966 mm |
| T3 | 2.498e-5 | 0.024966 mm → 2.495e-5 | 0.024967 mm | 1.0005 | [1.000320, 1.000721) | 7 probes, 0.024967–0.025025 mm |

*Measured.* **Eight pinned edges, every one above 1 with its display interval entirely above
1.** Near 0.01 mm the plateaus are regular: about 2e-5 mm wide (0.2 % of `d`), and the
lower-edge ratio **rises as `d` falls** — 1.0013, 1.0023, 1.0030, 1.0040 from T1b down to T1c.
**T1d is a control, not a pinned edge**: its lowest probe, 0.00994 mm, still returns 9.981e-6
and reads **1.0041**, the highest torus ratio in this record, and nothing below it was
probed, so its edge is unbracketed. The scan stopped there with the ratio still rising, as it
was at the spine's finest rung.

**Cone**, `cone(500mm, 400mm, 1000mm)`:

| edge | plateau `a` (m) | outer probe → `a` | `d_lo` | a/d at `d_lo` | display interval | on plateau |
|---|---|---|---|---|---|---|
| C2a | 4.757e-6 | 0.004786 mm → 4.743e-6 | 0.0047864 mm | 0.9939 | [0.993753, 0.993963) | 13 probes, 0.0047864–0.0047994 mm |
| C2 | 4.769e-6 | 0.0047994 mm → 4.757e-6 | 0.0047996 mm | 0.9936 | [0.993520, 0.993729) | 5 probes, 0.0047996–0.00481 mm |
| C1a | 5.005e-6 | 0.0050352 mm → 4.988e-6 | 0.0050356 mm | 0.9939 | [0.993823, 0.994023) | 13 probes, 0.0050356–0.0050498 mm |
| C1 | 5.018e-6 | 0.0050498 mm → 5.005e-6 | 0.00505 mm | 0.9937 | [0.993564, 0.993763) | 6 probes, 0.00505–0.0050644 mm |
| C3a | 5.031e-6 | 0.0050644 mm → 5.018e-6 | 0.0050648 mm | 0.9933 | [0.993227, 0.993426) | 14 probes, 0.0050648–0.0050788 mm |
| C3 | 5.045e-6 | 0.0050788 mm → 5.031e-6 | 0.005079 mm | 0.9933 | [0.993207, 0.993405) | 4 probes, 0.005079–0.00509 mm |

*Measured.* **Six pinned edges, all below 1** — 0.9933 to 0.9939, every display interval
entirely below 1. The plateaus are 1.3–1.4e-5 mm wide (0.27–0.29 % of `d`), with steps of
12–17 quanta. The six lower-edge ratios lie within 0.0006 of each other, so at this regime
point the dense walk shows no approach to 1 between 0.00479 and 0.00508 mm.

**Fillet blend**, `fillet(box(1000mm, 1000mm, 1000mm), 490mm)`:

| edge | plateau `a` (m) | outer probe → `a` | `d_lo` | a/d at `d_lo` | display interval | on plateau |
|---|---|---|---|---|---|---|
| F2 | 9.818e-6 | 0.0099832 mm → 9.782e-6 | 0.0099836 mm | 0.9834 | [0.983362, 0.983463) | 8 probes, 0.0099836–0.01002 mm |
| F1 | 3.753e-5 | 0.0381584 mm → 2.903e-5 | 0.03816 mm | 0.9835 | [0.983359, 0.983622) | 5 probes, 0.03816–0.0388 mm |
| F3 | 3.925e-5 | 0.0399328 mm → 3.036e-5 | 0.0399344 mm | 0.9829 | [0.982736, 0.982988) | 9 probes, 0.0399344–0.0406 mm |
| F4 | 2.774e-4 | 0.2831904 mm → 2.653e-4 | 0.2831936 mm | 0.9795 | [0.979365, 0.979719) | 19 probes, 0.2831936–0.301 mm |

*Measured.* **Four pinned edges, all below 1** — 0.9795 to 0.9835. Below F1 and F3 the outer
probe falls into a **trough plateau** (2.903e-5 and 3.036e-5 m, ratios ≈ 0.76). The trough
Stage A met at 0.27 mm and Stage B at 0.0418 mm recurs directly beneath plateau lower edges.
F4's derived bound of 0.9907 did not materialise: the 2.774e-4 plateau's edge is at
0.2831936 mm, reading 0.9795.

**The leading edges**, in the main note's §1.5 P1 format: a selection of the probes around
each class's highest pinned edge, read as three independent column groups.

| torus `d` | a (m) | a/d | | cone `d` | a (m) | a/d | | fillet blend `d` | a (m) | a/d |
|---|---|---|---|---|---|---|---|---|---|---|
| 0.00994 mm | 9.981e-6 | 1.0041 | T1d control | 0.0050344 mm | 4.988e-6 | 0.9908 | | 0.03804 mm | 3.649e-5 | 0.9593 |
| 0.0099472 mm | 9.981e-6 | 1.0034 | outer | 0.0050352 mm | 4.988e-6 | 0.9906 | outer | 0.0381584 mm | 2.903e-5 | 0.7608 |
| **0.0099476 mm** | 9.987e-6 | **1.0040** | ← T1c `d_lo` | **0.0050356 mm** | 5.005e-6 | **0.9939** | ← C1a `d_lo` | **0.03816 mm** | 3.753e-5 | **0.9835** |
| 0.009948 mm | 9.987e-6 | 1.0039 | | 0.005036 mm | 5.005e-6 | 0.9938 | | 0.0382 mm | 3.753e-5 | 0.9825 |
| 0.00995 mm | 9.987e-6 | 1.0037 | | 0.005038 mm | 5.005e-6 | 0.9934 | | 0.0384 mm | 3.753e-5 | 0.9773 |
| 0.00996 mm | 9.987e-6 | 1.0027 | | 0.00504 mm | 5.005e-6 | 0.9931 | | 0.0386 mm | 3.753e-5 | 0.9723 |
| 0.0099668 mm | 9.987e-6 | 1.0020 | T1c top | 0.0050498 mm | 5.005e-6 | 0.9911 | C1a top | 0.0388 mm | 3.753e-5 | 0.9673 |
| 0.0099672 mm | 9.997e-6 | 1.0030 | T1a `d_lo` | 0.00505 mm | 5.018e-6 | 0.9937 | C1 `d_lo` | 0.039 mm | 3.817e-5 | 0.9787 |

On every leading plateau the ratio falls monotonically from the lower edge as `d` rises,
exactly as the plateau model predicts, and both sides of each pinned edge return different
`a`.

**Determinism.** 38 re-runs, 38 byte-identical `a`: both sides of all 18 pinned edges, T1d's
lowest probe, and cone's highest probe overall (the `top/bottom` = 0.998 spot-check, now run
three times). Across the whole record, 77 probes were measured more than once and none
returned a different `a`. Load ran from 50 to 437 1-min loadavg across Stage C.

---

## 5. Verdicts

| class | best a/d found | where | display interval | verdict | status | n |
|---|---|---|---|---|---|---|
| torus | **1.0041** | regime point, `d` = 0.00994 mm (T1d) | [1.004074, 1.004176) | **(b) K > 1 established** | **lower bound** | **1** † |
| cone | **1.0018** | `top/bottom` = 0.998, `d` = 0.005 mm (spot-check) | [1.0017, 1.0019) | **(b)** at the spot-check; (a) at the regime point, ≤ 0.9939 | **lower bound** | **1** † |
| fillet blend | 0.9835 | regime point, `d` = 0.03816 mm (F1) | [0.983359, 0.983622) | (a) every interval below 1 | **lower bound** | 0 † |

The verdict is #7128's (main note §1.5): (a) every interval lies below 1; (b) some interval lies
entirely above 1; (c) the best interval straddles 1. No class landed on (c). `n` = ⌈log₂ `K`⌉,
0 when `K` ≤ 1, is *derived*. † Each `n` is derived from a lower bound, so the true `n` can
only be higher.

**Status under the §2 criterion: `lower bound` on all three, with both clauses failing.**

* **Torus.** Clause (1): the spine's leader is its finest rung (0.01 mm), and Stage C's scan
  below it kept rising, to 1.0041 at 0.00994 mm. Clause (2): Stage C's edges and the
  `minor/major` = 0.012 spot-check (1.0030) exceed the spine's 1.0010.
* **Cone.** Clause (1): the leader is the spine's last rung (0.005 mm). Clause (2): Stage B's
  0.9937 and the 0.9 / 0.98 / 0.998 spot-checks exceed the spine's 0.9922.
* **Fillet blend.** Clause (1): the leader is the finest rung reached (0.01 mm). Clause (2):
  Stage B's 0.9825 at 0.0382 mm and Stage C's 0.9835 at 0.03816 mm exceed the spine's 0.9818.

**What this is, and is not.** A dense search raises a lower bound and cannot prove a supremum
over a continuum, so no number in this section is a supremum.

* **Torus — `K` > 1 is established at the regime point itself.** Eight independent pinned
  edges lie entirely above 1, plus the spot-check. The value 1.0041 is a floor. The ratio was
  still rising at the finest probe; the spine stopped at 0.01 mm on cost (354 s); and the
  thicker `minor/major` points could not be measured at 0.01 mm (600 s timeouts).
* **Cone — `K` > 1 is established for the class, but not at the regime point.** At
  `top/bottom` = 0.8 the dense walk stays below 1: best 0.9939, all six edge intervals below 1.
  Yet the spine there was still rising at its last rung. The crossing comes from one
  spot-checked coordinate, 0.998, measured three times with the same `a`, at one `d`; that
  coordinate was not walked. 1.0018 is a floor on both counts.
* **Fillet blend — no measured interval reaches 1.** That is not a bound below 1: below
  0.1 mm the spine oscillates within [0.9518, 0.9818] without a turnover, the walk stopped on
  cost at 0.01 mm, and `n` = 0 is not shown safe.

---

## 6. Totals

Every count is computed by one script over `runs.tsv`. **498 runs**:

| | runs | new probes (rep 1) | re-runs (rep ≥ 2) |
|---|---|---|---|
| gate (§1.2) | 21 | 21 rows, 11 distinct probes | — |
| sentinel arms (§1.3) | 6 | — | — |
| Stage A | 81 | 68 | 13 |
| Stage B | 144 | 126 | 18 |
| Stage C | 246 | 208 | 38 |

A probe is a distinct (class, subject, `d`). The gate's nine §1.2 subject probes ran through
both harnesses (§1.4), and the committed torus subject at 10 mm was reached three ways, so its
21 rows are 11 probes. Twelve later rep-1 probes repeat an earlier stage's `d`: the three
regime points at 0.3 mm (gate and Stage A), and 0.29 and 0.3 mm on each class plus cone 0.005
and 0.006 mm and fillet blend 0.04 mm (Stages A and B). No Stage-C probe repeats an earlier
`d`. That makes **401 distinct probes over 492 measurement runs, 91 repeats**: 77 probes ran
more than once, and none returned a different `a`. Distinct `d` at the three regime points:
torus 132, cone 142, fillet blend 115.

Every measurement run printed the §0 Caveat-2 datum line except **2 `TIMEOUT`s**, the torus
`minor/major` = 0.05 and 0.5 spot-checks at 0.01 mm. Those are cost results. There were no
`NO-DATUM` and no `SED-FAILED` among measurement runs. The six sentinel-arm runs produced their
intended tokens (2 `NO-DATUM`, 1 `SED-FAILED`, 2 `TIMEOUT`) plus the fine-literal datum.

The finest `d` measured is 0.00475 mm (cone; torus 0.00994 mm, fillet blend 0.00998 mm). The
most expensive datum took 477 s (torus, 0.009964 mm); the most expensive run was a 600 s
`TIMEOUT`. No class met a budget wall on its spine.
