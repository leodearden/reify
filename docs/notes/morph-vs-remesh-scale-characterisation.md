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
1.98 s, a 2.9x increase. The morph arm is the deliberately serial,
unpreconditioned Jacobi-CG path, and this is what that costs at 61,617 DOF.

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
