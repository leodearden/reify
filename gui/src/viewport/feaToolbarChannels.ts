/**
 * feaToolbarChannels — pure helper computing the FEA toolbar's channel
 * dropdown options for a given mesh set and (optionally) the caller's
 * currently-selected channel.
 *
 * This module is intentionally small and has NO Three.js, SolidJS, or store
 * dependencies so it can be unit-tested in isolation (see
 * `__tests__/viewport/feaToolbarChannels.test.ts`).
 *
 * Unlike `pickDefaultScalarChannel` (which selects ONE default channel from
 * the full set of non-empty channels present), this helper returns the FULL
 * list of options to offer in the dropdown. It owns the whole option-list
 * policy — base list, errorIndicator extension, and both widening sources —
 * which lived inline in `Viewport.tsx`'s `feaChannelOptions` memo until task
 * 5828 moved it here.
 *
 * ── Policy ───────────────────────────────────────────────────────────────────
 *
 * BASE_FEA_CHANNELS is always offered, regardless of mesh content.
 * 'errorIndicator' extends that BASE SEGMENT (not the widened extras) when at
 * least one mesh actually carries a non-empty errorIndicator channel, so the
 * option never appears for data that isn't there.
 *
 * The list is then widened by two deliberately narrow sources (task 5669).
 * Both are needed; neither is sufficient alone:
 *
 *  1. The caller's *current* channel, seeded before the mesh scan. This is
 *     what actually closes the desync in full generality — including for a
 *     channel outside PREFERRED_FEA_CHANNELS entirely, since
 *     `pickDefaultScalarChannel`'s last-resort branch auto-selects the
 *     lexicographically smallest non-empty channel whatever it is named.
 *     Viewport threads its store's channel in here rather than letting this
 *     helper derive one from the live mesh set, because auto-enable is
 *     one-shot: a later mesh-set replacement (e.g. a shell result swapped for
 *     a solid-only rebuild) can leave the store's channel pointing at a
 *     channel absent from the *new* mesh set entirely. Without this seed the
 *     option list would collapse back to the base list on that second
 *     delivery, and the rendered `<select>` would desync from the store.
 *
 *  2. Members of PREFERRED_FEA_CHANNELS carrying non-empty data somewhere in
 *     the active mesh set. This is what makes the *sibling* shell surfaces
 *     selectable: with only (1), a shell mesh would offer 'vonMises_top' but
 *     the user could never switch to '_mid'/'_bottom'.
 *
 * The mesh scan is deliberately restricted to PREFERRED_FEA_CHANNELS rather
 * than admitting every non-empty scalar channel present: a design carrying
 * arbitrary vertex scalars would otherwise list them all as selectable "FEA"
 * channels, which is a broader change than the defect requires and would cost
 * the toolbar its FEA-specific meaning. Anything outside the preference list
 * still reaches the dropdown via (1) when it is in fact the selected channel,
 * which is the only case that can desync.
 *
 * Ordering contract: the base segment keeps its own fixed order, and the seed
 * and scan extras are sorted together as ONE lexicographic set appended after
 * it — so the option order is insertion-order independent, and the seed does
 * not merely trail the scan results.
 */

import type { MeshData } from '../types';
import { PREFERRED_FEA_CHANNELS } from './defaultScalarChannel';

/** Base channel options, always offered regardless of mesh content. */
export const BASE_FEA_CHANNELS: readonly string[] = ['vonMises', 'displacement_magnitude'];

/**
 * Compute the channel dropdown options for the FEA toolbar.
 *
 * @param meshes - Record of mesh key → MeshData (from Viewport props).
 * @param currentChannel - The caller's currently-selected channel, if any.
 *   Optional: omitting it yields the un-seeded list. Seeded into the widened
 *   extras when it is not already offered, so the rendered `<select>` can
 *   never desync from the caller's state.
 * @returns BASE_FEA_CHANNELS, plus 'errorIndicator' appended when any mesh has
 *   a non-empty `scalar_channels['errorIndicator']`, followed by the
 *   lexicographically sorted widening extras (see the module docstring).
 */
export function feaToolbarChannels(
  meshes: Record<string, MeshData>,
  currentChannel?: string,
): string[] {
  const base = [...BASE_FEA_CHANNELS];

  const hasErrorIndicator = Object.values(meshes).some(
    (mesh) => (mesh.scalar_channels?.['errorIndicator']?.length ?? 0) > 0,
  );
  if (hasErrorIndicator) {
    base.push('errorIndicator');
  }

  const extra = new Set<string>();
  if (currentChannel && !base.includes(currentChannel)) {
    extra.add(currentChannel);
  }
  for (const mesh of Object.values(meshes)) {
    if (!mesh.scalar_channels) continue;
    for (const name of PREFERRED_FEA_CHANNELS) {
      if (base.includes(name) || extra.has(name)) continue;
      const data = mesh.scalar_channels[name];
      if (data && data.length > 0) {
        extra.add(name);
      }
    }
  }

  return extra.size > 0 ? [...base, ...[...extra].sort()] : base;
}
