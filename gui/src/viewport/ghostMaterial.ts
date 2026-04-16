import { MeshBasicMaterial, FrontSide } from 'three';
import { THEME_TOKENS } from '../theme';

/**
 * Creates a ghost material for rendering entities in a translucent ghost state.
 *
 * Properties:
 * - Color: Catppuccin surface0 (#313244) — neutral tone that doesn't compete
 *   with accent-colored opaque meshes.
 * - Low opacity (0.15) provides spatial context while remaining clearly distinct
 *   from opaque meshes.
 * - depthWrite: false prevents ghost geometry from occluding other objects.
 * - polygonOffset shifts ghost faces slightly so they don't Z-fight with
 *   coplanar opaque geometry.
 */
export function createGhostMaterial(): MeshBasicMaterial {
  return new MeshBasicMaterial({
    color: THEME_TOKENS.surface0,
    transparent: true,
    opacity: 0.15,
    depthWrite: false,
    side: FrontSide,
    polygonOffset: true,
    polygonOffsetFactor: 1,
    polygonOffsetUnits: 1,
  });
}
