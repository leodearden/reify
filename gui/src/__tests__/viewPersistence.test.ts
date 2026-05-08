// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import type { PersistentViewState } from '../types';
import type { ViewDefinition } from '../stores/autoViewGenerator';
import type { CameraState } from '../stores/viewportStore';
import type { VisibilityState } from '../types';
import {
  loadViewPersistence,
  saveViewPersistence,
  STORAGE_KEY_PREFIX,
  createDebouncedSaver,
} from '../stores/viewPersistence';

/**
 * Type-level shape test: construct a full PersistentViewState literal and
 * confirm it satisfies the interface. TypeScript will error here until the
 * type is defined in types.ts (step-2).
 */
function _typeLevelCheck(): void {
  const view1: ViewDefinition = {
    id: 'user:view-1',
    name: 'My View',
    auto: false,
    visibility: { 'Assembly.flange': 'show' as VisibilityState },
  };

  const cam: CameraState = {
    position: [0, 0, 10],
    target: [0, 0, 0],
    up: [0, 1, 0],
    zoom: 1.0,
  };

  // This will fail to compile until PersistentViewState is defined in types.ts
  const state: PersistentViewState = {
    version: '2',
    activeViewId: 'user:view-1',
    userViews: [view1],
    explicit: { 'Assembly.flange': 'show' as VisibilityState },
    viewportCameras: { 'viewport-main': cam },
    timestamp: '2026-04-22T00:00:00.000Z',
  };

  // Verify field types by assignment
  const _version: '2' = state.version;
  const _activeViewId: string = state.activeViewId;
  const _userViews: ViewDefinition[] = state.userViews;
  const _explicit: Record<string, VisibilityState> = state.explicit;
  const _viewportCameras: Record<string, CameraState> = state.viewportCameras;
  const _timestamp: string = state.timestamp;

  void _version;
  void _activeViewId;
  void _userViews;
  void _explicit;
  void _viewportCameras;
  void _timestamp;
  void state;
}

void _typeLevelCheck;

describe('PersistentViewState — runtime constructor shape', () => {
  it('constructs a valid PersistentViewState with required fields', () => {
    // This tests that the shape is usable at runtime (not just compile-time)
    // once the type is defined.
    const cam: CameraState = {
      position: [1, 2, 3],
      target: [0, 0, 0],
      up: [0, 1, 0],
      zoom: 2.0,
    };

    const view: ViewDefinition = {
      id: 'user:v1',
      name: 'Test View',
      auto: false,
      visibility: {},
    };

    const state: PersistentViewState = {
      version: '2',
      activeViewId: 'user:v1',
      userViews: [view],
      explicit: { 'Root.geometry': 'hidden' as VisibilityState },
      viewportCameras: { main: cam },
      timestamp: new Date().toISOString(),
    };

    expect(state.version).toBe('2');
    expect(state.activeViewId).toBe('user:v1');
    expect(state.userViews).toHaveLength(1);
    expect(state.userViews[0].id).toBe('user:v1');
    expect(state.explicit).toEqual({ 'Root.geometry': 'hidden' });
    expect(state.viewportCameras['main']).toEqual(cam);
    expect(typeof state.timestamp).toBe('string');
  });

  it('version field must be the literal string "2"', () => {
    // Runtime check that version is stored and retrieved as "2"
    const state: PersistentViewState = {
      version: '2',
      activeViewId: 'auto:default',
      userViews: [],
      explicit: {},
      viewportCameras: {},
      timestamp: '2026-04-22T00:00:00.000Z',
    };
    expect(state.version).toBe('2');
    expect(state.version).not.toBe(2); // must be string, not number
  });

  it('userViews array holds ViewDefinition objects with all required fields', () => {
    const views: ViewDefinition[] = [
      { id: 'user:a', name: 'Alpha', auto: false, visibility: {} },
      { id: 'user:b', name: 'Beta', auto: false, visibility: { 'X.y': 'ghost' as VisibilityState } },
    ];

    const state: PersistentViewState = {
      version: '2',
      activeViewId: 'user:a',
      userViews: views,
      explicit: {},
      viewportCameras: {},
      timestamp: '2026-04-22T00:00:00.000Z',
    };

    expect(state.userViews[0].name).toBe('Alpha');
    expect(state.userViews[1].visibility['X.y']).toBe('ghost');
  });

  it('viewportCameras maps viewport id to CameraState', () => {
    const cam1: CameraState = { position: [1, 0, 0], target: [0, 0, 0], up: [0, 1, 0], zoom: 1 };
    const cam2: CameraState = { position: [0, 1, 0], target: [0, 0, 0], up: [0, 0, 1], zoom: 2 };

    const state: PersistentViewState = {
      version: '2',
      activeViewId: 'auto:default',
      userViews: [],
      explicit: {},
      viewportCameras: { 'vp-left': cam1, 'vp-right': cam2 },
      timestamp: '2026-04-22T00:00:00.000Z',
    };

    expect(state.viewportCameras['vp-left'].position).toEqual([1, 0, 0]);
    expect(state.viewportCameras['vp-right'].zoom).toBe(2);
  });
});

// ---------------------------------------------------------------------------
// Step-3: loadViewPersistence / saveViewPersistence / STORAGE_KEY_PREFIX tests
// ---------------------------------------------------------------------------

/** Minimal valid PersistentViewState for testing. */
function makeState(overrides?: Partial<PersistentViewState>): PersistentViewState {
  return {
    version: '2',
    activeViewId: 'auto:default',
    userViews: [],
    explicit: {},
    viewportCameras: {},
    timestamp: '2026-04-22T00:00:00.000Z',
    ...overrides,
  };
}

const TEST_PATH = '/home/user/project/bracket.ri';
const TEST_PATH_B = '/home/user/project/other.ri';

describe('STORAGE_KEY_PREFIX', () => {
  it('is the expected prefix string', () => {
    expect(STORAGE_KEY_PREFIX).toBe('reify:views:');
  });
});

describe('loadViewPersistence', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('(a) returns null when no entry for that path', () => {
    const result = loadViewPersistence(TEST_PATH);
    expect(result).toBeNull();
  });

  it('(b) saveViewPersistence writes JSON under `reify:views:{absPath}`', () => {
    const state = makeState();
    saveViewPersistence(TEST_PATH, state);
    const key = `${STORAGE_KEY_PREFIX}${TEST_PATH}`;
    const raw = localStorage.getItem(key);
    expect(raw).not.toBeNull();
    expect(JSON.parse(raw!).version).toBe('2');
  });

  it('(c) load parses a valid stored entry round-trip', () => {
    const state = makeState({
      activeViewId: 'user:my-view',
      userViews: [
        { id: 'user:my-view', name: 'My View', auto: false, visibility: {} },
      ],
      explicit: { 'Root.geometry': 'hidden' as VisibilityState },
    });
    saveViewPersistence(TEST_PATH, state);
    const loaded = loadViewPersistence(TEST_PATH);
    expect(loaded).not.toBeNull();
    expect(loaded!.version).toBe('2');
    expect(loaded!.activeViewId).toBe('user:my-view');
    expect(loaded!.userViews).toHaveLength(1);
    expect(loaded!.explicit['Root.geometry']).toBe('hidden');
  });

  it('(d) returns null on corrupt JSON', () => {
    localStorage.setItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`, '{not valid json!!!');
    expect(loadViewPersistence(TEST_PATH)).toBeNull();
  });

  it('(e) returns null when required field is missing (type-guard)', () => {
    // version field missing
    localStorage.setItem(
      `${STORAGE_KEY_PREFIX}${TEST_PATH}`,
      JSON.stringify({
        activeViewId: 'auto:default',
        userViews: [],
        explicit: {},
        viewportCameras: {},
        timestamp: '2026-04-22T00:00:00.000Z',
        // no version
      }),
    );
    expect(loadViewPersistence(TEST_PATH)).toBeNull();
  });

  it('(e2) returns null when userViews is not an array', () => {
    localStorage.setItem(
      `${STORAGE_KEY_PREFIX}${TEST_PATH}`,
      JSON.stringify({
        version: '2',
        activeViewId: 'auto:default',
        userViews: 'wrong',
        explicit: {},
        viewportCameras: {},
        timestamp: '2026-04-22T00:00:00.000Z',
      }),
    );
    expect(loadViewPersistence(TEST_PATH)).toBeNull();
  });

  it('returns null for legacy v1 schema (Task 3233 — invalidate Y-up cameras)', () => {
    // Direct JSON injection bypasses the TS-typed PersistentViewState literal
    // (which carries the new `version: '2'` constraint after the bump).
    // This mirrors the raw-JSON pattern in test (e) above.
    localStorage.setItem(
      `${STORAGE_KEY_PREFIX}${TEST_PATH}`,
      JSON.stringify({
        version: '1',
        activeViewId: 'auto:default',
        userViews: [],
        explicit: {},
        viewportCameras: { 'design-main': { position: [0, 10, 0], target: [0, 0, 0], up: [0, 1, 0], zoom: 1 } },
        timestamp: '2026-04-22T00:00:00.000Z',
      }),
    );
    expect(loadViewPersistence(TEST_PATH)).toBeNull();
  });

  it('(f) multi-path isolation — save on path A does not affect path B', () => {
    const stateA = makeState({ activeViewId: 'user:alpha' });
    const stateB = makeState({ activeViewId: 'user:beta' });

    saveViewPersistence(TEST_PATH, stateA);
    saveViewPersistence(TEST_PATH_B, stateB);

    const loadedA = loadViewPersistence(TEST_PATH);
    const loadedB = loadViewPersistence(TEST_PATH_B);

    expect(loadedA!.activeViewId).toBe('user:alpha');
    expect(loadedB!.activeViewId).toBe('user:beta');
  });

  it('(f2) clear path A does not affect path B', () => {
    const stateA = makeState({ activeViewId: 'user:alpha' });
    const stateB = makeState({ activeViewId: 'user:beta' });

    saveViewPersistence(TEST_PATH, stateA);
    saveViewPersistence(TEST_PATH_B, stateB);

    // Remove path A entry
    localStorage.removeItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`);

    expect(loadViewPersistence(TEST_PATH)).toBeNull();
    expect(loadViewPersistence(TEST_PATH_B)!.activeViewId).toBe('user:beta');
  });
});

// ---------------------------------------------------------------------------
// Step-5: createDebouncedSaver tests
// ---------------------------------------------------------------------------

describe('createDebouncedSaver', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('(a) 3 rapid calls within delayMs produce only 1 localStorage write after delay', () => {
    const saver = createDebouncedSaver(100);
    const stateA = makeState({ activeViewId: 'user:a' });
    const stateB = makeState({ activeViewId: 'user:b' });
    const stateC = makeState({ activeViewId: 'user:c' });

    saver.schedule(TEST_PATH, stateA);
    saver.schedule(TEST_PATH, stateB);
    saver.schedule(TEST_PATH, stateC);

    // No write yet (debounce window still open)
    expect(localStorage.getItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`)).toBeNull();

    vi.advanceTimersByTime(100);

    // Exactly one write should have happened — with the LAST state
    const raw = localStorage.getItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`);
    expect(raw).not.toBeNull();
    expect(JSON.parse(raw!).activeViewId).toBe('user:c');
  });

  it('(b) timestamp in the written state reflects the last call time', () => {
    const saver = createDebouncedSaver(100);
    // Inject a custom timestamp by using a state with known timestamp
    const state = makeState({ timestamp: '2026-04-22T12:00:00.000Z' });

    saver.schedule(TEST_PATH, state);
    vi.advanceTimersByTime(100);

    const raw = localStorage.getItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`);
    expect(JSON.parse(raw!).timestamp).toBe('2026-04-22T12:00:00.000Z');
  });

  it('(c) calls after delay write again (each schedule+advance produces a write)', () => {
    const saver = createDebouncedSaver(100);
    const state1 = makeState({ activeViewId: 'user:first' });
    const state2 = makeState({ activeViewId: 'user:second' });

    saver.schedule(TEST_PATH, state1);
    vi.advanceTimersByTime(100);

    const raw1 = localStorage.getItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`);
    expect(JSON.parse(raw1!).activeViewId).toBe('user:first');

    saver.schedule(TEST_PATH, state2);
    vi.advanceTimersByTime(100);

    const raw2 = localStorage.getItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`);
    expect(JSON.parse(raw2!).activeViewId).toBe('user:second');
  });

  it('(d) flush() writes immediately without waiting for delay', () => {
    const saver = createDebouncedSaver(100);
    const state = makeState({ activeViewId: 'user:flush-test' });

    saver.schedule(TEST_PATH, state);

    // Do NOT advance timers — flush should write immediately
    expect(localStorage.getItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`)).toBeNull();
    saver.flush();

    const raw = localStorage.getItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`);
    expect(raw).not.toBeNull();
    expect(JSON.parse(raw!).activeViewId).toBe('user:flush-test');
  });

  it('(d2) after flush, advancing timers does NOT write again', () => {
    const saver = createDebouncedSaver(100);
    const state = makeState({ activeViewId: 'user:after-flush' });

    saver.schedule(TEST_PATH, state);
    saver.flush();

    // Clear to detect any spurious second write
    localStorage.clear();
    vi.advanceTimersByTime(200);

    // No second write should have occurred
    expect(localStorage.getItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`)).toBeNull();
  });

  it('cancel() prevents the write from happening', () => {
    const saver = createDebouncedSaver(100);
    const state = makeState({ activeViewId: 'user:cancelled' });

    saver.schedule(TEST_PATH, state);
    saver.cancel();
    vi.advanceTimersByTime(200);

    expect(localStorage.getItem(`${STORAGE_KEY_PREFIX}${TEST_PATH}`)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Step-37: Camera state round-trip tests
// ---------------------------------------------------------------------------

describe('viewPersistence — camera state round-trip (step-37)', () => {
  beforeEach(() => localStorage.clear());

  it('save then load preserves viewportCameras position, target, up, and zoom exactly', () => {
    const cams: Record<string, CameraState> = {
      'design-main': { position: [3, 4, 5], target: [1, 2, 3], up: [0, 0, 1], zoom: 1.5 },
      'def-preview': { position: [10, 0, 0], target: [0, 0, 0], up: [0, 1, 0], zoom: 2.0 },
    };

    const state: PersistentViewState = {
      version: '2',
      activeViewId: 'auto:default',
      userViews: [],
      explicit: {},
      viewportCameras: cams,
      timestamp: '2026-04-23T00:00:00.000Z',
    };

    saveViewPersistence(TEST_PATH, state);
    const loaded = loadViewPersistence(TEST_PATH);

    expect(loaded).not.toBeNull();
    // Each camera field must deep-equal the original
    expect(loaded!.viewportCameras['design-main'].position).toEqual([3, 4, 5]);
    expect(loaded!.viewportCameras['design-main'].target).toEqual([1, 2, 3]);
    expect(loaded!.viewportCameras['design-main'].up).toEqual([0, 0, 1]);
    expect(loaded!.viewportCameras['design-main'].zoom).toBe(1.5);

    expect(loaded!.viewportCameras['def-preview'].position).toEqual([10, 0, 0]);
    expect(loaded!.viewportCameras['def-preview'].target).toEqual([0, 0, 0]);
    expect(loaded!.viewportCameras['def-preview'].up).toEqual([0, 1, 0]);
    expect(loaded!.viewportCameras['def-preview'].zoom).toBe(2.0);
  });

  it('multiple viewports are all preserved through JSON serialisation', () => {
    const cameras: Record<string, CameraState> = {};
    for (let i = 0; i < 4; i++) {
      cameras[`vp-${i}`] = { position: [i, 0, 0], target: [0, 0, 0], up: [0, 1, 0], zoom: i + 1 };
    }

    const state: PersistentViewState = {
      version: '2',
      activeViewId: 'auto:default',
      userViews: [],
      explicit: {},
      viewportCameras: cameras,
      timestamp: '2026-04-23T00:00:00.000Z',
    };

    saveViewPersistence(TEST_PATH, state);
    const loaded = loadViewPersistence(TEST_PATH);

    expect(loaded).not.toBeNull();
    for (let i = 0; i < 4; i++) {
      expect(loaded!.viewportCameras[`vp-${i}`].position[0]).toBe(i);
      expect(loaded!.viewportCameras[`vp-${i}`].zoom).toBe(i + 1);
    }
  });
});
