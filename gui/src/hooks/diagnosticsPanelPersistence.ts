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

/**
 * Load persisted panel size. Returns null if missing, invalid, or wrong shape.
 *
 * Validation: each dimension must be a finite positive number within the sane
 * range (0, 10000]. Values outside this range (including NaN, ±Infinity,
 * negative numbers, zero, or values above 10000) are rejected and null is
 * returned, causing the panel to fall back to computeDefaultDialogSize.
 */
export function loadDiagnosticsPanelSize(): { width: number; height: number } | null {
  try {
    const raw = localStorage.getItem(DIAGNOSTICS_PANEL_SIZE_KEY);
    if (raw === null) return null;

    const parsed = JSON.parse(raw);
    if (
      parsed !== null &&
      typeof parsed === 'object' &&
      isValidDimension(parsed.width) &&
      isValidDimension(parsed.height)
    ) {
      return { width: parsed.width, height: parsed.height };
    }
    return null;
  } catch {
    return null;
  }
}

/** Returns true iff v is a finite positive number within the sane range (0, 10000]. */
function isValidDimension(v: unknown): v is number {
  return typeof v === 'number' && Number.isFinite(v) && v > 0 && v <= 10000;
}

/** Save panel size to localStorage. */
export function saveDiagnosticsPanelSize(size: { width: number; height: number }): void {
  try {
    localStorage.setItem(DIAGNOSTICS_PANEL_SIZE_KEY, JSON.stringify(size));
  } catch {
    // Silently ignore — localStorage may be full or unavailable
  }
}

export interface ComputeDefaultDialogSizeParams {
  /** Number of characters in the longest diagnostic message. */
  longestMessageChars: number;
  /** Browser viewport width in pixels. */
  viewportWidth: number;
  /** Browser viewport height in pixels. */
  viewportHeight: number;
  /** Approximate pixel width of one monospace character. Default: 8. */
  monoCharPx?: number;
  /** Extra horizontal chrome (padding, badges, etc.) in pixels. Default: 80. */
  chromePx?: number;
  /** Minimum dialog width in pixels. Default: 480. */
  minWidth?: number;
  /** Maximum dialog width as a fraction of viewport width. Default: 0.9. */
  maxFractionOfViewport?: number;
}

/**
 * Compute a sensible default dialog size based on viewport dimensions and
 * the longest message in the diagnostics list.
 *
 * Width: clamp(longestMessageChars * monoCharPx + chromePx, minWidth, viewportWidth * maxFraction)
 * Height: min(viewportHeight * 0.8, 600)
 */
export function computeDefaultDialogSize({
  longestMessageChars,
  viewportWidth,
  viewportHeight,
  monoCharPx = 8,
  chromePx = 80,
  minWidth = 480,
  maxFractionOfViewport = 0.9,
}: ComputeDefaultDialogSizeParams): { width: number; height: number } {
  const contentWidth = longestMessageChars * monoCharPx + chromePx;
  const maxWidth = viewportWidth * maxFractionOfViewport;
  const width = Math.max(minWidth, Math.min(contentWidth, maxWidth));
  const height = Math.min(viewportHeight * 0.8, 600);
  return { width, height };
}
