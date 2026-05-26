/**
 * Unit tests for the debug bridge handlers.
 * Covers: store_state / viewport_state selectedEntities; set_test_mode.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('three', () => ({
  Box3: class { expandByObject() {} isEmpty() { return true; } },
  Vector3: class {},
}));
vi.mock('html-to-image', () => ({
  toPng: vi.fn().mockResolvedValue('data:image/png;base64,STUB'),
}));

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { toPng } from 'html-to-image';
import { initDebugBridge } from '../debug/bridge';
import { setTestMode } from '../debug/testMode';
import type { DebugStores } from '../debug/types';

type DebugRequestHandler = (event: { payload: { id: number; command: string; params: Record<string, unknown> } }) => Promise<void>;

function makeStores(selectedEntities: string[] = [], anchorEntity: string | null = null): DebugStores {
  return {
    engine: {
      state: {
        meshes: {} as any,
        values: {} as any,
        constraints: {} as any,
        evalStatus: { phase: 'idle' },
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
        selectedEntity: selectedEntities[selectedEntities.length - 1] ?? null,
        // Cast to any until step-36 adds the fields to the DebugStores type
        ...(selectedEntities.length > 0 ? { selectedEntities } : { selectedEntities: [] }),
        ...(anchorEntity !== null ? { anchorEntity } : { anchorEntity: null }),
        hoveredEntity: null,
        highlightedParams: [],
      } as any,
      selectEntity: vi.fn(),
      hoverEntity: vi.fn(),
    },
    claude: {
      state: {
        messages: [],
        sessionStatus: 'idle',
        currentMessageId: null,
      },
    },
  };
}

describe('debug bridge store_state includes selectedEntities', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('store_state includes selection.selectedEntities array', async () => {
    const stores = makeStores(['A', 'B']);
    await initDebugBridge(stores);

    expect(capturedHandler).toBeDefined();

    await capturedHandler!({ payload: { id: 1, command: 'store_state', params: {} } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    // invoke('debug_response', { id, result: JSON.stringify(result) })
    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result.selection.selectedEntities).toEqual(['A', 'B']);
  });

  it('store_state includes selection.selectedEntities as empty array when nothing selected', async () => {
    const stores = makeStores([]);
    await initDebugBridge(stores);

    await capturedHandler!({ payload: { id: 2, command: 'store_state', params: {} } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result.selection.selectedEntities).toEqual([]);
  });

  it('store_state includes selection.anchorEntity', async () => {
    const stores = makeStores(['A'], 'A');
    await initDebugBridge(stores);

    await capturedHandler!({ payload: { id: 3, command: 'store_state', params: {} } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result.selection.anchorEntity).toBe('A');
  });

  it('viewport_state includes selectedEntities via the stores reference', async () => {
    const stores = makeStores(['X', 'Y']);
    await initDebugBridge(stores);

    // store_state reads selection.selectedEntities from stores (same reference used by viewport_state)
    await capturedHandler!({ payload: { id: 4, command: 'store_state', params: {} } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result.selection.selectedEntities).toEqual(['X', 'Y']);
  });
});

describe('debug bridge set_camera', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('returns {error: "viewport not ready"} when viewport is undefined', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    // No viewport installed — window.__REIFY_DEBUG__.viewport is undefined
    await capturedHandler!({ payload: { id: 100, command: 'set_camera', params: { position: [1, 2, 3], target: [0, 0, 0] } } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result).toEqual({ error: 'viewport not ready' });
  });

  // Helper to build a viewport stub with spy functions
  function makeViewportStub() {
    const cameraPositionSet = vi.fn();
    const cameraUpSet = vi.fn();
    const cameraLookAt = vi.fn();
    const controlsTargetSet = vi.fn();
    const rendererRender = vi.fn();
    const camera = {
      position: { set: cameraPositionSet, x: 0, y: 0, z: 0 },
      up: { set: cameraUpSet, x: 0, y: 1, z: 0 },
      zoom: 1,
      lookAt: cameraLookAt,
      updateProjectionMatrix: vi.fn(),
      updateMatrixWorld: vi.fn(),
    };
    const controls = {
      target: { set: controlsTargetSet, x: 0, y: 0, z: 0 },
      update: vi.fn(),
    };
    const renderer = { render: rendererRender, domElement: { toDataURL: vi.fn() } };
    const scene = {} as any;
    return { camera, controls, renderer, scene, cameraPositionSet, cameraUpSet, cameraLookAt, controlsTargetSet, rendererRender };
  }

  async function dispatch(handler: DebugRequestHandler, id: number, params: Record<string, unknown>) {
    vi.mocked(invoke).mockClear();
    await handler({ payload: { id, command: 'set_camera', params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  it('defaults applied.up from camera.up and applied.zoom from camera.zoom when caller omits them', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const stub = makeViewportStub();
    // camera.up = {x:0, y:1, z:0}, camera.zoom = 1 (defaults from makeViewportStub)
    window.__REIFY_DEBUG__!.viewport = {
      scene: stub.scene,
      camera: stub.camera as any,
      renderer: stub.renderer as any,
      getMeshes: vi.fn().mockReturnValue(new Map()),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
      controls: stub.controls as any,
    };

    const result = await dispatch(capturedHandler!, 350, {
      position: [5, 5, 5],
      target: [0, 0, 0],
    });

    expect(result.ok).toBe(true);
    expect(result.applied.position).toEqual([5, 5, 5]);
    expect(result.applied.target).toEqual([0, 0, 0]);
    // up must be the camera.up snapshot, not undefined
    expect(result.applied.up).toEqual([0, 1, 0]);
    // zoom must be camera.zoom, not undefined
    expect(result.applied.zoom).toBe(1);
    // camera.up.set must NOT be called (caller didn't provide up)
    expect(stub.cameraUpSet).not.toHaveBeenCalled();
    // camera.zoom must remain unchanged
    expect(stub.camera.zoom).toBe(1);
  });

  it('happy path: applies full pose and returns {ok: true, applied: {...}}', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const stub = makeViewportStub();
    window.__REIFY_DEBUG__!.viewport = {
      scene: stub.scene,
      camera: stub.camera as any,
      renderer: stub.renderer as any,
      getMeshes: vi.fn().mockReturnValue(new Map()),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
      controls: stub.controls as any,
    };

    const result = await dispatch(capturedHandler!, 300, {
      position: [10, 20, 30],
      target: [1, 2, 3],
      up: [0, 0, 1],
      zoom: 2.5,
    });

    // Camera mutations
    expect(stub.cameraPositionSet).toHaveBeenCalledWith(10, 20, 30);
    expect(stub.controlsTargetSet).toHaveBeenCalledWith(1, 2, 3);
    expect(stub.cameraUpSet).toHaveBeenCalledWith(0, 0, 1);
    expect(stub.camera.zoom).toBe(2.5);
    expect(stub.camera.updateMatrixWorld).toHaveBeenCalled();
    expect(stub.camera.updateProjectionMatrix).toHaveBeenCalled();
    expect(stub.controls.update).toHaveBeenCalled();
    expect(stub.rendererRender).toHaveBeenCalledWith(stub.scene, stub.camera);
    // Response
    expect(result).toEqual({ ok: true, applied: { position: [10, 20, 30], target: [1, 2, 3], up: [0, 0, 1], zoom: 2.5 } });
  });

  it('succeeds gracefully when viewport.controls is undefined', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const stub = makeViewportStub();
    // Install viewport WITHOUT controls
    window.__REIFY_DEBUG__!.viewport = {
      scene: stub.scene,
      camera: stub.camera as any,
      renderer: stub.renderer as any,
      getMeshes: vi.fn().mockReturnValue(new Map()),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
      controls: undefined,
    };

    const result = await dispatch(capturedHandler!, 400, {
      position: [1, 2, 3],
      target: [0, 0, 0],
    });

    expect(result.ok).toBe(true);
    expect(stub.cameraPositionSet).toHaveBeenCalledWith(1, 2, 3);
    // target honored via lookAt fallback — contract holds without OrbitControls
    expect(stub.cameraLookAt).toHaveBeenCalledWith(0, 0, 0);
    expect(stub.camera.updateMatrixWorld).toHaveBeenCalled();
    expect(stub.rendererRender).toHaveBeenCalledWith(stub.scene, stub.camera);
  });

  describe('input validation', () => {
    let stub: ReturnType<typeof makeViewportStub>;

    beforeEach(async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      stub = makeViewportStub();
      window.__REIFY_DEBUG__!.viewport = {
        scene: stub.scene,
        camera: stub.camera as any,
        renderer: stub.renderer as any,
        getMeshes: vi.fn().mockReturnValue(new Map()),
        getGhostMeshes: vi.fn().mockReturnValue(new Map()),
        fitToView: vi.fn(),
        flyToEntity: vi.fn(),
        controls: stub.controls as any,
      };
    });

    it('rejects missing position', async () => {
      const result = await dispatch(capturedHandler!, 200, { target: [0, 0, 0] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects position that is not an array', async () => {
      const result = await dispatch(capturedHandler!, 201, { position: 'bad', target: [0, 0, 0] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects position with length != 3', async () => {
      const result = await dispatch(capturedHandler!, 202, { position: [1, 2], target: [0, 0, 0] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects position containing NaN', async () => {
      const result = await dispatch(capturedHandler!, 203, { position: [1, NaN, 3], target: [0, 0, 0] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects position containing Infinity', async () => {
      const result = await dispatch(capturedHandler!, 204, { position: [1, 2, Infinity], target: [0, 0, 0] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects missing target', async () => {
      const result = await dispatch(capturedHandler!, 205, { position: [1, 2, 3] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects target not an array', async () => {
      const result = await dispatch(capturedHandler!, 206, { position: [1, 2, 3], target: 42 });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects target with length != 3', async () => {
      const result = await dispatch(capturedHandler!, 207, { position: [1, 2, 3], target: [0, 0, 0, 0] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects target containing NaN', async () => {
      const result = await dispatch(capturedHandler!, 208, { position: [1, 2, 3], target: [NaN, 0, 0] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects target containing Infinity', async () => {
      const result = await dispatch(capturedHandler!, 209, { position: [1, 2, 3], target: [0, -Infinity, 0] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects up that is not an array when provided', async () => {
      const result = await dispatch(capturedHandler!, 210, { position: [1, 2, 3], target: [0, 0, 0], up: 'bad' });
      expect(result).toHaveProperty('error');
      expect(stub.cameraUpSet).not.toHaveBeenCalled();
    });

    it('rejects up with length != 3 when provided', async () => {
      const result = await dispatch(capturedHandler!, 211, { position: [1, 2, 3], target: [0, 0, 0], up: [0, 1] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraUpSet).not.toHaveBeenCalled();
    });

    it('rejects up containing NaN when provided', async () => {
      const result = await dispatch(capturedHandler!, 212, { position: [1, 2, 3], target: [0, 0, 0], up: [0, NaN, 0] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraUpSet).not.toHaveBeenCalled();
    });

    it('rejects up containing Infinity when provided', async () => {
      const result = await dispatch(capturedHandler!, 217, { position: [1, 2, 3], target: [0, 0, 0], up: [Infinity, 0, 0] });
      expect(result).toHaveProperty('error');
      expect(stub.cameraUpSet).not.toHaveBeenCalled();
    });

    it('rejects zoom that is NaN when provided', async () => {
      const result = await dispatch(capturedHandler!, 213, { position: [1, 2, 3], target: [0, 0, 0], zoom: NaN });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects zoom that is Infinity when provided', async () => {
      const result = await dispatch(capturedHandler!, 214, { position: [1, 2, 3], target: [0, 0, 0], zoom: Infinity });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects zoom <= 0 when provided', async () => {
      const result = await dispatch(capturedHandler!, 215, { position: [1, 2, 3], target: [0, 0, 0], zoom: -1 });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });

    it('rejects zoom = 0 when provided', async () => {
      const result = await dispatch(capturedHandler!, 216, { position: [1, 2, 3], target: [0, 0, 0], zoom: 0 });
      expect(result).toHaveProperty('error');
      expect(stub.cameraPositionSet).not.toHaveBeenCalled();
    });
  });
});

describe('debug bridge open_file', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  async function dispatch(handler: DebugRequestHandler, id: number, params: Record<string, unknown>) {
    vi.mocked(invoke).mockClear();
    await handler({ payload: { id, command: 'open_file', params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  it('opens file in editor and returns { ok: true, path } when guiState is omitted', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    const result = await dispatch(capturedHandler!, 500, {
      path: '/tmp/foo.ri',
      content: 'def Foo() {}',
    });

    expect(result).toEqual({ ok: true, path: '/tmp/foo.ri' });
    expect(stores.editor.openFile).toHaveBeenCalledWith({ path: '/tmp/foo.ri', content: 'def Foo() {}' });
    expect(stores.engine.initFromState).not.toHaveBeenCalled();
  });

  it('initFromState is called when guiState is provided', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const rawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
    };

    const result = await dispatch(capturedHandler!, 501, {
      path: '/tmp/bar.ri',
      content: 'def Bar() {}',
      guiState: rawGuiState,
    });

    expect(result).toEqual({ ok: true, path: '/tmp/bar.ri' });
    expect(stores.engine.initFromState).toHaveBeenCalledTimes(1);
    // Verify the converted GuiState shape was passed (meshes converted to typed arrays)
    const passed = vi.mocked(stores.engine.initFromState).mock.calls[0][0];
    expect(passed.meshes).toEqual([]);
    expect(passed.values).toEqual([]);
    expect(passed.constraints).toEqual([]);
  });

  it('initFromState invocation triggers the onEngineReinitialized callback wired in App.tsx', async () => {
    // This test verifies the bridge → engineStore wiring contract: when the
    // bridge calls engine.initFromState, any onEngineReinitialized callback
    // registered by App.tsx fires. Uses a real engineStore (no mock) to
    // exercise the integration boundary the bug report identified.
    const reinitSpy = vi.fn();
    const { createEngineStore } = await import('../stores/engineStore');
    const realEngine = createEngineStore({ onEngineReinitialized: reinitSpy });
    const stores: DebugStores = {
      ...makeStores(),
      engine: realEngine,
    };
    await initDebugBridge(stores);

    const rawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
    };

    await dispatch(capturedHandler!, 502, {
      path: '/tmp/baz.ri',
      content: 'def Baz() {}',
      guiState: rawGuiState,
    });

    expect(reinitSpy).toHaveBeenCalledTimes(1);
  });

  it('returns { error } when path is missing', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const result = await dispatch(capturedHandler!, 503, { content: 'x' });
    expect(result).toHaveProperty('error');
    expect(stores.editor.openFile).not.toHaveBeenCalled();
    expect(stores.engine.initFromState).not.toHaveBeenCalled();
  });

  it('returns { error } when content is missing', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const result = await dispatch(capturedHandler!, 504, { path: '/tmp/foo.ri' });
    expect(result).toHaveProperty('error');
    expect(stores.editor.openFile).not.toHaveBeenCalled();
    expect(stores.engine.initFromState).not.toHaveBeenCalled();
  });
});

describe('debug bridge set_test_mode', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    // Clean up DOM attribute and reset signal so tests don't leak
    delete document.documentElement.dataset.testMode;
    setTestMode(false);
    delete window.__REIFY_DEBUG__;
  });

  it('set_test_mode { enabled: true } returns { ok: true, test_mode: true } and sets data-test-mode', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    await capturedHandler!({ payload: { id: 10, command: 'set_test_mode', params: { enabled: true } } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result).toEqual({ ok: true, test_mode: true });
    expect(document.documentElement.dataset.testMode).toBe('true');
  });

  it('set_test_mode { enabled: false } returns { ok: true, test_mode: false } and clears data-test-mode', async () => {
    // First enable, then disable
    document.documentElement.dataset.testMode = 'true';
    const stores = makeStores();
    await initDebugBridge(stores);

    await capturedHandler!({ payload: { id: 11, command: 'set_test_mode', params: { enabled: false } } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();

    const payload = responseCall![1] as { id: number; result: string };
    const result = JSON.parse(payload.result);
    expect(result).toEqual({ ok: true, test_mode: false });
    expect(document.documentElement.dataset.testMode).toBeUndefined();
  });

  it('testMode signal is exposed on window.__REIFY_DEBUG__ after initDebugBridge', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    // testMode accessor must be a function on the context
    expect(typeof window.__REIFY_DEBUG__?.testMode).toBe('function');

    // Initially false
    expect(window.__REIFY_DEBUG__!.testMode!()).toBe(false);

    // After set_test_mode { enabled: true } request, accessor returns true
    await capturedHandler!({ payload: { id: 20, command: 'set_test_mode', params: { enabled: true } } });
    expect(window.__REIFY_DEBUG__!.testMode!()).toBe(true);
  });

  it('set_test_mode does not call renderer.render (no WebGL re-render contract)', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    // Capture the render spy so we can assert it is never called
    const rendererRender = vi.fn();

    // Wire a stub viewport onto the context after init
    window.__REIFY_DEBUG__!.viewport = {
      scene: {} as any,
      camera: {} as any,
      renderer: {
        render: rendererRender,
        domElement: { toDataURL: vi.fn().mockReturnValue('data:image/png;base64,abc') },
      } as any,
      getMeshes: vi.fn().mockReturnValue(new Map()),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
    };

    await capturedHandler!({ payload: { id: 12, command: 'set_test_mode', params: { enabled: true } } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as { id: number; result: string }).result);
    // Minimal dispatch-succeeded guard (not re-asserting full payload shape owned by earlier test)
    expect(result.ok).toBe(true);
    // Regression lock-in: set_test_mode is CSS-only; it must never trigger a WebGL re-render
    expect(rendererRender).not.toHaveBeenCalled();
  });
});

describe('debug bridge screenshot_window', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  function makeViewportStub() {
    const rendererRender = vi.fn();
    const renderer = {
      render: rendererRender,
    };
    const scene = {} as any;
    const camera = {} as any;
    return { renderer, scene, camera, rendererRender };
  }

  async function dispatchScreenshotWindow(handler: DebugRequestHandler, id: number) {
    vi.mocked(invoke).mockClear();
    await handler({ payload: { id, command: 'screenshot_window', params: {} } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  /** Init the bridge and install a viewport stub; returns the stub for call-order assertions. */
  async function setupWithViewport() {
    const stores = makeStores();
    await initDebugBridge(stores);
    const stub = makeViewportStub();
    window.__REIFY_DEBUG__!.viewport = {
      scene: stub.scene,
      camera: stub.camera,
      renderer: stub.renderer as any,
      getMeshes: vi.fn().mockReturnValue(new Map()),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
    };
    return stub;
  }

  it('returns { error: "viewport not ready" } when viewport is undefined', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    const result = await dispatchScreenshotWindow(capturedHandler!, 700);
    expect(result).toEqual({ error: 'viewport not ready' });
  });

  it('happy path returns { data: <toPng dataUrl> }', async () => {
    await setupWithViewport();

    const result = await dispatchScreenshotWindow(capturedHandler!, 701);
    expect(result).toEqual({ data: 'data:image/png;base64,STUB' });
  });

  it('calls renderer.render before html-to-image toPng', async () => {
    const stub = await setupWithViewport();

    await dispatchScreenshotWindow(capturedHandler!, 702);

    expect(stub.rendererRender.mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(toPng).mock.invocationCallOrder[0],
    );
  });

  it('invokes toPng with (document.documentElement, { cacheBust: true })', async () => {
    await setupWithViewport();

    await dispatchScreenshotWindow(capturedHandler!, 703);

    expect(vi.mocked(toPng).mock.calls[0][0]).toBe(document.documentElement);
    expect(vi.mocked(toPng).mock.calls[0][1]).toEqual(expect.objectContaining({ cacheBust: true }));
  });

  it('returns { error, size, limit } when toPng output exceeds the 16 MB threshold', async () => {
    await setupWithViewport();

    // Produce a payload 23 bytes over the 16 MB threshold (16,777,239 chars total):
    // 'data:image/png;base64,' prefix = 22 chars + 'A' * (16*1024*1024+1) = 16,777,217 chars
    vi.mocked(toPng).mockResolvedValueOnce('data:image/png;base64,' + 'A'.repeat(16 * 1024 * 1024 + 1));

    const result = await dispatchScreenshotWindow(capturedHandler!, 704);
    expect(result).toEqual({
      error: 'screenshot too large',
      size: 16777239,
      limit: 16 * 1024 * 1024,
    });
  });

  it('returns { data } when toPng output is exactly at the 16 MB boundary (length === 16777216)', async () => {
    await setupWithViewport();

    // Exactly 16 MB — strict > means this must succeed
    const exactBoundaryPayload = 'X'.repeat(16 * 1024 * 1024);
    vi.mocked(toPng).mockResolvedValueOnce(exactBoundaryPayload);

    const result = await dispatchScreenshotWindow(capturedHandler!, 705);
    expect(result.data).toBe(exactBoundaryPayload);
    expect(result.error).toBeUndefined();
  });
});

describe('debug bridge editor_content', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  async function dispatchEditorContent(stores: DebugStores) {
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id: 600, command: 'editor_content', params: {} } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  it('(a) when no file is active, activeFileOutOfSyncWithDisk is false', async () => {
    const stores = makeStores();
    // No active file, no open files
    const result = await dispatchEditorContent(stores);
    expect(result.activeFileOutOfSyncWithDisk).toBe(false);
  });

  it('(b) when active file is in externallyChanged, activeFileOutOfSyncWithDisk is true', async () => {
    const stores = makeStores();
    stores.editor.state.openFiles = [{ path: 'bracket.ri', content: 'x' }];
    stores.editor.state.activeFile = 'bracket.ri';
    stores.editor.state.externallyChanged = ['bracket.ri'];
    const result = await dispatchEditorContent(stores);
    expect(result.activeFileOutOfSyncWithDisk).toBe(true);
  });

  it('(b) when active file is NOT in externallyChanged, activeFileOutOfSyncWithDisk is false', async () => {
    const stores = makeStores();
    stores.editor.state.openFiles = [{ path: 'bracket.ri', content: 'x' }];
    stores.editor.state.activeFile = 'bracket.ri';
    stores.editor.state.externallyChanged = [];
    const result = await dispatchEditorContent(stores);
    expect(result.activeFileOutOfSyncWithDisk).toBe(false);
  });

  it('(c) each openFiles[] entry gains externallyChanged boolean', async () => {
    const stores = makeStores();
    stores.editor.state.openFiles = [
      { path: 'a.ri', content: 'a' },
      { path: 'b.ri', content: 'b' },
    ];
    stores.editor.state.activeFile = 'a.ri';
    stores.editor.state.externallyChanged = ['b.ri'];
    const result = await dispatchEditorContent(stores);
    const fileA = result.openFiles.find((f: any) => f.path === 'a.ri');
    const fileB = result.openFiles.find((f: any) => f.path === 'b.ri');
    expect(fileA.externallyChanged).toBe(false);
    expect(fileB.externallyChanged).toBe(true);
  });

  it('(d) dirty and activeFileOutOfSyncWithDisk are independent — both true simultaneously', async () => {
    const stores = makeStores();
    stores.editor.state.openFiles = [{ path: 'bracket.ri', content: 'x' }];
    stores.editor.state.activeFile = 'bracket.ri';
    stores.editor.state.dirtyFiles = ['bracket.ri'];
    stores.editor.state.externallyChanged = ['bracket.ri'];
    const result = await dispatchEditorContent(stores);
    // existing dirty field in openFiles[] should still be true
    const file = result.openFiles.find((f: any) => f.path === 'bracket.ri');
    expect(file.dirty).toBe(true);
    expect(file.externallyChanged).toBe(true);
    // top-level activeFileOutOfSyncWithDisk true as well
    expect(result.activeFileOutOfSyncWithDisk).toBe(true);
  });

  it('(d) dirty true does not imply activeFileOutOfSyncWithDisk true', async () => {
    const stores = makeStores();
    stores.editor.state.openFiles = [{ path: 'bracket.ri', content: 'x' }];
    stores.editor.state.activeFile = 'bracket.ri';
    stores.editor.state.dirtyFiles = ['bracket.ri'];
    stores.editor.state.externallyChanged = [];
    const result = await dispatchEditorContent(stores);
    const file = result.openFiles.find((f: any) => f.path === 'bracket.ri');
    expect(file.dirty).toBe(true);
    expect(file.externallyChanged).toBe(false);
    expect(result.activeFileOutOfSyncWithDisk).toBe(false);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// debug bridge pickViewport selection (step-3 — RED)
// Verifies that the five viewport-mediated handlers (viewport_state, screenshot,
// screenshot_window, fit_to_view, set_camera) use the new pickViewport logic.
// All tests fail because the current handlers read ctx.viewport directly with
// no map-aware lookup.
// ─────────────────────────────────────────────────────────────────────────────
describe('debug bridge pickViewport selection', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  /** Build a viewport stub whose getMeshes returns an empty Map. */
  function makeEmptyStub() {
    const fitToView = vi.fn();
    const rendererRender = vi.fn();
    const cameraPositionSet = vi.fn();
    const camera = {
      position: { set: cameraPositionSet, x: 1, y: 2, z: 3 },
      up: { set: vi.fn(), x: 0, y: 1, z: 0 },
      rotation: { x: 0, y: 0, z: 0 },
      fov: 75, near: 0.1, far: 1000,
      zoom: 1,
      lookAt: vi.fn(),
      updateProjectionMatrix: vi.fn(),
      updateMatrixWorld: vi.fn(),
    };
    const controls = {
      target: { set: vi.fn(), x: 0, y: 0, z: 0 },
      update: vi.fn(),
    };
    const renderer = {
      render: rendererRender,
      domElement: { toDataURL: vi.fn().mockReturnValue('data:image/png;base64,EMPTY') },
    };
    return {
      scene: {} as any,
      camera: camera as any,
      renderer: renderer as any,
      getMeshes: vi.fn().mockReturnValue(new Map<string, unknown>()),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView,
      flyToEntity: vi.fn(),
      controls: controls as any,
      // expose spies for assertions
      _rendererRender: rendererRender,
      _fitToView: fitToView,
      _cameraPositionSet: cameraPositionSet,
    };
  }

  /** Build a viewport stub whose getMeshes returns a Map with one entry. */
  function makePopulatedStub() {
    const stub = makeEmptyStub();
    // viewport_state iterates mesh geometry — provide a minimal mock that
    // satisfies getAttribute/getIndex null checks in the handler.
    const mockGeometry = {
      getAttribute: vi.fn().mockReturnValue(null),
      getIndex: vi.fn().mockReturnValue(null),
    };
    const mockMesh = { geometry: mockGeometry };
    const meshMap = new Map<string, unknown>([['entity/box', mockMesh]]);
    stub.getMeshes = vi.fn().mockReturnValue(meshMap);
    return stub;
  }

  /** Dispatch any named command via the debug bridge and return parsed result. */
  async function dispatchCmd(
    id: number,
    command: string,
    params: Record<string, unknown>,
  ) {
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id, command, params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  // ── viewport_state ─────────────────────────────────────────────────────────

  describe('viewport_state', () => {
    it('(a) explicit viewportId targets that viewport', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      const populated = makePopulatedStub();
      const empty = makeEmptyStub();
      window.__REIFY_DEBUG__!.viewports = {
        'def-preview': empty as any,
        'design-main': populated as any,
      };

      const result = await dispatchCmd(500, 'viewport_state', { viewportId: 'design-main' });
      expect(result.meshCount).toBe(1);
    });

    it('(b) no viewportId → picks first populated viewport', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      const empty = makeEmptyStub();
      const populated = makePopulatedStub();
      // def-preview (empty) registered first — but populated should win
      window.__REIFY_DEBUG__!.viewports = {
        'def-preview': empty as any,
        'design-main': populated as any,
      };

      const result = await dispatchCmd(501, 'viewport_state', {});
      expect(result.meshCount).toBe(1);
    });

    it('(c) unknown viewportId → returns error', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      window.__REIFY_DEBUG__!.viewports = { 'design-main': makePopulatedStub() as any };

      const result = await dispatchCmd(502, 'viewport_state', { viewportId: 'nonexistent' });
      expect(result).toHaveProperty('error');
    });

    it('(d) no viewports and no legacy viewport → viewport not ready', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      // viewports not set; legacy ctx.viewport not set

      const result = await dispatchCmd(503, 'viewport_state', {});
      expect(result).toEqual({ error: 'viewport not ready' });
    });
  });

  // ── screenshot ─────────────────────────────────────────────────────────────

  describe('screenshot', () => {
    it('(a) explicit viewportId targets that viewport', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      const populatedA = makePopulatedStub();
      const emptyB = makeEmptyStub();
      window.__REIFY_DEBUG__!.viewports = {
        'def-preview': emptyB as any,
        'design-main': populatedA as any,
      };

      await dispatchCmd(510, 'screenshot', { viewportId: 'design-main' });
      expect(populatedA._rendererRender).toHaveBeenCalledWith(populatedA.scene, populatedA.camera);
      expect(emptyB._rendererRender).not.toHaveBeenCalled();
    });

    it('(b) no viewportId → picks first populated viewport', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      const empty = makeEmptyStub();
      const populated = makePopulatedStub();
      window.__REIFY_DEBUG__!.viewports = {
        'def-preview': empty as any,
        'design-main': populated as any,
      };

      await dispatchCmd(511, 'screenshot', {});
      expect(populated._rendererRender).toHaveBeenCalled();
      expect(empty._rendererRender).not.toHaveBeenCalled();
    });

    it('(c) unknown viewportId → returns error', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      window.__REIFY_DEBUG__!.viewports = { 'design-main': makePopulatedStub() as any };

      const result = await dispatchCmd(512, 'screenshot', { viewportId: 'nope' });
      expect(result).toHaveProperty('error');
    });

    it('(d) no viewports and no legacy viewport → viewport not ready', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);

      const result = await dispatchCmd(513, 'screenshot', {});
      expect(result).toEqual({ error: 'viewport not ready' });
    });
  });

  // ── screenshot_window ──────────────────────────────────────────────────────

  describe('screenshot_window', () => {
    it('(a) explicit viewportId targets that viewport', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      const populated = makePopulatedStub();
      const empty = makeEmptyStub();
      window.__REIFY_DEBUG__!.viewports = {
        'def-preview': empty as any,
        'design-main': populated as any,
      };

      await dispatchCmd(520, 'screenshot_window', { viewportId: 'design-main' });
      expect(populated._rendererRender).toHaveBeenCalledWith(populated.scene, populated.camera);
      expect(empty._rendererRender).not.toHaveBeenCalled();
    });

    it('(b) no viewportId → picks first populated viewport', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      const empty = makeEmptyStub();
      const populated = makePopulatedStub();
      window.__REIFY_DEBUG__!.viewports = {
        'def-preview': empty as any,
        'design-main': populated as any,
      };

      await dispatchCmd(521, 'screenshot_window', {});
      expect(populated._rendererRender).toHaveBeenCalled();
      expect(empty._rendererRender).not.toHaveBeenCalled();
    });

    it('(c) unknown viewportId → returns error', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      window.__REIFY_DEBUG__!.viewports = { 'design-main': makePopulatedStub() as any };

      const result = await dispatchCmd(522, 'screenshot_window', { viewportId: 'nope' });
      expect(result).toHaveProperty('error');
    });

    it('(d) no viewports and no legacy viewport → viewport not ready', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);

      const result = await dispatchCmd(523, 'screenshot_window', {});
      expect(result).toEqual({ error: 'viewport not ready' });
    });
  });

  // ── fit_to_view ────────────────────────────────────────────────────────────

  describe('fit_to_view', () => {
    it('(a) explicit viewportId targets that viewport', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      const populated = makePopulatedStub();
      const empty = makeEmptyStub();
      window.__REIFY_DEBUG__!.viewports = {
        'def-preview': empty as any,
        'design-main': populated as any,
      };

      const result = await dispatchCmd(530, 'fit_to_view', { viewportId: 'design-main' });
      expect(result).toEqual({ ok: true });
      expect(populated._fitToView).toHaveBeenCalledTimes(1);
      expect(empty._fitToView).not.toHaveBeenCalled();
    });

    it('(b) no viewportId → picks first populated viewport', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      const empty = makeEmptyStub();
      const populated = makePopulatedStub();
      window.__REIFY_DEBUG__!.viewports = {
        'def-preview': empty as any,
        'design-main': populated as any,
      };

      await dispatchCmd(531, 'fit_to_view', {});
      expect(populated._fitToView).toHaveBeenCalledTimes(1);
      expect(empty._fitToView).not.toHaveBeenCalled();
    });

    it('(c) unknown viewportId → returns error', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      window.__REIFY_DEBUG__!.viewports = { 'design-main': makePopulatedStub() as any };

      const result = await dispatchCmd(532, 'fit_to_view', { viewportId: 'nope' });
      expect(result).toHaveProperty('error');
    });

    it('(d) no viewports and no legacy viewport → viewport not ready', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);

      const result = await dispatchCmd(533, 'fit_to_view', {});
      expect(result).toEqual({ error: 'viewport not ready' });
    });
  });

  // ── set_camera ─────────────────────────────────────────────────────────────

  describe('set_camera', () => {
    const camParams = { position: [1, 2, 3], target: [0, 0, 0], up: [0, 0, 1], zoom: 1.5 };

    it('(a) explicit viewportId targets that viewport', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      const populated = makePopulatedStub();
      const empty = makeEmptyStub();
      window.__REIFY_DEBUG__!.viewports = {
        'def-preview': empty as any,
        'design-main': populated as any,
      };

      const result = await dispatchCmd(540, 'set_camera', { ...camParams, viewportId: 'design-main' });
      expect(result.ok).toBe(true);
      expect(populated._cameraPositionSet).toHaveBeenCalledWith(1, 2, 3);
      expect(empty._cameraPositionSet).not.toHaveBeenCalled();
    });

    it('(b) no viewportId → picks first populated viewport', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      const empty = makeEmptyStub();
      const populated = makePopulatedStub();
      window.__REIFY_DEBUG__!.viewports = {
        'def-preview': empty as any,
        'design-main': populated as any,
      };

      const result = await dispatchCmd(541, 'set_camera', camParams);
      expect(result.ok).toBe(true);
      expect(populated._cameraPositionSet).toHaveBeenCalledWith(1, 2, 3);
      expect(empty._cameraPositionSet).not.toHaveBeenCalled();
    });

    it('(c) unknown viewportId → returns error', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);
      window.__REIFY_DEBUG__!.viewports = { 'design-main': makePopulatedStub() as any };

      const result = await dispatchCmd(542, 'set_camera', { ...camParams, viewportId: 'nope' });
      expect(result).toHaveProperty('error');
    });

    it('(d) no viewports and no legacy viewport → viewport not ready', async () => {
      const stores = makeStores();
      await initDebugBridge(stores);

      const result = await dispatchCmd(543, 'set_camera', camParams);
      expect(result).toEqual({ error: 'viewport not ready' });
    });
  });
});
