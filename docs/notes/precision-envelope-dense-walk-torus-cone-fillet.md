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
| HEAD | `a030ee7528` (branch `task/7463`); last `crates/` commit `d2cf03022b` (2026-10-10 21:41) |
| binary vs HEAD | 134 `crates/` files differ between `6e0047c757` and HEAD. On the measurement path, `crates/reify-kernel-occt` and `crates/reify-eval/src/tolerance_combine.rs` are unchanged, and `crates/reify-cli/src` differs by one comment-only commit (`14a0c4c5c2`, `dev.rs`). `reify-kernel-gmsh` changed (FEA meshing, not facet deviation). The reproduction gate (§1.2) is the empirical check |
| kernel | OCCT 7.8: 26 `libTK*.so.7.8` ldd lines, 26 distinct sonames, from `/usr/lib/x86_64-linux-gnu` (`libTKernel.so.7.8.1`); 16 are direct `NEEDED` entries of the binary. 27 `libTK*.so.7.9` lines resolve from `/opt/reify-deps/lib`; the binary needs none of them directly, while `libgmsh.so.4.15` needs 19 |
| machine | AMD Ryzen 9 3950X, 16C/32T (same box as the main note's §0), Linux 7.0.0-31 |
| load | 97.92 1-min loadavg at session start; 147 – 153 at probe start across §1.2's gate. Per §0 Caveat 1 this moves wall clocks only |

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
