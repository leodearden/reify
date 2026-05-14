# PRD: Structural Stability / Buckling Analysis

Status: stub — deferred, candidate v0.5+. Sibling to v0.3 `structural-analysis-fea.md` and v0.4 `structural-analysis-shells.md`. Filed 2026-05-05 from shells PRD spillover.

## Goal

Add linear (eigenvalue) buckling analysis to Reify's structural stack, so that designers can compute critical buckling loads for slender structures — columns, panels, shells under compression. Linear elastic stress alone is insufficient for these designs; a structure can have low stress but buckle at a fraction of the design load.

## Background

Buckling failure is structurally distinct from stress failure:

- A perfectly straight Euler column of yield-stress steel buckles at much less than yield-stress for slenderness ratios above ~100.
- Thin-walled cylinders under axial compression buckle at <30% of theoretical stress capacity.
- Sandwich panels and stiffened skins fail by local buckling long before material yield.

Linear buckling analysis solves the eigenvalue problem `(K + λ K_g) φ = 0`, where K is the linear stiffness matrix, K_g is the geometric (stress-induced) stiffness from a reference load case, λ is the load multiplier at buckling (eigenvalue), and φ is the buckled shape (eigenvector). The first few eigenvalues give critical buckling loads in increasing order.

This is a *separate analysis kind* from linear-elastostatic — different solver call, different result type, different post-processing — but it reuses the kernel infrastructure (element matrices, mesh, BCs, materials).

## Why deferred to v0.5+

- v0.3 linear-elastostatic FEA must ship and validate first (kernel, BC framework, materials).
- v0.4 shell elements are particularly critical for buckling — slender structures that buckle are usually thin enough to be shell-modeled. Buckling on a tet-only foundation would underserve the typical use case.
- Eigenvalue solver is a significant addition to the kernel surface — Lanczos / Arnoldi / shift-invert variants — distinct from the linear-system solver in v0.3. Worth waiting until linear-static is stable so the eigenvalue addition is purely additive.
- Buckling analysis benefits substantially from non-linear pre-stress (large-deformation effects in K_g); linear-buckling alone is approximate. May benefit from waiting for non-linear pre-stress capability rather than landing linear-buckling alone.

## Sketch of approach

The kernel-surface slice — eigensolver, P1-tet K_g, `solve_buckling` stdlib entry, `BucklingResult` shape, and the GUI mode-shape-frame implementation — is contracted in `docs/prds/v0_5/buckling-eigensolver.md` (commit 8059aa59ba). The bullets below describe the user-facing product surface; consult that PRD for the authoritative kernel contract.

- **`solve_buckling(body, material, loads, supports, options) -> BucklingResult`** — separate stdlib kernel binding, sibling to `solve_elastic_static`. Internally runs a linear-static solve to compute pre-stress, assembles K_g, then eigenvalue-solves.
- **`BucklingResult`:** ordered list of `Mode { eigenvalue, mode_shape: Field<Point3, Vector3<Length>> }`. Critical load = lowest eigenvalue × reference load magnitude.
- **Eigenvalue solver:** Lanczos with shift-invert via faer-rs. Compute the lowest k modes (k user-specifiable, default 5). Geometric multiplicity handling for symmetric structures.
- **Material extension:** no extension needed — uses the same `ElasticMaterial` as linear-static.
- **Result interpretation helpers:** `critical_load(result) -> Force`, `mode_shape(result, n) -> Field<...>`, `safety_factor_buckling(result, applied_load) -> Number`.
- **GUI rendering:** mode shapes are displacement fields × eigenvalue scaling — animate by sweeping a phase parameter; render alongside undeformed geometry.

## Pre-conditions for activating

- v0.3 `structural-analysis-fea.md` kernel shipped and validated.
- v0.4 `structural-analysis-shells.md` shipped (most buckling cases are shell-dominated).
- Documented user demand (column design, pressure-vessel external pressure, panel buckling under shear).
- Technical foundation gates for the kernel-surface slice (GR-001 struct-ctor runtime, ComputeNode contract, FEA stack engine integration, GR-016 channel contract scaffold, plus forward-looking #3117 Field-in-param) are enumerated in `docs/prds/v0_5/buckling-eigensolver.md` §11.

## Open design questions

- **Linear vs. non-linear buckling.** Linear (eigenvalue) buckling assumes small pre-buckling deformation; non-linear (Riks-arc-length) tracks the full load-deflection path through the buckling event and post-buckling. Linear is easier and standard for first-cut design; non-linear is more accurate but adds substantial solver surface. Lean: linear first (this PRD), non-linear as separate PRD if demand emerges. Resolved (2026-05-12): Linear (eigenvalue) only for v0.5; non-linear deferred to a future PRD. See `docs/prds/v0_5/buckling-eigensolver.md` §3.
- **Number-of-modes default.** Lowest 5 is conventional; may be too few for symmetric structures with degenerate modes. Configurable; default 10 might be safer. Resolved (2026-05-12): n_modes = 10. See `docs/prds/v0_5/buckling-eigensolver.md` §3.
- **Imperfection sensitivity.** Real shells buckle far below the linear-buckling eigenvalue because of geometric imperfections. Standard treatment: scale the first mode shape into the geometry at small amplitude and re-analyze. Worth providing as a stdlib helper but adds workflow complexity. Resolved (2026-05-12): Out of scope for v0.5. See `docs/prds/v0_5/buckling-eigensolver.md` §3.
- **Reference load magnitude.** Eigenvalue is a scalar multiplier on the reference load. User-experience question: do we ask for a "reference unit load" or scale automatically against actual applied load? Lean: just use applied load as reference — eigenvalue then directly = safety factor against buckling. Resolved (2026-05-12): λ *is* the safety factor; `applied_load` argument is informational. See `docs/prds/v0_5/buckling-eigensolver.md` §3.
- **Multi-step / load-following analyses.** Pressure vessels under combined internal pressure + external pressure need each load type as a separate eigenvalue problem; combination requires non-linear analysis. Resolved (2026-05-12): Out of scope; per-case envelope handled by `MultiCaseBucklingResult`. See `docs/prds/v0_5/buckling-eigensolver.md` §3.

## Out of scope for this PRD

- Non-linear (Riks / arc-length) buckling tracking through the buckling event — separate PRD if demand emerges.
- Imperfection-sensitive analysis with stochastic imperfections — research-grade.
- Dynamic / flutter / aeroelastic instability — different physics, separate PRD if relevant.
- Material instability (necking, plasticity-driven localization) — non-linear plasticity territory.
- Thermal buckling (buckling under thermal stress) — depends on `structural-analysis-thermal.md` if filed.

## Relationship to other PRDs and tasks

- **Direct extension of `structural-analysis-fea.md`** — reuses K assembly, BCs, materials, mesher; adds K_g assembly and eigenvalue solver.
- **Strong dependency on `structural-analysis-shells.md`** — slender structures are shell-modeled; tet-only buckling would cover only a niche.
- **Composes with `multi-load-case-fea.md`** — buckling load factor per load case is a natural envelope.
- **Composes with `fea-gui-rendering.md` / `fea-gui-rendering-shells.md`** — mode-shape animation is the dominant visualization need.
- **May seed `structural-analysis-modal.md`** — modal (vibration) analysis is also an eigenvalue problem on the same K, so the eigenvalue solver infrastructure ships with both.
- **Forward-reference — backend event channel inventoried in `docs/prds/v0_3/gui-event-channel-inventory.md`** — the `mode-shape-frame` channel (consumed by `BucklingPanel` for mode-shape animation) is a deferred milestone listed in inventory §2.2. Active backend emitter wiring is owned by `docs/prds/v0_5/buckling-eigensolver.md` §13 task ι (GR-024 Phase 9); the original inventory PRD task λ is superseded by that task. Do not file duplicate emitter work here. See also `docs/gui-event-channels.md`.
