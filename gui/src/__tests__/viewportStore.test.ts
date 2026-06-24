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

  describe('setDefPath', () => {
    it('setDefPath on def-preview returns true and updates defPath', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setDefPath('def-preview', 'BoltFlange');
        expect(result).toBe(true);
        expect(store.state.viewports['def-preview'].defPath).toBe('BoltFlange');
        dispose();
      });
    });

    it('setDefPath with null clears defPath back to null', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        store.setDefPath('def-preview', 'BoltFlange');
        const result = store.setDefPath('def-preview', null);
        expect(result).toBe(true);
        expect(store.state.viewports['def-preview'].defPath).toBeNull();
        dispose();
      });
    });

    it('setDefPath on unknown id returns false and mutates nothing', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setDefPath('unknown', 'X');
        expect(result).toBe(false);
        expect(store.state.viewports['design-main'].defPath).toBeNull();
        expect(store.state.viewports['def-preview'].defPath).toBeNull();
        dispose();
      });
    });
  });

  describe('forceExpanded', () => {
    it('initial forceExpanded is false on both viewports', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        expect(store.state.viewports['design-main'].forceExpanded).toBe(false);
        expect(store.state.viewports['def-preview'].forceExpanded).toBe(false);
        dispose();
      });
    });

    it('setForceExpanded on def-preview returns true and toggles only that viewport flag', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setForceExpanded('def-preview', true);
        expect(result).toBe(true);
        expect(store.state.viewports['def-preview'].forceExpanded).toBe(true);
        expect(store.state.viewports['design-main'].forceExpanded).toBe(false);
        dispose();
      });
    });

    it('setForceExpanded on unknown id returns false and mutates nothing', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setForceExpanded('unknown', true);
        expect(result).toBe(false);
        expect(store.state.viewports['design-main'].forceExpanded).toBe(false);
        expect(store.state.viewports['def-preview'].forceExpanded).toBe(false);
        dispose();
      });
    });
  });

  describe('splitRatio', () => {
    it('(a) fresh store exposes state.splitRatio === 0.5', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        expect(store.state.splitRatio).toBe(0.5);
        dispose();
      });
    });

    it('(b) setSplitRatio(0.7) returns true and updates state.splitRatio to 0.7', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setSplitRatio(0.7);
        expect(result).toBe(true);
        expect(store.state.splitRatio).toBe(0.7);
        dispose();
      });
    });

    it('(c) setSplitRatio(-1) clamps to lower bound 0.1', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setSplitRatio(-1);
        expect(result).toBe(true);
        expect(store.state.splitRatio).toBe(0.1);
        dispose();
      });
    });

    it('(d) setSplitRatio(1.5) clamps to upper bound 0.9', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setSplitRatio(1.5);
        expect(result).toBe(true);
        expect(store.state.splitRatio).toBe(0.9);
        dispose();
      });
    });

    it('(e) setSplitRatio(0.5) is idempotent and returns true', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setSplitRatio(0.5);
        expect(result).toBe(true);
        expect(store.state.splitRatio).toBe(0.5);
        dispose();
      });
    });

    it('(f) two fresh stores do not share splitRatio state', () => {
      createRoot((dispose) => {
        const storeA = createViewportStore();
        storeA.setSplitRatio(0.3);
        const storeB = createViewportStore();
        // storeB should start at 0.5, unaffected by storeA's mutation
        expect(storeB.state.splitRatio).toBe(0.5);
        dispose();
      });
    });

    it('(g) setSplitRatio(NaN) returns false and does not corrupt state', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setSplitRatio(NaN);
        expect(result).toBe(false);
        // splitRatio must remain at the initial value — NaN must not be stored
        expect(store.state.splitRatio).toBe(0.5);
        dispose();
      });
    });

    it('(h) setSplitRatio(Infinity) returns false and does not corrupt state', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = store.setSplitRatio(Infinity);
        expect(result).toBe(false);
        expect(store.state.splitRatio).toBe(0.5);
        dispose();
      });
    });
  });

  describe('addPane — N-pane generalization', () => {
    it('(a) a fresh store has exactly two defaults: design-main and def-preview', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const ids = Object.keys(store.state.viewports);
        expect(ids).toHaveLength(2);
        expect(ids).toContain('design-main');
        expect(ids).toContain('def-preview');
        dispose();
      });
    });

    it('(b) addPane(1) returns "pane-1", inserts a pane viewport with correct shape, defaults survive', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const id = store.addPane(1);
        expect(id).toBe('pane-1');
        const ids = Object.keys(store.state.viewports);
        expect(ids).toContain('pane-1');
        expect(ids).toContain('design-main');
        expect(ids).toContain('def-preview');
        const pane = store.state.viewports['pane-1'];
        expect(pane).toBeDefined();
        expect(pane.type).toBe('pane');
        expect((pane as any).paneIndex).toBe(1);
        dispose();
      });
    });

    it('(c) addPane(2) after addPane(1) coexists — 4 entries total', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        store.addPane(1);
        const id2 = store.addPane(2);
        expect(id2).toBe('pane-2');
        expect(Object.keys(store.state.viewports)).toHaveLength(4);
        expect(store.state.viewports['pane-2']).toBeDefined();
        dispose();
      });
    });

    it('(d) addPane is idempotent — re-adding pane index 1 returns "pane-1" and does not grow the map', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        store.addPane(1);
        const countAfterFirst = Object.keys(store.state.viewports).length;
        const id = store.addPane(1);
        expect(id).toBe('pane-1');
        expect(Object.keys(store.state.viewports)).toHaveLength(countAfterFirst);
        dispose();
      });
    });

    it('(e) addPane(0) returns "design-main" and creates no new entry; design-main stays type "design"', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const id = store.addPane(0);
        expect(id).toBe('design-main');
        // No new entry — map stays at 2
        expect(Object.keys(store.state.viewports)).toHaveLength(2);
        // design-main retains its original type
        expect(store.state.viewports['design-main'].type).toBe('design');
        dispose();
      });
    });

    it('(f) new pane camera is defined and not aliased with design-main camera arrays', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        store.addPane(1);
        const pane = store.state.viewports['pane-1'];
        const main = store.state.viewports['design-main'];
        expect(pane.camera).toBeDefined();
        expect(pane.camera.position).not.toBe(main.camera.position);
        expect(pane.camera.target).not.toBe(main.camera.target);
        expect(pane.camera.up).not.toBe(main.camera.up);
        dispose();
      });
    });
  });

  describe('removePane', () => {
    it('(a) removePane(1) after addPane(1) returns true and removes pane-1; defaults survive', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        store.addPane(1);
        expect(store.state.viewports['pane-1']).toBeDefined();
        const result = (store as any).removePane(1);
        expect(result).toBe(true);
        expect(store.state.viewports['pane-1']).toBeUndefined();
        // Defaults must survive
        expect(store.state.viewports['design-main']).toBeDefined();
        expect(store.state.viewports['def-preview']).toBeDefined();
        dispose();
      });
    });

    it('(b) removePane(1) on absent pane returns false, no mutation', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const countBefore = Object.keys(store.state.viewports).length;
        const result = (store as any).removePane(1);
        expect(result).toBe(false);
        expect(Object.keys(store.state.viewports)).toHaveLength(countBefore);
        dispose();
      });
    });

    it('(c) removePane(0) returns false and design-main is untouched (pane-0 alias protected)', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        const result = (store as any).removePane(0);
        expect(result).toBe(false);
        expect(store.state.viewports['design-main']).toBeDefined();
        dispose();
      });
    });

    it('(d) removePane never deletes defaults — calling on design-main index returns false', () => {
      createRoot((dispose) => {
        const store = createViewportStore();
        // Negative index also protected
        const r1 = (store as any).removePane(-1);
        expect(r1).toBe(false);
        expect(store.state.viewports['design-main']).toBeDefined();
        expect(store.state.viewports['def-preview']).toBeDefined();
        dispose();
      });
    });
  });

  describe('store isolation', () => {
    it('mutation of store A does not leak to store B', () => {
      createRoot((dispose) => {
        const storeA = createViewportStore();
        // Mutate storeA via all public mutation APIs
        storeA.setActiveViewport('def-preview');
        storeA.assignView('design-main', 'auto:default');
        storeA.updateCamera('design-main', { position: [7, 8, 9], target: [1, 2, 3], up: [0, 0, 1], zoom: 4 });

        // Create a fresh storeB — must be completely pristine
        const storeB = createViewportStore();
        expect(storeB.state.viewports['design-main'].active).toBe(true);
        expect(storeB.state.viewports['def-preview'].active).toBe(false);
        expect(storeB.state.viewports['design-main'].viewId).toBeNull();
        expect(storeB.state.viewports['design-main'].camera).toEqual({
          position: [5, 5, 5],
          target: [0, 0, 0],
          up: [0, 0, 1],
          zoom: 1,
        });
        dispose();
      });
    });

    it('camera tuples are not aliased between two fresh stores', () => {
      createRoot((dispose) => {
        const storeA = createViewportStore();
        const storeB = createViewportStore();

        // Different store instances must not share array references
        expect(storeA.state.viewports['design-main'].camera.position).not.toBe(
          storeB.state.viewports['design-main'].camera.position,
        );
        expect(storeA.state.viewports['design-main'].camera.target).not.toBe(
          storeB.state.viewports['design-main'].camera.target,
        );
        expect(storeA.state.viewports['design-main'].camera.up).not.toBe(
          storeB.state.viewports['design-main'].camera.up,
        );

        // Within the same store, no aliasing between the two seeded viewports
        expect(storeA.state.viewports['design-main'].camera.position).not.toBe(
          storeA.state.viewports['def-preview'].camera.position,
        );
        expect(storeA.state.viewports['design-main'].camera.target).not.toBe(
          storeA.state.viewports['def-preview'].camera.target,
        );
        expect(storeA.state.viewports['design-main'].camera.up).not.toBe(
          storeA.state.viewports['def-preview'].camera.up,
        );
        dispose();
      });
    });

    it('camera tuples are not aliased with a subsequently-created store even after mutation', () => {
      createRoot((dispose) => {
        const storeA = createViewportStore();
        // Mutate storeA's camera
        storeA.updateCamera('design-main', { position: [7, 8, 9], target: [1, 2, 3], up: [0, 0, 1], zoom: 4 });

        // Create storeB after mutation — must still have the pristine default camera
        const storeB = createViewportStore();
        expect(storeB.state.viewports['design-main'].camera).toEqual({
          position: [5, 5, 5],
          target: [0, 0, 0],
          up: [0, 0, 1],
          zoom: 1,
        });
        dispose();
      });
    });
  });
});
