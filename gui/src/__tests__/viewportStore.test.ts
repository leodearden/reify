import { describe, it, expect } from 'vitest';
import { createRoot } from 'solid-js';
import { createViewportStore } from '../stores';

describe('viewportStore', () => {
  describe('initial state', () => {
    it('has exactly two viewport entries: design-main and def-preview', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const ids = Object.keys(store.state.viewports);
        expect(ids).toHaveLength(2);
        expect(ids).toContain('design-main');
        expect(ids).toContain('def-preview');
        dispose();
      });
    });

    it('design-main has correct initial shape', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const vp = store.state.viewports['design-main'];
        expect(vp.id).toBe('design-main');
        expect(vp.type).toBe('design');
        expect(vp.viewId).toBeNull();
        expect(vp.defPath).toBeNull();
        expect(vp.active).toBe(true);
        // camera should be present (default camera state)
        expect(vp.camera).toBeDefined();
        expect(Array.isArray(vp.camera.position)).toBe(true);
        expect(vp.camera.position).toHaveLength(3);
        expect(Array.isArray(vp.camera.target)).toBe(true);
        expect(Array.isArray(vp.camera.up)).toBe(true);
        expect(typeof vp.camera.zoom).toBe('number');
        dispose();
      });
    });

    it('def-preview has correct initial shape', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const vp = store.state.viewports['def-preview'];
        expect(vp.id).toBe('def-preview');
        expect(vp.type).toBe('def-preview');
        expect(vp.viewId).toBeNull();
        expect(vp.defPath).toBeNull();
        expect(vp.active).toBe(false);
        expect(vp.camera).toBeDefined();
        dispose();
      });
    });

    it('getViewport("design-main") returns the design-main entry', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const vp = store.getViewport('design-main');
        expect(vp).toBeDefined();
        expect(vp!.id).toBe('design-main');
        expect(vp!.type).toBe('design');
        expect(vp!.active).toBe(true);
        dispose();
      });
    });

    it('getViewport("unknown") returns undefined', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        expect(store.getViewport('unknown')).toBeUndefined();
        dispose();
      });
    });
  });

  describe('setActiveViewport', () => {
    it('flips design-main to inactive and def-preview to active', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setActiveViewport('def-preview');
        expect(result).toBe(true);
        expect(store.state.viewports['design-main'].active).toBe(false);
        expect(store.state.viewports['def-preview'].active).toBe(true);
        dispose();
      });
    });

    it('restores initial arrangement when switching back to design-main', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        store.setActiveViewport('def-preview');
        const result = store.setActiveViewport('design-main');
        expect(result).toBe(true);
        expect(store.state.viewports['design-main'].active).toBe(true);
        expect(store.state.viewports['def-preview'].active).toBe(false);
        dispose();
      });
    });

    it('returns false and mutates nothing when id is unknown', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setActiveViewport('unknown-id');
        expect(result).toBe(false);
        // Active flags should be unchanged
        expect(store.state.viewports['design-main'].active).toBe(true);
        expect(store.state.viewports['def-preview'].active).toBe(false);
        dispose();
      });
    });
  });

  describe('assignView', () => {
    it('assignView sets viewId and returns true', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.assignView('design-main', 'auto:default');
        expect(result).toBe(true);
        expect(store.state.viewports['design-main'].viewId).toBe('auto:default');
        dispose();
      });
    });

    it('assignView with null clears the assignment', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        store.assignView('design-main', 'auto:default');
        store.assignView('design-main', null);
        expect(store.state.viewports['design-main'].viewId).toBeNull();
        dispose();
      });
    });

    it('assignView on unknown id returns false and does not mutate state', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.assignView('unknown-id', 'auto:default');
        expect(result).toBe(false);
        // Other viewports should be untouched
        expect(store.state.viewports['design-main'].viewId).toBeNull();
        expect(store.state.viewports['def-preview'].viewId).toBeNull();
        dispose();
      });
    });

    it('assignView does not touch defPath', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        store.assignView('design-main', 'auto:default');
        // defPath should remain null — it is not modified by assignView
        expect(store.state.viewports['design-main'].defPath).toBeNull();
        dispose();
      });
    });
  });

  describe('updateCamera', () => {
    it('persists the camera state for a viewport', () => {
      createRoot((dispose) => {
        const newCamera = { position: [1, 2, 3] as [number, number, number], target: [0, 0, 0] as [number, number, number], up: [0, 1, 0] as [number, number, number], zoom: 2 };
        const store = createViewportStore();
        const result = store.updateCamera('design-main', newCamera);
        expect(result).toBe(true);
        expect(store.getViewport('design-main')!.camera).toEqual(newCamera);
        dispose();
      });
    });

    it('subsequent updateCamera on the same viewport overwrites the previous state', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        store.updateCamera('design-main', { position: [1, 2, 3], target: [0, 0, 0], up: [0, 1, 0], zoom: 2 });
        const second = { position: [4, 5, 6] as [number, number, number], target: [1, 1, 1] as [number, number, number], up: [0, 0, 1] as [number, number, number], zoom: 3 };
        store.updateCamera('design-main', second);
        expect(store.getViewport('design-main')!.camera).toEqual(second);
        dispose();
      });
    });

    it('updating one viewport camera does not affect the other', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const previewCamera = { ...store.getViewport('def-preview')!.camera };
        store.updateCamera('design-main', { position: [9, 9, 9], target: [1, 1, 1], up: [0, 0, 1], zoom: 5 });
        // def-preview camera should be unchanged
        expect(store.getViewport('def-preview')!.camera).toEqual(previewCamera);
        dispose();
      });
    });

    it('updateCamera on unknown id returns false and does not mutate', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const before = { ...store.getViewport('design-main')!.camera };
        const result = store.updateCamera('unknown-id', { position: [1, 2, 3], target: [0, 0, 0], up: [0, 1, 0], zoom: 99 });
        expect(result).toBe(false);
        expect(store.getViewport('design-main')!.camera).toEqual(before);
        dispose();
      });
    });
  });
});
