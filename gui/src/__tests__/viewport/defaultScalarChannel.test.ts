/**
 * Unit suite for pickDefaultScalarChannel — step-1 RED.
 *
 * This file imports from ../../viewport/defaultScalarChannel, which does not
 * yet exist. The suite MUST fail (module absent → import error) in step-1 —
 * that is the RED state.
 */
import { describe, it, expect } from 'vitest';
import {
  pickDefaultScalarChannel,
  PREFERRED_FEA_CHANNELS,
} from '../../viewport/defaultScalarChannel';
import type { MeshData } from '../../types';

// ─── helpers ─────────────────────────────────────────────────────────────────

/** Build a MeshData with the given scalar_channels (name → values). */
function makeMesh(channels: Record<string, number[]>): MeshData {
  const scalar_channels: Record<string, Float32Array> = {};
  for (const [name, values] of Object.entries(channels)) {
    scalar_channels[name] = new Float32Array(values);
  }
  return {
    entity_path: 'test',
    vertices: new Float32Array(0),
    indices: new Uint32Array(0),
    normals: null,
    scalar_channels,
  } as unknown as MeshData;
}

function makeMeshNoChannels(): MeshData {
  return {
    entity_path: 'test',
    vertices: new Float32Array(0),
    indices: new Uint32Array(0),
    normals: null,
  } as unknown as MeshData;
}

// ─── tests ────────────────────────────────────────────────────────────────────

describe('pickDefaultScalarChannel', () => {
  it('(a) empty meshes record → undefined', () => {
    expect(pickDefaultScalarChannel({})).toBeUndefined();
  });

  it('(b) mesh with no scalar_channels → undefined', () => {
    const meshes = { m: makeMeshNoChannels() };
    expect(pickDefaultScalarChannel(meshes)).toBeUndefined();
  });

  it('(c) mesh with only an empty channel array → undefined', () => {
    const meshes = { m: makeMesh({ vonMises: [] }) };
    expect(pickDefaultScalarChannel(meshes)).toBeUndefined();
  });

  it('(d) single non-empty vonMises → "vonMises"', () => {
    const meshes = { m: makeMesh({ vonMises: [1.0, 2.0] }) };
    expect(pickDefaultScalarChannel(meshes)).toBe('vonMises');
  });

  it('(e) shell channels inserted as {vonMises_bottom, vonMises_mid, vonMises_top} → "vonMises_top"', () => {
    const meshes = {
      m: makeMesh({
        vonMises_bottom: [1],
        vonMises_mid: [2],
        vonMises_top: [3],
      }),
    };
    expect(pickDefaultScalarChannel(meshes)).toBe('vonMises_top');
  });

  it('(f) same shell channels in reversed insertion order → still "vonMises_top" (insertion-order independent)', () => {
    const meshes = {
      m: makeMesh({
        vonMises_top: [3],
        vonMises_mid: [2],
        vonMises_bottom: [1],
      }),
    };
    expect(pickDefaultScalarChannel(meshes)).toBe('vonMises_top');
  });

  it('(g) both vonMises and vonMises_top present → "vonMises" (highest preference wins)', () => {
    const meshes = { m: makeMesh({ vonMises: [5], vonMises_top: [3] }) };
    expect(pickDefaultScalarChannel(meshes)).toBe('vonMises');
  });

  it('(h) preferred name present but EMPTY is skipped — {vonMises: [], vonMises_top: [..]} → "vonMises_top"', () => {
    const meshes = { m: makeMesh({ vonMises: [], vonMises_top: [1, 2] }) };
    expect(pickDefaultScalarChannel(meshes)).toBe('vonMises_top');
  });

  it('(i) no preferred names — arbitrary {zeta, alpha, mu} → lexicographically smallest "alpha"', () => {
    const meshes = { m: makeMesh({ zeta: [1], alpha: [2], mu: [3] }) };
    expect(pickDefaultScalarChannel(meshes)).toBe('alpha');
  });

  it('(i2) no preferred names — order-independent (reversed insertion)', () => {
    const meshes = { m: makeMesh({ mu: [3], alpha: [2], zeta: [1] }) };
    expect(pickDefaultScalarChannel(meshes)).toBe('alpha');
  });

  it('(j) non-empty channel on a second mesh is found (pools across Object.values(meshes))', () => {
    const meshes = {
      a: makeMeshNoChannels(),
      b: makeMesh({ vonMises: [9.9] }),
    };
    expect(pickDefaultScalarChannel(meshes)).toBe('vonMises');
  });

  it('(k) preferred channel empty in mesh A but non-empty in mesh B → union picks it', () => {
    // Union semantics: vonMises is empty in A but non-empty in B, so it enters the set.
    const meshes = {
      a: makeMesh({ vonMises: [] }),
      b: makeMesh({ vonMises: [1] }),
    };
    expect(pickDefaultScalarChannel(meshes)).toBe('vonMises');
  });

  it('(l) preferred channel empty in both meshes, lower-preference non-empty in mesh B → lower-preference wins', () => {
    // vonMises is always empty; vonMises_bottom is non-empty only in B.
    // Expected: vonMises_bottom (first preferred name present in the union of non-empty channels).
    const meshes = {
      a: makeMesh({ vonMises: [] }),
      b: makeMesh({ vonMises_bottom: [9] }),
    };
    expect(pickDefaultScalarChannel(meshes)).toBe('vonMises_bottom');
  });
});

describe('PREFERRED_FEA_CHANNELS', () => {
  // Assert the ordering invariants that matter for preference selection, without
  // pinning the full array equality.  Adding a new preferred channel (e.g.
  // 'vonMises_membrane') must not break these tests as long as it doesn't
  // reorder the existing four entries relative to each other.
  it("'vonMises' precedes 'vonMises_top'", () => {
    const idx = (n: string) => PREFERRED_FEA_CHANNELS.indexOf(n);
    expect(idx('vonMises')).toBeGreaterThanOrEqual(0);
    expect(idx('vonMises_top')).toBeGreaterThanOrEqual(0);
    expect(idx('vonMises')).toBeLessThan(idx('vonMises_top'));
  });

  it("'vonMises_top' precedes 'vonMises_mid' and 'vonMises_bottom'", () => {
    const idx = (n: string) => PREFERRED_FEA_CHANNELS.indexOf(n);
    expect(idx('vonMises_top')).toBeGreaterThanOrEqual(0);
    expect(idx('vonMises_mid')).toBeGreaterThanOrEqual(0);
    expect(idx('vonMises_bottom')).toBeGreaterThanOrEqual(0);
    expect(idx('vonMises_top')).toBeLessThan(idx('vonMises_mid'));
    expect(idx('vonMises_top')).toBeLessThan(idx('vonMises_bottom'));
  });
});
