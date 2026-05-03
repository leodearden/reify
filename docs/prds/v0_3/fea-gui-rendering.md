# PRD: GUI Rendering of FEA Results

Status: stub — deferred, GUI milestone within v0.3. Sibling to `structural-analysis-fea.md`. Filed 2026-05-02 from FEA PRD spillover.

## Goal

Render FEA results in the Reify GUI so that designers can see what their analysis is doing — stress contour plots, deformed-shape rendering, probe-point queries, in-flight solver progress, diagnostic overlays. The v0.3 FEA PRD ships headless-first; this PRD makes FEA usable in the interactive GUI session.

## Background

The v0.3 FEA PRD (`structural-analysis-fea.md`) explicitly defers GUI rendering to a separate milestone. Without it, FEA is a CLI feature: you run `reify build`, get an `ElasticResult`, and have to script your own visualization (or import to Paraview / similar). Useful for batch / CI / deterministic reproducibility, but inadequate for the interactive design loop that motivates Reify.

What designers actually need to do during interactive use:
- See where the stress is concentrated (color-mapped contour over the body surface).
- See how the body deforms under load (deformed-shape view, with adjustable warp factor).
- Click a point to read off the local stress / displacement value.
- Watch the solver iterate during a long solve (progress bar + convergence trace).
- Get visual diagnostic feedback when something goes wrong (highlight unconstrained DOF directions, problem elements, BCs that didn't resolve).

This is a substantial GUI-side workstream and benefits from being scoped separately from the headless kernel work. Both ship as part of v0.3 ideally; the headless path is releasable on its own if the GUI work slips.

## Why deferred (and separate from FEA PRD)

- Decouples kernel work from GUI work — different skills, different review surfaces, both can land in parallel.
- GUI architecture for field rendering doesn't yet exist in the Tauri layer — needs design surface.
- Headless FEA is independently valuable (CI, batch builds, scripted workflows); shipping it without waiting on GUI keeps the path clear.

## Sketch of approach

Six pieces, each separately useful:

1. **Stress contour plot** — colormap (perceptually uniform default like viridis; engineering-rainbow available) applied to the body's surface mesh, sampled from the stress field. Min/max range auto-set to result range, user-overridable. Live update on parameter change.
2. **Deformed-shape view** — surface mesh nodes displaced by the displacement field × warp factor (slider, default 1.0; common to use 10× or 100× for visualization). Side-by-side with undeformed shape, or overlay with translucency.
3. **Probe-point query** — click on a surface point; popup shows local displacement vector, stress tensor, von Mises, principal stresses. Pinnable so multiple probes can stay on screen.
4. **In-flight solver progress** — for solves taking >1s, render a progress overlay: CG iteration count, current residual, ETA estimate, cancel button. Convergence trace mini-plot.
5. **Diagnostic overlays** — when the solver fails or warns, highlight the relevant geometry (unconstrained body shown with rigid-body-mode arrows, problem elements shown with red outlines, unresolved selectors shown as ghost geometry).
6. **Auto-resolve loop progress** — for `param x = auto` driven by FEA, a dedicated overlay shows: current best parameter values, current FEA-derived constraint values, iteration history (line chart of converging max von Mises against thickness, etc.).

## Pre-conditions for activating

- v0.3 FEA kernel (`structural-analysis-fea.md` tasks #16, #17, #20) shipped — concrete consumer with validated outputs.
- GUI architecture extension for field rendering (some new components in the Tauri/React layer).
- Existing surface-mesh rendering pipeline can accept per-vertex scalar/vector attributes (probably needs a small extension).

## Open design questions

- **Rendering pipeline** — extend the existing surface-mesh viewer (cleaner integration), or new dedicated FEA viewport (cleaner separation)? Lean: extend, with a "FEA mode" toggle.
- **Colormap conventions** — engineering-rainbow is what users expect from commercial CAD-FEA tools but is perceptually misleading. Default to perceptually-uniform (viridis), provide rainbow as opt-in. Document the choice.
- **Field-rendering perf** — large stress fields (millions of points) need GPU-side sampling. Current GUI rendering is mostly CPU-side; FEA pushes the limits.
- **Live update vs. on-completion** — during a long solve, do we render partial results progressively, or wait until done? Lean: progressive when the solver supports it (progressive trait, FEA task #15); batched otherwise.
- **Probe persistence** — pinned probes survive parameter changes? (Lean: yes, re-evaluated against new result.) Survive geometry changes? (Probably not, hard to map a 3D point through topology.)
- **Auto-resolve overlay layout** — how much screen real estate? Probably collapsible side-panel, default visible during active auto-resolve.
- **Multi-result comparison** — for "before/after" or "load case A vs B" comparisons, side-by-side viewport? Out of scope for v0.3 lean; revisit.

## Out of scope for this PRD

- Vector-field visualization (arrows, streamlines for displacement direction) — useful but not minimum viable; v0.4 add-on.
- Multi-result comparison view (before/after, load case A vs B) — useful for design exploration; v0.4 feature.
- Result export to Paraview / VTK / other external viz tools — separate "FEA result export" PRD if demand emerges.
- Animation of deformation under varying parameters — useful but heavy; v0.4 candidate.
- Stress visualization on cutting planes / cross sections — useful for thick bodies; v0.4 candidate.

## Relationship to other PRDs and tasks

- **Direct dependent of `structural-analysis-fea.md`** — needs the kernel and validated outputs first.
- **Composes with `mesh-morphing.md`** — when mesh morphs, the GUI display has to follow smoothly; live re-rendering is naturally fast on morphed meshes.
- **Composes with `multi-load-case-fea.md`** — multi-load workflows want a way to compare results across cases; affects probe and overlay design.
- **Will need extensions for `structural-analysis-shells.md`** — shell elements render differently (mid-surface + thickness extrusion); GUI needs awareness of element kind.
- **Extends `prd-m6-gui.md`** — adds the field-rendering surface to the existing GUI architecture.
