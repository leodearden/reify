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
});
