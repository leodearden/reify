/**
 * Unit tests for createLayoutStore().
 * Covers: initial state (defaults + localStorage restore), setters, debounced persistence.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createRoot } from 'solid-js';

import {
  createLayoutStore,
  DEFAULT_EDITOR_WIDTH,
  DEFAULT_SIDE_WIDTH,
  DEFAULT_DESIGN_TREE_HEIGHT,
  DEFAULT_PROPERTY_HEIGHT,
  DEFAULT_CONSTRAINT_HEIGHT,
} from '../stores/layoutStore';
import { STORAGE_KEY, loadPanelLayout } from '../hooks/useLayoutPersistence';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Run fn inside a SolidJS root and dispose immediately after. */
function withRoot<T>(fn: () => T): T {
  let result!: T;
  createRoot((dispose) => {
    result = fn();
    dispose();
  });
  return result;
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

beforeEach(() => {
  localStorage.clear();
});

// ---------------------------------------------------------------------------
// Initial state
// ---------------------------------------------------------------------------

describe('createLayoutStore — initial state', () => {
  it('(a) uses DEFAULT_* when localStorage is empty', () => {
    withRoot(() => {
      const store = createLayoutStore();
      expect(store.state.editorWidth).toBe(DEFAULT_EDITOR_WIDTH);
      expect(store.state.sideWidth).toBe(DEFAULT_SIDE_WIDTH);
      expect(store.state.designTreeHeight).toBe(DEFAULT_DESIGN_TREE_HEIGHT);
      expect(store.state.propertyHeight).toBe(DEFAULT_PROPERTY_HEIGHT);
      expect(store.state.constraintHeight).toBe(DEFAULT_CONSTRAINT_HEIGHT);
    });
  });

  it('(b) restores all 5 values from a full saved layout', () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        editorWidth: 400,
        sideWidth: 350,
        designTreeHeight: 180,
        propertyHeight: 250,
        constraintHeight: 150,
      }),
    );
    withRoot(() => {
      const store = createLayoutStore();
      expect(store.state.editorWidth).toBe(400);
      expect(store.state.sideWidth).toBe(350);
      expect(store.state.designTreeHeight).toBe(180);
      expect(store.state.propertyHeight).toBe(250);
      expect(store.state.constraintHeight).toBe(150);
    });
  });

  it('(c) forward-compat: missing sub-panel heights fall back to DEFAULT_*', () => {
    // Seed only the two required fields (older save format)
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ editorWidth: 400, sideWidth: 350 }),
    );
    withRoot(() => {
      const store = createLayoutStore();
      expect(store.state.editorWidth).toBe(400);
      expect(store.state.sideWidth).toBe(350);
      expect(store.state.designTreeHeight).toBe(DEFAULT_DESIGN_TREE_HEIGHT);
      expect(store.state.propertyHeight).toBe(DEFAULT_PROPERTY_HEIGHT);
      expect(store.state.constraintHeight).toBe(DEFAULT_CONSTRAINT_HEIGHT);
    });
  });

  it('exported DEFAULT_* constants match App.tsx original values', () => {
    expect(DEFAULT_EDITOR_WIDTH).toBe(300);
    expect(DEFAULT_SIDE_WIDTH).toBe(300);
    expect(DEFAULT_DESIGN_TREE_HEIGHT).toBe(160);
    expect(DEFAULT_PROPERTY_HEIGHT).toBe(200);
    expect(DEFAULT_CONSTRAINT_HEIGHT).toBe(140);
  });
});

// ---------------------------------------------------------------------------
// Setters + debounced persistence (added in step-3)
// ---------------------------------------------------------------------------

describe('createLayoutStore — setters + debounced persistence', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('(a) setEditorWidth updates state synchronously', () => {
    createRoot((dispose) => {
      const store = createLayoutStore();
      store.setEditorWidth(500);
      expect(store.state.editorWidth).toBe(500);
      dispose();
    });
  });

  it('(b) functional updater works for setSideWidth', () => {
    createRoot((dispose) => {
      const store = createLayoutStore();
      const initial = store.state.sideWidth;
      store.setSideWidth((w) => w + 50);
      expect(store.state.sideWidth).toBe(initial + 50);
      dispose();
    });
  });

  it('(c) persistence round-trip: setter → flush effects → advance 300ms → localStorage updated', async () => {
    await createRoot(async (dispose) => {
      const store = createLayoutStore();
      store.setEditorWidth(555);
      store.setSideWidth(444);
      store.setDesignTreeHeight(170);
      store.setPropertyHeight(210);
      store.setConstraintHeight(130);

      // Flush Solid's microtask queue so the createEffect reads new values
      await Promise.resolve();

      vi.advanceTimersByTime(300);

      const saved = loadPanelLayout();
      expect(saved?.editorWidth).toBe(555);
      expect(saved?.sideWidth).toBe(444);
      expect(saved?.designTreeHeight).toBe(170);
      expect(saved?.propertyHeight).toBe(210);
      expect(saved?.constraintHeight).toBe(130);
      dispose();
    });
  });

  it('(d) debounce coalesces — two rapid setEditorWidth calls write only the final value', async () => {
    await createRoot(async (dispose) => {
      const store = createLayoutStore();

      store.setEditorWidth(100);
      store.setEditorWidth(200);

      await Promise.resolve();
      vi.advanceTimersByTime(300);

      const saved = loadPanelLayout();
      expect(saved?.editorWidth).toBe(200);
      dispose();
    });
  });

  it('(e) all 5 setters update state', () => {
    createRoot((dispose) => {
      const store = createLayoutStore();
      store.setEditorWidth(301);
      store.setSideWidth(302);
      store.setDesignTreeHeight(161);
      store.setPropertyHeight(201);
      store.setConstraintHeight(141);
      expect(store.state.editorWidth).toBe(301);
      expect(store.state.sideWidth).toBe(302);
      expect(store.state.designTreeHeight).toBe(161);
      expect(store.state.propertyHeight).toBe(201);
      expect(store.state.constraintHeight).toBe(141);
      dispose();
    });
  });
});
