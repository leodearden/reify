/**
 * End-to-end probe wiring through the REAL Viewport: selection's click
 * classifier, meshManager (BVH raycast, barycentric, sampleProbe), ProbeSystem,
 * probeStore, ProbePopup and feaModeStore all run unmocked on real three.js.
 *
 * Only what jsdom cannot run is mocked: scene.ts (WebGLRenderer needs a GL
 * context — the mock hands back a real Scene and PerspectiveCamera), controls.ts
 * (OrbitControls calls setPointerCapture), wireManager.ts (see its mock) and the
 * Tauri bridge.
 *
 * Fixture: one triangle A(-1,-1,0) B(1,-1,0) C(0,1,0) with vonMises [200,200,400],
 * a field linear in y (300 + 100·y). The mount auto-fit looks down -z at the bbox
 * centre (0,0,0), which sits at barycentric (0.25, 0.25, 0.5), so a click at the
 * canvas centre probes exactly 300.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import type { Object3D, Scene } from 'three';
import type { MeshData } from '../../types';
import { createFeaModeStore } from '../../stores';
import type { FeaModeStore } from '../../stores';

interface RendererStub {
  render: ReturnType<typeof vi.fn>;
  dispose: ReturnType<typeof vi.fn>;
  setSize: ReturnType<typeof vi.fn>;
  domElement: HTMLCanvasElement;
}

let lastScene: Scene | null = null;
let lastRenderer: RendererStub | null = null;

vi.mock('../../viewport/scene', async () => {
  const { Scene, PerspectiveCamera } = await import('three');
  return {
    createScene: vi.fn((canvas: HTMLCanvasElement) => {
      const scene = new Scene();
      const camera = new PerspectiveCamera(50, 1, 0.01, 1000);
      camera.position.set(0, 0, 5);
      camera.updateMatrixWorld();
      const renderer: RendererStub = {
        render: vi.fn(),
        dispose: vi.fn(),
        setSize: vi.fn(),
        domElement: canvas,
      };
      lastScene = scene;
      lastRenderer = renderer;
      return {
        scene,
        camera,
        renderer,
        resize: vi.fn(),
        adjustClipping: vi.fn(),
        fitHelpers: vi.fn(),
        grid: { visible: true },
        axes: { visible: true },
        axisLabels: { visible: true },
        disposeAxisLabels: vi.fn(),
      };
    }),
  };
});

vi.mock('../../viewport/controls', async () => {
  const { Vector3 } = await import('three');
  return {
    createControls: vi.fn(() => ({
      controls: {
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        target: new Vector3(),
      },
      update: vi.fn(),
      dispose: vi.fn(),
    })),
  };
});

// wireManager imports the 'three/addons' barrel, whose lottie module calls canvas.getContext at load.
vi.mock('../../viewport/wireManager', () => ({
  createWireManager: vi.fn(() => ({ sync: vi.fn(), dispose: vi.fn(), setResolution: vi.fn() })),
}));

vi.mock('../../bridge', () => ({
  subscribeFeaCaseToStore: vi.fn(() => Promise.resolve(() => {})),
  setActiveFeaCase: vi.fn(() => Promise.resolve()),
}));

let rafCallbacks: FrameRequestCallback[] = [];
vi.stubGlobal(
  'requestAnimationFrame',
  vi.fn((cb: FrameRequestCallback) => {
    rafCallbacks.push(cb);
    return rafCallbacks.length;
  }),
);
vi.stubGlobal('cancelAnimationFrame', vi.fn());
vi.stubGlobal(
  'ResizeObserver',
  class {
    observe = vi.fn();
    unobserve = vi.fn();
    disconnect = vi.fn();
  },
);

import { Viewport } from '../../viewport/Viewport';

beforeEach(() => {
  rafCallbacks = [];
  lastScene = null;
  lastRenderer = null;
});

const VALUE_300 = /\b300(?:\.0+)?\b/;

type Vec3 = [number, number, number];

/** The fixture triangle, optionally shifted by `translate` and carrying a uniform FEA `displacement`. */
function bracketMesh(
  opts: { vonMises?: Vec3; translate?: Vec3; displacement?: Vec3 } = {},
): MeshData {
  const { vonMises = [200, 200, 400], translate = [0, 0, 0], displacement } = opts;
  const corners: Vec3[] = [[-1, -1, 0], [1, -1, 0], [0, 1, 0]];
  const offsetBy = (d: Vec3) => new Float32Array(corners.flatMap((c) => c.map((x, i) => x + translate[i] + d[i])));
  return {
    entity_path: 'bracket',
    vertices: offsetBy([0, 0, 0]),
    indices: new Uint32Array([0, 1, 2]),
    normals: null,
    scalar_channels: { vonMises: new Float32Array(vonMises) },
    ...(displacement ? { displaced_positions: offsetBy(displacement) } : {}),
  };
}

function expectAt(marker: Object3D, [x, y, z]: Vec3): void {
  expect(marker.position.x).toBeCloseTo(x);
  expect(marker.position.y).toBeCloseTo(y);
  expect(marker.position.z).toBeCloseTo(z);
}

function stubCanvasRect(left: number): void {
  const canvas = screen.getByTestId('viewport-canvas');
  canvas.getBoundingClientRect = () => ({
    left,
    top: 0,
    width: 100,
    height: 100,
    right: left + 100,
    bottom: 100,
    x: left,
    y: 0,
    toJSON: () => {},
  });
}

function click(x: number, y: number, upX = x, upY = y): void {
  const canvas = screen.getByTestId('viewport-canvas');
  canvas.dispatchEvent(new MouseEvent('pointerdown', { clientX: x, clientY: y }));
  canvas.dispatchEvent(new MouseEvent('pointerup', { clientX: upX, clientY: upY }));
}

/** Clicks and returns the scene children the click added. */
function clickAndCollectNewChildren(x: number, y: number, upX = x, upY = y): Object3D[] {
  const before = new Set(lastScene!.children);
  click(x, y, upX, upY);
  return lastScene!.children.filter((child) => !before.has(child));
}

/** Clicks the canvas centre and returns the single marker the probe added. */
function placeProbe(): Object3D {
  const added = clickAndCollectNewChildren(50, 50);
  expect(added).toHaveLength(1);
  return added[0];
}

function flushFrame(): void {
  const pending = rafCallbacks;
  rafCallbacks = [];
  for (const cb of pending) cb(performance.now());
}

function renderViewport(
  opts: {
    feaModeStore?: FeaModeStore;
    onSelect?: ReturnType<typeof vi.fn>;
    meshes?: () => Record<string, MeshData>;
  } = {},
) {
  const meshes = opts.meshes ?? (() => ({ bracket: bracketMesh() }));
  const result = render(() => (
    <Viewport
      viewportId="probe-vp"
      meshes={meshes()}
      feaModeStore={opts.feaModeStore}
      onSelect={opts.onSelect}
    />
  ));
  stubCanvasRect(0);
  return result;
}

describe('Viewport probe wiring', () => {
  it('(a) in FEA mode a click on the body shows the von Mises value at the picked point', () => {
    renderViewport({ feaModeStore: createFeaModeStore() });

    const added = clickAndCollectNewChildren(50, 50);

    const rows = screen.getAllByTestId('probe-row');
    expect(screen.getByTestId('probe-popup')).toBeTruthy();
    expect(rows).toHaveLength(1);
    expect(rows[0].textContent).toMatch(VALUE_300);
    expect(rows[0].textContent).toContain('bracket');
    expect(added).toHaveLength(1);
  });

  it('(b) without a feaModeStore a click places no probe and still selects', () => {
    const onSelect = vi.fn();
    renderViewport({ onSelect });

    const added = clickAndCollectNewChildren(50, 50);

    expect(screen.queryByTestId('probe-popup')).toBeNull();
    expect(added).toHaveLength(0);
    expect(onSelect).toHaveBeenCalledWith('bracket', expect.anything());
  });

  it('(c) with FEA mode off a click places no probe and still selects', () => {
    const store = createFeaModeStore();
    store.tryAutoEnable();
    store.setEnabled(false);
    const onSelect = vi.fn();
    renderViewport({ feaModeStore: store, onSelect });

    const added = clickAndCollectNewChildren(50, 50);

    expect(store.state.enabled).toBe(false);
    expect(screen.queryByTestId('probe-popup')).toBeNull();
    expect(added).toHaveLength(0);
    expect(onSelect).toHaveBeenCalledWith('bracket', expect.anything());
  });

  it('(d) a drag in FEA mode places no probe', () => {
    renderViewport({ feaModeStore: createFeaModeStore() });

    const added = clickAndCollectNewChildren(50, 50, 70, 50);

    expect(screen.queryByTestId('probe-popup')).toBeNull();
    expect(added).toHaveLength(0);
  });

  it('(e) leaving FEA mode tears the probes down; re-entering starts empty', () => {
    const store = createFeaModeStore();
    renderViewport({ feaModeStore: store });
    const marker = placeProbe();

    store.setEnabled(false);

    expect(screen.queryByTestId('probe-popup')).toBeNull();
    expect(marker.parent).toBeNull();

    store.setEnabled(true);

    expect(screen.queryByTestId('probe-popup')).toBeNull();
    placeProbe();
    expect(screen.getAllByTestId('probe-row')).toHaveLength(1);
  });

  it('(f) unmount removes the probe markers from the scene', () => {
    const { unmount } = renderViewport({ feaModeStore: createFeaModeStore() });
    const marker = placeProbe();

    unmount();

    expect(marker.parent).toBeNull();
  });

  it('(g) the row remove button drops the probe, its popup and its marker', () => {
    renderViewport({ feaModeStore: createFeaModeStore() });
    const marker = placeProbe();

    fireEvent.click(screen.getByTestId('probe-remove'));

    expect(screen.queryByTestId('probe-row')).toBeNull();
    expect(screen.queryByTestId('probe-popup')).toBeNull();
    expect(marker.parent).toBeNull();
  });

  it('(h) placing a probe requests a frame so the marker draws without camera motion', () => {
    renderViewport({ feaModeStore: createFeaModeStore() });
    const render = lastRenderer!.render;
    flushFrame();
    render.mockClear();
    flushFrame();
    expect(render).not.toHaveBeenCalled();

    placeProbe();
    flushFrame();

    expect(render).toHaveBeenCalled();
  });
});

function renderWithMeshSignal(initial: MeshData = bracketMesh(), feaModeStore = createFeaModeStore()) {
  const [meshes, setMeshes] = createSignal<Record<string, MeshData>>({ bracket: initial });
  renderViewport({ feaModeStore, meshes });
  return setMeshes;
}

describe('probes follow re-evaluation', () => {
  it('(a) a re-solve updates the probe readout to the new field', () => {
    const setMeshes = renderWithMeshSignal();
    placeProbe();
    expect(screen.getByTestId('probe-row').textContent).toMatch(VALUE_300);

    setMeshes({ bracket: bracketMesh({ vonMises: [400, 400, 800] }) });

    const text = screen.getByTestId('probe-row').textContent;
    expect(text).toMatch(/\b600(?:\.0+)?\b/);
    expect(text).not.toMatch(VALUE_300);
  });

  it('(b) losing the entity marks the probe stale; Re-pin drops it', () => {
    const setMeshes = renderWithMeshSignal();
    const marker = placeProbe();

    setMeshes({});

    const row = screen.getByTestId('probe-row');
    expect(row.classList.contains('probe-stale')).toBe(true);
    expect(row.textContent).toMatch(VALUE_300);

    fireEvent.click(screen.getByTestId('probe-repin'));

    expect(screen.queryByTestId('probe-popup')).toBeNull();
    expect(marker.parent).toBeNull();
  });
});

describe('probe markers follow the displayed surface', () => {
  it('(a) a re-solve that moves the surface carries the marker with it', () => {
    const setMeshes = renderWithMeshSignal();
    const marker = placeProbe();
    expectAt(marker, [0, 0, 0]);

    setMeshes({ bracket: bracketMesh({ translate: [1, 0, 0] }) });

    expectAt(marker, [1, 0, 0]);
  });

  it('(b) the deformed view carries the marker onto the warped surface and back', () => {
    const store = createFeaModeStore();
    renderWithMeshSignal(bracketMesh({ displacement: [0, 0, 0.5] }), store);
    const marker = placeProbe();

    store.setShowDeformed(true);
    expectAt(marker, [0, 0, 0.5]);

    store.setWarpFactor(2);
    expectAt(marker, [0, 0, 1]);

    store.setShowDeformed(false);
    expectAt(marker, [0, 0, 0]);
  });

  it('(c) a stale marker keeps its last displayed position', () => {
    const setMeshes = renderWithMeshSignal();
    const marker = placeProbe();
    setMeshes({ bracket: bracketMesh({ translate: [1, 0, 0] }) });

    setMeshes({});

    expect(screen.getByTestId('probe-row').classList.contains('probe-stale')).toBe(true);
    expectAt(marker, [1, 0, 0]);
  });
});

describe('probe picking tracks the canvas position', () => {
  it('a probe click after the pane moves still probes the picked point', () => {
    renderViewport({ feaModeStore: createFeaModeStore() });
    click(50, 50);
    expect(screen.getAllByTestId('probe-row')).toHaveLength(1);

    stubCanvasRect(300);
    click(350, 50);

    const rows = screen.getAllByTestId('probe-row');
    expect(rows).toHaveLength(2);
    for (const row of rows) expect(row.textContent).toMatch(VALUE_300);
  });
});
