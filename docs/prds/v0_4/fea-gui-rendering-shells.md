# PRD: GUI Rendering of Shell-Element FEA Results

Status: stub — deferred, candidate v0.4. Sibling to v0.4 `structural-analysis-shells.md` and v0.3 `fea-gui-rendering.md`. Filed 2026-05-05 from shells PRD spillover.

## Goal

Extend the v0.3 FEA GUI rendering pipeline to handle shell-element results — mid-surface display, thickness-extruded reconstruction for visual continuity with the user's input geometry, top/bottom surface stress display for bending-stress visualization, mixed shell/tet body rendering, and shell-normal debug overlays.

## Background

The v0.3 `fea-gui-rendering.md` PRD ships contour plots, deformed-shape view, probes, in-flight progress, and diagnostics — all designed against tet element results where there's a single body-surface mesh and a single stress field. Shell elements break those assumptions:

- **Geometry rendering ambiguity:** the analysis happens on a mid-surface, but the user authored a thin body. Showing the mid-surface alone is unfaithful to their input; showing only the original body hides the analysis discretization. Need both, with a clear toggle.
- **Bending stress is the dominant signal on flexures, and it lives on the top and bottom surfaces, not the mid-surface.** A contour plot of "shell stress" without disambiguating top/mid/bottom hides the most important information.
- **Mixed shell/tet bodies (auto-segmented per OQ3 in shells PRD):** one body has shell sub-mesh in thin regions and tet sub-mesh in thick regions. The renderer needs to handle both element kinds in a single body and unify the contour color-mapping across them.
- **Shell normals and orientation matter visually:** mid-surface extraction assigns a normal direction; if it's flipped, "top" and "bottom" stress fields swap. A debug overlay showing the shell normal direction helps diagnose extraction failures.

## Why deferred (and separate from `fea-gui-rendering.md`)

- Timing: `fea-gui-rendering.md` is v0.3 milestone; shells are v0.4. Folding shell-GUI work into v0.3 would either block v0.3 release on v0.4 decisions, or leave half-defined shell tasks against kernel work that doesn't yet exist at v0.3 task time.
- Scope: shell GUI work is small (4-6 tasks) but distinct in shape from the v0.3 contour/probe/auto-resolve work.
- Composition: this PRD reuses every piece of v0.3 GUI infrastructure (mesh attribute pipeline, colormap system, probe persistence, visual regression harness) — additive rather than restructuring.

## Sketch of approach

Five additions to the existing GUI surface:

1. **Geometry display mode toggle.** Three modes: (a) **mid-surface** — render the analysis mid-surface mesh directly (matches what the solver sees); (b) **extruded** — reconstruct the original thin body from mid-surface + thickness for visual continuity with input; (c) **both** — extruded with mid-surface overlaid as a translucent ribbon. Mode persists per-document, defaults to extruded.
2. **Top/mid/bottom stress toggle.** When viewing shell results, a three-position toggle picks which through-thickness stress field drives the contour. Default: max(|top|, |bottom|) — the dominant bending stress signal. Pure mid-surface available for membrane-dominated cases. Composes with the existing min/max/auto colormap range modes.
3. **Mixed shell/tet body rendering.** A single body containing both element kinds: render shell sub-mesh as extruded thin region and tet sub-mesh as the standard tet surface, contour-colormap unified across both so a continuous color scale spans the whole body. MPC-tied interface visualized as a thin highlighted band on toggle.
4. **Shell-normal debug overlay.** Toggleable arrow field on the mid-surface showing per-element shell normal direction. Useful when extraction returns a flipped normal and "top" / "bottom" stress fields are swapped — dominant diagnostic for "why does my flexure stress look wrong."
5. **Thickness visualization mode.** Heat map on mid-surface showing local thickness (free output of the medial extractor). Useful when varying-thickness shells PRD lands; for v0.4 (constant thickness per body), mostly a sanity-check that extraction got the thickness right.

Visual regression baselines (under `gui/test/fixtures/fea-shells/`):
- Cantilever flexure under tip load — extruded mode, top-stress contour, mid-surface overlay toggle.
- Pinched cylinder — classic shell benchmark; mode-comparison view across element formulations.
- Mixed shell/tet body (flexure attached to block) — both element kinds visible, unified colormap.
- Flipped-normal failure case — diagnostic overlay shows normal direction issue.

## Pre-conditions for activating

- v0.4 `structural-analysis-shells.md` shipped: shell kernel, mid-surface extractor, MPC tying, ElasticResult extension for top/mid/bottom stress fields.
- v0.3 `fea-gui-rendering.md` shipped: visual regression harness (debug MCP extensions), MeshData per-vertex attribute pipeline, colormap system, probe persistence.

## Open design questions

- **Default geometry mode** — extruded (matches input) vs. mid-surface (matches analysis). Lean extruded; designers' mental model is the thin body, not the discretization.
- **Top/bottom toggle UX** — three-position toggle vs. side-by-side dual-viewport vs. transparency-blended overlay. Lean toggle for simplicity; revisit with side-by-side if user pain emerges.
- **Probe semantics on shells** — clicking a point: which stress value does the popup show? Lean: show all three (top/mid/bottom) as a stacked card; promote one based on the toggle state.
- **Mixed-body colormap range** — auto-range across the whole body, or per-element-kind? Lean unified (single range across both kinds); per-kind would be misleading because the user reads the colormap as "danger level" and that needs to be globally consistent.
- **Thickness heat-map availability for constant-thickness shells** — mostly a no-op visualization (everything is one color); only meaningful with varying-thickness shells PRD. Can be implemented in v0.4 against the constant-thickness path as a primitive ready for the v0.5 extension.

## Out of scope for this PRD

- Per-ply stress visualization for composite shells — covered by `composite-laminated-shells.md` GUI work, when that lands.
- Animated buckling mode shapes — covered by `structural-stability-buckling.md` GUI work.
- Cross-section / cutting-plane stress display — same v0.4-candidate item as in `fea-gui-rendering.md`; not shell-specific.
- Mid-surface extraction failure diagnostics in the GUI — overlaps with the v0.4 shells PRD's diagnostic tasks; cross-referenced rather than re-scoped here.

## Relationship to other PRDs and tasks

- **Direct dependent of `structural-analysis-shells.md`** — needs shell kernel, ElasticResult extensions, and mid-surface extractor.
- **Direct extension of `fea-gui-rendering.md`** — adds shell-aware rendering on top of the v0.3 contour/probe/auto-resolve infrastructure. No restructuring; only additive.
- **Composes with `varying-thickness-shells.md`** — thickness heat-map mode becomes meaningful when thickness varies; the rendering primitive ships in v0.4.
- **Composes with `composite-laminated-shells.md`** — per-ply stress display extends the top/mid/bottom toggle into a per-ply selector; future v0.5+ extension.
- **Composes with `mesh-morphing.md`** — mid-surface morphs alongside the original body geometry; rendering follows automatically.
