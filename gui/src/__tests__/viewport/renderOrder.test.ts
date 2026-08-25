import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

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

// Resolved via fileURLToPath rather than `new URL(path, import.meta.url)`: Vite rewrites
// that form into an asset/glob import, and with a non-literal path it pulls EVERY file in
// viewport/ into the module graph — which fails collection on DualViewport.module.css.
const VIEWPORT_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '../../viewport');

/**
 * Read the single renderOrder literal a lower-tier module declares.
 *
 * renderOrder.ts documents four tiers but only OWNS the helper tier: the underlay
 * (-1) and scene-content overlay (1) values are still module-private literals in
 * meshManager.ts / feaDiagnosticOverlay.ts, neither exported nor importable, and
 * both modules are outside this task's scope. Without this scan the ladder is
 * documentation-only below tier 10 — bumping the FEA overlay to 11 would break the
 * documented invariant with every test still green.
 *
 * A missing match FAILS rather than skipping: if a declaration is renamed or moved,
 * that is exactly when the ladder needs re-checking by hand. Delete this scan once
 * those constants move into renderOrder.ts and can simply be imported.
 *
 * @param relPath - Module filename relative to gui/src/viewport.
 * @param pattern - Regex whose first capture group is the renderOrder literal.
 */
function declaredRenderOrder(relPath: string, pattern: RegExp): number {
  const match = readFileSync(resolve(VIEWPORT_DIR, relPath), 'utf8').match(pattern);
  if (!match) {
    throw new Error(
      `Could not find ${pattern} in viewport/${relPath}. The renderOrder ladder documented ` +
        `in viewport/renderOrder.ts is pinned against this declaration — if it moved or was ` +
        `renamed, re-check the tier it occupies and update this scan (or, better, move the ` +
        `constant into renderOrder.ts and import it here).`,
    );
  }
  return Number(match[1]);
}

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

    // ...and clear the two tiers renderOrder.ts documents but does not own. These are
    // read from the real declarations, NOT from a copy of their values, so bumping the
    // FEA overlay into the helper tier fails here instead of silently breaking the
    // documented ladder (and the coplanar-tie guarantee the helper tier depends on).
    const overlayRenderOrder = declaredRenderOrder(
      'feaDiagnosticOverlay.ts',
      /^const OVERLAY_RENDER_ORDER = (-?\d+);/m,
    );
    const underlayRenderOrder = declaredRenderOrder(
      'meshManager.ts',
      /^\s*overlay\.renderOrder = (-?\d+);/m,
    );

    expect(GRID_RENDER_ORDER).toBeGreaterThan(overlayRenderOrder);
    expect(GRID_RENDER_ORDER).toBeGreaterThan(underlayRenderOrder);
    // Both stay in their documented bands: underlay strictly below the mesh tier,
    // scene-content overlay strictly above it and strictly below the helpers.
    expect(underlayRenderOrder).toBeLessThan(0);
    expect(overlayRenderOrder).toBeGreaterThan(0);
  });
});
