/**
 * ProbeSystem — point-on-mesh FEA probe controller and UI panel.
 *
 * createProbeSystem(options) owns its own Raycaster (mirroring createSelection).
 * It does NOT modify selection.ts.
 *
 * API:
 *   addProbe(entityPath, faceId, bary) — sample + store + scene marker
 *   pickAndAddProbe(event) — raycast → computeBarycentric → addProbe
 *   resampleAll() — re-sample all probes; update or markStale + grey marker
 *   removeProbe(id) — remove from store + scene + dispose
 *   refreshMarkers() — reconcile marker Map with current store probes
 *   dispose() — tear down all markers and resources
 *
 * <ProbePopup store={store} onRemove={fn} onRepin={fn} /> — thin SolidJS panel.
 */
import { For, Show } from 'solid-js';
import { Raycaster, SphereGeometry, MeshBasicMaterial, Mesh, Vector2 } from 'three';
import type { Scene, PerspectiveCamera } from 'three';
import type { ProbeStore, BarycentricUV, ProbeSample } from '../stores/probeStore';
import type { MeshManagerContext } from './meshManager';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ProbeSystemOptions {
  scene: Scene;
  camera: PerspectiveCamera;
  domElement: HTMLElement;
  getMeshes: () => Map<string, Mesh>;
  meshManager: MeshManagerContext;
  store: ProbeStore;
}

export interface ProbeSystemContext {
  addProbe(entityPath: string, faceId: number, bary: BarycentricUV): string | null;
  pickAndAddProbe(event: MouseEvent): void;
  resampleAll(): void;
  removeProbe(id: string): void;
  refreshMarkers(): void;
  dispose(): void;
}

// Marker colours
const MARKER_COLOR_ACTIVE = 0xff4444;  // red-ish for active probes
const MARKER_COLOR_STALE  = 0x888888;  // grey for stale probes
const MARKER_OPACITY_STALE = 0.4;

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

export function createProbeSystem(options: ProbeSystemOptions): ProbeSystemContext {
  const { scene, camera, domElement, getMeshes, meshManager, store } = options;

  // Own raycaster — does NOT share with selection.ts
  const raycaster = new Raycaster();
  (raycaster as any).firstHitOnly = true;

  let cachedRect: DOMRect | null = null;

  function getRect(): DOMRect {
    if (cachedRect === null) {
      cachedRect = domElement.getBoundingClientRect();
    }
    return cachedRect;
  }

  // Map from probe id → Three.js Mesh marker (+ its geometry/material for disposal)
  const markerMap = new Map<string, Mesh>();

  // ---------------------------------------------------------------------------
  // Marker helpers
  // ---------------------------------------------------------------------------

  function computeWorldPosition(
    entityPath: string,
    faceId: number,
    bary: BarycentricUV,
  ): { x: number; y: number; z: number } {
    // Try to compute from the live mesh geometry
    const meshes = getMeshes();
    const mesh = meshes.get(entityPath);
    if (mesh) {
      const geo = mesh.geometry as any;
      const posAttr = geo.getAttribute?.('position');
      const indexAttr = geo.index;
      if (posAttr && indexAttr) {
        const pos = posAttr.array as Float32Array;
        const idx = indexAttr.array as Uint32Array | number[];
        const base = 3 * faceId;
        if (base + 2 < idx.length) {
          const ia = idx[base], ib = idx[base + 1], ic = idx[base + 2];
          const [u, v, w] = bary;
          return {
            x: u * pos[ia * 3]     + v * pos[ib * 3]     + w * pos[ic * 3],
            y: u * pos[ia * 3 + 1] + v * pos[ib * 3 + 1] + w * pos[ic * 3 + 1],
            z: u * pos[ia * 3 + 2] + v * pos[ib * 3 + 2] + w * pos[ic * 3 + 2],
          };
        }
      }
    }
    return { x: 0, y: 0, z: 0 };
  }

  function createMarker(
    entityPath: string,
    faceId: number,
    bary: BarycentricUV,
    stale: boolean,
  ): Mesh {
    const geo = new SphereGeometry(0.02, 8, 8);
    const mat = new MeshBasicMaterial({
      color: stale ? MARKER_COLOR_STALE : MARKER_COLOR_ACTIVE,
      transparent: true,
      opacity: stale ? MARKER_OPACITY_STALE : 1.0,
    });
    const marker = new Mesh(geo, mat);
    const pos = computeWorldPosition(entityPath, faceId, bary);
    marker.position.set(pos.x, pos.y, pos.z);
    return marker;
  }

  function disposeMarker(marker: Mesh): void {
    const geo = marker.geometry as SphereGeometry;
    const mat = marker.material as MeshBasicMaterial;
    geo.dispose();
    mat.dispose();
  }

  function updateMarkerStyle(marker: Mesh, stale: boolean): void {
    const mat = marker.material as MeshBasicMaterial;
    mat.color = stale ? MARKER_COLOR_STALE as any : MARKER_COLOR_ACTIVE as any;
    mat.opacity = stale ? MARKER_OPACITY_STALE : 1.0;
    mat.transparent = stale;
  }

  // ---------------------------------------------------------------------------
  // addProbe
  // ---------------------------------------------------------------------------

  function addProbe(entityPath: string, faceId: number, bary: BarycentricUV): string | null {
    const sample = meshManager.sampleProbe(entityPath, faceId, bary);
    const id = store.addProbe({ entity_path: entityPath, face_id: faceId, barycentric_uv: bary, sample });

    const marker = createMarker(entityPath, faceId, bary, false);
    markerMap.set(id, marker);
    scene.add(marker);

    return id;
  }

  // ---------------------------------------------------------------------------
  // pickAndAddProbe
  // ---------------------------------------------------------------------------

  function pickAndAddProbe(event: MouseEvent): void {
    const rect = getRect();
    const ndc = new Vector2(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      -((event.clientY - rect.top) / rect.height) * 2 + 1,
    );
    raycaster.setFromCamera(ndc, camera);

    const meshes = Array.from(getMeshes().values());
    const hits = raycaster.intersectObjects(meshes);
    if (hits.length === 0) return;

    const hit = hits[0];
    const entityPath = (hit.object as Mesh).name;
    const faceId = hit.faceIndex ?? 0;
    const point = hit.point as { x: number; y: number; z: number };

    const bary = meshManager.computeBarycentric(entityPath, faceId, point);
    if (bary === null) return;

    addProbe(entityPath, faceId, bary);
  }

  // ---------------------------------------------------------------------------
  // resampleAll
  // ---------------------------------------------------------------------------

  function resampleAll(): void {
    for (const probe of store.state.probes) {
      const sample = meshManager.sampleProbe(probe.entity_path, probe.face_id, probe.barycentric_uv);
      if (sample !== null) {
        store.updateSample(probe.id, sample);
      } else {
        store.markStale(probe.id);
      }
    }
    refreshMarkers();
  }

  // ---------------------------------------------------------------------------
  // refreshMarkers
  // ---------------------------------------------------------------------------

  function refreshMarkers(): void {
    // Restyle existing markers based on current stale state
    for (const probe of store.state.probes) {
      const marker = markerMap.get(probe.id);
      if (marker) {
        updateMarkerStyle(marker, probe.stale);
      }
    }
    // Remove markers for probes no longer in the store
    for (const [id, marker] of markerMap) {
      if (!store.state.probes.find((p) => p.id === id)) {
        scene.remove(marker);
        disposeMarker(marker);
        markerMap.delete(id);
      }
    }
  }

  // ---------------------------------------------------------------------------
  // removeProbe
  // ---------------------------------------------------------------------------

  function removeProbe(id: string): void {
    const marker = markerMap.get(id);
    if (marker) {
      scene.remove(marker);
      disposeMarker(marker);
      markerMap.delete(id);
    }
    store.removeProbe(id);
  }

  // ---------------------------------------------------------------------------
  // dispose
  // ---------------------------------------------------------------------------

  function dispose(): void {
    for (const [, marker] of markerMap) {
      scene.remove(marker);
      disposeMarker(marker);
    }
    markerMap.clear();
    store.clear();
    cachedRect = null;
  }

  return { addProbe, pickAndAddProbe, resampleAll, removeProbe, refreshMarkers, dispose };
}

// ---------------------------------------------------------------------------
// <ProbePopup> SolidJS component
// ---------------------------------------------------------------------------

export interface ProbePopupProps {
  store: ProbeStore;
  onRemove: (id: string) => void;
  onRepin: (id: string) => void;
}

function formatVec(v: [number, number, number]): string {
  return `(${v[0].toFixed(3)}, ${v[1].toFixed(3)}, ${v[2].toFixed(3)})`;
}

export function ProbePopup(props: ProbePopupProps) {
  return (
    <div class="probe-popup" data-testid="probe-popup">
      <For each={props.store.state.probes}>
        {(probe) => (
          <div
            class={`probe-row${probe.stale ? ' probe-stale' : ''}`}
            data-testid="probe-row"
          >
            <span class="probe-entity">{probe.entity_path}</span>
            <Show when={probe.sample !== null}>
              <Show when={probe.sample?.displacement !== null}>
                <span class="probe-displacement">
                  Δ {formatVec(probe.sample!.displacement!)}
                </span>
              </Show>
              <Show when={probe.sample?.vonMises !== null}>
                <span class="probe-vonmises" data-testid="probe-vonmises">
                  σ_vm {probe.sample!.vonMises!.toFixed(3)}
                </span>
              </Show>
            </Show>
            <Show when={probe.stale}>
              <button
                class="probe-repin-btn"
                data-testid="probe-repin"
                onClick={() => props.onRepin(probe.id)}
              >
                Re-pin
              </button>
            </Show>
            <button
              class="probe-remove-btn"
              data-testid="probe-remove"
              onClick={() => props.onRemove(probe.id)}
            >
              ×
            </button>
          </div>
        )}
      </For>
    </div>
  );
}
