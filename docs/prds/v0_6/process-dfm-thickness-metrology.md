# PRD (forward stub / DEFERRED): `std.process` thickness-metrology DFM (min-wall + min-feature)

**Status:** DEFERRED forward-design stub · **Author session:** 2026-06-08 · **Milestone:** v0_6+
**Splits from:** `docs/prds/v0_6/process-dfm-geometry-metrology.md` (superseded) — this is the
**research-gated half**; the ship-now half (overhang + draft) is `process-dfm-overhang-draft.md`.
**Relationship:** consumes the auto-measurement engine + `DFMRule.subject : Solid` activated by
`process-dfm-overhang-draft.md`; adds the two numerically-hard thickness measurements.

> This is a **stub**, not a ready-to-decompose PRD. It records scope, the substrate gap, the honest
> numeric floor, and the known hazards so the excluded work is tracked, not dropped. It must be
> promoted to a full B+H PRD (re-walk all gates, prototype the SDF wire, set the resolution-bounded
> floor, decide the non-convex correctness scope) before activation. Do **not** decompose it as-is.
> **Promotion warrants a fresh `/prd` session** — the feasibility below is the hand-off.

---

## §0 — Why this is deferred (the substrate gap)

`process-dfm-overhang-draft.md` ships the auto-measurement engine for the *surface-normal-vs-direction*
family (overhang, draft) because every primitive already exists. The **thickness** family
(min-wall, min-feature) is genuinely research-grade and gated on an **unbuilt eval-reachable
solid→SDF wire** — not on missing algorithm research.

| Measurement | Process category | Approach (most reachable) | Hazard |
|---|---|---|---|
| **Min wall thickness** | `Subtracting`/`Adding`/`Parting` | Voxel-SDF bidirectional gradient walk (`d⁺ + d⁻`) | Resolution-bounded **lower bound** (G6); non-convex sign-monotonicity; `not(has_openvdb)` degradation |
| **Min feature size** | several | SDF ridge-min (`2 × min interior \|φ\| at a ridge`) on the same substrate | Definition ambiguity — pinned to *thinnest solid cross-section* (NOT edge/face/gap) |

### The exact substrate finding (2026-06-08 feasibility sweep)

- **The thickness primitive EXISTS and is eval-wired.** `reify-shell-extract::bidirectional_distances`
  (`crates/reify-shell-extract/src/medial.rs:863`) walks the SDF gradient in `+g` and `−g` to
  opposing surfaces; local wall thickness `= d⁺ + d⁻`. It runs in parallel and is regression-tested
  on slab/sphere fixtures, consumed via `shell_extract_compute.rs`.
- **The kernel SDF primitive EXISTS and is on main.** Task **#3095** (top-level, `done`, merged
  `70f0449139`) built B-rep→Voxel narrow-band-SDF realization + the per-voxel SDF query API in
  `reify-kernel-openvdb` (`realize_voxel_from_mesh`, `sample_sdf_at`). Mesh→Voxel conversion
  descriptor **#3438** is `done` too. *(Note: the historical subtask id `3095.2` is deprecated /
  never scheduled; cite the top-level `#3095`.)*
- **The eval-reachable wire is the gap (a C-17-shape orphan).** Those primitives are **not on the
  `GeometryKernel` trait** (its Voxel ops return `OperationFailed`), `reify-eval` **never
  instantiates `OpenVdbKernel` in production**, and they are called **only from reify-eval test
  code**. The one production SDF-into-eval path is `read_vdb_file` (importing a *pre-computed*
  `.vdb`). So "get an SDF from a realized solid during `reify check`" does not exist as a production
  path.
- **The shell-extract migration that would have built it is CANCELLED.** `shell_extract_compute.rs:11-15`
  defers the realization-read API ("migrate the SDF from the γ-only `value_inputs[1]` seam to
  `realization_inputs[0]`") to "δ/ε/ζ" — but shell-extract bridge **δ (#3551) is `cancelled`** and
  γ/γ.1 (#3834/#3886) are `cancelled`. **No active/pending task owns the eval-reachable solid→SDF
  wire.** This PRD owns it (G4 below).

---

## §1 — Consumer (G1, provisional)

A DFM-minded designer whose part has thin walls / thin features and who wants `reify check` to flag
them **without** declaring the measured thickness — the engine realizes `subject : Solid`, builds an
SDF, measures the thinnest wall/feature, and compares against the process capability
(`min_feature_size`, already a param on `Subtracting`/`Adding`/`Parting`). Reuses the
auto-measurement check-time pass from `process-dfm-overhang-draft.md`; adds a thickness measurement
arm.

---

## §2 — Sketch (provisional, to be hardened on promotion)

1. **Build the eval-reachable solid→SDF wire (this PRD OWNS it).** Either (a) put the Voxel/SDF ops
   on `GeometryKernel` + instantiate the OpenVDB kernel in the eval check path, or (b) route a
   BRep→Mesh→Voxel conversion through the multi-kernel dispatcher (descriptors #3095/#3438 done) to
   a queryable `SampledField`. The chain `tessellate(solid) → realize_voxel_from_mesh → SampledField`
   exists in pieces; this PRD assembles it.
2. **Min-wall measurement.** Reuse `bidirectional_distances`; add a **min-reduction** over `d⁺ + d⁻`
   across medial voxels (today `compute_medial_mask` discards the distances — `medial.rs:497-501`).
3. **Min-feature measurement.** SDF ridge-min on the same substrate: `2 × min over ridge points of
   |φ|`. Ships only after the wire (#2) lands.
4. **Wire into the auto-measurement pass** (overhang-draft PRD) as a new category arm; emit
   `{W,E}_DFM_MIN_WALL` / `_MIN_FEATURE` via `dfm::diagnose`.

---

## §3 — Honest numeric floor (G6 — CRITICAL, domain = numerical)

Min-wall is a **sampled estimate, never exact** (the brief's esc-3453 guessed-% / esc-3770
impossible-1e-12 failure modes). The floor is tied to a **measurable resolution parameter**, not a
guessed percentage:

> `min_wall_thickness(solid)` returns a **conservative lower bound** at voxel resolution `h =
> voxel_size`, accurate to `± (h + chord_tol)` where `chord_tol` is the BRep tessellation chord
> tolerance. Features thinner than `~2h` (below the narrow-band half-width) may be missed and are
> reported as such, never silently rounded. The value is biased low so a *passing* DFM check is
> trustworthy.

**RED test asserts an INEQUALITY on a known-thickness fixture** (2.0 mm-walled box, voxel size `h`):
```
let t = min_wall_thickness(box_2mm);
assert!((t - 2.0mm).abs() <= h + chord_tol);  // resolution band, NOT an exact float
assert!(t <= 2.0mm + h);                       // conservative-lower-bound contract
```
Pin `chord_tol` to a fixed constant (or use an analytic-box fixture so `chord_tol = 0`, tightening
the band to `±h`). **Never** assert a relative %; **never** assert machine-epsilon.

**Min-feature definition (pinned, anti-ambiguity):** the **thinnest solid cross-section** =
`2 × min interior SDF magnitude at a local ridge`. It measures the narrowest material (thin wall /
rib / web). It does **NOT** measure edge length, face diameter, hole diameter, or gap-between-surfaces
(that overlaps the existing `Distance`/`min_clearance` query).

---

## §4 — Hazards to resolve on promotion

- **Non-convex sign-monotonicity.** `walk_to_zero` assumes sign-monotonicity along the gradient
  (`medial.rs:881-897`); non-convex thin solids (C-channels) can return the first crossing, not the
  geometric nearest. The promotion must either validate on a non-convex fixture (L-bracket /
  C-channel) before trusting the inequality, or restrict the v1 honest scope to convex-ish walls.
- **`not(has_openvdb)` degradation.** `cfg(has_openvdb)` is environment-conditional. A stub build
  has **no** SDF capability — the query must return a self-describing `Undef` + diagnostic, never a
  fake number. The RED test must cover skip-or-degrade on stub builds.
- **Tessellation error stacks on voxel error.** The SDF is built from a tessellated mesh, not the
  exact BRep — the floor band must include `chord_tol` (or use an analytic fixture).
- **Performance.** The medial walk is `O(N³ · max_thickness_voxels)`; at 256³ it is heavy and
  currently un-optimized. Bound the grid size or accept a coarse default `h` (which widens the
  honest floor — a clean, stated trade-off). May warrant §3.4 ComputeNode wrapping (≥~50 ms).

---

## §5 — Cross-PRD seam ownership (G4)

| Seam | Owner |
|---|---|
| Auto-measurement check-time pass + `DFMRule.subject : Solid` + `dfm::diagnose` | `process-dfm-overhang-draft.md` (**upstream** — this PRD consumes + adds a thickness arm) |
| Eval-reachable **solid→SDF wire** (`GeometryKernel` Voxel ops + eval instantiation, OR multi-kernel BRep→Mesh→Voxel route) | **THIS PRD** (no active task owns it; shell-extract's migration is cancelled) |
| Kernel SDF primitive (`realize_voxel_from_mesh` / `sample_sdf_at`) + Mesh→Voxel descriptor | `#3095` / `#3438` (**done, on main**) — substrate this PRD builds on, **not** a dependency on pending work |
| Thickness walk (`bidirectional_distances` / `compute_medial_mask`) | `reify-shell-extract` (exists, eval-wired) — reuse + add a min-reduction |
| Min-wall / min-feature measurements + DFM arm | **THIS PRD** |

---

## §6 — Promotion checklist (before activating)

- [ ] Prototype the eval-reachable solid→SDF wire (§2.1) — the one real unknown; decide trait-op vs
      multi-kernel-route.
- [ ] Set the resolution-bounded floor (§3) and write the inequality RED test on a known-thickness
      fixture; pin `chord_tol`.
- [ ] Decide the non-convex correctness scope (validate on a C-channel, or restrict v1 to convex).
- [ ] Specify `not(has_openvdb)` self-describing degradation.
- [ ] Pin the min-feature definition + add the min-reduction to `compute_medial_mask`.
- [ ] Re-walk G1–G6 + META; write the capability manifest. Cite `#3095`/`#3438` as done substrate
      (top-level ids — `3095.2` is a deprecated subtask).
