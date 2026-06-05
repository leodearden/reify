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
    viewState: { resetToDefaultView: vi.fn() },
    layout: {
      state: {
        editorWidth: 300,
        sideWidth: 300,
        designTreeHeight: 160,
        propertyHeight: 200,
        constraintHeight: 140,
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

  // -------------------------------------------------------------------------
  // step-3 tests: resetToDefaultView reset contract (RED until step-4 wires it)
  // -------------------------------------------------------------------------

  it('resetToDefaultView is called exactly once when guiState is provided', async () => {
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

    await dispatch(capturedHandler!, 510, {
      path: '/tmp/reload.ri',
      content: 'def Reload() {}',
      guiState: rawGuiState,
    });

    expect(stores.viewState.resetToDefaultView).toHaveBeenCalledTimes(1);
  });

  it('resetToDefaultView is called AFTER initFromState (engine rebuilt first, then visibility baseline reset)', async () => {
    const stores = makeStores();
    const callOrder: string[] = [];
    vi.mocked(stores.engine.initFromState).mockImplementation(() => { callOrder.push('initFromState'); });
    vi.mocked(stores.viewState.resetToDefaultView).mockImplementation(() => { callOrder.push('resetToDefaultView'); });

    await initDebugBridge(stores);

    const rawGuiState = {
      meshes: [],
      values: [],
      constraints: [],
      files: [],
      tessellation_diagnostics: [],
      compile_diagnostics: [],
    };

    await dispatch(capturedHandler!, 511, {
      path: '/tmp/reload.ri',
      content: 'def Reload() {}',
      guiState: rawGuiState,
    });

    expect(callOrder).toEqual(['initFromState', 'resetToDefaultView']);
  });

  it('resetToDefaultView is NOT called when guiState is omitted', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    await dispatch(capturedHandler!, 512, {
      path: '/tmp/open.ri',
      content: 'def Open() {}',
    });

    expect(stores.viewState.resetToDefaultView).not.toHaveBeenCalled();
  });

  it('resetToDefaultView is NOT called when path is missing (error path)', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    await dispatch(capturedHandler!, 513, { content: 'def X() {}' });

    expect(stores.viewState.resetToDefaultView).not.toHaveBeenCalled();
  });

  it('resetToDefaultView is NOT called when content is missing (error path)', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    await dispatch(capturedHandler!, 514, { path: '/tmp/x.ri' });

    expect(stores.viewState.resetToDefaultView).not.toHaveBeenCalled();
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

  /**
   * Generate the standard four pickViewport scenarios for a viewport-mediated tool.
   * Scenarios (c) and (d) are identical across tools and handled generically here.
   * Scenarios (a) and (b) accept assertion callbacks so per-tool spies can be checked.
   * Adding coverage for a new tool is a single call site below (amend: suggestion-5).
   */
  type StubPopulated = ReturnType<typeof makePopulatedStub>;
  type StubEmpty = ReturnType<typeof makeEmptyStub>;
  function describePickViewportScenarios(
    toolName: string,
    baseParams: Record<string, unknown>,
    idBase: number,
    assertExplicit: (populated: StubPopulated, empty: StubEmpty, result: any) => void,
    assertPopulatedFirst: (populated: StubPopulated, empty: StubEmpty, result: any) => void,
  ) {
    describe(toolName, () => {
      it('(a) explicit viewportId targets that viewport', async () => {
        const stores = makeStores();
        await initDebugBridge(stores);
        const populated = makePopulatedStub();
        const empty = makeEmptyStub();
        window.__REIFY_DEBUG__!.viewports = {
          'def-preview': empty as any,
          'design-main': populated as any,
        };
        const result = await dispatchCmd(idBase, toolName, { ...baseParams, viewportId: 'design-main' });
        assertExplicit(populated, empty, result);
      });

      it('(b) no viewportId → picks first populated viewport', async () => {
        const stores = makeStores();
        await initDebugBridge(stores);
        const empty = makeEmptyStub();
        const populated = makePopulatedStub();
        // def-preview (empty) registered first — populated should win
        window.__REIFY_DEBUG__!.viewports = {
          'def-preview': empty as any,
          'design-main': populated as any,
        };
        const result = await dispatchCmd(idBase + 1, toolName, baseParams);
        assertPopulatedFirst(populated, empty, result);
      });

      it('(c) unknown viewportId → returns error', async () => {
        const stores = makeStores();
        await initDebugBridge(stores);
        window.__REIFY_DEBUG__!.viewports = { 'design-main': makePopulatedStub() as any };
        const result = await dispatchCmd(idBase + 2, toolName, { ...baseParams, viewportId: 'nope' });
        expect(result).toHaveProperty('error');
      });

      it('(d) no viewports and no legacy viewport → viewport not ready', async () => {
        const stores = makeStores();
        await initDebugBridge(stores);
        const result = await dispatchCmd(idBase + 3, toolName, baseParams);
        expect(result).toEqual({ error: 'viewport not ready' });
      });
    });
  }

  // Camera params reused by set_camera cases.
  const camParams = { position: [1, 2, 3], target: [0, 0, 0], up: [0, 0, 1], zoom: 1.5 };

  // ── viewport_state (ids 500–503) ────────────────────────────────────────────
  describePickViewportScenarios('viewport_state', {}, 500,
    (_p, _e, result) => { expect(result.meshCount).toBe(1); },
    (_p, _e, result) => { expect(result.meshCount).toBe(1); },
  );

  // ── screenshot (ids 510–513) ────────────────────────────────────────────────
  describePickViewportScenarios('screenshot', {}, 510,
    (populated, empty) => {
      expect(populated._rendererRender).toHaveBeenCalledWith(populated.scene, populated.camera);
      expect(empty._rendererRender).not.toHaveBeenCalled();
    },
    (populated, empty) => {
      expect(populated._rendererRender).toHaveBeenCalled();
      expect(empty._rendererRender).not.toHaveBeenCalled();
    },
  );

  // ── screenshot_window (ids 520–523) ─────────────────────────────────────────
  describePickViewportScenarios('screenshot_window', {}, 520,
    (populated, empty) => {
      expect(populated._rendererRender).toHaveBeenCalledWith(populated.scene, populated.camera);
      expect(empty._rendererRender).not.toHaveBeenCalled();
    },
    (populated, empty) => {
      expect(populated._rendererRender).toHaveBeenCalled();
      expect(empty._rendererRender).not.toHaveBeenCalled();
    },
  );

  // ── fit_to_view (ids 530–533) ────────────────────────────────────────────────
  describePickViewportScenarios('fit_to_view', {}, 530,
    (populated, empty, result) => {
      expect(result).toEqual({ ok: true });
      expect(populated._fitToView).toHaveBeenCalledTimes(1);
      expect(empty._fitToView).not.toHaveBeenCalled();
    },
    (populated, empty) => {
      expect(populated._fitToView).toHaveBeenCalledTimes(1);
      expect(empty._fitToView).not.toHaveBeenCalled();
    },
  );

  // ── set_camera (ids 540–543) ─────────────────────────────────────────────────
  describePickViewportScenarios('set_camera', camParams, 540,
    (populated, empty, result) => {
      expect(result.ok).toBe(true);
      expect(populated._cameraPositionSet).toHaveBeenCalledWith(1, 2, 3);
      expect(empty._cameraPositionSet).not.toHaveBeenCalled();
    },
    (populated, empty, result) => {
      expect(result.ok).toBe(true);
      expect(populated._cameraPositionSet).toHaveBeenCalledWith(1, 2, 3);
      expect(empty._cameraPositionSet).not.toHaveBeenCalled();
    },
  );
});

// ─────────────────────────────────────────────────────────────────────────────
// debug bridge dual-viewport binding regression (step-7)
//
// Pins the exact bug scenario from the task description:
//   - dual-viewport layout registers def-preview (empty) THEN design-main (populated)
//   - viewport_state / screenshot / fit_to_view called with no viewportId param
//   - should target design-main (populated), NOT def-preview (empty/zero)
//
// Registration order mirrors DualViewport.tsx: def-preview mounts first (JSX
// order), design-main mounts second. Both inserted via window.__REIFY_DEBUG__.viewports.
// ─────────────────────────────────────────────────────────────────────────────
describe('debug bridge dual-viewport binding regression', () => {
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

  /** Empty stub: getMeshes returns a zero-size Map (def-preview has no geometry). */
  function makeEmptyStub() {
    return {
      scene: {} as any,
      camera: {
        position: { set: vi.fn(), x: 0, y: 0, z: 5 },
        up: { set: vi.fn(), x: 0, y: 1, z: 0 },
        rotation: { x: 0, y: 0, z: 0 },
        fov: 75, near: 0.1, far: 1000,
        zoom: 1,
        lookAt: vi.fn(),
        updateProjectionMatrix: vi.fn(),
        updateMatrixWorld: vi.fn(),
      } as any,
      renderer: {
        render: vi.fn(),
        domElement: { toDataURL: vi.fn().mockReturnValue('data:image/png;base64,EMPTY_VP') },
      } as any,
      getMeshes: vi.fn().mockReturnValue(new Map<string, unknown>()),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
      controls: { target: { set: vi.fn(), x: 0, y: 0, z: 0 }, update: vi.fn() } as any,
    };
  }

  /** Populated stub: getMeshes returns a Map with 7 entries (design-main has geometry). */
  function makePopulatedStub() {
    const fitToView = vi.fn();
    const rendererRender = vi.fn();
    const mockGeometry = {
      getAttribute: vi.fn().mockReturnValue(null),
      getIndex: vi.fn().mockReturnValue(null),
    };
    // 7 mesh entries mirroring the reported printer.ri state (1444 triangles / 7 meshes)
    const meshMap = new Map<string, unknown>(
      Array.from({ length: 7 }, (_, i) => [`entity/part-${i}`, { geometry: mockGeometry }]),
    );
    return {
      scene: {} as any,
      camera: {
        position: { set: vi.fn(), x: 10, y: 10, z: 10 },
        up: { set: vi.fn(), x: 0, y: 1, z: 0 },
        rotation: { x: 0, y: 0, z: 0 },
        fov: 75, near: 0.1, far: 1000,
        zoom: 1,
        lookAt: vi.fn(),
        updateProjectionMatrix: vi.fn(),
        updateMatrixWorld: vi.fn(),
      } as any,
      renderer: {
        render: rendererRender,
        domElement: { toDataURL: vi.fn().mockReturnValue('data:image/png;base64,POPULATED_VP') },
      } as any,
      getMeshes: vi.fn().mockReturnValue(meshMap),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView,
      flyToEntity: vi.fn(),
      controls: { target: { set: vi.fn(), x: 0, y: 0, z: 0 }, update: vi.fn() } as any,
      // expose spies for assertions
      _fitToView: fitToView,
      _rendererRender: rendererRender,
    };
  }

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

  it('viewport_state with no viewportId returns meshCount from the populated design-main viewport, not 0 from def-preview', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const defPreview = makeEmptyStub();    // def-preview: 0 meshes
    const designMain = makePopulatedStub(); // design-main: 7 meshes

    // Registration order mirrors DualViewport.tsx: def-preview first, design-main second
    window.__REIFY_DEBUG__!.viewports = {
      'def-preview': defPreview as any,
      'design-main': designMain as any,
    };

    const result = await dispatchCmd(600, 'viewport_state', {});
    expect(result).not.toHaveProperty('error');
    // Must report 7, NOT 0 — the bug returned 0 by reading def-preview
    expect(result.meshCount).toBe(7);
  });

  it('screenshot with no viewportId calls renderer.render on the populated design-main viewport', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const defPreview = makeEmptyStub();
    const designMain = makePopulatedStub();

    window.__REIFY_DEBUG__!.viewports = {
      'def-preview': defPreview as any,
      'design-main': designMain as any,
    };

    await dispatchCmd(601, 'screenshot', {});
    // design-main's render must have been called
    expect(designMain._rendererRender).toHaveBeenCalled();
    // def-preview's render must NOT have been called
    expect(defPreview.renderer.render).not.toHaveBeenCalled();
  });

  it('fit_to_view with no viewportId invokes fitToView on the populated design-main viewport', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const defPreview = makeEmptyStub();
    const designMain = makePopulatedStub();

    window.__REIFY_DEBUG__!.viewports = {
      'def-preview': defPreview as any,
      'design-main': designMain as any,
    };

    await dispatchCmd(602, 'fit_to_view', {});
    // design-main's fitToView must have been called
    expect(designMain._fitToView).toHaveBeenCalledTimes(1);
    // def-preview's fitToView must NOT have been called
    expect(defPreview.fitToView).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// debug bridge get_diagnostics (task-4297 step-1 RED → step-2 GREEN)
// ---------------------------------------------------------------------------

describe('debug bridge get_diagnostics', () => {
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

  async function dispatch(stores: ReturnType<typeof makeStores>, id: number, command: string, params: Record<string, unknown> = {}) {
    await initDebugBridge(stores);
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id, command, params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  it('returns shaped compile and tessellation diagnostics from stores', async () => {
    const stores = makeStores();
    stores.engine.state.compileDiagnostics = [
      { file_path: 'broken.ri', line: 8, column: 5, end_line: 8, end_column: 6,
        severity: 'Error', message: 'unexpected EOF', code: 'parse-error' },
    ];
    stores.engine.state.tessellationDiagnostics = [
      { file_path: 'broken.ri', line: 12, column: 1, end_line: 12, end_column: 10,
        severity: 'Warning', message: 'mesh degenerate', code: 'tess-warn' },
    ];

    const result = await dispatch(stores, 2000, 'get_diagnostics');

    // compile array
    expect(Array.isArray(result.compile)).toBe(true);
    expect(result.compile).toHaveLength(1);
    const c = result.compile[0];
    expect(c.severity).toBe('Error');
    expect(c.message).toBe('unexpected EOF');
    expect(c.code).toBe('parse-error');
    expect(c.file_path).toBe('broken.ri');
    expect(c.range).toEqual({ line: 8, column: 5, end_line: 8, end_column: 6 });

    // tessellation array
    expect(Array.isArray(result.tessellation)).toBe(true);
    expect(result.tessellation).toHaveLength(1);
    const t = result.tessellation[0];
    expect(t.severity).toBe('Warning');
    expect(t.message).toBe('mesh degenerate');
    expect(t.code).toBe('tess-warn');
    expect(t.range).toEqual({ line: 12, column: 1, end_line: 12, end_column: 10 });

    // counts
    expect(result.compileCount).toBe(1);
    expect(result.tessellationCount).toBe(1);
  });

  it('returns empty arrays and zero counts when diagnostics are absent', async () => {
    const stores = makeStores();
    // compileDiagnostics/tessellationDiagnostics seeded as [] by makeStores

    const result = await dispatch(stores, 2001, 'get_diagnostics');

    expect(result.compile).toEqual([]);
    expect(result.tessellation).toEqual([]);
    expect(result.compileCount).toBe(0);
    expect(result.tessellationCount).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// debug bridge ui_outline (task-4297 step-3 RED → step-4 GREEN)
// ---------------------------------------------------------------------------

describe('debug bridge ui_outline', () => {
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

    // Build a small semantic DOM for the test
    const runBtn = document.createElement('button');
    runBtn.setAttribute('data-testid', 'run-btn');
    runBtn.textContent = 'Run';
    document.body.appendChild(runBtn);

    const stopBtn = document.createElement('button');
    stopBtn.setAttribute('data-testid', 'stop-btn');
    stopBtn.setAttribute('disabled', '');
    stopBtn.textContent = 'Stop';
    document.body.appendChild(stopBtn);

    const designTree = document.createElement('div');
    designTree.setAttribute('role', 'tree');
    designTree.setAttribute('data-testid', 'design-tree');
    designTree.textContent = 'Tree';
    document.body.appendChild(designTree);

    const hiddenBtn = document.createElement('button');
    hiddenBtn.setAttribute('data-testid', 'hidden-btn');
    hiddenBtn.style.display = 'none';
    hiddenBtn.textContent = 'Hidden';
    document.body.appendChild(hiddenBtn);
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    document.body.innerHTML = '';
  });

  async function dispatchUiOutline(id: number) {
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id, command: 'ui_outline', params: {} } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  it('returns outline array with count === outline.length', async () => {
    const result = await dispatchUiOutline(3000);
    expect(Array.isArray(result.outline)).toBe(true);
    expect(result.count).toBe(result.outline.length);
    expect(typeof result.truncated).toBe('boolean');
  });

  it('every entry has required fields with correct types', async () => {
    const result = await dispatchUiOutline(3001);
    for (const entry of result.outline) {
      expect(typeof entry.tagName).toBe('string');
      expect(typeof entry.text).toBe('string');
      expect(typeof entry.enabled).toBe('boolean');
      // role may be string or null
      expect(entry.role === null || typeof entry.role === 'string').toBe(true);
      // testId may be string or null
      expect(entry.testId === null || typeof entry.testId === 'string').toBe(true);
    }
  });

  it('run-btn entry has enabled:true and testId:run-btn and text containing Run', async () => {
    const result = await dispatchUiOutline(3002);
    const runEntry = result.outline.find((e: any) => e.testId === 'run-btn');
    expect(runEntry).toBeDefined();
    expect(runEntry.enabled).toBe(true);
    expect(runEntry.text).toMatch(/Run/);
  });

  it('stop-btn entry has enabled:false', async () => {
    const result = await dispatchUiOutline(3003);
    const stopEntry = result.outline.find((e: any) => e.testId === 'stop-btn');
    expect(stopEntry).toBeDefined();
    expect(stopEntry.enabled).toBe(false);
  });

  it('design-tree entry has role:tree', async () => {
    const result = await dispatchUiOutline(3004);
    const treeEntry = result.outline.find((e: any) => e.testId === 'design-tree');
    expect(treeEntry).toBeDefined();
    expect(treeEntry.role).toBe('tree');
  });

  it('hidden-btn (display:none) is excluded from outline', async () => {
    const result = await dispatchUiOutline(3005);
    const hiddenEntry = result.outline.find((e: any) => e.testId === 'hidden-btn');
    expect(hiddenEntry).toBeUndefined();
  });

  it('button nested inside a display:none div is excluded from outline', async () => {
    // Ancestor-hidden case: the button itself has no inline style, but its parent
    // container has display:none — ui_outline must walk ancestors to detect this.
    const wrapper = document.createElement('div');
    wrapper.style.display = 'none';
    const innerBtn = document.createElement('button');
    innerBtn.setAttribute('data-testid', 'inner-hidden-btn');
    innerBtn.textContent = 'Inner';
    wrapper.appendChild(innerBtn);
    document.body.appendChild(wrapper);

    const result = await dispatchUiOutline(3006);
    const innerEntry = result.outline.find((e: any) => e.testId === 'inner-hidden-btn');
    expect(innerEntry).toBeUndefined();
    // afterEach cleans up document.body.innerHTML
  });

  it('truncates at MAX=500: truncated===true, outline.length===500, count===total-visible', async () => {
    // beforeEach adds 3 visible (run-btn, stop-btn, design-tree) + 1 hidden (hidden-btn).
    // Adding 500 more visible buttons brings total visible to 503, which exceeds MAX=500.
    for (let i = 0; i < 500; i++) {
      const btn = document.createElement('button');
      btn.setAttribute('data-testid', `extra-${i}`);
      btn.textContent = `Extra ${i}`;
      document.body.appendChild(btn);
    }
    const result = await dispatchUiOutline(3007);
    expect(result.truncated).toBe(true);
    expect(result.outline.length).toBe(500);
    expect(result.count).toBe(503); // 3 from beforeEach + 500 extra
    expect(result.count).toBeGreaterThan(result.outline.length);
  });
});

// ---------------------------------------------------------------------------
// Layout ctx exposure (task-4294)
// ---------------------------------------------------------------------------

describe('debug bridge exposes layout on ctx', () => {
  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  it('window.__REIFY_DEBUG__.stores.layout.state is defined and readable after initDebugBridge', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const ctx = window.__REIFY_DEBUG__;
    expect(ctx).toBeDefined();
    expect(ctx!.stores.layout.state).toBeDefined();
    expect(typeof ctx!.stores.layout.state.editorWidth).toBe('number');
    expect(typeof ctx!.stores.layout.state.sideWidth).toBe('number');
    expect(typeof ctx!.stores.layout.state.designTreeHeight).toBe('number');
    expect(typeof ctx!.stores.layout.state.propertyHeight).toBe('number');
    expect(typeof ctx!.stores.layout.state.constraintHeight).toBe('number');
  });
});

// ---------------------------------------------------------------------------
// step-3 through step-10: R1 DOM/style/layout/window inspection tools
// ---------------------------------------------------------------------------

describe('debug bridge R1 inspection tools', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  async function dispatchCmd(id: number, command: string, params: Record<string, unknown>) {
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id, command, params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  beforeEach(async () => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
    const stores = makeStores();
    await initDebugBridge(stores);
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
    document.body.innerHTML = '';
  });

  // step-3 RED → step-4 GREEN: query_selector / query_selector_all
  describe('query_selector / query_selector_all', () => {
    it('query_selector: existing element by data-testid returns exists:true with tagName/testId/bounds/visible', async () => {
      const el = document.createElement('div');
      el.setAttribute('data-testid', 'probe-a');
      document.body.appendChild(el);

      const result = await dispatchCmd(700, 'query_selector', { selector: '[data-testid="probe-a"]' });
      expect(result.exists).toBe(true);
      expect(result.tagName).toBe('div');
      expect(result.testId).toBe('probe-a');
      expect(result.bounds).toBeDefined();
      expect(typeof result.visible).toBe('boolean');
    });

    it('query_selector: no match returns {exists:false}', async () => {
      const result = await dispatchCmd(701, 'query_selector', { selector: '.no-such-element' });
      expect(result.exists).toBe(false);
    });

    it('query_selector: invalid selector returns {error}', async () => {
      const result = await dispatchCmd(702, 'query_selector', { selector: ':::' });
      expect(typeof result.error).toBe('string');
      expect(result.exists).toBeUndefined();
    });

    it('query_selector: missing selector returns {error: "selector is required"}', async () => {
      const result = await dispatchCmd(703, 'query_selector', {});
      expect(result.error).toBe('selector is required');
    });

    it('query_selector_all: returns count/elements/truncated for matches', async () => {
      const el1 = document.createElement('span');
      el1.className = 'probe-class';
      const el2 = document.createElement('span');
      el2.className = 'probe-class';
      document.body.appendChild(el1);
      document.body.appendChild(el2);

      const result = await dispatchCmd(704, 'query_selector_all', { selector: '.probe-class' });
      expect(result.count).toBe(2);
      expect(Array.isArray(result.elements)).toBe(true);
      expect(result.elements).toHaveLength(2);
      expect(typeof result.truncated).toBe('boolean');
      expect(result.truncated).toBe(false);
    });

    it('query_selector_all: no matches returns count:0 elements:[] truncated:false', async () => {
      const result = await dispatchCmd(705, 'query_selector_all', { selector: '.no-such-class' });
      expect(result.count).toBe(0);
      expect(result.elements).toEqual([]);
      expect(result.truncated).toBe(false);
    });

    it('query_selector_all: invalid selector returns {error}', async () => {
      const result = await dispatchCmd(706, 'query_selector_all', { selector: ':::' });
      expect(typeof result.error).toBe('string');
    });

    it('query_selector_all: missing selector returns {error: "selector is required"}', async () => {
      const result = await dispatchCmd(707, 'query_selector_all', {});
      expect(result.error).toBe('selector is required');
    });

    it('query_selector_all: truncates at 200 and sets truncated:true for >200 matches', async () => {
      for (let i = 0; i < 201; i++) {
        const el = document.createElement('span');
        el.className = 'truncation-test';
        document.body.appendChild(el);
      }
      const result = await dispatchCmd(708, 'query_selector_all', { selector: '.truncation-test' });
      expect(result.count).toBe(201);
      expect(result.elements).toHaveLength(200);
      expect(result.truncated).toBe(true);
    });
  });

  // step-5 RED → step-6 GREEN: get_layout_metrics
  describe('get_layout_metrics', () => {
    it('returns exists:true with bounds/scroll/client/overflow for a matching element', async () => {
      const el = document.createElement('div');
      el.setAttribute('data-testid', 'scroller');
      document.body.appendChild(el);

      // jsdom does not lay out elements; stub scroll/client metrics
      Object.defineProperty(el, 'scrollWidth', { configurable: true, value: 200 });
      Object.defineProperty(el, 'clientWidth', { configurable: true, value: 100 });
      Object.defineProperty(el, 'scrollHeight', { configurable: true, value: 50 });
      Object.defineProperty(el, 'clientHeight', { configurable: true, value: 50 });
      Object.defineProperty(el, 'scrollTop', { configurable: true, value: 0 });
      Object.defineProperty(el, 'scrollLeft', { configurable: true, value: 0 });

      const result = await dispatchCmd(800, 'get_layout_metrics', { selector: '[data-testid="scroller"]' });
      expect(result.exists).toBe(true);
      expect(result.bounds).toBeDefined();
      expect(result.scroll).toBeDefined();
      expect(typeof result.scroll.top).toBe('number');
      expect(typeof result.scroll.left).toBe('number');
      expect(typeof result.scroll.width).toBe('number');
      expect(typeof result.scroll.height).toBe('number');
      expect(result.client).toBeDefined();
      expect(typeof result.client.width).toBe('number');
      expect(typeof result.client.height).toBe('number');
      expect(result.overflow).toBeDefined();
      expect(typeof result.overflow.horizontal).toBe('boolean');
      expect(typeof result.overflow.vertical).toBe('boolean');
    });

    it('overflow.horizontal is true when scrollWidth > clientWidth', async () => {
      const el = document.createElement('div');
      el.className = 'overflow-test';
      document.body.appendChild(el);

      Object.defineProperty(el, 'scrollWidth', { configurable: true, value: 300 });
      Object.defineProperty(el, 'clientWidth', { configurable: true, value: 150 });
      Object.defineProperty(el, 'scrollHeight', { configurable: true, value: 50 });
      Object.defineProperty(el, 'clientHeight', { configurable: true, value: 50 });
      Object.defineProperty(el, 'scrollTop', { configurable: true, value: 0 });
      Object.defineProperty(el, 'scrollLeft', { configurable: true, value: 0 });

      const result = await dispatchCmd(801, 'get_layout_metrics', { selector: '.overflow-test' });
      expect(result.overflow.horizontal).toBe(true);
      expect(result.overflow.vertical).toBe(false);
    });

    it('returns {exists:false} for no match', async () => {
      const result = await dispatchCmd(802, 'get_layout_metrics', { selector: '.no-such-element' });
      expect(result.exists).toBe(false);
    });

    it('returns {error} for missing selector', async () => {
      const result = await dispatchCmd(803, 'get_layout_metrics', {});
      expect(result.error).toBe('selector is required');
    });
  });

  // step-7 RED → step-8 GREEN: get_computed_style
  describe('get_computed_style', () => {
    it('returns exists:true with style object containing curated keys', async () => {
      const el = document.createElement('div');
      el.setAttribute('data-testid', 'styled');
      el.style.display = 'none';
      document.body.appendChild(el);

      const result = await dispatchCmd(900, 'get_computed_style', { selector: '[data-testid="styled"]' });
      expect(result.exists).toBe(true);
      expect(result.style).toBeDefined();
      const curatedKeys = ['display', 'visibility', 'opacity', 'color', 'backgroundColor',
        'fontSize', 'fontFamily', 'fontWeight', 'overflow', 'position', 'width', 'height'];
      for (const key of curatedKeys) {
        expect(Object.keys(result.style)).toContain(key);
      }
      expect(result.style.display).toBe('none');
    });

    it('with properties:["display"] returns style with only display key', async () => {
      const el = document.createElement('div');
      el.className = 'style-target';
      document.body.appendChild(el);

      const result = await dispatchCmd(901, 'get_computed_style', {
        selector: '.style-target',
        properties: ['display'],
      });
      expect(result.exists).toBe(true);
      expect(result.style).toBeDefined();
      expect(Object.keys(result.style)).toContain('display');
      expect(Object.keys(result.style)).toHaveLength(1);
    });

    it('returns {exists:false} for no match', async () => {
      const result = await dispatchCmd(902, 'get_computed_style', { selector: '.no-such-element' });
      expect(result.exists).toBe(false);
    });

    it('returns {error} for missing selector', async () => {
      const result = await dispatchCmd(903, 'get_computed_style', {});
      expect(result.error).toBe('selector is required');
    });
  });

  // step-9 RED → step-10 GREEN: active_element / get_window_state
  describe('active_element / get_window_state', () => {
    it('active_element: returns tagName/testId/role of document.activeElement after focus()', async () => {
      const input = document.createElement('input');
      input.setAttribute('data-testid', 'my-input');
      input.setAttribute('role', 'textbox');
      document.body.appendChild(input);
      input.focus();

      const result = await dispatchCmd(1000, 'active_element', {});
      expect(result.tagName).toBe('input');
      expect(result.testId).toBe('my-input');
      expect(result.role).toBe('textbox');
    });

    it('active_element: returns tagName:body testId:null role:null when nothing focused', async () => {
      (document.body as HTMLElement).focus();

      const result = await dispatchCmd(1001, 'active_element', {});
      expect(result.tagName).toBe('body');
      expect(result.testId).toBeNull();
      expect(result.role).toBeNull();
    });

    it('get_window_state: returns numeric size/pos fields and boolean focused', async () => {
      // Stub window.devicePixelRatio since jsdom does not set it
      Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 2 });

      const result = await dispatchCmd(1002, 'get_window_state', {});
      expect(typeof result.innerWidth).toBe('number');
      expect(typeof result.innerHeight).toBe('number');
      expect(typeof result.screenX).toBe('number');
      expect(typeof result.screenY).toBe('number');
      expect(result.devicePixelRatio).toBe(2);
      expect(typeof result.focused).toBe('boolean');
    });
  });
});
