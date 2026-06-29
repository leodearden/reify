# FEA Cantilever Baseline Screenshots

This directory holds the golden-master PNG baselines for the cantilever
contour + deformed-shape visual-regression scenes added by task 2968:

| File                              | Scene description                              |
|-----------------------------------|------------------------------------------------|
| `cantilever_contour.png`          | Undeformed von-Mises contour (FEA auto-enabled)|
| `cantilever_deformed_warp1.png`   | Deformed shape — warp 1× (true scale)          |
| `cantilever_deformed_warp100.png` | Deformed shape — warp 100× (amplified)         |

Fixture: `gui/test/fixtures/fea/cantilever_tip_load.ri`
(1 m × 0.1 m × 0.1 m steel beam, tip PointLoad 1000 N, root FixedSupport)

## Capturing baselines

Baselines are **out-of-headless-gate** artifacts. They must be captured with a
live GUI build and are then committed so the `npm run test:visual` pixel-diff
(≤ 1 % mismatch, `mismatchPctLimit=0.01`) can run regression checks.

Run from the repository root on a host with a display (or via `Xvfb`):

```bash
# 1. Build the GUI (release or debug)
scripts/run-gui-dev.sh gui/test/fixtures/fea/cantilever_tip_load.ri &
# … or: scripts/run-gui.sh gui/test/fixtures/fea/cantilever_tip_load.ri &

# 2. Capture all scenarios (including the three cantilever FEA ones)
UPDATE_BASELINES=1 npm --prefix gui run test:visual
```

The harness writes (creating this directory if absent):

- `gui/test/screenshots/fea/cantilever_contour.png`
- `gui/test/screenshots/fea/cantilever_deformed_warp1.png`
- `gui/test/screenshots/fea/cantilever_deformed_warp100.png`

Commit the three PNGs once captured.

## Why the baselines aren't present yet

The task-2968 implementation agent ran in a headless worktree without a live
binary, so the live GUI could not be spawned. The headless gate (steps s1–s9)
is fully green. The baseline PNG capture is the only remaining deliverable.

See: `gui/test/visual/scenarios.ts` (scenario catalogue; feaView field + entries)
     `gui/test/visual/run.ts`       (harness — feaViewActions wiring)
     `gui/test/fixtures/fea/cantilever_tip_load.ri` (self-contained fixture)

## Known assumptions

**`open_file` resets `showDeformed`:** The deformed-scene harness sequences
(`cantilever_deformed_warp1`, `cantilever_deformed_warp100`) call
`click_element(fea-mode-show-deformed-toggle)` to enable the deformed overlay.
The `fea-mode-show-deformed-toggle` checkbox is **non-idempotent** — it flips
`showDeformed` state on each click.  The click sequence therefore assumes
`showDeformed` is `false` at the start of each scenario, i.e. that `open_file`
resets the FEA view store to its default state.

If captured baselines look incorrect (e.g. the warp100 scene appears undeformed),
verify that `open_file` triggers a `feaModeStore` reset.  A future
`get_element_attribute` debug tool would allow an idempotent "click only if
not already checked" approach and eliminate this assumption.

## Deferred scenes

The following scenes are **explicitly deferred** — they are NOT silently missing.
Each is gated on a capability or infrastructure fix that is absent at task-2968
scope.

### Pressurised-cylinder scene
- **Status:** deferred to a follow-on task
- **Gate:** arbitrary-geometry FEA producers (structural-analysis-fea P1 = #4091,
  P2 = #4092) are required to produce a cylinder mesh and its FEA result model.
  The prismatic-geometry-only result-model seam (landed at task-2968) does not
  support cylinders.

### Bracket auto-resolve scene
- **Status:** deferred to a follow-on task
- **Gate:** the auto-resolve panel (capability manifest M-015) is absent.
  A bracket scene requires the user-facing resolve controls to select and trigger
  a solve, which the debug-MCP cannot substitute for without the panel.

### Full-window probe / overlay capture
- **Status:** deferred pending `screenshot_window` harness fix (#2954)
- **Gate:** `screenshot_window` (M-001 / #2954) is not yet implemented — only
  the viewport WebGL framebuffer capture (`screenshot`) works today. Probe
  readout and overlay labels are outside the viewport WebGL region and cannot
  be captured until the full-window path is available.
