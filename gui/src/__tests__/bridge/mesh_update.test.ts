/**
 * Integration test for the mesh-update Tauri event bridge — shell-extract
 * fields (task 3597 η).
 *
 * Verifies the full IPC ingestion path:
 *   RawMeshData (wire JSON) → listen handler → convertRawMesh → MeshData
 *                                                                (typed arrays)
 *
 * This is the user-observable signal (b) from the task description:
 * when the backend emits a mesh-update event carrying the three new fields
 * (element_kind, region_tags, vector_channels), onMeshUpdate delivers a
 * MeshData to the callback with correctly-typed values.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { listen } from '@tauri-apps/api/event';

// Must be declared at module scope before any imports from mockEvents.ts.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { mockTauriEvent, clearAllMockEvents } from '../test_utils/mockEvents';
import { onMeshUpdate } from '../../bridge';
import type { RawMeshData } from '../../types';

describe('mesh-update bridge — shell extensions (η)', () => {
  beforeEach(() => {
    vi.mocked(listen).mockReset();
    clearAllMockEvents();
    vi.clearAllMocks();
  });

  it('delivers typed-array shell-extract fields to the callback', async () => {
    const meshHandle = mockTauriEvent<RawMeshData>('mesh-update');
    const cb = vi.fn();

    await onMeshUpdate(cb);

    meshHandle.emit({
      entity_path: 'Bracket.shell',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
      element_kind: [1],
      region_tags: [42],
      vector_channels: { shell_normal_per_face: [0, 0, 1] },
    });

    expect(cb).toHaveBeenCalledOnce();

    const mesh = cb.mock.calls[0][0];

    // element_kind: number[] [1] → Uint8Array
    expect(mesh.element_kind).toBeInstanceOf(Uint8Array);
    expect(Array.from(mesh.element_kind)).toEqual([1]);

    // region_tags: number[] [42] → Uint32Array
    expect(mesh.region_tags).toBeInstanceOf(Uint32Array);
    expect(Array.from(mesh.region_tags)).toEqual([42]);

    // vector_channels: Record<string, number[]> → Record<string, Float32Array>
    expect(mesh.vector_channels).toBeDefined();
    expect(mesh.vector_channels['shell_normal_per_face']).toBeInstanceOf(Float32Array);
    expect(Array.from(mesh.vector_channels['shell_normal_per_face'])).toEqual([0, 0, 1]);
  });

  it('delivers MeshData without new fields when they are absent from the wire payload', async () => {
    const meshHandle = mockTauriEvent<RawMeshData>('mesh-update');
    const cb = vi.fn();

    await onMeshUpdate(cb);

    meshHandle.emit({
      entity_path: 'Tet.body',
      vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
      indices: [0, 1, 2],
      normals: null,
    });

    expect(cb).toHaveBeenCalledOnce();

    const mesh = cb.mock.calls[0][0];
    expect(mesh.element_kind).toBeUndefined();
    expect(mesh.region_tags).toBeUndefined();
    expect(mesh.vector_channels).toBeUndefined();
  });
});
