/**
 * scalarRange — pure helper for computing the min/max range of a named scalar
 * channel across a set of MeshData objects.
 *
 * This module is intentionally small and has NO Three.js, SolidJS, or store
 * dependencies so it can be unit-tested in isolation (see step-1 RED suite).
 *
 * Consumer contract — two arms, selected by the channel's tag
 * (`MeshData.scalar_channel_tags`, task #6185):
 *
 * **Unsigned or untagged channels** (today's behaviour, unchanged):
 *   - Values v < 0 are excluded.  This is NOT arbitrary clamping: it
 *     implements the `SCALAR_CHANNEL_OOB_SENTINEL = -1.0` consumer contract
 *     (gui/src-tauri/src/types.rs), where OOB/out-of-solid vertices are marked
 *     with a negative finite value precisely because the wire's finite-value
 *     guard bars NaN.  Excluding them keeps OOB vertices from dragging the
 *     colormap range.
 *   - Von-Mises stress is physically ≥ 0, so the filter is also correct on
 *     physical grounds for the channels that exist today.
 *
 * **Signed channels** (`signed: true`, e.g. an angle in radians):
 *   - The true min/max is used — negatives are real data.  A signed channel
 *     cannot use the -1.0 sentinel (it is a legal value there), so its
 *     producer supplies an in-band finite value at OOB vertices; there is
 *     nothing for this function to exclude.
 *
 * **Both arms** exclude non-finite values (NaN, ±Infinity).
 *
 * Signedness is DERIVED from the meshes rather than passed by the caller, so a
 * producer that starts stamping `ScalarChannelTag::angle()` gets correct range
 * behaviour with no frontend change.
 */

import type { MeshData } from '../types';
import { isSignedChannel } from './channelTags';

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

  // Derived once: ANY mesh tagging the channel signed makes it signed here.
  const signed = isSignedChannel(meshes, channel);

  for (const mesh of Object.values(meshes)) {
    const data = mesh.scalar_channels?.[channel];
    if (!data) continue;

    for (let i = 0; i < data.length; i++) {
      const v = data[i];
      // NaN and ±Infinity are excluded on both arms.
      if (!Number.isFinite(v)) continue;
      // Unsigned/untagged only: drop negatives, which carry the
      // SCALAR_CHANNEL_OOB_SENTINEL (-1.0) marker semantics.  For a signed
      // channel -1.0 is legal data, so this arm must not fire.
      if (!signed && v < 0) continue;

      if (v < min) min = v;
      if (v > max) max = v;
      found = true;
    }
  }

  return found ? { min, max } : null;
}
