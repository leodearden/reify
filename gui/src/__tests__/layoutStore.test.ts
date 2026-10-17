/**
 * Unit tests for createLayoutStore() and the LAYOUT_DIMENSIONS-derived
 * loadPanelLayout(): initial state (defaults + localStorage restore), setters,
 * debounced persistence, reset().
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createRoot } from 'solid-js';

import {
  LAYOUT_DIMENSIONS,
  createLayoutStore,
  loadPanelLayout,
} from '../stores/layoutStore';
import { STORAGE_KEY } from '../hooks/useLayoutPersistence';

const defaults = Object.fromEntries(LAYOUT_DIMENSIONS.map((d) => [d.key, d.default]));

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
  it('(a) uses every table default when localStorage is empty', () => {
    withRoot(() => {
      const store = createLayoutStore();
      expect({ ...store.state }).toEqual(defaults);
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

  it('(c) forward-compat: rows missing from the saved layout fall back to their table default', () => {
    // Seed only the two required fields (older save format)
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ editorWidth: 400, sideWidth: 350 }),
    );
    withRoot(() => {
      const store = createLayoutStore();
      expect({ ...store.state }).toEqual({ ...defaults, editorWidth: 400, sideWidth: 350 });
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

// ---------------------------------------------------------------------------
// reset()
// ---------------------------------------------------------------------------

describe('createLayoutStore — reset()', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('(r1) restores every table row after each row was perturbed through set()', () => {
    withRoot(() => {
      const store = createLayoutStore();
      for (const row of LAYOUT_DIMENSIONS) {
        if (row.kind === 'px') store.set(row.key, row.default + 37);
        else store.set(row.key, !row.default);
      }
      for (const row of LAYOUT_DIMENSIONS) {
        expect(store.state[row.key]).not.toBe(row.default);
      }
      store.reset();
      expect({ ...store.state }).toEqual(defaults);
    });
  });

  it('(r2) the reset layout reaches localStorage through the debounced persistence', async () => {
    vi.useFakeTimers();
    await createRoot(async (dispose) => {
      try {
        const store = createLayoutStore();
        store.setProblemsHeight(400);
        store.setProblemsCollapsed((c) => !c);
        await Promise.resolve();
        vi.advanceTimersByTime(300);
        expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({
          problemsHeight: 400,
          problemsCollapsed: false,
        });

        store.reset();
        await Promise.resolve();
        vi.advanceTimersByTime(300);
        expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toEqual(defaults);
      } finally {
        dispose();
      }
    });
  });
});

// ---------------------------------------------------------------------------
// loadPanelLayout — validation derived from LAYOUT_DIMENSIONS
// ---------------------------------------------------------------------------

describe('loadPanelLayout (derived from LAYOUT_DIMENSIONS)', () => {
  const fullLayout = {
    editorWidth: 400,
    sideWidth: 350,
    designTreeHeight: 180,
    propertyHeight: 250,
    constraintHeight: 150,
    problemsHeight: 200,
    problemsCollapsed: false,
  };

  const store = (value: unknown) => localStorage.setItem(STORAGE_KEY, JSON.stringify(value));

  it('returns null when nothing is stored', () => {
    expect(loadPanelLayout()).toBeNull();
  });

  it('returns null when the stored JSON is corrupt', () => {
    localStorage.setItem(STORAGE_KEY, '{not valid json!!!');
    expect(loadPanelLayout()).toBeNull();
  });

  it('round-trips a full layout', () => {
    store(fullLayout);
    expect(loadPanelLayout()).toEqual(fullLayout);
  });

  const requiredRows = LAYOUT_DIMENSIONS.filter((d) => 'requiredInSaved' in d && d.requiredInSaved);

  it('the table marks at least one row requiredInSaved', () => {
    expect(requiredRows.length).toBeGreaterThan(0);
  });

  for (const row of requiredRows) {
    it(`returns null when required row ${row.key} is absent`, () => {
      const { [row.key]: _dropped, ...rest } = fullLayout;
      store(rest);
      expect(loadPanelLayout()).toBeNull();
    });

    it(`returns null when required row ${row.key} has the wrong type`, () => {
      store({ ...fullLayout, [row.key]: row.kind === 'px' ? 'wide' : 1 });
      expect(loadPanelLayout()).toBeNull();
    });
  }

  it('omits optional rows absent from the saved JSON: the result is exactly the saved subset', () => {
    const requiredOnly = Object.fromEntries(
      requiredRows.map((row) => [row.key, fullLayout[row.key]]),
    );
    store(requiredOnly);
    expect(loadPanelLayout()).toEqual(requiredOnly);

    // A layout saved before the docked problems panel existed.
    const { problemsHeight: _h, problemsCollapsed: _c, ...oldLayout } = fullLayout;
    store(oldLayout);
    expect(loadPanelLayout()).toEqual(oldLayout);
  });

  it('omits a px row stored as a string and a flag row stored as a number, keeping the rest', () => {
    store({ ...fullLayout, designTreeHeight: '180', problemsCollapsed: 1 });
    const { designTreeHeight: _d, problemsCollapsed: _c, ...rest } = fullLayout;
    expect(loadPanelLayout()).toEqual(rest);
  });
});
