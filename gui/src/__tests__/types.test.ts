/**
 * Runtime tests for types.ts — specifically for convertRawMesh and convertRawGuiState.
 * Task 2959: verify the new optional scalar_channels and displaced_positions
 * fields are correctly converted from number[] → Float32Array.
 * Task 3229: verify compile_diagnostics is copied by convertRawGuiState.
 */
import { describe, it, expect } from 'vitest';
import { convertRawMesh, convertRawGuiState } from '../types';
import type { RawMeshData, RawGuiState, DiagnosticInfo } from '../types';

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

  // --- shell-extract fields (task 3597) ---

  it('converts vector_channels number[] → Float32Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'Shell.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      vector_channels: {
        shell_normal_per_face: [0, 0, 1],
        shell_tangent_per_vertex: [1, 0, 0, 0, 1, 0, 0, 0, 1],
      },
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.vector_channels).toBeDefined();
    expect(mesh.vector_channels!['shell_normal_per_face']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.vector_channels!['shell_normal_per_face'])).toEqual([0, 0, 1]);
    expect(mesh.vector_channels!['shell_tangent_per_vertex']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.vector_channels!['shell_tangent_per_vertex'])).toEqual([1, 0, 0, 0, 1, 0, 0, 0, 1]);
  });

  it('leaves vector_channels undefined when absent', () => {
    const raw: RawMeshData = {
      entity_path: 'Tet.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.vector_channels).toBeUndefined();
  });

  it('converts element_kind number[] → Uint8Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'Shell.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
      indices: [0, 1, 2, 0, 2, 3],
      normals: null,
      element_kind: [0, 1],
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.element_kind).toBeInstanceOf(Uint8Array);
    expect(Array.from(mesh.element_kind!)).toEqual([0, 1]);
  });

  it('leaves element_kind undefined when absent', () => {
    const raw: RawMeshData = {
      entity_path: 'Tet.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.element_kind).toBeUndefined();
  });

  it('converts region_tags number[] → Uint32Array when present', () => {
    const raw: RawMeshData = {
      entity_path: 'Shell.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      region_tags: [42],
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.region_tags).toBeInstanceOf(Uint32Array);
    expect(Array.from(mesh.region_tags!)).toEqual([42]);
  });

  it('leaves region_tags undefined when absent', () => {
    const raw: RawMeshData = {
      entity_path: 'Tet.body',
      vertices: [0, 0, 0],
      indices: [0],
      normals: null,
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.region_tags).toBeUndefined();
  });

  it('converts all three new shell-extract fields together', () => {
    const raw: RawMeshData = {
      entity_path: 'Shell.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      element_kind: [1],
      region_tags: [99],
      vector_channels: { shell_normal_per_face: [0, 0, 1] },
    };
    const mesh = convertRawMesh(raw);
    expect(mesh.element_kind).toBeInstanceOf(Uint8Array);
    expect(Array.from(mesh.element_kind!)).toEqual([1]);
    expect(mesh.region_tags).toBeInstanceOf(Uint32Array);
    expect(Array.from(mesh.region_tags!)).toEqual([99]);
    expect(mesh.vector_channels!['shell_normal_per_face']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.vector_channels!['shell_normal_per_face'])).toEqual([0, 0, 1]);
  });
});

describe('convertRawGuiState', () => {
  it('copies compile_diagnostics from raw to converted state', () => {
    const diag: DiagnosticInfo = {
      file_path: 'test.ri',
      line: 5,
      column: 3,
      end_line: 5,
      end_column: 20,
      severity: 'Warning',
      message: "unknown port type 'Foo'",
      code: null,
    };
    const raw: RawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [diag],
    };
    const state = convertRawGuiState(raw);
    expect(state.compile_diagnostics).toHaveLength(1);
    expect(state.compile_diagnostics[0].severity).toBe('Warning');
    expect(state.compile_diagnostics[0].message).toContain('unknown port type');
    expect(state.compile_diagnostics[0].file_path).toBe('test.ri');
  });
});
