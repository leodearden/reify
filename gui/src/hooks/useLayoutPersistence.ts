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

export function loadPanelLayout(): PanelLayout | null {
  return null;
}

export function savePanelLayout(_layout: PanelLayout): void {
  // stub
}
