/**
 * Wire manager for tensegrity member-type viewport styling (T0b).
 *
 * Renders `TensegrityWireData` arrays as fat-line objects in the Three.js scene,
 * with distinct colour and linewidth per member kind (struts vs cables).
 *
 * Fat-lines (`LineSegments2` + `LineMaterial`) are used because WebGL ignores
 * `LineBasicMaterial.linewidth`; `LineMaterial` respects `linewidth` in screen px
 * but requires a `resolution` uniform updated on viewport resize.
 *
 * Design decisions:
 * - STRUT_COLOR: Catppuccin red `#f38ba8` — compression members (heavy, structural).
 * - CABLE_COLOR: Catppuccin blue `#89b4fa` — tension members (thin, light).
 * - STRUT_LINEWIDTH > CABLE_LINEWIDTH to match PRD "heavy vs thin" intent.
 */

import { LineSegments2, LineSegmentsGeometry, LineMaterial } from 'three/addons';
import type { Scene } from 'three';
import type { TensegrityWireData } from '../types';

// ─── Styling constants (Catppuccin Mocha palette) ─────────────────────────────

/** Catppuccin Mocha Red — compression struts (heavy, primary structural element). */
const STRUT_COLOR = '#f38ba8';
/** Catppuccin Mocha Blue — tension cables (thin, secondary tensioning element). */
const CABLE_COLOR = '#89b4fa';

/** Screen-space linewidth in pixels for compression struts. */
const STRUT_LINEWIDTH = 3;
/** Screen-space linewidth in pixels for tension cables. */
const CABLE_LINEWIDTH = 1;

// ─── Types ─────────────────────────────────────────────────────────────────────

interface WireGroup {
  lineSegments: LineSegments2;
  geometry: LineSegmentsGeometry;
  material: LineMaterial;
}

export interface WireManagerContext {
  /**
   * Sync the scene with the given wire list. Groups wires by kind and adds/replaces
   * per-kind fat-line objects. Call with `[]` to remove all wire objects.
   */
  sync: (wires: TensegrityWireData[]) => void;
  /**
   * Update the LineMaterial resolution uniform for all current wire groups.
   * Must be called on viewport resize (fat-lines require this to compute screen-space width).
   */
  setResolution: (width: number, height: number) => void;
  /**
   * Remove all wire objects from the scene and dispose their geometry and material.
   */
  dispose: () => void;
}

// ─── Factory ──────────────────────────────────────────────────────────────────

/**
 * Create a wire manager bound to the given Three.js scene.
 *
 * @param scene - The Three.js scene to add/remove wire objects to/from.
 */
export function createWireManager(scene: Scene): WireManagerContext {
  /** Map from kind string to the current WireGroup object in the scene. */
  const activeGroups = new Map<string, WireGroup>();

  /** Remove a WireGroup from the scene and dispose its resources. */
  function disposeGroup(group: WireGroup): void {
    scene.remove(group.lineSegments);
    group.geometry.dispose();
    group.material.dispose();
  }

  function sync(wires: TensegrityWireData[]): void {
    // Group wires by kind.
    const byKind = new Map<string, TensegrityWireData[]>();
    for (const wire of wires) {
      const list = byKind.get(wire.kind);
      if (list) {
        list.push(wire);
      } else {
        byKind.set(wire.kind, [wire]);
      }
    }

    // Determine which kinds are no longer present → remove their groups.
    for (const [kind, group] of activeGroups) {
      if (!byKind.has(kind)) {
        disposeGroup(group);
        activeGroups.delete(kind);
      }
    }

    // Build/replace a LineSegments2 per kind.
    for (const [kind, kindWires] of byKind) {
      // Pack positions as flat [x1,y1,z1, x2,y2,z2, ...] array.
      const positions: number[] = [];
      for (const w of kindWires) {
        positions.push(w.x1, w.y1, w.z1, w.x2, w.y2, w.z2);
      }

      const color = kind === 'strut' ? STRUT_COLOR : CABLE_COLOR;
      const linewidth = kind === 'strut' ? STRUT_LINEWIDTH : CABLE_LINEWIDTH;

      // Dispose any previously-existing group for this kind.
      const existing = activeGroups.get(kind);
      if (existing) {
        disposeGroup(existing);
        activeGroups.delete(kind);
      }

      // Create new fat-line group.
      const geometry = new LineSegmentsGeometry();
      geometry.setPositions(positions);

      const material = new LineMaterial({ color, linewidth });

      const lineSegments = new LineSegments2(geometry, material);
      scene.add(lineSegments);

      activeGroups.set(kind, { lineSegments, geometry, material });
    }
  }

  function setResolution(width: number, height: number): void {
    for (const group of activeGroups.values()) {
      group.material.resolution.set(width, height);
    }
  }

  function dispose(): void {
    for (const group of activeGroups.values()) {
      disposeGroup(group);
    }
    activeGroups.clear();
  }

  return { sync, setResolution, dispose };
}
