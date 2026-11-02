# Morph vs. from-scratch remesh: measured wall-clock at 10K and 100K elements

Measured digest for the characterisation harness
`crates/reify-mesh-morph/tests/morph_scale_characterisation.rs` (task #6638).
The harness is the apparatus; this note is its output. It is the authoritative
copy — the harness's module doc states purpose and usage and points here rather
than carrying tables that drift against the code they sit in.

Run it with:

```text
cargo test -p reify-mesh-morph --test morph_scale_characterisation -- --ignored --nocapture --test-threads=1
```

## Why it was measured

Task #2953 ("end-to-end slider-responsiveness benchmark", PRD
`docs/prds/v0_3/mesh-morphing.md:139`) asserts a **>=10x wall-clock reduction**
for morph-vs-always-remesh at the 100K element scale. That threshold had no
measurement basis in this repo: nothing in `reify-mesh-morph` read a clock, and
the only number the PRD offered was the design-time estimate at
`docs/prds/v0_3/mesh-morphing.md:11` — "at 100K elements, that's ~3s serial /
~0.3s parallel per tick of mesh time" — which carries no host, no fixture and no
provenance.

The harness measures both arms on the same bracket geometry at the same two
scales on one host, so a threshold can be derived from a measurement rather than
from an estimate. It is a harness, not a gate: nothing in it asserts a
performance property.

## The measurement

Recorded 2026-08-26 at commit `8690244f66` on an AMD Ryzen 9 3950X (16 cores /
32 threads), gmsh 4.15.2 from `/opt/reify-deps`, **release profile**,
`--test-threads=1`. Verbatim harness output:

```text
surface n=8   volume_tets=9936    surface_tris=1884    surface_verts=944
surface n=18  volume_tets=108756  surface_tris=9284    surface_verts=4644
gmsh ladder input: the n=18 surface (9284 tris) for every rung
gmsh  mesh_size=0.060 tets=2786     nodes=2590     wall=759.511558ms  Ok
gmsh  mesh_size=0.045 tets=5009     nodes=3732     wall=1.246024003s  Ok
gmsh  mesh_size=0.035 tets=9079     nodes=5481     wall=685.374063ms  Ok
gmsh  mesh_size=0.028 tets=15633    nodes=7922     wall=895.190798ms  Ok
gmsh  mesh_size=0.022 tets=30922    nodes=13201    wall=1.187368702s  Ok
gmsh  mesh_size=0.017 tets=63121    nodes=22297    wall=1.484223142s  Ok
gmsh  mesh_size=0.014 tets=109078   nodes=34748    wall=1.979128648s  Ok
morph n=8   tets=9936     nodes=2169   dof=6507   wall=536.853352ms   Ok
morph n=18  tets=108756   nodes=20539  dof=61617  wall=123.981639207s Ok
PAIR  n=8   morph_tets=9936    gmsh_tets=9079    mismatch=-8.6%
            morph_wall=536.853352ms  gmsh_wall=685.374063ms  gmsh/morph=1.28x
PAIR  n=18  morph_tets=108756  gmsh_tets=109078  mismatch=+0.3%
            morph_wall=123.981639207s gmsh_wall=1.979128648s gmsh/morph=0.02x
```

Release is the headline because it is the profile under which the morph arm
looks **best**; see [Profile invariance](#profile-invariance) for the dev-profile
counterpart.

## What this says about #2953

At the best-matched pairing in the whole table (+0.3% count mismatch, the two
arms within 322 tets of each other) the morph took **123.98 s** and the
from-scratch remesh **1.98 s** — the morph is **~63x SLOWER**. Against a
>=10x-*faster* target that is roughly three orders of magnitude in the wrong
direction: #2953's premise is not merely unmet at 100K, it is **inverted**.

The 100K morph returned **`Ok`**, not `SolverNotConverged`. That is not a solver
giving up; it is 124 s of converged solving. The distinction matters, because
"it timed out" invites a tuning fix and "it converged, slowly" does not.

At 10K the morph does win, but modestly: 536.9 ms vs 685.4 ms, i.e. 1.28x. That
margin is smaller than the -8.6% count mismatch on the same pairing, so the
table supports "roughly par at 10K", not a quantified advantage.

Against the other figure the harness was built to test: the PRD's "~3s serial
per tick" at 100K measured **123.98 s**, ~41x the estimate.

## Where the inversion comes from

Over 9.9K -> 108.8K elements (10.9x) the morph goes 0.537 s -> 123.98 s, a 231x
increase — roughly `O(N^2.2)`. Over 9.1K -> 109.1K (12.0x) gmsh goes 0.685 s ->
1.98 s, a 2.9x increase.

**CORRECTION 2026-09-23.** The original text here read "the morph arm is the
deliberately serial, unpreconditioned Jacobi-CG path, and this is what that
costs at 61,617 DOF." That attribution was a guess and it is wrong. The harness
was rebuilt at main HEAD (release) and the n=18 morph leg sampled 104 times
with gdb (`perf` is unavailable on this host: `perf_event_paranoid=4`), with
an exact CG iteration count taken by a breakpoint on
`reify_solver_elastic::solver::spmv_seq`:

```text
top frame, 104 samples over the n=18 morph leg
 100  96.2%  reify_solver_elastic::boundary::dirichlet::apply_dirichlet_row_elimination
   2   1.9%  reify_solver_elastic::assembly::global::assemble_global_stiffness
   1   1.0%  sysmalloc
   1   1.0%  (pre-timer input prep in the harness)
   0   0.0%  any CG / SpMV / faer symbol
CG iterations: n=18 -> 208 (61,617 DOF), n=8 -> 89 (6,507 DOF), both converged
baseline wall-clock this run: n=8 592 ms, n=18 156.1 s (loaded host)
```

The cost is `apply_dirichlet_row_elimination`
(`crates/reify-solver-elastic/src/boundary/dirichlet.rs`): for every
prescribed DOF it scans all `n` rows of K, so it is O(n × |bcs|). The morph
pins every surface node — 13,932 prescribed DOFs at n=18 against 61,617 rows —
which is exactly the "pinned-surface scale" its then-current `# Complexity`
note named as the case where a column-indexed mirror is needed. The CG solve is at most
~3% of the leg (rule-of-three bound on 0/104 samples; 208 iterations). Fix:
task #7834. Expected post-fix n=18 morph leg: single-digit seconds — roughly
par with gmsh, not 10x faster. The re-measurement, #7834's acceptance step, is
recorded in [After the Dirichlet fix](#after-the-dirichlet-fix-7834-2026-09-24)
below.

The two curves cross just above the 10K scale, which is why a measurement taken
only at 10K would have supported the PRD's premise and a measurement at 100K
destroys it.

## Reading the ladder honestly

The three coarsest gmsh rungs are NOT monotone in wall-clock (0.060 -> 759 ms,
0.045 -> 1.246 s, 0.035 -> 685 ms). Below ~15K tets the rung time is dominated by
a fixed per-call setup cost of roughly 0.7 s, not by the meshing work, so
rung-to-rung differences down there are noise. Only the three finest rungs are in
a regime where the ladder is informative.

## Profile invariance

The same harness under the default **dev** profile, same host, same commit
`8690244f66`:

```text
gmsh  mesh_size=0.060 tets=2786     wall=2.096886588s  Ok
gmsh  mesh_size=0.045 tets=5009     wall=1.240103105s  Ok
gmsh  mesh_size=0.035 tets=9079     wall=851.066645ms  Ok
gmsh  mesh_size=0.028 tets=15633    wall=899.449821ms  Ok
gmsh  mesh_size=0.022 tets=30922    wall=1.457362516s  Ok
gmsh  mesh_size=0.017 tets=63121    wall=2.387901194s  Ok
gmsh  mesh_size=0.014 tets=109078   wall=4.642405879s  Ok
morph n=8   tets=9936    dof=6507   wall=1.213329894s   Ok
morph n=18  tets=108756  dof=61617  wall=123.129397408s Ok
PAIR  n=8   mismatch=-8.6%  gmsh/morph=0.70x
PAIR  n=18  mismatch=+0.3%  gmsh/morph=0.04x
```

The 100K morph leg is **123.13 s under dev vs 123.98 s under release** — 0.7%
apart. That is worth stating explicitly, because the obvious objection to any
Rust-side timing ("you measured a debug build") does not apply to the number the
whole harness turns on. Workspace `Cargo.toml` (task 4055) sets
`[profile.dev.package."*"] opt-level = 3` and
`[profile.dev.package.reify-solver-elastic] opt-level = 2` precisely because faer
is ~500-1000x slower unoptimised, so the morph's hot path — faer's CG and
`reify-solver-elastic`'s assembly — is compiled at optimised codegen under BOTH
profiles.

So the inversion does not rest on a profile choice: the morph is ~63x slower
than the remesh under release and ~27x slower under dev.

Where the two profiles DO diverge is exactly where this note already warns the
reader not to look — the coarse rungs and the n=8 morph, whose times are
dominated by fixed setup and unoptimised marshalling (gmsh's 0.060 rung moves
0.76 s -> 2.10 s while its 0.045 rung does not move at all). The dev run is
therefore also independent corroboration that the coarse end of the ladder is
noise rather than signal.

## Reproducibility, and what to quote

Re-run 2026-09-16 at commit `63ceb1c7f9` (dev profile, same host) after the
review-amendment passes — which deferred `boundary_surface`'s orientation step to
the emitted survivors, moved it into its own module and pre-sized its tables.

Every STRUCTURAL number reproduced EXACTLY: both surface extractions (n=8 gives
1884 tris / 944 verts, n=18 gives 9284 / 4644), all seven gmsh achieved tet
counts (2786, 5009, 9079, 15633, 30922, 63121, 109078) and both morph legs (9936
tets / 6507 dof, 108756 / 61617). That is the check that matters for an
extractor refactor: an identical achieved gmsh ladder off the same extracted
surface is end-to-end evidence that what gmsh was handed did not change.

The wall-clocks moved, on a host quiesced for neither run: the 100K morph leg
re-measured 168.56 s against the 123.13 s of the dev run above, and gmsh's finest
rung 3.22 s against 4.64 s. The headline did not move — `gmsh/morph = 0.02x` at
100K, the same figure the release table reports (morph ~52x slower here against
~63x there).

The 10K pairing came out at 0.75x, against 0.70x for the earlier dev run and
1.28x under release. Which arm wins at 10K therefore depends on the profile,
which is the concrete form of "roughly par": the n=8 morph leg is the one the
[Profile invariance](#profile-invariance) section flags as dominated by
unoptimised marshalling.

**Quote the sign and the order of magnitude; re-measure the multiplier.** These
are single-run numbers from one host, with a residual count mismatch on each
pairing, on a host quiesced for no run. They bound the ratio's order of
magnitude; they do not pin a threshold. Nothing here should be promoted to a
CI-blocking bound without repetition and the statistics the harness deliberately
does not collect. The 100K result is large enough (63x, on a +0.3% pairing) that
run-to-run variance cannot plausibly account for its SIGN.

## After the Dirichlet fix (#7834, 2026-09-24)

Re-measured after `apply_dirichlet_row_elimination` moved from a per-BC scan of
every row to a once-per-call index of the constrained columns — O(nnz + n) per
call, independent of the number of prescribed DOFs. Recorded 2026-09-24 at
commit `5496aca9f3` (the #7834 branch head when measured: the rewrite plus its
guards and docs) on the same AMD Ryzen 9 3950X (16 cores / 32 threads), gmsh
4.15.2 from `/opt/reify-deps`, **release profile**, `--test-threads=1`:

```text
cargo test -p reify-mesh-morph --test morph_scale_characterisation --release -- --ignored --nocapture --test-threads=1
```

Nothing in the harness, fixture, mesh sizes, CG options or modes changed. The
host was shared and quiesced for neither run. Load averages (1 / 5 / 15 min, on
32 hardware threads) were 80.34 / 150.47 / 143.20 before run 1, 70.08 / 139.06 /
139.66 after it, 64.62 / 128.97 / 136.24 before run 2 and 69.85 / 125.95 /
135.09 after it. Both runs are shown in the order taken; neither was selected.

Verbatim harness output, with the `[task-6638] ` prefix stripped and each PAIR
line wrapped after its `mismatch=` field as in the 2026-08-26 block (the
continuation is otherwise unaltered, `(gmsh mesh_size=…)` suffix included).
Run 1:

```text
surface n=8   volume_tets=9936    surface_tris=1884    surface_verts=944
surface n=18  volume_tets=108756  surface_tris=9284    surface_verts=4644
gmsh ladder input: the n=18 surface (9284 tris) for every rung
gmsh  mesh_size=0.060 tets=2786     nodes=2590     wall=984.760822ms Ok
gmsh  mesh_size=0.045 tets=5009     nodes=3732     wall=1.125640838s Ok
gmsh  mesh_size=0.035 tets=9079     nodes=5481     wall=982.074543ms Ok
gmsh  mesh_size=0.028 tets=15633    nodes=7922     wall= 1.10055991s Ok
gmsh  mesh_size=0.022 tets=30922    nodes=13201    wall=1.095318884s Ok
gmsh  mesh_size=0.017 tets=63121    nodes=22297    wall=1.082351434s Ok
gmsh  mesh_size=0.014 tets=109078   nodes=34748    wall=1.626825202s Ok
morph n=8   tets=9936     nodes=2169     dof=6507     wall=261.709984ms Ok
morph n=18  tets=108756   nodes=20539    dof=61617    wall=5.646647045s Ok
PAIR  n=8   morph_tets=9936     gmsh_tets=9079     mismatch=-8.6%
            morph_wall=261.709984ms gmsh_wall=982.074543ms gmsh/morph=3.75x (gmsh mesh_size=0.035)
PAIR  n=18  morph_tets=108756   gmsh_tets=109078   mismatch=+0.3%
            morph_wall=5.646647045s gmsh_wall=1.626825202s gmsh/morph=0.29x (gmsh mesh_size=0.014)
```

Run 2, taken immediately after:

```text
surface n=8   volume_tets=9936    surface_tris=1884    surface_verts=944
surface n=18  volume_tets=108756  surface_tris=9284    surface_verts=4644
gmsh ladder input: the n=18 surface (9284 tris) for every rung
gmsh  mesh_size=0.060 tets=2786     nodes=2590     wall=974.128269ms Ok
gmsh  mesh_size=0.045 tets=5009     nodes=3732     wall=1.425027622s Ok
gmsh  mesh_size=0.035 tets=9079     nodes=5481     wall=1.428426511s Ok
gmsh  mesh_size=0.028 tets=15633    nodes=7922     wall=850.924793ms Ok
gmsh  mesh_size=0.022 tets=30922    nodes=13201    wall=1.095975131s Ok
gmsh  mesh_size=0.017 tets=63121    nodes=22297    wall=2.702342281s Ok
gmsh  mesh_size=0.014 tets=109078   nodes=34748    wall=1.811810987s Ok
morph n=8   tets=9936     nodes=2169     dof=6507     wall=311.490648ms Ok
morph n=18  tets=108756   nodes=20539    dof=61617    wall= 6.19706894s Ok
PAIR  n=8   morph_tets=9936     gmsh_tets=9079     mismatch=-8.6%
            morph_wall=311.490648ms gmsh_wall=1.428426511s gmsh/morph=4.59x (gmsh mesh_size=0.035)
PAIR  n=18  morph_tets=108756   gmsh_tets=109078   mismatch=+0.3%
            morph_wall= 6.19706894s gmsh_wall=1.811810987s gmsh/morph=0.29x (gmsh mesh_size=0.014)
```

Reading:

- **The 100K morph leg fell by more than an order of magnitude**: 5.65 s and
  6.20 s, against 123.98 s under release on 2026-08-26 (roughly 20x less) and
  156.1 s on the loaded host of the 2026-09-23 profile. The 10K leg fell too:
  262 ms and 311 ms, against 536.9 ms. The drop is the size the 2026-09-23
  profile predicted, with 100 of 104 samples in the routine this fix rewrote.
  But no profile of the post-fix leg was taken, and the harness has no
  assembly/solve split, so nothing here attributes the remaining seconds
  between assembly, BC elimination and CG.
- **At 100K the morph is still slower than the remesh.** The +0.3% pairing
  (108,756 vs 109,078 tets) gives `gmsh/morph = 0.29x` in both runs: the morph
  takes ~3.5x as long as single-threaded gmsh, where before it took ~63x as
  long. The sign is unchanged; the order of magnitude is not.
- **At 10K the morph now wins.** The -8.6% pairing (9,936 vs 9,079 tets) gives
  `gmsh/morph = 3.75x` and `4.59x`, a margin that now exceeds the count
  mismatch. But the paired gmsh rung sits in the fixed-setup regime described in
  [Reading the ladder honestly](#reading-the-ladder-honestly), so read this as
  "the morph wins at 10K", not as a multiplier. The dev profile was not
  re-measured.
- **The "single-digit seconds, roughly par, not 10x faster" expectation held,
  in its "possibly slower" form.** The 100K leg is single-digit seconds and
  nowhere near 10x faster than the remesh. "Roughly par" holds only in order of
  magnitude: the morph remains ~3.5x slower. So #2953's >=10x-faster premise is
  still unmet at 100K. It is no longer inverted by more than an order of
  magnitude, but it is still on the wrong side of 1x.

As everywhere in this note: quote the sign and the order of magnitude, and
re-measure the multiplier.

## After the extractor collapse (#7596, 2026-11-02)

The gmsh arm's input surface now comes from
`reify_solver_elastic::boundary_surface_mesh` instead of the harness-local
extractor (`tests/calibration/boundary.rs::boundary_surface`, deleted by #7596).
The two emit the same oriented triangle set. A throwaway check, run before the
deletion, compared both surfaces in position space, with each triangle rotated
(never reflected) to a canonical start vertex. The sets were equal at n = 4, 8
and 18, and both the index order and the vertex order differed at every n. The
library extractor emits triangles in element order rather than sorted, and
vertices in ascending original index rather than in first-sighting order.
Nothing else changed: harness, fixture, mesh sizes, CG options and modes are as
before.

Recorded 2026-11-02 on the same AMD Ryzen 9 3950X (16 cores / 32 threads), gmsh
4.15.2 from `/opt/reify-deps`, **release profile**, `--test-threads=1`, with the
same command as the previous section:

```text
cargo test -p reify-mesh-morph --test morph_scale_characterisation --release -- --ignored --nocapture --test-threads=1
```

Main had moved since 2026-09-24. A same-day baseline was therefore taken at the
unmodified base, commit `62df8a0d34`, so that any change in a gmsh count can be
attributed to the extractor swap alone. The post-collapse runs are at
`3c189a00a8`, the task/7596 commit "refactor(7596): collapse the harness-local
boundary extractor onto boundary_surface_mesh". The later #7596 commits touch
only documentation and a `#[cfg(test)]` test in `reify-solver-elastic`, nothing
the driver executes. The host was shared and quiesced for no run.
Load averages (1 / 5 / 15 min, on 32 hardware threads), in the order taken:

- baseline: 157.81 / 126.71 / 129.29 before the invocation, which included a
  7m19s release build; 97.38 / 112.95 / 125.86 after the run;
- post-collapse, with the release build done beforehand by a separate
  `--no-run`: 117.06 / 201.91 / 214.06 before run 1, 118.96 / 199.50 / 213.15
  after run 1 (also before run 2, which began immediately), and
  122.68 / 196.36 / 211.90 after run 2.

All three runs are shown in the order taken; none was selected. The output is
verbatim, `[task-6638]` lines only, formatted as in the previous section.

Baseline (pre-collapse, `62df8a0d34`):

```text
surface n=8   volume_tets=9936    surface_tris=1884    surface_verts=944
surface n=18  volume_tets=108756  surface_tris=9284    surface_verts=4644
gmsh ladder input: the n=18 surface (9284 tris) for every rung
gmsh  mesh_size=0.060 tets=2786     nodes=2590     wall=816.080106ms Ok
gmsh  mesh_size=0.045 tets=5009     nodes=3732     wall=1.116731343s Ok
gmsh  mesh_size=0.035 tets=9079     nodes=5481     wall=717.028515ms Ok
gmsh  mesh_size=0.028 tets=15633    nodes=7922     wall=1.725377217s Ok
gmsh  mesh_size=0.022 tets=30922    nodes=13201    wall=1.336099373s Ok
gmsh  mesh_size=0.017 tets=63121    nodes=22297    wall=1.292885929s Ok
gmsh  mesh_size=0.014 tets=109078   nodes=34748    wall= 2.10633102s Ok
morph n=8   tets=9936     nodes=2169     dof=6507     wall=260.392382ms Ok
morph n=18  tets=108756   nodes=20539    dof=61617    wall=6.620130145s Ok
PAIR  n=8   morph_tets=9936     gmsh_tets=9079     mismatch=-8.6%
            morph_wall=260.392382ms gmsh_wall=717.028515ms gmsh/morph=2.75x (gmsh mesh_size=0.035)
PAIR  n=18  morph_tets=108756   gmsh_tets=109078   mismatch=+0.3%
            morph_wall=6.620130145s gmsh_wall= 2.10633102s gmsh/morph=0.32x (gmsh mesh_size=0.014)
```

Post-collapse run 1 (`3c189a00a8`):

```text
surface n=8   volume_tets=9936    surface_tris=1884    surface_verts=944
surface n=18  volume_tets=108756  surface_tris=9284    surface_verts=4644
gmsh ladder input: the n=18 surface (9284 tris) for every rung
gmsh  mesh_size=0.060 tets=2683     nodes=2439     wall= 526.97401ms Ok
gmsh  mesh_size=0.045 tets=4900     nodes=3570     wall=603.682761ms Ok
gmsh  mesh_size=0.035 tets=8969     nodes=5296     wall=911.225895ms Ok
gmsh  mesh_size=0.028 tets=15509    nodes=7759     wall=882.170482ms Ok
gmsh  mesh_size=0.022 tets=30779    nodes=12983    wall=870.236468ms Ok
gmsh  mesh_size=0.017 tets=62731    nodes=21988    wall=1.489727978s Ok
gmsh  mesh_size=0.014 tets=109079   nodes=34547    wall=1.757206045s Ok
morph n=8   tets=9936     nodes=2169     dof=6507     wall=152.684786ms Ok
morph n=18  tets=108756   nodes=20539    dof=61617    wall=4.677706814s Ok
PAIR  n=8   morph_tets=9936     gmsh_tets=8969     mismatch=-9.7%
            morph_wall=152.684786ms gmsh_wall=911.225895ms gmsh/morph=5.97x (gmsh mesh_size=0.035)
PAIR  n=18  morph_tets=108756   gmsh_tets=109079   mismatch=+0.3%
            morph_wall=4.677706814s gmsh_wall=1.757206045s gmsh/morph=0.38x (gmsh mesh_size=0.014)
```

Post-collapse run 2, taken immediately after:

```text
surface n=8   volume_tets=9936    surface_tris=1884    surface_verts=944
surface n=18  volume_tets=108756  surface_tris=9284    surface_verts=4644
gmsh ladder input: the n=18 surface (9284 tris) for every rung
gmsh  mesh_size=0.060 tets=2683     nodes=2439     wall=534.143885ms Ok
gmsh  mesh_size=0.045 tets=4900     nodes=3570     wall=677.577092ms Ok
gmsh  mesh_size=0.035 tets=8969     nodes=5296     wall=980.527376ms Ok
gmsh  mesh_size=0.028 tets=15509    nodes=7759     wall=884.453667ms Ok
gmsh  mesh_size=0.022 tets=30779    nodes=12983    wall=1.174304177s Ok
gmsh  mesh_size=0.017 tets=62731    nodes=21988    wall=1.415823294s Ok
gmsh  mesh_size=0.014 tets=109079   nodes=34547    wall=2.390678639s Ok
morph n=8   tets=9936     nodes=2169     dof=6507     wall=235.106085ms Ok
morph n=18  tets=108756   nodes=20539    dof=61617    wall=3.681855423s Ok
PAIR  n=8   morph_tets=9936     gmsh_tets=8969     mismatch=-9.7%
            morph_wall=235.106085ms gmsh_wall=980.527376ms gmsh/morph=4.17x (gmsh mesh_size=0.035)
PAIR  n=18  morph_tets=108756   gmsh_tets=109079   mismatch=+0.3%
            morph_wall=3.681855423s gmsh_wall=2.390678639s gmsh/morph=0.65x (gmsh mesh_size=0.014)
```

Reading:

- **The surface counts reproduced exactly**: 1884 tris / 944 verts at n=8 and
  9284 / 4644 at n=18, in all three runs. That agrees with the set-equality
  check above.
- **The gmsh ladder did NOT reproduce, so triangle and vertex order alone moved
  HXT's output.** The baseline matches the 2026-09-24 ladder exactly, so main
  had not moved the input. The two post-collapse runs match each other exactly,
  so the new counts are deterministic and not noise. Every rung moved. Per rung,
  baseline to post-collapse:

  | mesh_size | tets before | tets after | Δ tets | Δ % | nodes before | nodes after | Δ nodes |
  |---|---|---|---|---|---|---|---|
  | 0.060 | 2786 | 2683 | -103 | -3.70% | 2590 | 2439 | -151 |
  | 0.045 | 5009 | 4900 | -109 | -2.18% | 3732 | 3570 | -162 |
  | 0.035 | 9079 | 8969 | -110 | -1.21% | 5481 | 5296 | -185 |
  | 0.028 | 15633 | 15509 | -124 | -0.79% | 7922 | 7759 | -163 |
  | 0.022 | 30922 | 30779 | -143 | -0.46% | 13201 | 12983 | -218 |
  | 0.017 | 63121 | 62731 | -390 | -0.62% | 22297 | 21988 | -309 |
  | 0.014 | 109078 | 109079 | +1 | +0.001% | 34748 | 34547 | -201 |

  The tet counts fell by 0.5-3.7% on the six coarser rungs. On the finest rung
  they rose by a single tet, and the node count still fell by 201. This is the
  converse of the [Reproducibility](#reproducibility-and-what-to-quote)
  section's check: there, an identical ladder showed that what gmsh was handed
  did not change. Here the triangle set is identical and the ladder is not, so
  the ORDER of the surface is part of what gmsh is handed.
- **The morph legs' counts are unchanged** (9936 tets / 6507 dof and 108756 /
  61617), as they must be: the morph never sees the extracted surface, because
  it builds its Dirichlet data from `fixtures::bracket`'s own surface-node
  indices. Their wall-clocks still moved. At 100K the leg took 6.62 s in the
  baseline and 4.68 s and 3.68 s after the collapse, with nothing it computes
  changed. That spread is host noise, and it is the scale to read the ratios
  against.
- **The pairings.** The 10K pairing still joins the 0.035 rung, now 8,969 gmsh
  tets against 9,936 morph tets (mismatch -9.7%, was -8.6%). The 100K pairing
  joins 109,079 against 108,756 (+0.3%, unchanged to one decimal).
  `gmsh/morph` at 100K is 0.38x and 0.65x after the collapse, against 0.32x in
  the same-day baseline (0.29x on 2026-09-24). At 10K it is 5.97x and 4.17x,
  against 2.75x. The spread across the three same-day runs is mostly the morph
  leg's noise described above, not the swap.
- **#2953's sign at 100K holds.** The morph is slower than the remesh in all
  three runs (ratio below 1x), within the same order of magnitude, so the
  >=10x-faster premise is still unmet. **The 10K winner holds**: the morph wins
  in all three runs. As before, the paired gmsh rung sits in the fixed-setup
  regime of [Reading the ladder honestly](#reading-the-ladder-honestly), so
  read it as "the morph wins at 10K", not as a multiplier.

As everywhere in this note: quote the sign and the order of magnitude, and
re-measure the multiplier.
