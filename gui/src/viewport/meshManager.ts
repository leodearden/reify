import {
  BufferGeometry,
  BufferAttribute,
  Mesh,
  MeshStandardMaterial,
  DoubleSide,
  Color,
  type Scene,
} from 'three';
import type { MeshData } from '../types';

/** Catppuccin accent palette for deterministic mesh coloring. */
const ACCENT_PALETTE = [
  '#89b4fa', // blue
  '#cba6f7', // mauve
  '#a6e3a1', // green
  '#fab387', // peach
  '#f38ba8', // red
  '#94e2d5', // teal
  '#f9e2af', // yellow
  '#f5c2e7', // pink
];

/** Simple string hash → palette index for deterministic color assignment. */
function hashEntityPath(path: string): number {
  let hash = 0;
  for (let i = 0; i < path.length; i++) {
    hash = ((hash << 5) - hash + path.charCodeAt(i)) | 0;
  }
  return Math.abs(hash) % ACCENT_PALETTE.length;
}

function colorForEntity(entityPath: string): Color {
  return new Color(ACCENT_PALETTE[hashEntityPath(entityPath)]);
}

export interface MeshManagerContext {
  sync: (meshes: Record<string, MeshData>) => void;
  dispose: () => void;
  getSceneMeshes: () => Map<string, Mesh>;
}

/**
 * Manages Three.js Mesh objects in a scene, syncing them against a
 * Record<string, MeshData> from the engine store.
 */
function validateMeshData(data: MeshData): boolean {
  if (data.vertices.length % 3 !== 0) {
    console.warn(`Invalid mesh data: vertices.length (${data.vertices.length}) is not divisible by 3`);
    return false;
  }
  const vertexCount = data.vertices.length / 3;
  for (let i = 0; i < data.indices.length; i++) {
    if (data.indices[i] >= vertexCount) {
      console.warn(`Invalid mesh data: index ${data.indices[i]} at position ${i} >= vertex count ${vertexCount}`);
      return false;
    }
  }
  return true;
}

export function createMeshManager(scene: Scene): MeshManagerContext {
  const meshMap = new Map<string, Mesh>();

  function createMeshFromData(entityPath: string, data: MeshData): Mesh {
    const geometry = new BufferGeometry();
    geometry.setAttribute('position', new BufferAttribute(data.vertices, 3));
    geometry.setIndex(new BufferAttribute(data.indices, 1));
    if (data.normals) {
      geometry.setAttribute('normal', new BufferAttribute(data.normals, 3));
    } else {
      geometry.computeVertexNormals();
    }

    const material = new MeshStandardMaterial({
      color: colorForEntity(entityPath),
      side: DoubleSide,
    });

    const mesh = new Mesh(geometry, material);
    mesh.name = entityPath;
    return mesh;
  }

  function updateMeshGeometry(mesh: Mesh, data: MeshData): void {
    const geometry = mesh.geometry as BufferGeometry;

    // Reuse existing BufferAttribute objects to avoid orphaning GPU-side WebGLBuffers
    const posAttr = geometry.getAttribute('position') as BufferAttribute | null;
    if (posAttr) {
      posAttr.array = data.vertices;
      (posAttr as { count: number }).count = data.vertices.length / 3;
      posAttr.needsUpdate = true;
    } else {
      geometry.setAttribute('position', new BufferAttribute(data.vertices, 3));
    }

    const indexAttr = geometry.index;
    if (indexAttr) {
      indexAttr.array = data.indices;
      (indexAttr as { count: number }).count = data.indices.length;
      indexAttr.needsUpdate = true;
    } else {
      geometry.setIndex(new BufferAttribute(data.indices, 1));
    }

    if (data.normals) {
      const normalAttr = geometry.getAttribute('normal') as BufferAttribute | null;
      if (normalAttr) {
        normalAttr.array = data.normals;
        (normalAttr as { count: number }).count = data.normals.length / 3;
        normalAttr.needsUpdate = true;
      } else {
        geometry.setAttribute('normal', new BufferAttribute(data.normals, 3));
      }
    } else if (geometry.getAttribute('normal')) {
      geometry.deleteAttribute('normal');
      geometry.computeVertexNormals();
    } else {
      geometry.computeVertexNormals();
    }

    // Invalidate cached bounding volumes so updated geometry is not incorrectly culled.
    // Setting to null forces Three.js to lazily recompute on next access.
    geometry.boundingSphere = null;
    geometry.boundingBox = null;
  }

  function removeMesh(entityPath: string): void {
    const mesh = meshMap.get(entityPath);
    if (!mesh) return;
    (mesh.geometry as BufferGeometry).dispose();
    (mesh.material as MeshStandardMaterial).dispose();
    scene.remove(mesh);
    meshMap.delete(entityPath);
  }

  function sync(meshes: Record<string, MeshData>): void {
    const incomingKeys = new Set(Object.keys(meshes));

    // Remove meshes no longer present
    for (const key of [...meshMap.keys()]) {
      if (!incomingKeys.has(key)) {
        removeMesh(key);
      }
    }

    // Add or update meshes
    for (const [entityPath, data] of Object.entries(meshes)) {
      if (!validateMeshData(data)) continue;
      if (meshMap.has(entityPath)) {
        updateMeshGeometry(meshMap.get(entityPath)!, data);
      } else {
        const mesh = createMeshFromData(entityPath, data);
        meshMap.set(entityPath, mesh);
        scene.add(mesh);
      }
    }
  }

  function dispose(): void {
    for (const key of [...meshMap.keys()]) {
      removeMesh(key);
    }
  }

  function getSceneMeshes(): Map<string, Mesh> {
    return meshMap;
  }

  return { sync, dispose, getSceneMeshes };
}
