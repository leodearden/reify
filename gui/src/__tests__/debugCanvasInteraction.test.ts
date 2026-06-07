/**
 * Unit tests for the canvas-interaction debug bridge handlers:
 *   pick_entity_at, orbit_camera, pan_camera, zoom_camera
 *
 * Uses REAL three.js (no vi.mock('three')) so Raycaster/BoxGeometry/BVH
 * work correctly.  Only @tauri-apps/api/* and html-to-image are mocked.
 *
 * Pattern mirrors debugContract.test.ts (real-three scene setup) and
 * listConsoleErrors.test.ts (listen-capture + dispatchAndGetResult).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// ─── Tauri / html-to-image mocks (no three mock!) ─────────────────────────────
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(),
  LogicalSize: class LogicalSize {
    constructor(w: number, h: number) {
      (this as any).width = w;
      (this as any).height = h;
    }
  },
}));
vi.mock('html-to-image', () => ({
  toPng: vi.fn().mockResolvedValue('data:image/png;base64,STUB'),
}));

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { initDebugBridge } from '../debug/bridge';
import { makeViewStateStoreMock } from './debugBridgeTestHelpers';
import type { DebugStores } from '../debug/types';

type DebugRequestHandler = (event: {
  payload: { id: number; command: string; params: Record<string, unknown> };
}) => Promise<void>;

// ─── makeStores ───────────────────────────────────────────────────────────────

function makeStores(): DebugStores {
  return {
    engine: {
      state: {
        meshes: {} as any,
        values: {} as any,
        constraints: {} as any,
        evalStatus: { phase: 'idle' },
        compileDiagnostics: [],
        tessellationDiagnostics: [],
      },
      initFromState: vi.fn(),
    },
    editor: {
      state: {
        openFiles: [],
        activeFile: null,
        dirtyFiles: [],
        externallyChanged: [],
        cursorPosition: null,
      },
      openFile: vi.fn(),
    },
    selection: {
      state: {
        selectedEntity: null,
        selectedEntities: [],
        anchorEntity: null,
        hoveredEntity: null,
        highlightedParams: [],
      } as any,
      selectEntity: vi.fn(),
      hoverEntity: vi.fn(),
      clearSelection: vi.fn(),
      toggleSelect: vi.fn(),
    },
    claude: {
      state: {
        messages: [],
        sessionStatus: 'idle',
        currentMessageId: null,
      },
    },
    viewState: makeViewStateStoreMock(),
    layout: {
      state: {
        editorWidth: 300,
        sideWidth: 300,
        designTreeHeight: 160,
        propertyHeight: 200,
        constraintHeight: 140,
      },
      setEditorWidth: vi.fn(),
      setSideWidth: vi.fn(),
      setDesignTreeHeight: vi.fn(),
      setPropertyHeight: vi.fn(),
      setConstraintHeight: vi.fn(),
    },
  };
}

// ─── dispatchAndGetResult ─────────────────────────────────────────────────────

/** Dispatch a command via the captured handler and return the parsed response. */
async function dispatchAndGetResult(
  handler: DebugRequestHandler,
  id: number,
  command: string,
  params: Record<string, unknown> = {},
): Promise<unknown> {
  vi.mocked(invoke).mockClear();
  await handler({ payload: { id, command, params } });
  const calls = vi.mocked(invoke).mock.calls;
  const responseCall = calls.find((c) => c[0] === 'debug_response');
  if (!responseCall) return undefined;
  const payload = responseCall[1] as { id: number; result: string };
  return JSON.parse(payload.result);
}

// ─── pick_entity_at ───────────────────────────────────────────────────────────

describe('pick_entity_at: bridge handler (real three)', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let selectEntitySpy: ReturnType<typeof vi.fn>;

  const CANVAS_RECT = {
    left: 0,
    top: 0,
    width: 800,
    height: 600,
    x: 0,
    y: 0,
    right: 800,
    bottom: 600,
    toJSON: () => ({}),
  } as DOMRect;

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;

    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    const stores = makeStores();
    selectEntitySpy = vi.mocked(stores.selection.selectEntity);
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    // Build a real three.js scene: PerspectiveCamera at (0,0,5) → BoxGeometry named 'entity/box'
    const { Scene, PerspectiveCamera, Mesh, BoxGeometry } = await import('three');

    const scene = new Scene();
    const camera = new PerspectiveCamera(75, 800 / 600, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.lookAt(0, 0, 0);
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();

    const geometry = new BoxGeometry(1, 1, 1);
    const mesh = new Mesh(geometry);
    mesh.name = 'entity/box';
    scene.add(mesh);

    // Renderer stub — domElement.getBoundingClientRect() returns 800×600 canvas
    const domElement = document.createElement('canvas');
    vi.spyOn(domElement, 'getBoundingClientRect').mockReturnValue(CANVAS_RECT);

    const renderer = {
      domElement,
      render: vi.fn(),
    };

    window.__REIFY_DEBUG__!.viewport = {
      scene,
      camera,
      renderer: renderer as any,
      getMeshes: () => new Map([['entity/box', mesh]]),
      getGhostMeshes: () => new Map(),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
    };
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('pick at canvas center (400,300) → hit entity/box', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 1, 'pick_entity_at', {
      x: 400,
      y: 300,
    }) as any;

    expect(result).toBeDefined();
    expect(result.hit).toBe(true);
    expect(result.entityPath).toBe('entity/box');
    expect(typeof result.point.x).toBe('number');
    expect(typeof result.point.y).toBe('number');
    expect(typeof result.point.z).toBe('number');
    expect(Number.isFinite(result.point.x)).toBe(true);
    expect(Number.isFinite(result.point.y)).toBe(true);
    expect(Number.isFinite(result.point.z)).toBe(true);
    expect(typeof result.distance).toBe('number');
    expect(Number.isFinite(result.distance)).toBe(true);
    expect(result.distance).toBeGreaterThan(0);
  });

  it('omitted coords → canvas center default → hit entity/box', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 2, 'pick_entity_at', {}) as any;

    // Center of 800×600 canvas → NDC(0,0) → ray along -Z → hits box
    expect(result).toBeDefined();
    expect(result.hit).toBe(true);
    expect(result.entityPath).toBe('entity/box');
  });

  it('pick at far corner (5,5) → miss', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 3, 'pick_entity_at', {
      x: 5,
      y: 5,
    }) as any;

    expect(result).toBeDefined();
    expect(result.hit).toBe(false);
  });

  it('pick is query-only — selectEntity is never called', async () => {
    await dispatchAndGetResult(capturedHandler!, 4, 'pick_entity_at', { x: 400, y: 300 });
    await dispatchAndGetResult(capturedHandler!, 5, 'pick_entity_at', {});
    await dispatchAndGetResult(capturedHandler!, 6, 'pick_entity_at', { x: 5, y: 5 });

    expect(selectEntitySpy).not.toHaveBeenCalled();
  });

  it('unknown viewportId → {error}', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 7, 'pick_entity_at', {
      viewportId: 'nope',
    }) as any;

    expect(result).toBeDefined();
    expect(typeof result.error).toBe('string');
    expect(result.error.length).toBeGreaterThan(0);
  });

  it('non-finite coords → {error}', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 8, 'pick_entity_at', {
      x: 'a',
      y: 0,
    }) as any;

    expect(result).toBeDefined();
    expect(typeof result.error).toBe('string');
    expect(result.error.length).toBeGreaterThan(0);
  });
});

// ─── orbit_camera / pan_camera / zoom_camera ─────────────────────────────────

describe('orbit_camera / pan_camera / zoom_camera: bridge handlers (real OrbitControls)', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;

    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });

    const stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    // Real three.js + real OrbitControls in jsdom
    // Camera at (0,5,5) looking at origin (a non-trivial azimuth/polar for better test coverage)
    const { Scene, PerspectiveCamera } = await import('three');
    const { OrbitControls } = await import('three/addons/controls/OrbitControls.js');

    const scene = new Scene();
    const camera = new PerspectiveCamera(75, 800 / 600, 0.1, 1000);
    camera.position.set(0, 5, 5);
    camera.lookAt(0, 0, 0);
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();

    // OrbitControls needs a DOM element; stub clientHeight for pan (avoid divide-by-zero)
    const domElement = document.createElement('div');
    Object.defineProperty(domElement, 'clientHeight', { value: 600 });
    Object.defineProperty(domElement, 'clientWidth', { value: 800 });
    vi.spyOn(domElement, 'getBoundingClientRect').mockReturnValue({
      left: 0,
      top: 0,
      width: 800,
      height: 600,
      x: 0,
      y: 0,
      right: 800,
      bottom: 600,
      toJSON: () => ({}),
    } as DOMRect);

    const controls = new OrbitControls(camera, domElement);
    controls.update();

    const renderer = {
      domElement,
      render: vi.fn(),
    };

    window.__REIFY_DEBUG__!.viewport = {
      scene,
      camera,
      renderer: renderer as any,
      getMeshes: () => new Map(),
      getGhostMeshes: () => new Map(),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
      controls: controls as any,
    };
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  // ── orbit_camera ──────────────────────────────────────────────────────────

  it('orbit_camera {dazimuth:0.5} → ok, azimuthDelta > 0', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 10, 'orbit_camera', {
      dazimuth: 0.5,
    }) as any;

    expect(result).toBeDefined();
    expect(result.ok).toBe(true);
    expect(typeof result.azimuth).toBe('number');
    expect(Number.isFinite(result.azimuth)).toBe(true);
    expect(typeof result.azimuthDelta).toBe('number');
    expect(result.azimuthDelta).toBeGreaterThan(0);
    expect(typeof result.polar).toBe('number');
    expect(typeof result.polarDelta).toBe('number');
    expect(result.camera).toBeDefined();
    expect(typeof result.camera.position.x).toBe('number');
  });

  it('orbit_camera {delevation:0.2} → ok, polarDelta > 0', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 11, 'orbit_camera', {
      delevation: 0.2,
    }) as any;

    expect(result.ok).toBe(true);
    expect(result.polarDelta).toBeGreaterThan(0);
  });

  it('orbit_camera unknown viewportId → {error}', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 12, 'orbit_camera', {
      viewportId: 'bad',
    }) as any;

    expect(typeof result.error).toBe('string');
  });

  it('orbit_camera non-finite dazimuth → {error}', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 13, 'orbit_camera', {
      dazimuth: 'x',
    }) as any;

    expect(typeof result.error).toBe('string');
  });

  // ── pan_camera ────────────────────────────────────────────────────────────

  it('pan_camera {dx:50,dy:0} → ok, target moved and all components finite', async () => {
    // Capture the target's x before panning (initial target is at origin, x=0).
    // Camera at (0,5,5) → right vector (1,0,0), so pan(50,0) shifts target.x by a
    // non-zero amount proportional to 50/clientHeight.  A no-op (dx/dy dropped, or
    // update() skipped) would still return finite numbers but leave target unchanged,
    // so we check the delta explicitly — mirrors orbit_camera's azimuthDelta>0 check.
    const targetX0 = (window.__REIFY_DEBUG__!.viewport as any).controls.target.x;

    const result = await dispatchAndGetResult(capturedHandler!, 20, 'pan_camera', {
      dx: 50,
      dy: 0,
    }) as any;

    expect(result.ok).toBe(true);
    expect(result.target).toBeDefined();
    expect(Number.isFinite(result.target.x)).toBe(true);
    expect(Number.isFinite(result.target.y)).toBe(true);
    expect(Number.isFinite(result.target.z)).toBe(true);
    expect(Number.isFinite(result.camera.position.x)).toBe(true);
    expect(Number.isFinite(result.camera.position.y)).toBe(true);
    expect(Number.isFinite(result.camera.position.z)).toBe(true);
    // Assert pan actually moved the target (not a no-op regression)
    expect(Math.abs(result.target.x - targetX0)).toBeGreaterThan(0);
  });

  it('pan_camera unknown viewportId → {error}', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 21, 'pan_camera', {
      viewportId: 'bad',
    }) as any;

    expect(typeof result.error).toBe('string');
  });

  // ── zoom_camera ───────────────────────────────────────────────────────────

  it('zoom_camera {scale:2} → ok, distance increased, distanceDelta > 0', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 30, 'zoom_camera', {
      scale: 2,
    }) as any;

    expect(result.ok).toBe(true);
    expect(typeof result.distance).toBe('number');
    expect(Number.isFinite(result.distance)).toBe(true);
    expect(typeof result.distanceDelta).toBe('number');
    expect(result.distanceDelta).toBeGreaterThan(0);
  });

  it('zoom_camera {scale:0.5} → ok, distance decreased, distanceDelta > 0', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 31, 'zoom_camera', {
      scale: 0.5,
    }) as any;

    expect(result.ok).toBe(true);
    expect(result.distanceDelta).toBeGreaterThan(0);
  });

  it('zoom_camera unknown viewportId → {error}', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 32, 'zoom_camera', {
      viewportId: 'bad',
    }) as any;

    expect(typeof result.error).toBe('string');
  });

  it('zoom_camera {scale:0} → {error} (scale must be > 0)', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 33, 'zoom_camera', {
      scale: 0,
    }) as any;

    expect(typeof result.error).toBe('string');
  });

  it('zoom_camera {scale:NaN} → {error}', async () => {
    const result = await dispatchAndGetResult(capturedHandler!, 34, 'zoom_camera', {
      scale: NaN,
    }) as any;

    expect(typeof result.error).toBe('string');
  });
});
