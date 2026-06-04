/**
 * defaultScalarChannel — pure helper for picking a deterministic default
 * scalar channel name across a set of MeshData objects.
 *
 * This module is intentionally small and has NO Three.js, SolidJS, or store
 * dependencies so it can be unit-tested in isolation (see step-1 RED suite).
 *
 * Selection policy (insertion-order independent):
 *   1. Collect the union of channel names that have at least one non-empty
 *      array across all meshes.
 *   2. Return the first member of PREFERRED_FEA_CHANNELS that is present in
 *      the collected set ('vonMises' for solids, 'vonMises_top' for shells).
 *   3. Otherwise return the lexicographically smallest collected name
 *      (guarantees determinism for any unknown channel naming).
 *   4. Return undefined when no non-empty channel exists.
 */

import type { MeshData } from '../types';

/**
 * Ordered preference list for the default FEA scalar channel.
 *
 * 'vonMises' is the feaModeStore's existing default channel and the natural
 * solid-mesh choice.  'vonMises_top' is the meaningful default surface for
 * shells (the outermost tensile face).  The list is checked in order so the
 * highest-priority non-empty channel wins, regardless of insertion order in
 * the wire JSON.
 */
export const PREFERRED_FEA_CHANNELS: readonly string[] = [
  'vonMises',
  'vonMises_top',
  'vonMises_mid',
  'vonMises_bottom',
];

/**
 * Pick the deterministic default scalar channel across all meshes.
 *
 * @param meshes - Record of mesh key → MeshData (from Viewport props).
 * @returns The selected channel name, or `undefined` if no non-empty channel
 *   exists in any mesh.
 */
export function pickDefaultScalarChannel(
  meshes: Record<string, MeshData>,
): string | undefined {
  // Collect all channel names that have at least one value across any mesh.
  const nonEmpty = new Set<string>();
  for (const mesh of Object.values(meshes)) {
    if (!mesh.scalar_channels) continue;
    for (const [name, data] of Object.entries(mesh.scalar_channels)) {
      if (data && data.length > 0) {
        nonEmpty.add(name);
      }
    }
  }

  if (nonEmpty.size === 0) return undefined;

  // Return the first preferred channel that is present.
  for (const preferred of PREFERRED_FEA_CHANNELS) {
    if (nonEmpty.has(preferred)) return preferred;
  }

  // Fall back to the lexicographically smallest name for determinism.
  return [...nonEmpty].sort()[0];
}
