import { describe, it, expect } from 'vitest';

import {
  GRID_RENDER_ORDER,
  AXES_RENDER_ORDER,
  AXIS_LABEL_RENDER_ORDER,
} from '../../viewport/renderOrder';

// No vi.mock('three') here: renderOrder.ts is deliberately a plain numeric-constant
// module with no three.js import, so it stays trivially importable from suites that
// DO mock 'three' (scene.test.ts, axisLabels.test.ts) without mock-factory ordering
// hazards.

describe('viewport render-order ladder', () => {
  it('every tier is a finite number', () => {
    for (const value of [GRID_RENDER_ORDER, AXES_RENDER_ORDER, AXIS_LABEL_RENDER_ORDER]) {
      expect(typeof value).toBe('number');
      expect(Number.isFinite(value)).toBe(true);
    }
  });

  it('the helper tier is strictly increasing: grid < axes < labels', () => {
    // three.js sorts by renderOrder BEFORE z in both painterSortStable (opaque) and
    // reversePainterSortStable (transparent), so this ordering is authoritative and
    // not depth-dependent. It is what lets every helper skip depthWrite: within the
    // tier, order alone decides who wins where members are coplanar (#4214).
    expect(GRID_RENDER_ORDER).toBeLessThan(AXES_RENDER_ORDER);
    expect(AXES_RENDER_ORDER).toBeLessThan(AXIS_LABEL_RENDER_ORDER);
  });

  it('the whole helper tier sits above the mesh tier, leaving 1..9 free for scene-content overlays', () => {
    // > 1 keeps feaDiagnosticOverlay.ts's OVERLAY_RENDER_ORDER = 1 (and any future
    // scene-content overlay in 1..9) strictly below the helpers, and keeps the tier
    // strictly above the default mesh tier 0 and meshManager.ts's undeformed
    // underlay at -1.
    expect(GRID_RENDER_ORDER).toBeGreaterThan(1);
  });
});
