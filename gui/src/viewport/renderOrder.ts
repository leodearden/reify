/**
 * The viewport's draw-order ladder, and the source of truth for every tier this
 * codebase sets.
 *
 * three.js sorts a render list by `renderOrder` BEFORE `z` — in both
 * `painterSortStable` (opaque pass) and `reversePainterSortStable` (transparent
 * pass) — so this ladder is authoritative and not depth-dependent:
 *
 *   -1      undeformed-geometry underlay        (this module — UNDERLAY_RENDER_ORDER)
 *    0      model meshes                        (three.js default)
 *    1..9   scene-content overlays              (this module — OVERLAY_RENDER_ORDER)
 *   10..12  viewport helper tier                (this module — the constants below)
 *
 * ## What this module owns
 *
 * Every tier this codebase controls is defined here — the mesh tier at 0 is
 * three.js's default, not one of ours — and each is imported at its use site:
 * `meshManager.ts` imports `UNDERLAY_RENDER_ORDER` and `feaDiagnosticOverlay.ts`
 * imports `OVERLAY_RENDER_ORDER`, so this module is the single source of truth
 * for every renderOrder the viewport sets — `renderOrder.test.ts` asserts
 * directly against the real symbols, not a scan of the modules that use them.
 *
 * ## The invariant this tier exists to enforce (#6587, #4214)
 *
 * Every helper (grid, axes, axis labels) is depth-TESTED but never
 * depth-WRITING:
 *
 *   - depthTest = true  — model geometry between the camera and a helper
 *     correctly occludes it. Helpers are full-scene-scale world objects, not a
 *     HUD, so they must obey the scene's depth cues. Disabling depthTest is
 *     what made the axis vectors and X/Y/Z labels draw straight through solid
 *     parts (#6587).
 *
 *   - depthWrite = false — a helper contributes nothing to the depth buffer, so
 *     it can never occlude another helper drawn after it. Within the tier,
 *     renderOrder alone therefore decides who wins where members are coplanar.
 *     That is how the grid's two centre lines stop z-fighting the exactly
 *     collinear X/Y axis vectors (#4214) WITHOUT disabling depthTest: the grid
 *     draws first and writes no depth, so the axes drawn after it can never
 *     fail their LEQUAL test against it — while real geometry, which DID write
 *     depth, still occludes both.
 *
 * Keep the tiers strictly increasing. Anything added here must also follow the
 * depthTest=true / depthWrite=false rule, or the coplanar-tie guarantee above
 * stops holding for the helpers below it.
 */

/** Undeformed-geometry underlay — drawn behind the deformed mesh (renderOrder=0 default). */
export const UNDERLAY_RENDER_ORDER = -1;

/** FEA diagnostic overlay Group — above the default mesh layer (renderOrder 0). */
export const OVERLAY_RENDER_ORDER = 1;

/** Ground-plane GridHelper — first in the helper tier, after all model geometry. */
export const GRID_RENDER_ORDER = 10;

/** Origin AxesHelper triad — after the grid, so the coplanar centre lines lose the tie. */
export const AXES_RENDER_ORDER = 11;

/** X/Y/Z text sprites — last, so a label always draws over the axis it annotates. */
export const AXIS_LABEL_RENDER_ORDER = 12;
