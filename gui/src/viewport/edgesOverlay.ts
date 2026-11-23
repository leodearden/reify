/**
 * The edges-in-black overlay: one black `LineSegments` of feature edges per
 * shown mesh, kept in its own Group so the meshes' recursive raycasts never hit
 * a line. Nothing is constructed while the overlay is disabled.
 */
import {
  EdgesGeometry,
  Group,
  LineBasicMaterial,
  LineSegments,
  type Mesh,
  type Scene,
} from 'three';
import { EDGES_OVERLAY_RENDER_ORDER } from './renderOrder';

/**
 * Crease angle in degrees: an edge is drawn where the normals of its two faces
 * differ by at least this much — CAD feature edges, not tessellation facets.
 */
export const EDGES_THRESHOLD_ANGLE_DEG = 30;

export const EDGES_COLOR = 0x000000;

export interface EdgesOverlay {
  /** Start drawing, building an overlay for every entry of `shown`. No-op when already enabled. */
  enable(shown: ReadonlyMap<string, Mesh>): void;
  /** Stop drawing and dispose every overlay. No-op when already disabled. */
  disable(): void;
  /** Replace the entity's overlay with one built from `mesh`'s current geometry. No-op while disabled. */
  rebuild(entityPath: string, mesh: Mesh): void;
  /** Drop the entity's overlay, if any. */
  remove(entityPath: string): void;
  /** A copy of the live overlays, keyed by entity path; empty while disabled. */
  overlays(): Map<string, LineSegments>;
}

interface LiveOverlay {
  group: Group;
  material: LineBasicMaterial;
  lines: Map<string, LineSegments>;
}

export function createEdgesOverlay(scene: Scene): EdgesOverlay {
  let live: LiveOverlay | null = null;

  function detach(state: LiveOverlay, entityPath: string): void {
    const line = state.lines.get(entityPath);
    if (!line) return;
    state.group.remove(line);
    line.geometry.dispose();
    state.lines.delete(entityPath);
  }

  function rebuild(entityPath: string, mesh: Mesh): void {
    if (!live) return;
    detach(live, entityPath);
    const line = new LineSegments(
      new EdgesGeometry(mesh.geometry, EDGES_THRESHOLD_ANGLE_DEG),
      live.material,
    );
    line.name = `edges:${entityPath}`;
    line.renderOrder = EDGES_OVERLAY_RENDER_ORDER;
    live.group.add(line);
    live.lines.set(entityPath, line);
  }

  function enable(shown: ReadonlyMap<string, Mesh>): void {
    if (live) return;
    const group = new Group();
    group.name = 'edgesOverlayGroup';
    scene.add(group);
    live = { group, material: new LineBasicMaterial({ color: EDGES_COLOR }), lines: new Map() };
    for (const [entityPath, mesh] of shown) rebuild(entityPath, mesh);
  }

  function disable(): void {
    if (!live) return;
    for (const entityPath of [...live.lines.keys()]) detach(live, entityPath);
    scene.remove(live.group);
    live.material.dispose();
    live = null;
  }

  function remove(entityPath: string): void {
    if (live) detach(live, entityPath);
  }

  function overlays(): Map<string, LineSegments> {
    return new Map(live?.lines ?? []);
  }

  return { enable, disable, rebuild, remove, overlays };
}
