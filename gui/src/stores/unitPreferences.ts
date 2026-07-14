/**
 * Per-cell display-unit preference persistence via localStorage (task #5199).
 *
 * Pure functions for loading and saving the Parameters panel's per-cell unit
 * picker choice, keyed by `cell_id`. Mirrors the diagnosticsPanelPersistence
 * pattern: a single JSON blob in one localStorage key, validated on load so
 * a corrupt or stale entry falls back to `null` (the picker's default-unit
 * selection) rather than throwing.
 */

export const UNIT_PREFERENCES_KEY = 'reify-unit-preferences';

/** Load the persisted unit label for a cell. Returns null if missing, invalid, or non-string. */
export function loadUnitPreference(cellId: string): string | null {
  try {
    const raw = localStorage.getItem(UNIT_PREFERENCES_KEY);
    if (raw === null) return null;

    const parsed = JSON.parse(raw);
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return null;
    }

    const value = (parsed as Record<string, unknown>)[cellId];
    return typeof value === 'string' ? value : null;
  } catch {
    return null;
  }
}

/** Save the chosen unit label for a cell, preserving every other cell's preference. */
export function saveUnitPreference(cellId: string, label: string): void {
  try {
    const raw = localStorage.getItem(UNIT_PREFERENCES_KEY);
    let all: Record<string, unknown> = {};
    if (raw !== null) {
      const parsed = JSON.parse(raw);
      if (parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed)) {
        all = parsed as Record<string, unknown>;
      }
    }
    all[cellId] = label;
    localStorage.setItem(UNIT_PREFERENCES_KEY, JSON.stringify(all));
  } catch {
    // Silently ignore — localStorage may be full or unavailable
  }
}
