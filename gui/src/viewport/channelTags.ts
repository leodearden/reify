/**
 * channelTags — pure helpers for reading the per-channel unit/dimension tags
 * that the backend stamps onto `MeshData.scalar_channel_tags` (task #6185).
 *
 * This module is intentionally small and has NO Three.js, SolidJS, or store
 * dependencies so it can be unit-tested in isolation — the same convention as
 * scalarRange.ts, defaultScalarChannel.ts and feaToolbarChannels.ts.
 *
 * Rust twin: `MeshData.scalar_channel_tags` in gui/src-tauri/src/types.rs.
 *
 * Two rules, both deliberately asymmetric:
 *
 * - **ANY-signed** — a channel is signed if *any* mesh tags it signed. If one
 *   producer says values can be negative, silently dropping them anywhere is
 *   data loss, whereas letting an extra non-negative mesh into the range
 *   merely widens the scale.
 * - **Unanimous-or-null** — a unit is reported only when every mesh that tags
 *   the channel agrees on it, so a legend never shows a unit that is wrong for
 *   some of the data feeding it. Meshes that do not tag the channel abstain
 *   rather than veto.
 *
 * An **untagged** channel is treated as unsigned with no unit, so payloads
 * predating the tag map keep exactly today's behaviour.
 */

import type { MeshData, ScalarChannelTag } from '../types';

/** Read the tag for `channel` on one mesh, or `undefined` when untagged. */
function tagOf(mesh: MeshData, channel: string): ScalarChannelTag | undefined {
  return mesh.scalar_channel_tags?.[channel];
}

/**
 * Whether `channel` may legitimately carry negative values anywhere in
 * `meshes` (the ANY-signed rule). Untagged channels — and channels absent from
 * every mesh — are unsigned.
 */
export function isSignedChannel(
  meshes: Record<string, MeshData>,
  channel: string,
): boolean {
  for (const mesh of Object.values(meshes)) {
    if (tagOf(mesh, channel)?.signed === true) return true;
  }
  return false;
}

/**
 * The display-ready unit symbol for `channel` (e.g. `'Pa'`, `'rad'`), or
 * `null` when no mesh tags it, when the tagged unit is the empty string, or
 * when two meshes disagree (the unanimous-or-null rule).
 */
export function channelUnit(
  meshes: Record<string, MeshData>,
  channel: string,
): string | null {
  let agreed: string | null = null;

  for (const mesh of Object.values(meshes)) {
    const unit = tagOf(mesh, channel)?.unit;
    // Absent tags and empty-string units abstain rather than veto.
    if (unit === undefined || unit === '') continue;
    if (agreed === null) {
      agreed = unit;
    } else if (agreed !== unit) {
      return null;
    }
  }

  return agreed;
}
