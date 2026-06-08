// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import {
  loadPanelLayout,
  savePanelLayout,
  STORAGE_KEY,
  clampPanelHeightsToFit,
  type ClampOptions,
} from '../hooks/useLayoutPersistence';

describe('useLayoutPersistence', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('loadPanelLayout returns null when no data in localStorage', () => {
    const result = loadPanelLayout();
    expect(result).toBeNull();
  });

  it('loadPanelLayout returns parsed PanelLayout when valid JSON exists', () => {
    const layout = {
      editorWidth: 400,
      sideWidth: 350,
      designTreeHeight: 180,
      propertyHeight: 250,
      constraintHeight: 150,
      problemsHeight: 160,
      problemsCollapsed: true,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(layout));

    const result = loadPanelLayout();
    expect(result).toEqual(layout);
  });

  it('savePanelLayout writes serialized layout to localStorage', () => {
    const layout = {
      editorWidth: 400,
      sideWidth: 350,
      designTreeHeight: 180,
      propertyHeight: 250,
      constraintHeight: 150,
      problemsHeight: 160,
      problemsCollapsed: true,
    };
    savePanelLayout(layout);

    const stored = localStorage.getItem(STORAGE_KEY);
    expect(stored).not.toBeNull();
    expect(JSON.parse(stored!)).toEqual(layout);
  });

  it('loadPanelLayout returns null when localStorage contains corrupted JSON', () => {
    localStorage.setItem(STORAGE_KEY, '{not valid json!!!');

    const result = loadPanelLayout();
    expect(result).toBeNull();
  });

  it('loadPanelLayout returns null when editorWidth or sideWidth is missing', () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ editorWidth: 400 }));

    const result = loadPanelLayout();
    expect(result).toBeNull();
  });

  it('loadPanelLayout omits sub-panel heights that are missing in stored data (forward-compat)', () => {
    // Older saved layouts won't have the three-splitter fields — the loader should
    // return them as undefined so callers apply defaults rather than returning null.
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ editorWidth: 400, sideWidth: 350 }));

    const result = loadPanelLayout();
    expect(result).toEqual({ editorWidth: 400, sideWidth: 350 });
  });

  it('(a) loadPanelLayout omits problemsHeight/problemsCollapsed when absent in stored JSON (forward-compat)', () => {
    // A layout saved before the docked panel existed should load without the new fields.
    const oldLayout = {
      editorWidth: 400,
      sideWidth: 350,
      designTreeHeight: 180,
      propertyHeight: 250,
      constraintHeight: 150,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(oldLayout));

    const result = loadPanelLayout();
    expect(result).toEqual(oldLayout);
    expect((result as Record<string, unknown>).problemsHeight).toBeUndefined();
    expect((result as Record<string, unknown>).problemsCollapsed).toBeUndefined();
  });

  it('(b) loadPanelLayout includes problemsHeight when it is a number and problemsCollapsed when it is a boolean', () => {
    const layout = {
      editorWidth: 400,
      sideWidth: 350,
      designTreeHeight: 180,
      propertyHeight: 250,
      constraintHeight: 150,
      problemsHeight: 200,
      problemsCollapsed: false,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(layout));

    const result = loadPanelLayout();
    expect((result as Record<string, unknown>).problemsHeight).toBe(200);
    expect((result as Record<string, unknown>).problemsCollapsed).toBe(false);
  });
});

describe('clampPanelHeightsToFit', () => {
  // Mirror the App.tsx constants so the tests reflect production behaviour.
  const opts: ClampOptions = {
    chatOpen: true,
    chatMinHeight: 160,
    minPanelHeight: 80,
    splitterThickness: 4,
  };
  // chatOpen=true → splitters=12, chatFloor=160 → reserve = 172
  // available = containerHeight - 172
  const reserve = 12 + 160;

  it('returns preferred unchanged when sum fits in available', () => {
    const preferred = { designTree: 160, property: 200, constraint: 140 };
    const containerHeight = preferred.designTree + preferred.property + preferred.constraint + reserve + 100;
    const result = clampPanelHeightsToFit(preferred, containerHeight, opts);
    expect(result).toEqual(preferred);
  });

  it('returns preferred unchanged at the boundary (sum === available)', () => {
    const preferred = { designTree: 160, property: 200, constraint: 140 };
    const containerHeight = preferred.designTree + preferred.property + preferred.constraint + reserve;
    const result = clampPanelHeightsToFit(preferred, containerHeight, opts);
    expect(result).toEqual(preferred);
  });

  it('proportionally shrinks panels when sum exceeds available', () => {
    // Live measurement from the bug: 584 + 508 + 150 in 817-px container with chat open.
    const preferred = { designTree: 584, property: 508, constraint: 150 };
    const containerHeight = 817;
    const available = containerHeight - reserve; // 645
    const result = clampPanelHeightsToFit(preferred, containerHeight, opts);
    // Each value should be roughly preferred * (645 / 1242), preserving order.
    const sum = result.designTree + result.property + result.constraint;
    expect(sum).toBeLessThanOrEqual(available);
    expect(result.designTree).toBeGreaterThan(result.property * 0.9); // ~scaled
    expect(result.designTree).toBeGreaterThan(result.constraint);
    expect(result.property).toBeGreaterThan(result.constraint);
    // Bug repro: post-clamp the chat-panel-bottom should fit in window.
    expect(sum + reserve).toBeLessThanOrEqual(containerHeight);
  });

  it('raises any panel below MIN_PANEL_HEIGHT to the floor', () => {
    // designTree very large vs property/constraint small. Proportional scaling
    // would push property below MIN; the helper must raise it.
    const preferred = { designTree: 1000, property: 50, constraint: 50 };
    // available so scaled property = 50 * (300 / 1100) ≈ 13 < 80. Result must
    // be raised to 80 and the larger panel reduced.
    const containerHeight = 300 + reserve;
    const result = clampPanelHeightsToFit(preferred, containerHeight, opts);
    expect(result.property).toBeGreaterThanOrEqual(opts.minPanelHeight);
    expect(result.constraint).toBeGreaterThanOrEqual(opts.minPanelHeight);
    expect(result.designTree + result.property + result.constraint).toBeLessThanOrEqual(300);
  });

  it('uses 2 splitters and no chat floor when chat is closed', () => {
    // chatOpen=false → splitters=8, chatFloor=0 → reserve=8
    const closedOpts: ClampOptions = { ...opts, chatOpen: false };
    const preferred = { designTree: 200, property: 200, constraint: 200 };
    // Container fits all three with closed-chat reserve but NOT with open-chat reserve.
    const containerHeight = 600 + 8;
    const result = clampPanelHeightsToFit(preferred, containerHeight, closedOpts);
    expect(result).toEqual(preferred);
    // Same container, chat open: must clamp because reserve grows by 164.
    const resultOpen = clampPanelHeightsToFit(preferred, containerHeight, opts);
    expect(resultOpen.designTree + resultOpen.property + resultOpen.constraint)
      .toBeLessThanOrEqual(containerHeight - reserve);
  });

  it('returns the floor (3 × MIN_PANEL_HEIGHT) for pathologically small containers', () => {
    // available < 3 * 80 = 240 → caller's CSS overflow:hidden will clip.
    const preferred = { designTree: 200, property: 200, constraint: 200 };
    const containerHeight = 100 + reserve; // available = 100, less than 240
    const result = clampPanelHeightsToFit(preferred, containerHeight, opts);
    expect(result).toEqual({
      designTree: opts.minPanelHeight,
      property: opts.minPanelHeight,
      constraint: opts.minPanelHeight,
    });
  });
});
