import { describe, it, expect } from 'vitest';

import {
  UNDERLAY_RENDER_ORDER,
  OVERLAY_RENDER_ORDER,
  GRID_RENDER_ORDER,
  AXES_RENDER_ORDER,
  AXIS_LABEL_RENDER_ORDER,
} from '../../viewport/renderOrder';

// No vi.mock('three') here: renderOrder.ts is deliberately a plain numeric-constant
// module with no three.js import, so it stays trivially importable from suites that
// DO mock 'three' (scene.test.ts, axisLabels.test.ts) without mock-factory ordering
// hazards.

describe('viewport render-order ladder', () => {
  it('the helper tier is strictly increasing: grid < axes < labels', () => {
    // three.js sorts by renderOrder BEFORE z in both painterSortStable (opaque) and
    // reversePainterSortStable (transparent), so this ordering is authoritative and
    // not depth-dependent. It is what lets every helper skip depthWrite: within the
    // tier, order alone decides who wins where members are coplanar (#4214).
    expect(GRID_RENDER_ORDER).toBeLessThan(AXES_RENDER_ORDER);
    expect(AXES_RENDER_ORDER).toBeLessThan(AXIS_LABEL_RENDER_ORDER);
  });

  it('the whole helper tier sits above the mesh tier and every scene-content tier below it', () => {
    // The default mesh tier is three.js's 0, so the helper tier must clear it.
    expect(GRID_RENDER_ORDER).toBeGreaterThan(0);

    // ...and clear the two tiers this module also owns. These are the real exported
    // symbols, NOT copies of their values, so bumping the FEA overlay into the helper
    // tier fails here instead of silently breaking the documented ladder (and the
    // coplanar-tie guarantee the helper tier depends on).
    expect(GRID_RENDER_ORDER).toBeGreaterThan(OVERLAY_RENDER_ORDER);
    expect(GRID_RENDER_ORDER).toBeGreaterThan(UNDERLAY_RENDER_ORDER);
    // Both stay in their documented bands: underlay strictly below the mesh tier,
    // scene-content overlay strictly above it and strictly below the helpers.
    expect(UNDERLAY_RENDER_ORDER).toBeLessThan(0);
    expect(OVERLAY_RENDER_ORDER).toBeGreaterThan(0);
  });
});
