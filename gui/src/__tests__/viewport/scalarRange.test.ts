/**
 * Unit suite for computeScalarRange — step-1 RED.
 *
 * This file imports from ../../viewport/scalarRange, which does not yet exist.
 * The suite MUST fail to import (module absent) in step-1 — that is the RED state.
 */
import { describe, it, expect } from 'vitest';
import { computeScalarRange } from '../../viewport/scalarRange';
import type { MeshData } from '../../types';

// ─── helpers ─────────────────────────────────────────────────────────────────

function makeMesh(channelName: string, values: number[]): MeshData {
  return {
    entity_path: 'test',
    vertices: new Float32Array(0),
    indices: new Uint32Array(0),
    normals: null,
    scalar_channels: {
      [channelName]: new Float32Array(values),
    },
  } as unknown as MeshData;
}

function makeMeshNoChannel(): MeshData {
  return {
    entity_path: 'test',
    vertices: new Float32Array(0),
    indices: new Uint32Array(0),
    normals: null,
  } as unknown as MeshData;
}

// ─── tests ────────────────────────────────────────────────────────────────────

describe('computeScalarRange', () => {
  it('(a) pools values across multiple meshes', () => {
    // mesh A: vonMises [1, 3], mesh B: vonMises [2, 9] → {min:1, max:9}
    const meshes = {
      a: makeMesh('vonMises', [1, 3]),
      b: makeMesh('vonMises', [2, 9]),
    };
    expect(computeScalarRange(meshes, 'vonMises')).toEqual({ min: 1, max: 9 });
  });

  it('(b) excludes the OOB sentinel -1.0 and all negative values', () => {
    // [-1, 2, 5] → {2, 5}; -1 is the SCALAR_CHANNEL_OOB_SENTINEL
    const meshes = { m: makeMesh('vonMises', [-1, 2, 5]) };
    expect(computeScalarRange(meshes, 'vonMises')).toEqual({ min: 2, max: 5 });
  });

  it('(b2) excludes any negative value, not just -1.0', () => {
    // [-3.5, 0, 4] → {0, 4}
    const meshes = { m: makeMesh('vonMises', [-3.5, 0, 4]) };
    expect(computeScalarRange(meshes, 'vonMises')).toEqual({ min: 0, max: 4 });
  });

  it('(c) excludes NaN and ±Infinity', () => {
    // [NaN, 4, Infinity, 6] → {4, 6}
    const meshes = { m: makeMesh('vonMises', [NaN, 4, Infinity, 6]) };
    expect(computeScalarRange(meshes, 'vonMises')).toEqual({ min: 4, max: 6 });
  });

  it('(c2) excludes -Infinity', () => {
    const meshes = { m: makeMesh('vonMises', [-Infinity, 3, 7]) };
    expect(computeScalarRange(meshes, 'vonMises')).toEqual({ min: 3, max: 7 });
  });

  it('(d1) returns null for empty meshes record', () => {
    expect(computeScalarRange({}, 'vonMises')).toBeNull();
  });

  it('(d2) returns null when the channel key is absent from every mesh', () => {
    const meshes = {
      a: makeMeshNoChannel(),
      b: makeMeshNoChannel(),
    };
    expect(computeScalarRange(meshes, 'vonMises')).toBeNull();
  });

  it('(d3) returns null when the channel array is empty (length 0)', () => {
    const meshes = { m: makeMesh('vonMises', []) };
    expect(computeScalarRange(meshes, 'vonMises')).toBeNull();
  });

  it('(d4) returns null when every value is filtered out (all -1.0 / NaN)', () => {
    const meshes = { m: makeMesh('vonMises', [-1, -1, NaN]) };
    expect(computeScalarRange(meshes, 'vonMises')).toBeNull();
  });

  it('(e) single finite non-negative value → {min:v, max:v}', () => {
    const meshes = { m: makeMesh('vonMises', [0.5]) };
    expect(computeScalarRange(meshes, 'vonMises')).toEqual({ min: 0.5, max: 0.5 });
  });

  it('(e2) single zero is valid → {min:0, max:0}', () => {
    const meshes = { m: makeMesh('vonMises', [0]) };
    expect(computeScalarRange(meshes, 'vonMises')).toEqual({ min: 0, max: 0 });
  });

  it('(f) ignores other channel keys', () => {
    // Only 'vonMises' requested; 'displacement_magnitude' has values [10, 20] — ignored
    const mesh = {
      entity_path: 'test',
      vertices: new Float32Array(0),
      indices: new Uint32Array(0),
      normals: null,
      scalar_channels: {
        vonMises: new Float32Array([3, 7]),
        displacement_magnitude: new Float32Array([10, 20]),
      },
    } as unknown as MeshData;
    expect(computeScalarRange({ m: mesh }, 'vonMises')).toEqual({ min: 3, max: 7 });
    expect(computeScalarRange({ m: mesh }, 'displacement_magnitude')).toEqual({ min: 10, max: 20 });
  });

  it('(f2) channel key absent → null even when other channels exist', () => {
    const mesh = {
      entity_path: 'test',
      vertices: new Float32Array(0),
      indices: new Uint32Array(0),
      normals: null,
      scalar_channels: {
        displacement_magnitude: new Float32Array([10, 20]),
      },
    } as unknown as MeshData;
    expect(computeScalarRange({ m: mesh }, 'vonMises')).toBeNull();
  });
});
