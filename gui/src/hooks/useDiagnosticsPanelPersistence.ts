/**
 * DiagnosticsPanel persistence via localStorage.
 * Pure functions for loading and saving panel size and line-wrap state.
 */

export const DIAGNOSTICS_LINE_WRAP_KEY = 'reify-diagnostics-line-wrap';
export const DIAGNOSTICS_PANEL_SIZE_KEY = 'reify-diagnostics-panel-size';

/** Load persisted line-wrap state. Returns null if missing, invalid, or non-boolean. */
export function loadDiagnosticsLineWrap(): boolean | null {
  try {
    const raw = localStorage.getItem(DIAGNOSTICS_LINE_WRAP_KEY);
    if (raw === null) return null;

    const parsed = JSON.parse(raw);
    if (typeof parsed === 'boolean') {
      return parsed;
    }
    return null;
  } catch {
    return null;
  }
}

/** Save line-wrap state to localStorage. */
export function saveDiagnosticsLineWrap(value: boolean): void {
  try {
    localStorage.setItem(DIAGNOSTICS_LINE_WRAP_KEY, JSON.stringify(value));
  } catch {
    // Silently ignore — localStorage may be full or unavailable
  }
}

/** Load persisted panel size. Returns null if missing, invalid, or wrong shape. */
export function loadDiagnosticsPanelSize(): { width: number; height: number } | null {
  try {
    const raw = localStorage.getItem(DIAGNOSTICS_PANEL_SIZE_KEY);
    if (raw === null) return null;

    const parsed = JSON.parse(raw);
    if (
      parsed !== null &&
      typeof parsed === 'object' &&
      typeof parsed.width === 'number' &&
      typeof parsed.height === 'number'
    ) {
      return { width: parsed.width, height: parsed.height };
    }
    return null;
  } catch {
    return null;
  }
}

/** Save panel size to localStorage. */
export function saveDiagnosticsPanelSize(size: { width: number; height: number }): void {
  try {
    localStorage.setItem(DIAGNOSTICS_PANEL_SIZE_KEY, JSON.stringify(size));
  } catch {
    // Silently ignore — localStorage may be full or unavailable
  }
}
