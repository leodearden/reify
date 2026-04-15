import {
  BufferGeometry,
  BufferAttribute,
  Mesh,
  MeshStandardMaterial,
  MeshBasicMaterial,
  Group,
  DoubleSide,
  Color,
  type Scene,
} from 'three';
import { computeBoundsTree, disposeBoundsTree } from 'three-mesh-bvh';
import type { MeshData, VisibilityState } from '../types';
import { createGhostMaterial } from './ghostMaterial';

// Patch BufferGeometry prototype for BVH acceleration
(BufferGeometry.prototype as any).computeBoundsTree = computeBoundsTree;
(BufferGeometry.prototype as any).disposeBoundsTree = disposeBoundsTree;

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
  setVisibility: (entityPath: string, state: VisibilityState) => void;
  getGhostMeshes: () => Map<string, Mesh>;
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
  const visibilityMap = new Map<string, VisibilityState>();
  const ghostMeshMap = new Map<string, Mesh>();

  // Single shared ghost material — one material instance per manager, not per ghost clone.
  const ghostMaterial: MeshBasicMaterial = createGhostMaterial();

  // Ghost Group: all ghost clones live here so they're separate from opaque meshes.
  const ghostGroup = new Group();
  ghostGroup.name = 'ghostGroup';
  scene.add(ghostGroup);

  function createMeshFromData(entityPath: string, data: MeshData): Mesh | null {
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

    try {
      (geometry as any).computeBoundsTree();
    } catch (err) {
      geometry.dispose();
      material.dispose();
      console.error(`Failed to build BVH for mesh '${entityPath}'`, err);
      return null;
    }

    const mesh = new Mesh(geometry, material);
    mesh.name = entityPath;
    return mesh;
  }

  function updateMeshGeometry(mesh: Mesh, data: MeshData): void {
    const geometry = mesh.geometry as BufferGeometry;

    // Reuse existing BufferAttribute objects when array length matches to avoid
    // orphaning GPU-side WebGLBuffers. When length differs, create new attribute
    // because WebGL buffers have fixed size and cannot be resized.
    const posAttr = geometry.getAttribute('position') as BufferAttribute | null;
    if (posAttr && posAttr.array.length === data.vertices.length) {
      posAttr.array = data.vertices;
      (posAttr as { count: number }).count = data.vertices.length / 3;
      posAttr.needsUpdate = true;
    } else {
      geometry.setAttribute('position', new BufferAttribute(data.vertices, 3));
    }

    const indexAttr = geometry.index;
    if (indexAttr && indexAttr.array.length === data.indices.length) {
      indexAttr.array = data.indices;
      (indexAttr as { count: number }).count = data.indices.length;
      indexAttr.needsUpdate = true;
    } else {
      geometry.setIndex(new BufferAttribute(data.indices, 1));
    }

    if (data.normals) {
      const normalAttr = geometry.getAttribute('normal') as BufferAttribute | null;
      if (normalAttr && normalAttr.array.length === data.normals.length) {
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

    // Rebuild BVH for the updated geometry
    try {
      (geometry as any).computeBoundsTree();
    } catch (err) {
      console.error(`Failed to rebuild BVH for mesh '${mesh.name}'`, err);
      removeMesh(mesh.name);
    }
  }

  function addGhostClone(entityPath: string, originalMesh: Mesh): void {
    const ghostClone = new Mesh(originalMesh.geometry, ghostMaterial);
    ghostClone.name = `ghost:${entityPath}`;
    ghostMeshMap.set(entityPath, ghostClone);
    ghostGroup.add(ghostClone);
  }

  function removeGhostClone(entityPath: string): void {
    const ghostClone = ghostMeshMap.get(entityPath);
    if (!ghostClone) return;
    ghostGroup.remove(ghostClone);
    ghostMeshMap.delete(entityPath);
  }

  function removeMesh(entityPath: string): void {
    const mesh = meshMap.get(entityPath);
    if (!mesh) return;

    const state = visibilityMap.get(entityPath) ?? 'show';

    // Remove from scene only if mesh is currently shown there
    if (state === 'show') {
      scene.remove(mesh);
    }

    // Clean up any ghost clone for this entity
    removeGhostClone(entityPath);

    (mesh.geometry as any).disposeBoundsTree();
    (mesh.geometry as BufferGeometry).dispose();
    (mesh.material as MeshStandardMaterial).dispose();
    meshMap.delete(entityPath);
    visibilityMap.delete(entityPath);
  }

  function setVisibility(entityPath: string, state: VisibilityState): void {
    const prevState = visibilityMap.get(entityPath) ?? 'show';
    visibilityMap.set(entityPath, state);

    const mesh = meshMap.get(entityPath);
    if (!mesh) {
      // Mesh hasn't arrived yet; visibilityMap pre-set will be applied when sync() adds it.
      return;
    }

    if (prevState === state) return; // no change

    if (prevState === 'show') {
      if (state === 'ghost') {
        scene.remove(mesh);
        addGhostClone(entityPath, mesh);
      } else if (state === 'hidden') {
        scene.remove(mesh);
      }
    } else if (prevState === 'ghost') {
      if (state === 'show') {
        removeGhostClone(entityPath);
        scene.add(mesh);
      } else if (state === 'hidden') {
        removeGhostClone(entityPath);
      }
    } else if (prevState === 'hidden') {
      if (state === 'show') {
        scene.add(mesh);
      } else if (state === 'ghost') {
        addGhostClone(entityPath, mesh);
      }
    }
  }

  function sync(meshes: Record<string, MeshData>): void {
    // Remove meshes no longer present
    for (const key of [...meshMap.keys()]) {
      if (!(key in meshes)) {
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
        if (mesh) {
          meshMap.set(entityPath, mesh);
          const state = visibilityMap.get(entityPath) ?? 'show';
          if (state === 'show') {
            scene.add(mesh);
          } else if (state === 'ghost') {
            addGhostClone(entityPath, mesh);
          }
          // 'hidden': don't add anywhere
        }
      }
    }
  }

  function dispose(): void {
    for (const key of [...meshMap.keys()]) {
      removeMesh(key);
    }
    ghostMaterial.dispose();
  }

  function getSceneMeshes(): Map<string, Mesh> {
    const result = new Map<string, Mesh>();
    for (const [key, mesh] of meshMap) {
      const state = visibilityMap.get(key) ?? 'show';
      if (state === 'show') {
        result.set(key, mesh);
      }
    }
    return result;
  }

  function getGhostMeshes(): Map<string, Mesh> {
    return ghostMeshMap;
  }

  return { sync, dispose, getSceneMeshes, setVisibility, getGhostMeshes };
}
