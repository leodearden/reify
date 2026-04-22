// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import type { PersistentViewState } from '../types';
import type { ViewDefinition } from '../stores/autoViewGenerator';
import type { CameraState } from '../stores/viewportStore';
import type { VisibilityState } from '../types';

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
    version: '1',
    activeViewId: 'user:view-1',
    userViews: [view1],
    explicit: { 'Assembly.flange': 'show' as VisibilityState },
    viewportCameras: { 'viewport-main': cam },
    timestamp: '2026-04-22T00:00:00.000Z',
  };

  // Verify field types by assignment
  const _version: '1' = state.version;
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
      version: '1',
      activeViewId: 'user:v1',
      userViews: [view],
      explicit: { 'Root.geometry': 'hidden' as VisibilityState },
      viewportCameras: { main: cam },
      timestamp: new Date().toISOString(),
    };

    expect(state.version).toBe('1');
    expect(state.activeViewId).toBe('user:v1');
    expect(state.userViews).toHaveLength(1);
    expect(state.userViews[0].id).toBe('user:v1');
    expect(state.explicit).toEqual({ 'Root.geometry': 'hidden' });
    expect(state.viewportCameras['main']).toEqual(cam);
    expect(typeof state.timestamp).toBe('string');
  });

  it('version field must be the literal string "1"', () => {
    // Runtime check that version is stored and retrieved as "1"
    const state: PersistentViewState = {
      version: '1',
      activeViewId: 'auto:default',
      userViews: [],
      explicit: {},
      viewportCameras: {},
      timestamp: '2026-04-22T00:00:00.000Z',
    };
    expect(state.version).toBe('1');
    expect(state.version).not.toBe(1); // must be string, not number
  });

  it('userViews array holds ViewDefinition objects with all required fields', () => {
    const views: ViewDefinition[] = [
      { id: 'user:a', name: 'Alpha', auto: false, visibility: {} },
      { id: 'user:b', name: 'Beta', auto: false, visibility: { 'X.y': 'ghost' as VisibilityState } },
    ];

    const state: PersistentViewState = {
      version: '1',
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
      version: '1',
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
