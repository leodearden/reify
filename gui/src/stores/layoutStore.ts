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
export const DEFAULT_PROBLEMS_HEIGHT = 160;
export const DEFAULT_PROBLEMS_COLLAPSED = true;

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
    problemsHeight: saved?.problemsHeight ?? DEFAULT_PROBLEMS_HEIGHT,
    problemsCollapsed: saved?.problemsCollapsed ?? DEFAULT_PROBLEMS_COLLAPSED,
  });

  // -------------------------------------------------------------------------
  // Setters — accept a plain value or a functional updater.
  // The `as any` cast is localised here; Solid's leaf-path setter natively
  // supports updater functions so the cast is safe for all 5 dimensions.
  // -------------------------------------------------------------------------

  type Updater = number | ((prev: number) => number);
  const makeSetter = (key: keyof PanelLayout) => (v: Updater) =>
    setState(key, v as any);

  const setEditorWidth = makeSetter('editorWidth');
  const setSideWidth = makeSetter('sideWidth');
  const setDesignTreeHeight = makeSetter('designTreeHeight');
  const setPropertyHeight = makeSetter('propertyHeight');
  const setConstraintHeight = makeSetter('constraintHeight');
  const setProblemsHeight = makeSetter('problemsHeight');

  type BooleanUpdater = boolean | ((prev: boolean) => boolean);
  const setProblemsCollapsed = (v: BooleanUpdater) =>
    setState('problemsCollapsed', v as any);

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
      problemsHeight: state.problemsHeight,
      problemsCollapsed: state.problemsCollapsed,
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
    setProblemsHeight,
    setProblemsCollapsed,
  };
}

export type LayoutStore = ReturnType<typeof createLayoutStore>;
