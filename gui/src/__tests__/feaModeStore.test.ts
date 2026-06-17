import { describe, it, expect } from 'vitest';
import { createRoot } from 'solid-js';
import { createFeaModeStore } from '../stores';

/**
 * Run `fn` inside a SolidJS root and dispose immediately after.
 * Removes repetitive createRoot boilerplate from each `it` block.
 */
function withRoot<T>(fn: () => T): T {
  let result!: T;
  createRoot((dispose) => {
    result = fn();
    dispose();
  });
  return result;
}

describe('feaModeStore', () => {
  describe('initial state', () => {
    it('defaults: enabled=false, channel=vonMises, palette=viridis, range={auto,0,1}, autoEnabledOnce=false', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        expect(store.state.enabled).toBe(false);
        expect(store.state.channel).toBe('vonMises');
        expect(store.state.palette).toBe('viridis');
        expect(store.state.range).toEqual({ mode: 'auto', min: 0, max: 1 });
        expect(store.state.autoEnabledOnce).toBe(false);
      });
    });
  });

  describe('tryAutoEnable', () => {
    it('first call flips state.enabled true and returns true', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        const result = store.tryAutoEnable();
        expect(result).toBe(true);
        expect(store.state.enabled).toBe(true);
      });
    });

    it('first call sets autoEnabledOnce to true', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.tryAutoEnable();
        expect(store.state.autoEnabledOnce).toBe(true);
      });
    });

    it('first call with channel arg updates state.channel', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.tryAutoEnable('displacement_magnitude');
        expect(store.state.channel).toBe('displacement_magnitude');
      });
    });

    it('first call without channel arg leaves state.channel unchanged', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.tryAutoEnable();
        expect(store.state.channel).toBe('vonMises');
      });
    });

    it('second call after setEnabled(false) does NOT re-enable (one-shot sticky)', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.tryAutoEnable();           // first call – fires
        store.setEnabled(false);          // user disables
        const result = store.tryAutoEnable(); // second call – should be no-op
        expect(result).toBe(false);
        expect(store.state.enabled).toBe(false);
      });
    });

    it('returns false on second call even without user disabling', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.tryAutoEnable();
        const result = store.tryAutoEnable();
        expect(result).toBe(false);
      });
    });

    it('user can still change channel after auto-enable', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.tryAutoEnable(); // fires with no channel arg (channel stays 'vonMises')
        store.setChannel('displacement_magnitude');
        expect(store.state.channel).toBe('displacement_magnitude');
      });
    });
  });

  describe('lockCurrent', () => {
    it('sets range to {mode:"locked", min, max, source:"current"} by default', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        const result = store.lockCurrent(5, 20);
        expect(result).toBe(true);
        expect(store.state.range).toEqual({ mode: 'locked', min: 5, max: 20, source: 'current' });
      });
    });

    it('uses explicit source string when provided', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.lockCurrent(7, 42, 'mesh:bracket');
        expect(store.state.range).toEqual({ mode: 'locked', min: 7, max: 42, source: 'mesh:bracket' });
      });
    });

    it('returns false and does not mutate when min is non-finite', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        const before = { ...store.state.range };
        const result = store.lockCurrent(NaN, 20);
        expect(result).toBe(false);
        expect(store.state.range).toEqual(before);
      });
    });

    it('returns false and does not mutate when max is non-finite', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        const before = { ...store.state.range };
        const result = store.lockCurrent(5, Infinity);
        expect(result).toBe(false);
        expect(store.state.range).toEqual(before);
      });
    });
  });

  describe('deformation state', () => {
    it('(a) state.showDeformed defaults to false after createFeaModeStore()', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        expect(store.state.showDeformed).toBe(false);
      });
    });

    it('(a) state.warpFactor defaults to 1.0 after createFeaModeStore()', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        expect(store.state.warpFactor).toBe(1.0);
      });
    });

    it('(b) setShowDeformed(true) updates state.showDeformed to true', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setShowDeformed(true);
        expect(store.state.showDeformed).toBe(true);
      });
    });

    it('(b) setShowDeformed(false) updates state.showDeformed back to false', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setShowDeformed(true);
        store.setShowDeformed(false);
        expect(store.state.showDeformed).toBe(false);
      });
    });

    it('(c) setWarpFactor(10) updates state.warpFactor to 10 and returns true', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        const result = store.setWarpFactor(10);
        expect(result).toBe(true);
        expect(store.state.warpFactor).toBe(10);
      });
    });

    it('(d) setWarpFactor(NaN) returns false and leaves state.warpFactor unchanged', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setWarpFactor(5);
        const result = store.setWarpFactor(NaN);
        expect(result).toBe(false);
        expect(store.state.warpFactor).toBe(5);
      });
    });

    it('(e) setWarpFactor(Infinity) returns false and leaves state.warpFactor unchanged', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setWarpFactor(5);
        const result = store.setWarpFactor(Infinity);
        expect(result).toBe(false);
        expect(store.state.warpFactor).toBe(5);
      });
    });

    it('(e) setWarpFactor(-Infinity) returns false and leaves state.warpFactor unchanged', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setWarpFactor(5);
        const result = store.setWarpFactor(-Infinity);
        expect(result).toBe(false);
        expect(store.state.warpFactor).toBe(5);
      });
    });

    it('(f) setWarpFactor(-1) returns false and leaves state.warpFactor unchanged (negative values rejected)', () => {
      // The warp slider is bounded to [0, 100]. Accepting negative values would create
      // a UI/store split: the slider clamps to 0 visually while the label shows a
      // negative. Rejecting negatives keeps the slider and store in sync.
      withRoot(() => {
        const store = createFeaModeStore();
        store.setWarpFactor(5);
        const result = store.setWarpFactor(-1);
        expect(result).toBe(false);
        expect(store.state.warpFactor).toBe(5);
      });
    });

    it('(f) setWarpFactor(0) returns true (zero is valid — shows undeformed shape)', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        const result = store.setWarpFactor(0);
        expect(result).toBe(true);
        expect(store.state.warpFactor).toBe(0);
      });
    });
  });

  describe('activeCaseId', () => {
    it('(a) activeCaseId defaults to null', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        expect(store.state.activeCaseId).toBeNull();
      });
    });

    it("(b) setActiveCaseId('operating') sets store.state.activeCaseId to 'operating'", () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setActiveCaseId('operating');
        expect(store.state.activeCaseId).toBe('operating');
      });
    });

    it('(c) setActiveCaseId(null) clears activeCaseId back to null', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setActiveCaseId('operating');
        store.setActiveCaseId(null);
        expect(store.state.activeCaseId).toBeNull();
      });
    });

    it('(d) setActiveCaseId does not mutate other fields', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        const enabledBefore = store.state.enabled;
        store.setActiveCaseId('overload');
        expect(store.state.enabled).toBe(enabledBefore);
      });
    });
  });

  describe('simple setters', () => {
    it('setEnabled toggles state.enabled', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setEnabled(true);
        expect(store.state.enabled).toBe(true);
        store.setEnabled(false);
        expect(store.state.enabled).toBe(false);
      });
    });

    it('setChannel updates state.channel', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setChannel('displacement_magnitude');
        expect(store.state.channel).toBe('displacement_magnitude');
      });
    });

    it('setPalette updates state.palette', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setPalette('magma');
        expect(store.state.palette).toBe('magma');
      });
    });

    it('setRange with valid fixed range updates state.range deeply', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        const result = store.setRange({ mode: 'fixed', min: 10, max: 50 });
        expect(result).toBe(true);
        expect(store.state.range).toEqual({ mode: 'fixed', min: 10, max: 50 });
      });
    });

    it('setRange returns false and does not mutate on non-finite bounds', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        const before = { ...store.state.range };

        expect(store.setRange({ mode: 'fixed', min: NaN, max: 50 })).toBe(false);
        expect(store.state.range).toEqual(before);

        expect(store.setRange({ mode: 'fixed', min: 0, max: NaN })).toBe(false);
        expect(store.state.range).toEqual(before);

        expect(store.setRange({ mode: 'fixed', min: 0, max: Infinity })).toBe(false);
        expect(store.state.range).toEqual(before);
      });
    });
  });

  // ── Task 3026 step-7: RED — availableCases + applyFeaCaseChanged ──
  describe('availableCases', () => {
    it('(a) state.availableCases defaults to []', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        expect(store.state.availableCases).toEqual([]); // FAILS until step-8 adds the field
      });
    });

    it('(b) applyFeaCaseChanged sets availableCases and activeCaseId', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.applyFeaCaseChanged({ // FAILS until step-8 adds applyFeaCaseChanged
          active_case_id: 'operating',
          available_cases: ['operating', 'overload', 'transport'],
        });
        expect(store.state.availableCases).toEqual(['operating', 'overload', 'transport']);
        expect(store.state.activeCaseId).toBe('operating');
      });
    });

    it('(c) applyFeaCaseChanged with empty available_cases resets availableCases to [] (single-case → dropdown hidden)', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        // First apply a multi-case payload
        store.applyFeaCaseChanged({ // FAILS until step-8 adds applyFeaCaseChanged
          active_case_id: 'operating',
          available_cases: ['operating', 'overload'],
        });
        expect(store.state.availableCases).toEqual(['operating', 'overload']);
        // Now apply an empty payload (single-case scene)
        store.applyFeaCaseChanged({
          active_case_id: 'default',
          available_cases: [],
        });
        expect(store.state.availableCases).toEqual([]);
        expect(store.state.activeCaseId).toBe('default');
      });
    });

    it('(d) applyFeaCaseChanged does not mutate other fields (enabled, channel, etc.)', () => {
      withRoot(() => {
        const store = createFeaModeStore();
        store.setEnabled(true);
        store.setChannel('displacement_magnitude');
        store.applyFeaCaseChanged({ // FAILS until step-8 adds applyFeaCaseChanged
          active_case_id: 'transport',
          available_cases: ['operating', 'overload', 'transport'],
        });
        expect(store.state.enabled).toBe(true);
        expect(store.state.channel).toBe('displacement_magnitude');
      });
    });
  });
});
