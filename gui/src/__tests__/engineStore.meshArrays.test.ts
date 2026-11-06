/**
 * engineStore.applyMeshUpdate swaps in a mesh update's typed arrays and never
 * writes into the arrays the entity held before. meshManager.sync relies on
 * this: it skips rebuilding derived geometry (the edges overlay) when an
 * entity's arrays are the identical objects it last built from.
 */
import { describe, it, expect, vi } from 'vitest';
import { createRoot } from 'solid-js';
import type { MeshData } from '../types';

// The store is never subscribed to engine events here.
vi.mock('../bridge', () => ({}));

import { createEngineStore } from '../stores/engineStore';

function mesh(vertices: number[], displaced?: number[]): MeshData {
  return {
    entity_path: 'Bracket.body',
    vertices: new Float32Array(vertices),
    indices: new Uint32Array([0, 1, 2]),
    normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
    ...(displaced ? { displaced_positions: new Float32Array(displaced) } : {}),
  };
}

describe('engineStore.applyMeshUpdate typed-array ownership', () => {
  it('swaps in an update\'s new arrays and leaves the previous arrays untouched', () => {
    createRoot((dispose) => {
      const { state, applyMeshUpdate } = createEngineStore();
      const first = mesh([0, 0, 0, 1, 0, 0, 0, 1, 0], [0, 0, 1, 1, 0, 1, 0, 1, 1]);
      applyMeshUpdate(first);
      const held = state.meshes['Bracket.body'];
      const oldVertices = held.vertices;
      const oldIndices = held.indices;
      const oldDisplaced = held.displaced_positions!;

      const second = mesh([5, 5, 5, 6, 5, 5, 5, 6, 5], [5, 5, 6, 6, 5, 6, 5, 6, 6]);
      applyMeshUpdate(second);

      const now = state.meshes['Bracket.body'];
      expect(now.vertices).toBe(second.vertices);
      expect(now.indices).toBe(second.indices);
      expect(now.displaced_positions).toBe(second.displaced_positions);
      expect([...oldVertices]).toEqual([0, 0, 0, 1, 0, 0, 0, 1, 0]);
      expect([...oldIndices]).toEqual([0, 1, 2]);
      expect([...oldDisplaced]).toEqual([0, 0, 1, 1, 0, 1, 0, 1, 1]);
      dispose();
    });
  });

  it('keeps the very same arrays when an update re-sends them', () => {
    createRoot((dispose) => {
      const { state, applyMeshUpdate } = createEngineStore();
      const first = mesh([0, 0, 0, 1, 0, 0, 0, 1, 0]);
      applyMeshUpdate(first);

      applyMeshUpdate({ ...first, normals: new Float32Array(9) });

      expect(state.meshes['Bracket.body'].vertices).toBe(first.vertices);
      expect(state.meshes['Bracket.body'].indices).toBe(first.indices);
      dispose();
    });
  });
});
