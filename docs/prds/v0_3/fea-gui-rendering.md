# PRD: GUI Rendering of FEA Results

Status: design resolved + decomposed (2026-05-04) — deferred, GUI milestone within v0.3. Sibling to `structural-analysis-fea.md`. Filed 2026-05-02 from FEA PRD spillover.

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

- v0.3 FEA kernel: ElasticResult typed contract (#2911), result interpolation (#2920), engine integration with `@optimized` + ComputeNode wiring (#2924), and Gmsh volume mesher (#2925) shipped — gives the GUI a concrete consumer with validated displacement / stress fields.
- Visual regression infrastructure (debug MCP extensions + harness) shipped — see "Prerequisite: visual regression infrastructure" below.
- Existing surface-mesh rendering pipeline extended to accept per-vertex scalar attributes (currently MeshData carries only positions / indices / normals). Plumbed by task GUI-1.

## Prerequisite: visual regression infrastructure

The existing Vitest harness covers component-level logic but cannot verify "does the contour render correctly under this stress field." Before FEA-specific GUI work begins, the debug MCP at `127.0.0.1:3939` (gated by `REIFY_DEBUG=1`) is extended into a deterministic GUI driver suitable for pixel-stable visual regression. Five tasks:

1. **`screenshot_window`** — full-window capture (panels, overlays, probe popups), not just the WebGL canvas. Uses Tauri 2's `WebviewWindow::capture()`. Complements the existing `screenshot` tool (which stays as the WebGL-only fast path).
2. **`wait_for_idle`** — block until the engine has settled and a fresh frame has rendered. Removes the polling shim that tests currently need around `engine_state` after `open_file`.
3. **`set_camera`** — explicit camera state (position + target + zoom). Without this, the same model framed differently produces different pixels. Required for stable diffs.
4. **`set_test_mode`** — freeze any animated UI (spinners, pulsing toasts) during a shot so animation phase doesn't introduce diffs. Likely small if any.
5. **Visual regression harness** — Node-side test driver that talks JSON-RPC to the MCP, takes named screenshots, diffs against PNG baselines under `gui/test/screenshots/` using SSIM-tolerant comparison (`pixelmatch` with threshold ≥0.99). Wires into CI as a separate job from Vitest.

Rationale: the MCP route is preferred over Playwright / tauri-driver because (a) it runs in-process and can wait deterministically on engine state without external polling, (b) it sidesteps the SolidJS-component-test vs. webkit2gtk-render impedance mismatch, and (c) the four tool additions are independently useful for non-FEA GUI work.

## Resolved design decisions (2026-05-04)

Captured from the design discussion:

- **Rendering pipeline**: extend the existing surface-mesh viewer with a "FEA mode" toggle. Rejected dedicated viewport — would duplicate camera, raycasting, BVH, selection, splitter integration for no payoff. Migration path to `ShaderMaterial` exists if range-scrubbing perf ever demands it.
- **Per-vertex attribute pipeline**: extend MeshData to carry optional Float32Array channels for scalar attributes (`vonMises`, `displacement_magnitude`, etc.) plus a packed displaced-position channel. Switch to `MeshPhongMaterial { vertexColors: true }` only when scalars are present. Plumbed through the Tauri IPC.
- **Colormap default**: viridis. Engineering-rainbow available via dropdown with a one-line tooltip on the rationale. Magma offered for hot-spot work. Range modes: auto (default), user-fixed, lock-to-current — the third is critical for side-by-side comparison without misleading auto-rescaling.
- **Field-rendering perf**: ship CPU-side per-vertex colour baking. Typical Reify mesh (10k–100k surface vertices) is well within JS budget. Bypass meshManager and directly mutate the colour `BufferAttribute` on range / colormap changes (no full mesh re-sync). Defer GPU-side sampling until a real workload demands it; migration to `ShaderMaterial` is a contained refactor when needed.
- **Live update vs. on-completion**: render coarse result once (post-coarse-solve), keep convergence-trace overlay live (cheap scalar plot), re-mesh + re-render only on solve completion. True progressive geometry display deferred to v0.4 — refinement changes vertex count and inflicts visible jitter without value.
- **Probe persistence**: probes survive parameter changes (re-evaluated against new ElasticResult on the same mesh — cheap point query). Probes do *not* silently vanish on topology change; render as greyed "stale" markers with last-known value and a "re-pin?" affordance. Pin by `(entity_path, face_id, barycentric_uv)` — when mesh-morphing PRD ships, probes follow the morph for free; surviving full re-mesh is out of scope.
- **Auto-resolve overlay**: another tab in the existing right-sidebar tab set (alongside PropertyEditor / DesignTree / MechanismPanel / ChatPanel). Auto-promotes to visible when an auto-resolve loop becomes active; auto-restores the user's prior layout on completion. No floating viewport overlay.
- **Multi-result comparison (v0.3 scope)**: out of scope as planned, but note for future: `DualViewport` already exists in the codebase. Wiring two ElasticResults into it for parameter-comparison views is mostly state management, not new architecture. The genuinely-new v0.4 feature is *load-case envelope visualization* (per `multi-load-case-fea.md`).
- **Failed-solve viewport state**: clear scalar attributes (revert to monochrome material), keep geometry visible, show diagnostic overlay layer. Don't leave stale colours from a prior successful solve — it implies a result that doesn't exist.

## Test plan

Visual regression baselines drive verification of FEA rendering. Reference scenes under `gui/test/fixtures/fea/` cover:

- Cantilever under tip load — contour rendering, deformed-shape view at warp 1× / 100×.
- Pressurised cylinder — von Mises contour, principal-stress probe popup.
- Unconstrained body (intentional failure) — diagnostic overlay with rigid-body-mode arrows.
- Bracket with `param x = auto` driven by max-stress constraint — auto-resolve panel layout, iteration history line chart.

For each scene: deterministic camera (via `set_camera`), `wait_for_idle`, capture, diff against PNG baseline. SSIM tolerance ≥0.99; per-platform baselines if needed. CI failure on diff outside tolerance.

Determinism is required end-to-end: same `.ri` input → same residual → same final mesh → same pixels. The FEA PRD's `#deterministic` opt-in and cache-as-determinism-anchor decisions cover the kernel side; the visual regression harness verifies the GUI side preserves it.

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
