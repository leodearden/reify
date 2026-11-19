import type { MeshData } from '../../types';

/** A one-vertex MeshData for `entityPath`, for tests that only route or count meshes. */
export function makeMesh(entityPath: string): MeshData {
  return {
    entity_path: entityPath,
    vertices: new Float32Array([0, 0, 0]),
    indices: new Uint32Array([0]),
    normals: null,
  };
}
