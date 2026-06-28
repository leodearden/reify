/**
 * FEA diagnostic overlay for the 3D viewport (#2966).
 *
 * Architecture: pure spec-builder helpers (WebGL-free, unit-testable with plain
 * numbers) + a createDiagnosticOverlay(scene) manager (THREE.js layer, step-8).
 *
 * Mirrors:
 * - bucklingAnimator.ts — computePointCloudBounds (WebGL-free bounds helper)
 * - wireManager.ts — createWireManager sync/dispose pattern (step-8)
 * - selection.ts — EdgesGeometry + LineSegments red-outline pattern (step-8)
 *
 * Design decisions:
 * - Translation DOFs → arrow along ±axis, color TRANSLATION_COLOR
 * - Rotation DOFs → arrow along spin axis, color ROTATION_COLOR, isRotation=true
 * - ProblemElements overlay: red EdgesGeometry outline of the affected mesh(es)
 *   (no precise per-tet outlines possible — surface mesh has no element→face provenance)
 * - UnresolvedSelector: list-only (no geometry rendered, data-deferred to P2/#4092)
 */

import {
  Group,
  ArrowHelper,
  Vector3,
  LineSegments,
  BufferGeometry,
  Float32BufferAttribute,
  LineBasicMaterial,
} from 'three';
import type { Scene } from 'three';
import type { MeshData } from '../types';
import type { DofDirectionInfo, FeaDiagnosticInfo } from '../types';

// ---------------------------------------------------------------------------
// Arrow color constants
// ---------------------------------------------------------------------------

/** CSS hex colour for translation rigid-body-mode arrows (yellow-orange). */
export const TRANSLATION_ARROW_COLOR = 0xffaa00;

/** CSS hex colour for rotation rigid-body-mode arrows (sky blue). */
export const ROTATION_ARROW_COLOR = 0x00aaff;

// ---------------------------------------------------------------------------
// Arrow spec type
// ---------------------------------------------------------------------------

/** One rigid-body arrow specification (pure data, no THREE.js types). */
export interface ArrowSpec {
  /** World-space origin (body centroid). */
  origin: [number, number, number];
  /** Unit direction vector along the DOF axis. */
  dir: [number, number, number];
  /** Arrow length in scene units (scales with mesh radius). */
  length: number;
  /** Hex colour number (0xRRGGBB). */
  color: number;
  /** True for Rotation* DOFs, false for Translation* DOFs. */
  isRotation: boolean;
}

// ---------------------------------------------------------------------------
// Pure helper: computeMeshesBounds
// ---------------------------------------------------------------------------

/**
 * Compute the bounding-box center and half-space-diagonal radius for a list
 * of MeshData objects.
 *
 * Pure function — no three.js dependency. Iterates over `MeshData.vertices`
 * (Float32Array, packed XYZ) across all meshes.
 *
 * Returns `{ center: [0,0,0], radius: 0 }` for an empty list or empty vertex arrays.
 *
 * Mirrors `bucklingAnimator.computePointCloudBounds` (same diagonal-radius math).
 */
export function computeMeshesBounds(
  meshes: MeshData[],
): { center: [number, number, number]; radius: number } {
  if (meshes.length === 0) return { center: [0, 0, 0], radius: 0 };

  let xMin = Infinity, xMax = -Infinity;
  let yMin = Infinity, yMax = -Infinity;
  let zMin = Infinity, zMax = -Infinity;
  let hasVertices = false;

  for (const mesh of meshes) {
    const verts = mesh.vertices;
    for (let i = 0; i < verts.length; i += 3) {
      const x = verts[i]!;
      const y = verts[i + 1]!;
      const z = verts[i + 2]!;
      if (x < xMin) xMin = x;
      if (x > xMax) xMax = x;
      if (y < yMin) yMin = y;
      if (y > yMax) yMax = y;
      if (z < zMin) zMin = z;
      if (z > zMax) zMax = z;
      hasVertices = true;
    }
  }

  if (!hasVertices) return { center: [0, 0, 0], radius: 0 };

  // Handle degenerate case: single point (all min == max)
  if (!isFinite(xMin)) return { center: [0, 0, 0], radius: 0 };

  const cx = (xMin + xMax) / 2;
  const cy = (yMin + yMax) / 2;
  const cz = (zMin + zMax) / 2;
  const dx = xMax - xMin, dy = yMax - yMin, dz = zMax - zMin;
  const radius = 0.5 * Math.sqrt(dx * dx + dy * dy + dz * dz);

  return { center: [cx, cy, cz], radius };
}

// ---------------------------------------------------------------------------
// DOF → axis direction lookup
// ---------------------------------------------------------------------------

/** Maps each DofDirectionInfo to its world-space unit axis. */
const DOF_AXIS: Record<DofDirectionInfo, [number, number, number]> = {
  TranslationX: [1, 0, 0],
  TranslationY: [0, 1, 0],
  TranslationZ: [0, 0, 1],
  RotationX: [1, 0, 0],
  RotationY: [0, 1, 0],
  RotationZ: [0, 0, 1],
};

// ---------------------------------------------------------------------------
// Pure helper: rigidBodyArrowSpecs
// ---------------------------------------------------------------------------

/**
 * Build an array of arrow specifications for rigid-body mode arrows.
 *
 * One ArrowSpec per DofDirection. Arrow origin is the body centroid (`center`);
 * arrow length scales with `radius` (so arrows are visible at any model scale).
 * Translation DOFs get `TRANSLATION_ARROW_COLOR`; rotation DOFs get
 * `ROTATION_ARROW_COLOR` and `isRotation: true`.
 *
 * Pure function — no THREE.js dependency. Returns `[]` for an empty modes list.
 */
export function rigidBodyArrowSpecs(
  modes: DofDirectionInfo[],
  center: [number, number, number],
  radius: number,
): ArrowSpec[] {
  if (modes.length === 0) return [];

  // Arrow length = 80% of radius, with a minimum so it is always visible.
  const length = Math.max(radius, 0.01) * 0.8;

  return modes.map((mode) => {
    const isRotation = mode.startsWith('Rotation');
    return {
      origin: center,
      dir: DOF_AXIS[mode],
      length,
      color: isRotation ? ROTATION_ARROW_COLOR : TRANSLATION_ARROW_COLOR,
      isRotation,
    };
  });
}

// ---------------------------------------------------------------------------
// Pure helper: problemElementOutlinePositions
// ---------------------------------------------------------------------------

/**
 * Build flat edge-position arrays for a LineSegments outline of the provided
 * meshes (coarse surface outline — no per-tet element provenance available).
 *
 * Returns a flat `number[]` of paired XYZ positions for each triangle edge:
 * [x0a,y0a,z0a, x0b,y0b,z0b, x1a,y1a,z1a, ...]. The caller can pass this
 * to `THREE.EdgesGeometry` / `LineSegments` without deduplication (duplicate
 * edges are visually harmless for the diagnostic overlay use-case).
 *
 * Pure function — no THREE.js dependency. Returns `[]` for empty mesh list.
 */
export function problemElementOutlinePositions(meshes: MeshData[]): number[] {
  const positions: number[] = [];

  for (const mesh of meshes) {
    const verts = mesh.vertices;
    const idxs = mesh.indices;

    for (let t = 0; t < idxs.length; t += 3) {
      const i0 = idxs[t]! * 3;
      const i1 = idxs[t + 1]! * 3;
      const i2 = idxs[t + 2]! * 3;

      // Edge 0→1
      positions.push(verts[i0]!, verts[i0 + 1]!, verts[i0 + 2]!);
      positions.push(verts[i1]!, verts[i1 + 1]!, verts[i1 + 2]!);
      // Edge 1→2
      positions.push(verts[i1]!, verts[i1 + 1]!, verts[i1 + 2]!);
      positions.push(verts[i2]!, verts[i2 + 1]!, verts[i2 + 2]!);
      // Edge 2→0
      positions.push(verts[i2]!, verts[i2 + 1]!, verts[i2 + 2]!);
      positions.push(verts[i0]!, verts[i0 + 1]!, verts[i0 + 2]!);
    }
  }

  return positions;
}

// ---------------------------------------------------------------------------
// Overlay manager (THREE.js layer)
// ---------------------------------------------------------------------------

/** renderOrder for the overlay Group — above the default mesh layer (renderOrder 0). */
const OVERLAY_RENDER_ORDER = 1;

/** Hex colour for problem-element outline (red). */
const PROBLEM_ELEMENT_COLOR = 0xff0000;

export interface DiagnosticOverlay {
  /** Rebuild the overlay objects from new diagnostics and mesh set. */
  sync(diagnostics: FeaDiagnosticInfo[], meshes: MeshData[]): void;
  /** Remove the overlay Group from the scene and dispose all GPU resources. */
  dispose(): void;
}

/**
 * Create a diagnostic overlay manager bound to the given THREE.js scene.
 *
 * Mirrors wireManager.ts — a sync/dispose interface that rebuilds a single
 * overlay Group on each sync and removes it on dispose.
 *
 * Design decisions:
 * - Unconstrained → ArrowHelpers at the mesh centroid, one per DofDirection
 * - ProblemElements → red LineSegments coarse outline of the affected mesh(es)
 * - UnresolvedSelector → list-only (no geometry, data-deferred to P2/#4092)
 * - renderOrder = OVERLAY_RENDER_ORDER (above default mesh layer 0)
 * - Replace-on-sync: the previous Group is removed+disposed before rebuilding
 */
export function createDiagnosticOverlay(scene: Scene): DiagnosticOverlay {
  let overlayGroup: InstanceType<typeof Group> | null = null;

  /** Remove the current Group from the scene and dispose its children's resources. */
  function removeAndDispose(): void {
    if (!overlayGroup) return;
    scene.remove(overlayGroup);
    // Dispose geometry and material of any LineSegments children (duck-typed so
    // it works regardless of whether the tests mock the classes).
    for (const child of overlayGroup.children) {
      const c = child as any;
      if (c.geometry?.dispose) c.geometry.dispose();
      if (c.material?.dispose) c.material.dispose();
    }
    overlayGroup = null;
  }

  function sync(diagnostics: FeaDiagnosticInfo[], meshes: MeshData[]): void {
    // Tear down any existing overlay before rebuilding.
    removeAndDispose();

    // UnresolvedSelector renders nothing; only Unconstrained and ProblemElements
    // produce geometry.
    const hasRenderable = diagnostics.some(
      (d) => d.kind === 'Unconstrained' || d.kind === 'ProblemElements',
    );
    if (!hasRenderable) return;

    const group = new Group();
    group.renderOrder = OVERLAY_RENDER_ORDER;

    const { center, radius } = computeMeshesBounds(meshes);

    for (const diag of diagnostics) {
      if (diag.kind === 'Unconstrained') {
        // One ArrowHelper per rigid-body DOF mode.
        const specs = rigidBodyArrowSpecs(diag.rigid_body_modes, center, radius);
        for (const spec of specs) {
          const dir = new Vector3(spec.dir[0], spec.dir[1], spec.dir[2]).normalize();
          const origin = new Vector3(spec.origin[0], spec.origin[1], spec.origin[2]);
          const arrow = new ArrowHelper(dir, origin, spec.length, spec.color);
          group.add(arrow);
        }
      } else if (diag.kind === 'ProblemElements') {
        // Coarse red edge outline of the affected mesh(es).
        const positions = problemElementOutlinePositions(meshes);
        const geom = new BufferGeometry();
        geom.setAttribute('position', new Float32BufferAttribute(positions, 3));
        const mat = new LineBasicMaterial({ color: PROBLEM_ELEMENT_COLOR });
        const lines = new LineSegments(geom, mat);
        group.add(lines);
      }
      // UnresolvedSelector: no geometry (data-deferred to P2/#4092)
    }

    overlayGroup = group;
    scene.add(group);
  }

  function dispose(): void {
    removeAndDispose();
  }

  return { sync, dispose };
}
