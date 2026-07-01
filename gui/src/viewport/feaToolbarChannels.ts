/**
 * feaToolbarChannels — pure helper computing the FEA toolbar's channel
 * dropdown options for a given mesh set.
 *
 * This module is intentionally small and has NO Three.js, SolidJS, or store
 * dependencies so it can be unit-tested in isolation (see step-9 RED suite).
 *
 * Unlike `pickDefaultScalarChannel` (which selects ONE default channel from
 * the full set of non-empty channels present), this helper returns the FULL
 * list of options to offer in the dropdown. It deliberately does NOT surface
 * every scalar channel that happens to be present (e.g. shell sub-channels
 * like 'vonMises_top'/'vonMises_mid'/'vonMises_bottom' stay out of the
 * dropdown) — it starts from the existing base list and appends
 * 'errorIndicator' only when at least one mesh actually carries a non-empty
 * errorIndicator channel, so the option never appears for data that isn't
 * there.
 */

import type { MeshData } from '../types';

/** Base channel options, always offered regardless of mesh content. */
export const BASE_FEA_CHANNELS: readonly string[] = ['vonMises', 'displacement_magnitude'];

/**
 * Compute the channel dropdown options for the FEA toolbar.
 *
 * @param meshes - Record of mesh key → MeshData (from Viewport props).
 * @returns BASE_FEA_CHANNELS, plus 'errorIndicator' appended when any mesh
 *   has a non-empty `scalar_channels['errorIndicator']`.
 */
export function feaToolbarChannels(meshes: Record<string, MeshData>): string[] {
  const channels = [...BASE_FEA_CHANNELS];

  const hasErrorIndicator = Object.values(meshes).some(
    (mesh) => (mesh.scalar_channels?.['errorIndicator']?.length ?? 0) > 0,
  );
  if (hasErrorIndicator) {
    channels.push('errorIndicator');
  }

  return channels;
}
