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
});

describe('PREFERRED_FEA_CHANNELS', () => {
  it('equals the documented ordered list', () => {
    expect(PREFERRED_FEA_CHANNELS).toEqual([
      'vonMises',
      'vonMises_top',
      'vonMises_mid',
      'vonMises_bottom',
    ]);
  });
});
