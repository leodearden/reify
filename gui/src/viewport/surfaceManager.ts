/**
 * Surface manager for tensegrity membrane viewport styling (β).
 *
 * Renders `TensegritySurfaceData` arrays as filled translucent Mesh objects in
 * the Three.js scene, with a shaded MeshStandardMaterial per member kind.
 *
 * Design decisions:
 * - MEMBRANE_COLOR: Catppuccin Mocha Teal `#94e2d5` — visually distinct from
 *   strut red (#f38ba8) and cable blue (#89b4fa).
 * - transparent: true, opacity ~0.4, side: DoubleSide — fabric seen from both faces.
 * - NON-INDEXED geometry built from inline x0..z2 corner coords (9 floats/triangle),
 *   then computeVertexNormals() for flat-shaded appearance.
 * - No setResolution — MeshStandardMaterial has no screen-space resolution uniform.
 */

import { BufferGeometry, BufferAttribute, Mesh, MeshStandardMaterial, DoubleSide } from 'three';
import type { Scene } from 'three';
import type { TensegritySurfaceData } from '../types';

// ─── Styling constants (Catppuccin Mocha palette) ─────────────────────────────

/** Catppuccin Mocha Teal — membrane surfaces (translucent, fabric-like). */
const MEMBRANE_COLOR = '#94e2d5';

/** Opacity for membrane surfaces — translucent so structure is visible through them. */
const MEMBRANE_OPACITY = 0.4;

// ─── Types ─────────────────────────────────────────────────────────────────────

interface SurfaceGroup {
  mesh: Mesh;
  geometry: BufferGeometry;
  material: MeshStandardMaterial;
}

export interface SurfaceManagerContext {
  /**
   * Sync the scene with the given facet list. Groups facets by kind and adds/replaces
   * per-kind Mesh objects. Call with `[]` to remove all surface objects.
   */
  sync: (facets: TensegritySurfaceData[]) => void;
  /**
   * Remove all surface objects from the scene and dispose their geometry and material.
   */
  dispose: () => void;
}

// ─── Factory ──────────────────────────────────────────────────────────────────

/**
 * Create a surface manager bound to the given Three.js scene.
 *
 * @param scene - The Three.js scene to add/remove surface objects to/from.
 */
export function createSurfaceManager(scene: Scene): SurfaceManagerContext {
  /** Map from kind string to the current SurfaceGroup object in the scene. */
  const activeGroups = new Map<string, SurfaceGroup>();

  /** Remove a SurfaceGroup from the scene and dispose its resources. */
  function disposeGroup(group: SurfaceGroup): void {
    scene.remove(group.mesh);
    group.geometry.dispose();
    group.material.dispose();
  }

  function sync(facets: TensegritySurfaceData[]): void {
    // NOTE: the Rust bridge emits one surface-list cell per template in practice
    // (α binds tensegrity_surfaces() to one cell), so duplicate facets are
    // unlikely.  If a module ever re-binds the surface list the same facets may
    // appear more than once and would be rendered as overlapping transparent
    // triangles.  De-dup by (kind, i0, i1, i2) can be added cheaply here if
    // that becomes an issue.

    // Group facets by kind.
    const byKind = new Map<string, TensegritySurfaceData[]>();
    for (const facet of facets) {
      const list = byKind.get(facet.kind);
      if (list) {
        list.push(facet);
      } else {
        byKind.set(facet.kind, [facet]);
      }
    }

    // Determine which kinds are no longer present → remove their groups.
    for (const [kind, group] of activeGroups) {
      if (!byKind.has(kind)) {
        disposeGroup(group);
        activeGroups.delete(kind);
      }
    }

    // Build/replace a Mesh per kind.
    for (const [kind, kindFacets] of byKind) {
      // Pack positions as flat [x0,y0,z0, x1,y1,z1, x2,y2,z2, ...] array.
      // 9 floats per triangle (non-indexed).
      const positions: number[] = [];
      for (const f of kindFacets) {
        positions.push(
          f.x0, f.y0, f.z0,
          f.x1, f.y1, f.z1,
          f.x2, f.y2, f.z2,
        );
      }

      // Dispose any previously-existing group for this kind.
      const existing = activeGroups.get(kind);
      if (existing) {
        disposeGroup(existing);
        activeGroups.delete(kind);
      }

      // Build non-indexed BufferGeometry from inline corner coords.
      const geometry = new BufferGeometry();
      geometry.setAttribute('position', new BufferAttribute(new Float32Array(positions), 3));
      geometry.computeVertexNormals();

      // Create MeshStandardMaterial: transparent, DoubleSide, kind-coloured.
      const color = MEMBRANE_COLOR; // Extend here for future kinds.
      const material = new MeshStandardMaterial({
        color,
        transparent: true,
        opacity: MEMBRANE_OPACITY,
        side: DoubleSide,
      });

      const mesh = new Mesh(geometry, material);
      scene.add(mesh);

      activeGroups.set(kind, { mesh, geometry, material });
    }
  }

  function dispose(): void {
    for (const group of activeGroups.values()) {
      disposeGroup(group);
    }
    activeGroups.clear();
  }

  return { sync, dispose };
}
