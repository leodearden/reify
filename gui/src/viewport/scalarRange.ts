/**
 * scalarRange — pure helper for computing the min/max range of a named scalar
 * channel across a set of MeshData objects.
 *
 * This module is intentionally small and has NO Three.js, SolidJS, or store
 * dependencies so it can be unit-tested in isolation (see step-1 RED suite).
 *
 * Consumer contract (mirrors gui/src-tauri/src/types.rs:225-228):
 *   - Values v < 0 are excluded (the SCALAR_CHANNEL_OOB_SENTINEL is -1.0;
 *     OOB/out-of-solid vertices must not perturb the colormap range).
 *   - Non-finite values (NaN, ±Infinity) are excluded.
 *   - Von-Mises stress is physically ≥ 0, so filtering v < 0 is also correct
 *     on physical grounds.
 */

import type { MeshData } from '../types';

/**
 * Compute the {min, max} range of `channel` across all meshes, ignoring
 * sentinel/non-finite values.  Returns `null` when no valid value exists
 * (empty mesh set, channel absent, all values filtered out).
 */
export function computeScalarRange(
  meshes: Record<string, MeshData>,
  channel: string,
): { min: number; max: number } | null {
  let min = Infinity;
  let max = -Infinity;
  let found = false;

  for (const mesh of Object.values(meshes)) {
    const data = mesh.scalar_channels?.[channel];
    if (!data) continue;

    for (let i = 0; i < data.length; i++) {
      const v = data[i];
      // Exclude the SCALAR_CHANNEL_OOB_SENTINEL (-1.0), all other negatives,
      // NaN, and ±Infinity per the types.rs:225-228 consumer contract.
      if (!Number.isFinite(v) || v < 0) continue;

      if (v < min) min = v;
      if (v > max) max = v;
      found = true;
    }
  }

  return found ? { min, max } : null;
}
