/**
 * View state persistence via localStorage.
 *
 * Pure functions for loading and saving per-file view state (Design Tree
 * visibility, user views, camera positions) keyed by absolute file path.
 *
 * Mirrors the structure of gui/src/hooks/useLayoutPersistence.ts — try/catch
 * around JSON.parse, per-field typeof guard, `catch { return null }` on any
 * failure, swallow set errors.
 */

import type { PersistentViewState } from '../types';

// ---------------------------------------------------------------------------
// Storage key
// ---------------------------------------------------------------------------

/** Prefix for all view-state localStorage keys. Full key: `${STORAGE_KEY_PREFIX}${absPath}`. */
export const STORAGE_KEY_PREFIX = 'reify:views:';

// ---------------------------------------------------------------------------
// Type guard
// ---------------------------------------------------------------------------

/**
 * Runtime type-guard for PersistentViewState read from localStorage.
 * Checks required fields and their types; returns false on any mismatch.
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
// Load
// ---------------------------------------------------------------------------

/**
 * Load persisted view state for `absPath` from localStorage.
 *
 * Returns `null` when:
 * - No entry exists for the path
 * - The stored value is not valid JSON
 * - Any required field is missing or has the wrong type (type-guard fails)
 * - An unexpected error occurs
 */
export function loadViewPersistence(absPath: string): PersistentViewState | null {
  try {
    const raw = localStorage.getItem(`${STORAGE_KEY_PREFIX}${absPath}`);
    if (raw === null) return null;

    const parsed: unknown = JSON.parse(raw);
    if (!isPersistentViewState(parsed)) {
      // Emit a diagnostic when a version field is present but wrong — most
      // commonly a legacy v1 entry left over from before the schema bump.
      // This gives users a single actionable line in the console instead of
      // a silent fallback to defaults.
      if (typeof parsed === 'object' && parsed !== null && 'version' in parsed) {
        const legacyVersion = (parsed as Record<string, unknown>)['version'];
        console.warn(
          `[viewPersistence] Discarding persisted state for "${absPath}": ` +
            `legacy schema version ${JSON.stringify(legacyVersion)} (expected "2"). ` +
            `Falling back to defaults.`,
        );
      }
      return null;
    }

    return parsed;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Save
// ---------------------------------------------------------------------------

/**
 * Write `state` to localStorage under the key `reify:views:{absPath}`.
 * Errors (quota exceeded, localStorage unavailable) are swallowed silently,
 * matching the pattern in `useLayoutPersistence.savePanelLayout`.
 */
export function saveViewPersistence(absPath: string, state: PersistentViewState): void {
  try {
    localStorage.setItem(`${STORAGE_KEY_PREFIX}${absPath}`, JSON.stringify(state));
  } catch {
    // Silently ignore — localStorage may be full or unavailable
  }
}

// ---------------------------------------------------------------------------
// Debounced saver (exported from step-6)
// ---------------------------------------------------------------------------
// (See createDebouncedSaver below — added in step-6)

export type DebouncedSaver = {
  /** Schedule a save; coalesces rapid calls within `delayMs`. */
  schedule(absPath: string, state: PersistentViewState): void;
  /** Write immediately, cancelling any pending timeout. */
  flush(): void;
  /** Cancel any pending timeout without writing. */
  cancel(): void;
};

/**
 * Creates a debounced saver that coalesces rapid calls within `delayMs`.
 *
 * - `schedule(absPath, state)` resets the timer and records the latest
 *   (absPath, state) pair; only the last pair is written.
 * - `flush()` cancels the pending timer and writes immediately.
 * - `cancel()` cancels the pending timer without writing.
 *
 * Step-6 implementation.
 */
export function createDebouncedSaver(delayMs = 500): DebouncedSaver {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pendingPath: string | null = null;
  let pendingState: PersistentViewState | null = null;

  function write(): void {
    if (pendingPath !== null && pendingState !== null) {
      saveViewPersistence(pendingPath, pendingState);
    }
  }

  return {
    schedule(absPath: string, state: PersistentViewState): void {
      pendingPath = absPath;
      pendingState = state;
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        write();
      }, delayMs);
    },

    flush(): void {
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
      write();
    },

    cancel(): void {
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
    },
  };
}
