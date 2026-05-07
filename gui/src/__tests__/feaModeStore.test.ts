import { describe, it, expect } from 'vitest';
import { createRoot } from 'solid-js';
import { createFeaModeStore } from '../stores';

describe('feaModeStore', () => {
  describe('initial state', () => {
    it('state.enabled is false', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        expect(store.state.enabled).toBe(false);
        dispose();
      });
    });

    it('state.channel is "vonMises"', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        expect(store.state.channel).toBe('vonMises');
        dispose();
      });
    });

    it('state.palette is "viridis"', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        expect(store.state.palette).toBe('viridis');
        dispose();
      });
    });

    it('state.range is {mode:"auto", min:0, max:1}', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        expect(store.state.range).toEqual({ mode: 'auto', min: 0, max: 1 });
        dispose();
      });
    });

    it('state.autoEnabledOnce is false', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        expect(store.state.autoEnabledOnce).toBe(false);
        dispose();
      });
    });
  });

  describe('simple setters', () => {
    it('setEnabled(true) flips state.enabled to true', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        store.setEnabled(true);
        expect(store.state.enabled).toBe(true);
        dispose();
      });
    });

    it('setEnabled(false) flips state.enabled back to false', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        store.setEnabled(true);
        store.setEnabled(false);
        expect(store.state.enabled).toBe(false);
        dispose();
      });
    });

    it('setChannel updates state.channel', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        store.setChannel('displacement_magnitude');
        expect(store.state.channel).toBe('displacement_magnitude');
        dispose();
      });
    });

    it('setPalette updates state.palette', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        store.setPalette('magma');
        expect(store.state.palette).toBe('magma');
        dispose();
      });
    });

    it('setRange with valid fixed range updates state.range deeply', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        const result = store.setRange({ mode: 'fixed', min: 10, max: 50 });
        expect(result).toBe(true);
        expect(store.state.range).toEqual({ mode: 'fixed', min: 10, max: 50 });
        dispose();
      });
    });

    it('setRange with NaN min returns false and does not mutate', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        const before = { ...store.state.range };
        const result = store.setRange({ mode: 'fixed', min: NaN, max: 50 });
        expect(result).toBe(false);
        expect(store.state.range).toEqual(before);
        dispose();
      });
    });

    it('setRange with NaN max returns false and does not mutate', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        const before = { ...store.state.range };
        const result = store.setRange({ mode: 'fixed', min: 0, max: NaN });
        expect(result).toBe(false);
        expect(store.state.range).toEqual(before);
        dispose();
      });
    });

    it('setRange with Infinity max returns false and does not mutate', () => {
      createRoot((dispose) => {
        const store = createFeaModeStore();
        const before = { ...store.state.range };
        const result = store.setRange({ mode: 'fixed', min: 0, max: Infinity });
        expect(result).toBe(false);
        expect(store.state.range).toEqual(before);
        dispose();
      });
    });
  });
});
