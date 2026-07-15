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

/**
 * Load every persisted cell -> unit-label preference in one pass. Returns
 * `{}` if the store is missing, corrupt, or not an object; non-string
 * entries are dropped.
 *
 * Callers that need many cells' preferences (e.g. PropertyEditor seeding a
 * whole panel of rows) should call this ONCE and read from the result,
 * rather than calling `loadUnitPreference` per cell — that re-parses the
 * entire blob from scratch on every call (task #5199 amend: this was
 * happening at least twice per picker-enabled row per render).
 */
export function loadAllUnitPreferences(): Record<string, string> {
  try {
    const raw = localStorage.getItem(UNIT_PREFERENCES_KEY);
    if (raw === null) return {};

    const parsed = JSON.parse(raw);
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return {};
    }

    const result: Record<string, string> = {};
    for (const [cellId, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof value === 'string') {
        result[cellId] = value;
      }
    }
    return result;
  } catch {
    return {};
  }
}

/** Load the persisted unit label for a cell. Returns null if missing, invalid, or non-string. */
export function loadUnitPreference(cellId: string): string | null {
  return loadAllUnitPreferences()[cellId] ?? null;
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
