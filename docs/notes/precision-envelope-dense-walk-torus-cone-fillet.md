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
