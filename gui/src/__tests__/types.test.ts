/**
 * Runtime tests for types.ts — specifically for convertRawMesh.
 * Task 2959: verify the new optional scalar_channels and displaced_positions
 * fields are correctly converted from number[] → Float32Array.
 */
import { describe, it, expect } from 'vitest';
import { convertRawMesh } from '../types';
import type { RawMeshData } from '../types';

describe('convertRawMesh', () => {
  it('converts scalar_channels number[] → Float32Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'FEA.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      scalar_channels: { vonMises: [10, 20, 30] },
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.scalar_channels).toBeDefined();
    expect(mesh.scalar_channels!['vonMises']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.scalar_channels!['vonMises'])).toEqual([10, 20, 30]);
  });

  it('converts multiple scalar channels independently', () => {
    const raw: RawMeshData = {
      entity_path: 'FEA.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      scalar_channels: {
        vonMises: [1, 2, 3],
        displacement_magnitude: [4, 5, 6],
      },
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.scalar_channels!['vonMises']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.scalar_channels!['vonMises'])).toEqual([1, 2, 3]);
    expect(mesh.scalar_channels!['displacement_magnitude']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.scalar_channels!['displacement_magnitude'])).toEqual([4, 5, 6]);
  });

  it('leaves scalar_channels undefined when absent from raw payload', () => {
    const raw: RawMeshData = {
      entity_path: 'Plain.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.scalar_channels).toBeUndefined();
  });

  it('converts displaced_positions number[] → Float32Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'FEA.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      displaced_positions: [1, 2, 3, 4, 5, 6, 7, 8, 9],
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.displaced_positions).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.displaced_positions!)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
  });

  it('leaves displaced_positions undefined when absent from raw payload', () => {
    const raw: RawMeshData = {
      entity_path: 'Plain.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    // undefined (not present) when field is absent from the raw payload
    expect(mesh.displaced_positions).toBeUndefined();
  });

  it('converts both scalar_channels and displaced_positions together', () => {
    const raw: RawMeshData = {
      entity_path: 'FEA.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      scalar_channels: { vonMises: [10, 20, 30] },
      displaced_positions: [1, 0, 0, 2, 0, 0, 3, 0, 0],
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.scalar_channels!['vonMises']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.scalar_channels!['vonMises'])).toEqual([10, 20, 30]);
    expect(mesh.displaced_positions).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.displaced_positions!)).toHaveLength(9);
  });
});
