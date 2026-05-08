/**
 * View state persistence via the sidecar file layer.
 *
 * Wraps the Tauri bridge's `readViewSidecar` / `writeViewSidecar` IPC commands
 * with an application-level type guard so callers receive a validated
 * `PersistentViewState` or `null` — not raw wire data.
 *
 * Load priority (per design decision):
 *   sidecar (.ri.views.json) > localStorage > defaults
 *
 * The same type guard used in viewPersistence.ts is duplicated here (rather
 * than shared) so the two layers can evolve independently without coupling.
 */

import { readViewSidecar, writeViewSidecar } from '../bridge';
import type { PersistentViewState } from '../types';

// ---------------------------------------------------------------------------
// Type guard — mirrors viewPersistence.isPersistentViewState
// ---------------------------------------------------------------------------

/**
 * Runtime type-guard for `PersistentViewState` coming off the wire.
 * Guards against wire-format drift between the Rust backend and the TS types.
 */
function isPersistentViewState(value: unknown): value is PersistentViewState {
  if (typeof value !== 'object' || value === null) return false;
  const v = value as Record<string, unknown>;

  if (v['version'] !== '2') return false;
  if (typeof v['activeViewId'] !== 'string') return false;
  if (!Array.isArray(v['userViews'])) return false;
  if (typeof v['explicit'] !== 'object' || v['explicit'] === null || Array.isArray(v['explicit']))
    return false;
  if (
    typeof v['viewportCameras'] !== 'object' ||
    v['viewportCameras'] === null ||
    Array.isArray(v['viewportCameras'])
  )
    return false;
  if (typeof v['timestamp'] !== 'string') return false;

  return true;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Read the view sidecar file for `riPath` (`{riPath}.views.json`).
 *
 * Returns `null` when:
 * - The sidecar file does not exist (bridge returns null)
 * - The payload fails the type guard (wire-format drift, schema mismatch)
 *
 * Rejects when the bridge reports an I/O or parse error (e.g. malformed JSON).
 */
export async function loadSidecar(riPath: string): Promise<PersistentViewState | null> {
  const raw = await readViewSidecar(riPath);
  if (raw === null) return null;
  if (!isPersistentViewState(raw)) return null;
  return raw;
}

/**
 * Write the view sidecar file for `riPath` (`{riPath}.views.json`).
 *
 * Rejects when the bridge reports an error (e.g. disk full, I/O failure).
 */
export async function saveSidecar(riPath: string, state: PersistentViewState): Promise<void> {
  await writeViewSidecar(riPath, state);
}
