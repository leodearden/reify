/**
 * Tests for gui/src/stores/probeStore.ts
 *
 * Follows the feaModeStore.test.ts pattern: createRoot + withRoot helper,
 * testing factory shape then mutation behaviour in TDD order.
 */
import { describe, it, expect } from 'vitest';
import { createRoot } from 'solid-js';
import type { BarycentricUV, ProbeSample, PinnedProbe } from '../stores/probeStore';
import { createProbeStore } from '../stores/probeStore';

/** Run fn inside a SolidJS root and dispose immediately. */
function withRoot<T>(fn: () => T): T {
  let result!: T;
  createRoot((dispose) => {
    result = fn();
    dispose();
  });
  return result;
}

// ---------------------------------------------------------------------------
// Step 1: Factory shape
// ---------------------------------------------------------------------------

describe('createProbeStore — factory shape', () => {
  it('returns a store object with state and mutation methods', () => {
    withRoot(() => {
      const store = createProbeStore();
      expect(store).toHaveProperty('state');
      expect(typeof store.addProbe).toBe('function');
      expect(typeof store.removeProbe).toBe('function');
      expect(typeof store.clear).toBe('function');
      expect(typeof store.updateSample).toBe('function');
      expect(typeof store.markStale).toBe('function');
    });
  });

  it('state.probes is an empty array on creation', () => {
    withRoot(() => {
      const store = createProbeStore();
      expect(Array.isArray(store.state.probes)).toBe(true);
      expect(store.state.probes).toHaveLength(0);
    });
  });

  it('type-compiles: BarycentricUV is a 3-tuple of numbers', () => {
    // Compile-time assertion only — if the import resolves, the shape is correct.
    const _bary: BarycentricUV = [0.2, 0.3, 0.5];
    void _bary;
  });

  it('type-compiles: ProbeSample has displacement, vonMises, scalars, vectors', () => {
    const _sample: ProbeSample = {
      displacement: [0.1, 0.2, 0.3],
      vonMises: 42.0,
      scalars: { vonMises: 42.0 },
      vectors: { flux: [1, 2, 3] },
    };
    void _sample;
    const _nullSample: ProbeSample = {
      displacement: null,
      vonMises: null,
      scalars: {},
      vectors: {},
    };
    void _nullSample;
  });

  it('type-compiles: PinnedProbe has id, entity_path, face_id, barycentric_uv, sample, stale', () => {
    const _probe: PinnedProbe = {
      id: 'probe-0',
      entity_path: 'Body.face',
      face_id: 3,
      barycentric_uv: [0.2, 0.3, 0.5],
      sample: null,
      stale: false,
    };
    void _probe;
  });
});

// ---------------------------------------------------------------------------
// Step 3: addProbe
// ---------------------------------------------------------------------------

describe('createProbeStore — addProbe', () => {
  it('addProbe appends a PinnedProbe with stale=false and returns its id', () => {
    withRoot(() => {
      const store = createProbeStore();
      const id = store.addProbe({
        entity_path: 'Body.face',
        face_id: 0,
        barycentric_uv: [0.2, 0.3, 0.5],
        sample: null,
      });
      expect(typeof id).toBe('string');
      expect(store.state.probes).toHaveLength(1);
      const probe = store.state.probes[0];
      expect(probe.id).toBe(id);
      expect(probe.entity_path).toBe('Body.face');
      expect(probe.face_id).toBe(0);
      expect(probe.barycentric_uv).toEqual([0.2, 0.3, 0.5]);
      expect(probe.sample).toBeNull();
      expect(probe.stale).toBe(false);
    });
  });

  it('two addProbe calls yield two coexisting probes with distinct ids', () => {
    withRoot(() => {
      const store = createProbeStore();
      const id1 = store.addProbe({
        entity_path: 'A',
        face_id: 0,
        barycentric_uv: [1, 0, 0],
        sample: null,
      });
      const id2 = store.addProbe({
        entity_path: 'B',
        face_id: 1,
        barycentric_uv: [0, 1, 0],
        sample: null,
      });
      expect(store.state.probes).toHaveLength(2);
      expect(id1).not.toBe(id2);
      expect(store.state.probes[0].entity_path).toBe('A');
      expect(store.state.probes[1].entity_path).toBe('B');
    });
  });

  it('addProbe with a sample preserves the sample', () => {
    withRoot(() => {
      const store = createProbeStore();
      const sample: ProbeSample = {
        displacement: [0.1, 0.2, 0.3],
        vonMises: 10.5,
        scalars: { vonMises: 10.5 },
        vectors: {},
      };
      store.addProbe({
        entity_path: 'C',
        face_id: 2,
        barycentric_uv: [0.25, 0.25, 0.5],
        sample,
      });
      expect(store.state.probes[0].sample).toEqual(sample);
    });
  });
});

// ---------------------------------------------------------------------------
// Step 5: removeProbe and clear
// ---------------------------------------------------------------------------

describe('createProbeStore — removeProbe / clear', () => {
  it('removeProbe(id) removes exactly that probe, leaves others intact', () => {
    withRoot(() => {
      const store = createProbeStore();
      const id1 = store.addProbe({ entity_path: 'A', face_id: 0, barycentric_uv: [1, 0, 0], sample: null });
      const id2 = store.addProbe({ entity_path: 'B', face_id: 1, barycentric_uv: [0, 1, 0], sample: null });
      store.removeProbe(id1);
      expect(store.state.probes).toHaveLength(1);
      expect(store.state.probes[0].id).toBe(id2);
    });
  });

  it('removeProbe with unknown id is a no-op', () => {
    withRoot(() => {
      const store = createProbeStore();
      store.addProbe({ entity_path: 'A', face_id: 0, barycentric_uv: [1, 0, 0], sample: null });
      store.removeProbe('does-not-exist');
      expect(store.state.probes).toHaveLength(1);
    });
  });

  it('clear() empties state.probes', () => {
    withRoot(() => {
      const store = createProbeStore();
      store.addProbe({ entity_path: 'A', face_id: 0, barycentric_uv: [1, 0, 0], sample: null });
      store.addProbe({ entity_path: 'B', face_id: 1, barycentric_uv: [0, 1, 0], sample: null });
      store.clear();
      expect(store.state.probes).toHaveLength(0);
    });
  });
});

// ---------------------------------------------------------------------------
// Step 7: updateSample and markStale
// ---------------------------------------------------------------------------

describe('createProbeStore — updateSample / markStale', () => {
  it('updateSample(id, sample) replaces sample and sets stale=false', () => {
    withRoot(() => {
      const store = createProbeStore();
      const id = store.addProbe({ entity_path: 'A', face_id: 0, barycentric_uv: [1, 0, 0], sample: null });
      // First mark stale so we can verify updateSample clears it
      store.markStale(id);
      expect(store.state.probes[0].stale).toBe(true);

      const newSample: ProbeSample = { displacement: [1, 2, 3], vonMises: 5.0, scalars: { vonMises: 5.0 }, vectors: {} };
      store.updateSample(id, newSample);
      expect(store.state.probes[0].sample).toEqual(newSample);
      expect(store.state.probes[0].stale).toBe(false);
    });
  });

  it('markStale(id) sets stale=true while preserving the existing sample', () => {
    withRoot(() => {
      const store = createProbeStore();
      const sample: ProbeSample = { displacement: [0.1, 0.2, 0.3], vonMises: 7.0, scalars: { vonMises: 7.0 }, vectors: {} };
      const id = store.addProbe({ entity_path: 'A', face_id: 0, barycentric_uv: [1, 0, 0], sample });
      store.markStale(id);
      expect(store.state.probes[0].stale).toBe(true);
      // Last-known sample must survive
      expect(store.state.probes[0].sample).toEqual(sample);
    });
  });

  it('updateSample with unknown id is a no-op', () => {
    withRoot(() => {
      const store = createProbeStore();
      const id = store.addProbe({ entity_path: 'A', face_id: 0, barycentric_uv: [1, 0, 0], sample: null });
      store.updateSample('no-such-id', { displacement: null, vonMises: null, scalars: {}, vectors: {} });
      expect(store.state.probes[0].sample).toBeNull();
      void id;
    });
  });

  it('markStale with unknown id is a no-op', () => {
    withRoot(() => {
      const store = createProbeStore();
      store.addProbe({ entity_path: 'A', face_id: 0, barycentric_uv: [1, 0, 0], sample: null });
      store.markStale('no-such-id');
      expect(store.state.probes[0].stale).toBe(false);
    });
  });
});
