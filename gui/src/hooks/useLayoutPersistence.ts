/**
 * Panel layout persistence via localStorage.
 * Pure functions for loading and saving panel dimensions.
 */

export const STORAGE_KEY = 'reify-panel-layout';

export type PanelLayout = {
  editorWidth: number;
  sideWidth: number;
  propertyHeight: number;
};

/** Load persisted panel layout from localStorage. Returns null if missing, invalid, or incomplete. */
export function loadPanelLayout(): PanelLayout | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return null;

    const parsed = JSON.parse(raw);
    if (
      typeof parsed === 'object' &&
      parsed !== null &&
      typeof parsed.editorWidth === 'number' &&
      typeof parsed.sideWidth === 'number' &&
      typeof parsed.propertyHeight === 'number'
    ) {
      return {
        editorWidth: parsed.editorWidth,
        sideWidth: parsed.sideWidth,
        propertyHeight: parsed.propertyHeight,
      };
    }
    return null;
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
