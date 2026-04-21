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
});
