/**
 * Tests for gui/src/viewport/ProbeSystem.tsx
 *
 * Covers:
 *   Step 13: createProbeSystem factory + addProbe → scene.add + store entry
 *   Step 15: pickAndAddProbe → raycasts, reads object.name/faceIndex/point → addProbe (or no-op on miss)
 *   Step 17: resampleAll → update or markStale, grey marker on stale
 *   Step 19: removeProbe disposes marker; dispose() tears down all markers
 *   Step 21: <ProbePopup> renders rows with values, stale styling, re-pin/remove callbacks
 *
 * Uses the inline vi.mock('three') pattern (selection.test.ts / meshManager.test.ts).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createRoot } from 'solid-js';
import { createProbeStore } from '../../stores/probeStore';
import type { ProbeSample } from '../../stores/probeStore';

// ---------------------------------------------------------------------------
// Three.js mock
// ---------------------------------------------------------------------------

const mockSceneAdd = vi.fn();
const mockSceneRemove = vi.fn();

const mockRaycasterSetFromCamera = vi.fn();
const mockRaycasterIntersectObjects = vi.fn((): any[] => []);
let lastRaycasterInstance: any = null;

const mockSphereGeometryInstances: any[] = [];
const mockMeshBasicMaterialInstances: any[] = [];
const mockMeshInstances: any[] = [];

vi.mock('three', () => {
  class MockRaycaster {
    setFromCamera = mockRaycasterSetFromCamera;
    intersectObjects = mockRaycasterIntersectObjects;
    firstHitOnly = false;
    constructor() {
      lastRaycasterInstance = this;
    }
  }

  class MockSphereGeometry {
    dispose = vi.fn();
    constructor(..._args: any[]) {
      mockSphereGeometryInstances.push(this);
    }
  }

  // Mirrors real THREE.Color: the material stores a Color instance, not a raw number.
  class MockColor {
    value: number;
    set = vi.fn((v: number) => { this.value = v; });
    constructor(v?: number) { this.value = v ?? 0; }
  }

  class MockMeshBasicMaterial {
    color: MockColor;
    opacity: number = 1;
    transparent: boolean = false;
    dispose = vi.fn();
    constructor(opts?: any) {
      this.color = new MockColor(opts?.color);
      this.opacity = opts?.opacity ?? 1;
      this.transparent = opts?.transparent ?? false;
      mockMeshBasicMaterialInstances.push(this);
    }
  }

  class MockMesh {
    geometry: any;
    material: any;
    position = { set: vi.fn(), x: 0, y: 0, z: 0 };
    name: string = '';
    constructor(geometry?: any, material?: any) {
      this.geometry = geometry;
      this.material = material;
      mockMeshInstances.push(this);
    }
  }

  class MockScene {
    children: any[] = [];
    add = mockSceneAdd;
    remove = mockSceneRemove;
  }

  class MockVector2 {
    x = 0; y = 0;
    constructor(x?: number, y?: number) { this.x = x ?? 0; this.y = y ?? 0; }
  }

  return {
    Raycaster: MockRaycaster,
    SphereGeometry: MockSphereGeometry,
    MeshBasicMaterial: MockMeshBasicMaterial,
    Mesh: MockMesh,
    Scene: MockScene,
    Vector2: MockVector2,
  };
});

// ---------------------------------------------------------------------------
// Imports after vi.mock so they get the mocked modules
// ---------------------------------------------------------------------------

import { createProbeSystem, ProbePopup } from '../../viewport/ProbeSystem';
import { Scene } from 'three';

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

beforeEach(() => {
  vi.clearAllMocks();
  mockSphereGeometryInstances.length = 0;
  mockMeshBasicMaterialInstances.length = 0;
  mockMeshInstances.length = 0;
  lastRaycasterInstance = null;
  // Reset default raycaster to return no intersections
  mockRaycasterIntersectObjects.mockReturnValue([]);
});

function makeSample(overrides?: Partial<ProbeSample>): ProbeSample {
  return {
    displacement: [0.1, 0.2, 0.3],
    vonMises: 42.0,
    scalars: { vonMises: 42.0 },
    vectors: {},
    ...overrides,
  };
}

function makeMeshManager(overrides?: Partial<{
  sampleProbe: (e: string, f: number, b: any) => ProbeSample | null;
  computeBarycentric: (e: string, f: number, p: any) => [number, number, number] | null;
}>) {
  return {
    sampleProbe: vi.fn<any>().mockReturnValue(makeSample()),
    computeBarycentric: vi.fn<any>().mockReturnValue([0.2, 0.3, 0.5]),
    ...overrides,
  };
}

function makeSetup(meshManagerOverrides?: Parameters<typeof makeMeshManager>[0]) {
  const scene = new Scene();
  const camera: any = {};
  const domElement: any = {
    getBoundingClientRect: vi.fn().mockReturnValue({ left: 0, top: 0, width: 100, height: 100 }),
  };
  const meshMap = new Map<string, any>();
  const getMeshes = () => meshMap;
  const meshManager = makeMeshManager(meshManagerOverrides);
  const store = createProbeStore();

  const system = createProbeSystem({ scene, camera, domElement, getMeshes, meshManager: meshManager as any, store });
  vi.clearAllMocks(); // Clear constructor calls from createProbeSystem itself
  return { scene, camera, domElement, getMeshes, meshMap, meshManager, store, system };
}

// ---------------------------------------------------------------------------
// Step 13: factory + addProbe
// ---------------------------------------------------------------------------

describe('createProbeSystem — factory', () => {
  it('returns a controller with the expected methods', () => {
    createRoot((dispose) => {
      const { system } = makeSetup();
      expect(typeof system.addProbe).toBe('function');
      expect(typeof system.pickAndAddProbe).toBe('function');
      expect(typeof system.resampleAll).toBe('function');
      expect(typeof system.removeProbe).toBe('function');
      expect(typeof system.refreshMarkers).toBe('function');
      expect(typeof system.dispose).toBe('function');
      dispose();
    });
  });

  it('sets raycaster.firstHitOnly = true on construction', () => {
    createRoot((dispose) => {
      makeSetup(); // Raycaster is created in createProbeSystem
      expect(lastRaycasterInstance).not.toBeNull();
      expect(lastRaycasterInstance!.firstHitOnly).toBe(true);
      dispose();
    });
  });
});

describe('createProbeSystem — addProbe', () => {
  it('addProbe calls sampleProbe, stores the probe in the store, and calls scene.add', () => {
    createRoot((dispose) => {
      const { system, meshManager, store } = makeSetup();

      system.addProbe('Body', 0, [0.2, 0.3, 0.5]);

      expect(meshManager.sampleProbe).toHaveBeenCalledWith('Body', 0, [0.2, 0.3, 0.5]);
      expect(store.state.probes).toHaveLength(1);
      expect(store.state.probes[0].entity_path).toBe('Body');
      expect(store.state.probes[0].face_id).toBe(0);
      expect(store.state.probes[0].barycentric_uv).toEqual([0.2, 0.3, 0.5]);
      expect(mockSceneAdd).toHaveBeenCalledTimes(1);
      dispose();
    });
  });

  it('addProbe creates a sphere marker (SphereGeometry + MeshBasicMaterial)', () => {
    createRoot((dispose) => {
      const { system } = makeSetup();
      system.addProbe('Body', 0, [0.2, 0.3, 0.5]);
      expect(mockSphereGeometryInstances).toHaveLength(1);
      expect(mockMeshBasicMaterialInstances).toHaveLength(1);
      dispose();
    });
  });

  it('addProbe returns the probe id', () => {
    createRoot((dispose) => {
      const { system } = makeSetup();
      const id = system.addProbe('Body', 0, [0.2, 0.3, 0.5]);
      expect(typeof id).toBe('string');
      dispose();
    });
  });

  it('two addProbe calls add two markers and two store entries', () => {
    createRoot((dispose) => {
      const { system, store } = makeSetup();
      system.addProbe('A', 0, [1, 0, 0]);
      system.addProbe('B', 1, [0, 1, 0]);
      expect(store.state.probes).toHaveLength(2);
      expect(mockSceneAdd).toHaveBeenCalledTimes(2);
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// Step 15: pickAndAddProbe
// ---------------------------------------------------------------------------

describe('createProbeSystem — pickAndAddProbe', () => {
  it('on a raycast hit: calls computeBarycentric then addProbe, yielding one stored probe', () => {
    createRoot((dispose) => {
      const { system, meshManager, store, domElement } = makeSetup();

      // Simulate a hit
      const mockHitMesh = { name: 'HitEntity' };
      const mockHitPoint = { x: 0.3, y: 0.5, z: 0 };
      mockRaycasterIntersectObjects.mockReturnValue([{
        object: mockHitMesh,
        faceIndex: 2,
        point: mockHitPoint,
      }]);

      const fakeEvent = { clientX: 30, clientY: 40 } as MouseEvent;
      system.pickAndAddProbe(fakeEvent);

      expect(mockRaycasterSetFromCamera).toHaveBeenCalledTimes(1);
      expect(meshManager.computeBarycentric).toHaveBeenCalledWith('HitEntity', 2, mockHitPoint);
      expect(store.state.probes).toHaveLength(1);
      expect(store.state.probes[0].entity_path).toBe('HitEntity');
      void domElement;
      dispose();
    });
  });

  it('on a raycast miss (empty intersections): no probe is added', () => {
    createRoot((dispose) => {
      const { system, store } = makeSetup();
      mockRaycasterIntersectObjects.mockReturnValue([]);
      system.pickAndAddProbe({ clientX: 50, clientY: 50 } as MouseEvent);
      expect(store.state.probes).toHaveLength(0);
      dispose();
    });
  });

  it('when computeBarycentric returns null: no probe is added', () => {
    createRoot((dispose) => {
      const { system, store } = makeSetup({ computeBarycentric: vi.fn().mockReturnValue(null) });
      mockRaycasterIntersectObjects.mockReturnValue([{
        object: { name: 'Hit' }, faceIndex: 0, point: { x: 0, y: 0, z: 0 },
      }]);
      system.pickAndAddProbe({ clientX: 10, clientY: 10 } as MouseEvent);
      expect(store.state.probes).toHaveLength(0);
      dispose();
    });
  });

  it('firstHitOnly flag is set on the internal Raycaster', () => {
    createRoot((dispose) => {
      makeSetup();
      expect(lastRaycasterInstance?.firstHitOnly).toBe(true);
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// Step 17: resampleAll
// ---------------------------------------------------------------------------

describe('createProbeSystem — resampleAll', () => {
  it('on successful resample: updateSample is called and stale stays false', () => {
    createRoot((dispose) => {
      const newSample = makeSample({ vonMises: 99.0 });
      const { system, meshManager, store } = makeSetup({
        sampleProbe: vi.fn().mockReturnValue(newSample),
      });

      const id = system.addProbe('Body', 0, [1 / 3, 1 / 3, 1 / 3]);
      vi.clearAllMocks();

      meshManager.sampleProbe.mockReturnValue(newSample);
      system.resampleAll();

      expect(store.state.probes[0].stale).toBe(false);
      expect(store.state.probes[0].sample).toEqual(newSample);
      void id;
      dispose();
    });
  });

  it('when sampleProbe returns null: probe is marked stale, last-known sample is preserved', () => {
    createRoot((dispose) => {
      const originalSample = makeSample({ vonMises: 42.0 });
      const { system, meshManager, store } = makeSetup({
        sampleProbe: vi.fn().mockReturnValue(originalSample),
      });

      system.addProbe('Body', 0, [1 / 3, 1 / 3, 1 / 3]);
      expect(store.state.probes[0].sample).toEqual(originalSample);

      // Next resample returns null → stale
      meshManager.sampleProbe.mockReturnValue(null);
      system.resampleAll();

      expect(store.state.probes[0].stale).toBe(true);
      // Last-known sample is preserved
      expect(store.state.probes[0].sample).toEqual(originalSample);
      dispose();
    });
  });

  it('stale probe has its marker greyed (color.set to STALE color + opacity < 1)', () => {
    createRoot((dispose) => {
      const { system, meshManager } = makeSetup({
        sampleProbe: vi.fn().mockReturnValue(makeSample()),
      });

      system.addProbe('Body', 0, [1 / 3, 1 / 3, 1 / 3]);
      // The marker material instance is the last created one
      const markerMaterial = mockMeshBasicMaterialInstances[mockMeshBasicMaterialInstances.length - 1];
      // Capture the Color instance BEFORE resampleAll so we can assert it was mutated in place
      const colorObj = markerMaterial.color;

      meshManager.sampleProbe.mockReturnValue(null);
      system.resampleAll();

      // Color must be set via .set() on the existing Color instance — not replaced
      expect(colorObj.set).toHaveBeenCalledWith(0x888888); // MARKER_COLOR_STALE
      expect(markerMaterial.color).toBe(colorObj);          // same instance, mutated in place
      // Opacity must also be reduced
      expect(markerMaterial.opacity).toBeLessThan(1);
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// Step 19: removeProbe and dispose
// ---------------------------------------------------------------------------

describe('createProbeSystem — removeProbe / dispose', () => {
  it('removeProbe(id) removes from store and removes the marker from scene', () => {
    createRoot((dispose) => {
      const { system, store } = makeSetup();
      const id = system.addProbe('Body', 0, [0.2, 0.3, 0.5]);
      vi.clearAllMocks();

      system.removeProbe(id!);

      expect(store.state.probes).toHaveLength(0);
      expect(mockSceneRemove).toHaveBeenCalledTimes(1);
      dispose();
    });
  });

  it('removeProbe with disposal: geometry.dispose and material.dispose are called', () => {
    createRoot((dispose) => {
      const { system } = makeSetup();
      const id = system.addProbe('Body', 0, [0.2, 0.3, 0.5]);
      const geo = mockSphereGeometryInstances[mockSphereGeometryInstances.length - 1];
      const mat = mockMeshBasicMaterialInstances[mockMeshBasicMaterialInstances.length - 1];

      system.removeProbe(id!);

      expect(geo.dispose).toHaveBeenCalled();
      expect(mat.dispose).toHaveBeenCalled();
      dispose();
    });
  });

  it('dispose() removes all markers from scene and disposes their resources', () => {
    createRoot((dispose) => {
      const { system } = makeSetup();
      system.addProbe('A', 0, [1, 0, 0]);
      system.addProbe('B', 1, [0, 1, 0]);
      vi.clearAllMocks();

      system.dispose();

      expect(mockSceneRemove).toHaveBeenCalledTimes(2);
      // All geometries and materials should be disposed
      for (const geo of mockSphereGeometryInstances) {
        expect(geo.dispose).toHaveBeenCalled();
      }
      for (const mat of mockMeshBasicMaterialInstances) {
        expect(mat.dispose).toHaveBeenCalled();
      }
      dispose();
    });
  });
});

// ---------------------------------------------------------------------------
// Step 21: <ProbePopup>
// ---------------------------------------------------------------------------

describe('<ProbePopup>', () => {
  it('renders no rows when store has no probes', () => {
    const store = createProbeStore();
    render(() => <ProbePopup store={store} onRemove={vi.fn()} onRepin={vi.fn()} />);
    const rows = document.querySelectorAll('[data-testid="probe-row"]');
    expect(rows).toHaveLength(0);
  });

  it('renders one row per probe with entity_path visible', () => {
    const store = createProbeStore();
    store.addProbe({
      entity_path: 'Body.face',
      face_id: 0,
      barycentric_uv: [0.2, 0.3, 0.5],
      sample: makeSample({ vonMises: 42.0 }),
    });
    render(() => <ProbePopup store={store} onRemove={vi.fn()} onRepin={vi.fn()} />);
    expect(screen.getByText(/Body\.face/)).toBeTruthy();
  });

  it('renders vonMises value when sample is present', () => {
    const store = createProbeStore();
    store.addProbe({
      entity_path: 'A',
      face_id: 0,
      barycentric_uv: [1, 0, 0],
      sample: makeSample({ vonMises: 42.0, displacement: [0.1, 0.2, 0.3] }),
    });
    render(() => <ProbePopup store={store} onRemove={vi.fn()} onRepin={vi.fn()} />);
    const vonMisesEl = screen.getByTestId('probe-vonmises');
    expect(vonMisesEl.textContent).toContain('42.000');
  });

  it('stale probe row has a stale marker class', () => {
    const store = createProbeStore();
    const id = store.addProbe({
      entity_path: 'Stale',
      face_id: 0,
      barycentric_uv: [1, 0, 0],
      sample: makeSample(),
    });
    store.markStale(id);

    render(() => <ProbePopup store={store} onRemove={vi.fn()} onRepin={vi.fn()} />);
    const staleEl = document.querySelector('.probe-stale');
    expect(staleEl).not.toBeNull();
  });

  it('re-pin button fires onRepin(id) when clicked on a stale probe', () => {
    const store = createProbeStore();
    const id = store.addProbe({
      entity_path: 'X',
      face_id: 0,
      barycentric_uv: [1, 0, 0],
      sample: makeSample(),
    });
    store.markStale(id);

    const onRepin = vi.fn();
    render(() => <ProbePopup store={store} onRemove={vi.fn()} onRepin={onRepin} />);
    const repinBtn = screen.getByTestId('probe-repin');
    fireEvent.click(repinBtn);
    expect(onRepin).toHaveBeenCalledWith(id);
  });

  it('remove button fires onRemove(id) when clicked', () => {
    const store = createProbeStore();
    const id = store.addProbe({
      entity_path: 'Y',
      face_id: 0,
      barycentric_uv: [0, 1, 0],
      sample: makeSample(),
    });

    const onRemove = vi.fn();
    render(() => <ProbePopup store={store} onRemove={onRemove} onRepin={vi.fn()} />);
    const removeBtn = screen.getByTestId('probe-remove');
    fireEvent.click(removeBtn);
    expect(onRemove).toHaveBeenCalledWith(id);
  });

  it('renders two rows for two probes', () => {
    const store = createProbeStore();
    store.addProbe({ entity_path: 'A', face_id: 0, barycentric_uv: [1, 0, 0], sample: makeSample() });
    store.addProbe({ entity_path: 'B', face_id: 1, barycentric_uv: [0, 1, 0], sample: makeSample() });
    render(() => <ProbePopup store={store} onRemove={vi.fn()} onRepin={vi.fn()} />);
    const rows = document.querySelectorAll('[data-testid="probe-row"]');
    expect(rows).toHaveLength(2);
  });
});
