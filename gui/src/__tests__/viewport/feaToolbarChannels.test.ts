/**
 * Unit suite for feaToolbarChannels — task 3001 step-9 RED.
 *
 * This file imports from ../../viewport/feaToolbarChannels, which does not
 * yet exist. The suite MUST fail (module absent → import error) in step-9 —
 * that is the RED state.
 */
import { describe, it, expect } from 'vitest';
import { feaToolbarChannels } from '../../viewport/feaToolbarChannels';
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

describe('feaToolbarChannels', () => {
  it('(a) empty meshes record → base list only', () => {
    expect(feaToolbarChannels({})).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(b) mesh with no scalar_channels → base list only', () => {
    const meshes = { m: makeMeshNoChannels() };
    expect(feaToolbarChannels(meshes)).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(c) mesh with vonMises only (no errorIndicator) → base list only', () => {
    const meshes = { m: makeMesh({ vonMises: [1.0, 2.0] }) };
    expect(feaToolbarChannels(meshes)).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(d) mesh with a non-empty errorIndicator channel → base list + errorIndicator appended', () => {
    const meshes = { m: makeMesh({ vonMises: [1.0], errorIndicator: [0.5, 0.6] }) };
    expect(feaToolbarChannels(meshes)).toEqual([
      'vonMises',
      'displacement_magnitude',
      'errorIndicator',
    ]);
  });

  it('(e) errorIndicator present but EMPTY on the only mesh → not appended', () => {
    const meshes = { m: makeMesh({ vonMises: [1.0], errorIndicator: [] }) };
    expect(feaToolbarChannels(meshes)).toEqual(['vonMises', 'displacement_magnitude']);
  });

  it('(f) errorIndicator empty on mesh A but non-empty on mesh B → appended (union semantics)', () => {
    const meshes = {
      a: makeMesh({ errorIndicator: [] }),
      b: makeMesh({ errorIndicator: [3.0] }),
    };
    expect(feaToolbarChannels(meshes)).toEqual([
      'vonMises',
      'displacement_magnitude',
      'errorIndicator',
    ]);
  });

  it('(g) multiple meshes each with non-empty errorIndicator → appended exactly once (no duplicates)', () => {
    const meshes = {
      a: makeMesh({ errorIndicator: [1.0] }),
      b: makeMesh({ errorIndicator: [2.0] }),
    };
    const result = feaToolbarChannels(meshes);
    expect(result).toEqual(['vonMises', 'displacement_magnitude', 'errorIndicator']);
    expect(result.filter((c) => c === 'errorIndicator')).toHaveLength(1);
  });

  it('(h) base list order is always ["vonMises", "displacement_magnitude", ...] regardless of channel insertion order', () => {
    const meshes = { m: makeMesh({ errorIndicator: [1.0], vonMises: [2.0] }) };
    expect(feaToolbarChannels(meshes)).toEqual([
      'vonMises',
      'displacement_magnitude',
      'errorIndicator',
    ]);
  });
});
