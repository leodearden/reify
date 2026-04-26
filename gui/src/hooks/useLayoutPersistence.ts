/**
 * Panel layout persistence via localStorage.
 * Pure functions for loading and saving panel dimensions.
 */

export const STORAGE_KEY = 'reify-panel-layout';

export type PanelLayout = {
  editorWidth: number;
  sideWidth: number;
  designTreeHeight: number;
  propertyHeight: number;
  constraintHeight: number;
};

/** Load persisted panel layout from localStorage. Returns null if missing, invalid, or incomplete.
 *  Missing sub-panel heights fall back to `undefined` so the caller can apply defaults —
 *  this keeps older saved layouts forward-compatible when new panels become resizable. */
export function loadPanelLayout(): Partial<PanelLayout> | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return null;

    const parsed = JSON.parse(raw);
    if (
      typeof parsed !== 'object' ||
      parsed === null ||
      typeof parsed.editorWidth !== 'number' ||
      typeof parsed.sideWidth !== 'number'
    ) {
      return null;
    }

    const out: Partial<PanelLayout> = {
      editorWidth: parsed.editorWidth,
      sideWidth: parsed.sideWidth,
    };
    if (typeof parsed.designTreeHeight === 'number') out.designTreeHeight = parsed.designTreeHeight;
    if (typeof parsed.propertyHeight === 'number') out.propertyHeight = parsed.propertyHeight;
    if (typeof parsed.constraintHeight === 'number') out.constraintHeight = parsed.constraintHeight;
    return out;
  } catch {
    return null;
  }
}

/** Save panel layout to localStorage. */
export function savePanelLayout(layout: PanelLayout): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // Silently ignore — localStorage may be full or unavailable
  }
}
