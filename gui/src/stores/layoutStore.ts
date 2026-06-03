/**
 * Layout store — holds the 5 pane/splitter dimensions that were previously
 * component-local signals in App.tsx.  Exposing them as a createStore-backed
 * `state` object lets the debug ctx read
 * `window.__REIFY_DEBUG__.stores.layout.state.*` uniformly.
 *
 * The store owns the load-on-init + debounced 300 ms persistence (moved out
 * of App.tsx so there is a single source of truth for the STORAGE_KEY).
 */

import { createEffect, onCleanup } from 'solid-js';
import { createStore } from 'solid-js/store';
import {
  loadPanelLayout,
  savePanelLayout,
  type PanelLayout,
} from '../hooks/useLayoutPersistence';

// ---------------------------------------------------------------------------
// Default values (mirrors App.tsx:97-101; App.tsx drops these after migration)
// ---------------------------------------------------------------------------

export const DEFAULT_EDITOR_WIDTH = 300;
export const DEFAULT_SIDE_WIDTH = 300;
export const DEFAULT_DESIGN_TREE_HEIGHT = 160;
export const DEFAULT_PROPERTY_HEIGHT = 200;
export const DEFAULT_CONSTRAINT_HEIGHT = 140;

// Re-export the PanelLayout type as LayoutState for naming consistency
export type LayoutState = PanelLayout;

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

const SAVE_DEBOUNCE_MS = 300;

export function createLayoutStore() {
  const saved = loadPanelLayout();

  const [state, setState] = createStore<PanelLayout>({
    editorWidth: saved?.editorWidth ?? DEFAULT_EDITOR_WIDTH,
    sideWidth: saved?.sideWidth ?? DEFAULT_SIDE_WIDTH,
    designTreeHeight: saved?.designTreeHeight ?? DEFAULT_DESIGN_TREE_HEIGHT,
    propertyHeight: saved?.propertyHeight ?? DEFAULT_PROPERTY_HEIGHT,
    constraintHeight: saved?.constraintHeight ?? DEFAULT_CONSTRAINT_HEIGHT,
  });

  // -------------------------------------------------------------------------
  // Setters — accept a plain value or a functional updater
  // (Solid's leaf-path setter natively supports updater functions)
  // -------------------------------------------------------------------------

  const setEditorWidth = (v: number | ((prev: number) => number)) =>
    setState('editorWidth', v as any);

  const setSideWidth = (v: number | ((prev: number) => number)) =>
    setState('sideWidth', v as any);

  const setDesignTreeHeight = (v: number | ((prev: number) => number)) =>
    setState('designTreeHeight', v as any);

  const setPropertyHeight = (v: number | ((prev: number) => number)) =>
    setState('propertyHeight', v as any);

  const setConstraintHeight = (v: number | ((prev: number) => number)) =>
    setState('constraintHeight', v as any);

  // -------------------------------------------------------------------------
  // Debounced persistence
  // -------------------------------------------------------------------------

  let saveTimeout: ReturnType<typeof setTimeout> | undefined;

  createEffect(() => {
    const layout: PanelLayout = {
      editorWidth: state.editorWidth,
      sideWidth: state.sideWidth,
      designTreeHeight: state.designTreeHeight,
      propertyHeight: state.propertyHeight,
      constraintHeight: state.constraintHeight,
    };
    clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => savePanelLayout(layout), SAVE_DEBOUNCE_MS);
  });

  onCleanup(() => clearTimeout(saveTimeout));

  return {
    state,
    setEditorWidth,
    setSideWidth,
    setDesignTreeHeight,
    setPropertyHeight,
    setConstraintHeight,
  };
}

export type LayoutStore = ReturnType<typeof createLayoutStore>;
