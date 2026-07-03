# FEA Baseline Screenshots

This directory holds the golden-master PNG baselines for the FEA
visual-regression scenes. It started with the cantilever contour +
deformed-shape scenes added by task 2968:

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

## L-shaped errorIndicator baseline (task 4906)

| File                            | Scene description                                                |
|----------------------------------|-------------------------------------------------------------------|
| `l_shaped_error_indicator.png`  | L-shaped domain, `errorIndicator` scalar channel, `adaptive: true`|

Fixture: `gui/test/fixtures/fea/l_shaped_error_indicator.ri`
(200 mm × 200 mm × 40 mm steel L-bracket — outer box minus one XY corner
quadrant, forming a re-entrant corner; 500 N tip `PointLoad`, root
`FixedSupport`)

Scenario: `l_shaped_error_indicator` in `gui/test/visual/scenarios.ts`
(`feaChannel: "errorIndicator"`). Unlike the cantilever scenes, this one
requires selecting a non-default scalar channel before the screenshot is
taken — the harness now does this automatically via the `set_fea_channel`
debug tool (task 4906), driving the FEA toolbar's channel `<select>`
directly (`click_element` cannot mutate a `<select>`'s value).

The channel `<select>` only exists in the DOM once the FEA toolbar is
enabled (`FeaModeToolbar.tsx`). This fixture doesn't need an explicit
enable step — the Viewport auto-enable effect (`feaModeStore.ts`
`autoEnabledOnce`) flips FEA mode on as soon as a mesh with non-empty
`scalar_channels` appears, which the adaptive solve's `errorIndicator`
data satisfies. Because that auto-enable is asynchronous relative to the
harness reaching the channel-select step, `run.ts` waits on
`fea-mode-channel-select` via `wait_for_selector` before calling
`set_fea_channel`, so a fixture that unexpectedly fails to auto-enable
surfaces a clear selector-timeout failure instead of an opaque "element
... not found" from `set_fea_channel` itself.

### Two facts required for the errorIndicator channel to exist at all

`ElasticResult.error_indicator` is populated `Some(...)` **only** on the
adaptive + isotropic solve branch
(`crates/reify-eval/src/compute_targets/elastic_static.rs:1095`); the
non-adaptive path always stays `None` (`:2021`). Without a populated
field, `engine.rs` never emits `scalar_channels["errorIndicator"]` and the
FEA toolbar never offers the channel as an option. The fixture therefore
sets both of:

1. `ElasticOptions(adaptive: true, ...)` — mandatory.
2. An isotropic material (`Steel_AISI_1045`, here) — the adaptive branch
   only matches `MaterialModel::Isotropic`.

### Capturing the baseline

Same out-of-headless-gate procedure as the cantilever baselines above:
capture with a live GUI build, then commit the PNG so `npm run test:visual`
can pixel-diff future runs.

```bash
# 1. Build and launch the GUI against this fixture
scripts/run-gui-dev.sh gui/test/fixtures/fea/l_shaped_error_indicator.ri &

# 2. Capture all scenarios (including l_shaped_error_indicator)
UPDATE_BASELINES=1 npm --prefix gui run test:visual
```

The harness writes `gui/test/screenshots/fea/l_shaped_error_indicator.png`.
As with the task-2968 cantilever baselines, this task's implementation
agent ran in a headless worktree without a live binary, so the PNG capture
is the only deliverable left for a human with a display — the headless
gate (fixture, scenario catalogue, `feaChannelActions` helper, and the
`set_fea_channel` bridge/ToolDef primitive) is fully green.

### Known limitation / deferred physics

**The captured baseline will NOT show a physical re-entrant-corner stress
concentration — read this before trusting the PNG as physics.** The `.ri`
FEA solve path is mesh-free: `solve_elastic_static` always builds a
synthetic `length×width×height` Freudenthal **box** grid, and the `body`
geometry argument never reaches the solver (task 4870, "body-arg realized
VolumeMesh", is **PENDING**; localized gmsh h-adaptivity, task 4909, is
gated on 4870). So the `errorIndicator` field is sampled over the bounding
box with **uniform** refinement — the L-shaped `body` supplies only the
render surface the contour is painted onto, not an input to the solve.
The baseline therefore shows a smooth bounding-box error field across the
L-surface, **not** a concentration at the re-entrant corner. The
physics-accurate baseline is deferred until both 4870 and 4909 land, at
which point this PNG should be recaptured.

This section is the **canonical** explanation. The fixture header
(`gui/test/fixtures/fea/l_shaped_error_indicator.ri`) and the scenario
comment (`gui/test/visual/scenarios.ts`, `l_shaped_error_indicator` entry)
each carry only a short pointer back here rather than a copy — so once
4870+4909 land and this baseline is recaptured, updating this section is
the only edit required to keep the docs in sync.

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
