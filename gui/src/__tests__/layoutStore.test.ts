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
  DEFAULT_PROBLEMS_HEIGHT,
  DEFAULT_PROBLEMS_COLLAPSED,
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
      expect(store.state.problemsHeight).toBe(DEFAULT_PROBLEMS_HEIGHT);
      expect(store.state.problemsCollapsed).toBe(DEFAULT_PROBLEMS_COLLAPSED);
    });
  });

  it('(b) restores all 7 values from a full saved layout', () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        editorWidth: 400,
        sideWidth: 350,
        designTreeHeight: 180,
        propertyHeight: 250,
        constraintHeight: 150,
        problemsHeight: 200,
        problemsCollapsed: false,
      }),
    );
    withRoot(() => {
      const store = createLayoutStore();
      expect(store.state.editorWidth).toBe(400);
      expect(store.state.sideWidth).toBe(350);
      expect(store.state.designTreeHeight).toBe(180);
      expect(store.state.propertyHeight).toBe(250);
      expect(store.state.constraintHeight).toBe(150);
      expect(store.state.problemsHeight).toBe(200);
      expect(store.state.problemsCollapsed).toBe(false);
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
      expect(store.state.problemsHeight).toBe(DEFAULT_PROBLEMS_HEIGHT);
      expect(store.state.problemsCollapsed).toBe(DEFAULT_PROBLEMS_COLLAPSED);
    });
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

  it('(f) setProblemsHeight updates state — plain value', () => {
    createRoot((dispose) => {
      const store = createLayoutStore();
      store.setProblemsHeight(240);
      expect(store.state.problemsHeight).toBe(240);
      dispose();
    });
  });

  it('(g) setProblemsHeight updates state — functional updater', () => {
    createRoot((dispose) => {
      const store = createLayoutStore();
      const initial = store.state.problemsHeight;
      store.setProblemsHeight((h) => h + 40);
      expect(store.state.problemsHeight).toBe(initial + 40);
      dispose();
    });
  });

  it('(h) setProblemsCollapsed updates state — boolean value', () => {
    createRoot((dispose) => {
      const store = createLayoutStore();
      store.setProblemsCollapsed(false);
      expect(store.state.problemsCollapsed).toBe(false);
      store.setProblemsCollapsed(true);
      expect(store.state.problemsCollapsed).toBe(true);
      dispose();
    });
  });

  it('(i) setProblemsCollapsed updates state — functional toggle c=>!c', () => {
    createRoot((dispose) => {
      const store = createLayoutStore();
      const initial = store.state.problemsCollapsed;
      store.setProblemsCollapsed((c) => !c);
      expect(store.state.problemsCollapsed).toBe(!initial);
      store.setProblemsCollapsed((c) => !c);
      expect(store.state.problemsCollapsed).toBe(initial);
      dispose();
    });
  });

  it('(j) persistence round-trip writes problemsHeight + problemsCollapsed', async () => {
    await createRoot(async (dispose) => {
      const store = createLayoutStore();
      store.setProblemsHeight(220);
      store.setProblemsCollapsed(false);

      await Promise.resolve();
      vi.advanceTimersByTime(300);

      const saved = loadPanelLayout();
      expect(saved?.problemsHeight).toBe(220);
      expect(saved?.problemsCollapsed).toBe(false);
      dispose();
    });
  });
});
