/**
 * Unit tests for the debug bridge handlers.
 * Covers: store_state / viewport_state selectedEntities; set_test_mode.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, cleanup } from '@solidjs/testing-library';
import { MenuBar } from '../panels/MenuBar';
import { FeaModeToolbar } from '../viewport/FeaModeToolbar';
import { createFeaModeStore } from '../stores';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(),
  // LogicalSize class whose instances carry { width, height } — used by set_window_size handler.
  LogicalSize: class LogicalSize {
    constructor(w: number, h: number) { (this as any).width = w; (this as any).height = h; }
  },
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
import { getCurrentWindow } from '@tauri-apps/api/window';
import { toPng } from 'html-to-image';
import { initDebugBridge, SET_FEA_CHANNEL_ERRORS, RESOLVE_BY_TESTID_ERRORS } from '../debug/bridge';
import { setTestMode } from '../debug/testMode';
import type { DebugStores } from '../debug/types';
import { makeViewStateStoreMock } from './debugBridgeTestHelpers';

type DebugRequestHandler = (event: { payload: { id: number; command: string; params: Record<string, unknown> } }) => Promise<void>;

/**
 * Build a describe-block's `dispatchCmd`: invoke the captured debug-request
 * handler and return the parsed `debug_response` payload.
 *
 * Every block in this file had its own byte-identical copy of this nine-line
 * body (17 of them), so a change to the response envelope meant 17 edits and any
 * missed one drifted silently. Takes a THUNK rather than the handler itself
 * because each block's `capturedHandler` is reassigned by its `beforeEach` —
 * capturing the value here would freeze it at `undefined`.
 *
 * The per-block `beforeEach`/`afterEach` pairs are deliberately NOT folded in:
 * they genuinely differ (some call `initDebugBridge` up front, others per test;
 * some `cleanup()`, others `vi.restoreAllMocks()`), so a shared one would have
 * to be parameterised into something longer than the four lines it replaced.
 */
function makeCmdDispatcher(getHandler: () => DebugRequestHandler | undefined) {
  return async function dispatchCmd(
    id: number,
    command: string,
    params: Record<string, unknown>,
  ) {
    vi.mocked(invoke).mockClear();
    await getHandler()!({ payload: { id, command, params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  };
}

// jsdom 25 does not implement document.elementFromPoint — the method is simply
// absent from the document prototype. vi.spyOn requires the property to exist
// before it can be overridden per test. Define a stub that returns null (matching
// jsdom's layout-less behaviour) so that vi.spyOn/.mockReturnValue works and
// vi.restoreAllMocks() reverts to this stub after each test.
if (typeof document.elementFromPoint !== 'function') {
  Object.defineProperty(document, 'elementFromPoint', {
    configurable: true,
    writable: true,
    value: (): Element | null => null,
  });
}

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
      setCompileDiagnostics: vi.fn(),
      setTessellationDiagnostics: vi.fn(),
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
      closeFile: vi.fn(),
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

describe('debug bridge editor_content live buffer', () => {
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

  async function dispatchEditorContentWithView(
    stores: DebugStores,
    editorView?: unknown,
  ) {
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
    if (editorView !== undefined) {
      window.__REIFY_DEBUG__!.editorView = editorView as any;
    }
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id: 601, command: 'editor_content', params: {} } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  it('primary: editorView present → content is live doc, not stale store snapshot', async () => {
    const stores = makeStores();
    stores.editor.state.openFiles = [{ path: 'bracket.ri', content: 'PRE-EDIT' }];
    stores.editor.state.activeFile = 'bracket.ri';
    const liveView = { state: { doc: { toString: () => 'POST-EDIT live', length: 14 } } } as any;
    const result = await dispatchEditorContentWithView(stores, liveView);
    expect(result.content).toBe('POST-EDIT live');
  });

  it('guard (i): no editorView → content falls back to store snapshot', async () => {
    const stores = makeStores();
    stores.editor.state.openFiles = [{ path: 'bracket.ri', content: 'PRE-EDIT' }];
    stores.editor.state.activeFile = 'bracket.ri';
    const result = await dispatchEditorContentWithView(stores);
    expect(result.content).toBe('PRE-EDIT');
  });

  it('guard (ii): activeFile null with editorView present → content is null', async () => {
    const stores = makeStores();
    // activeFile stays null (default makeStores)
    const liveView = { state: { doc: { toString: () => 'x', length: 1 } } } as any;
    const result = await dispatchEditorContentWithView(stores, liveView);
    expect(result.content).toBeNull();
  });

  it('active openFiles[] length reflects live buffer, non-active entries stay store-derived', async () => {
    const stores = makeStores();
    stores.editor.state.openFiles = [
      { path: 'bracket.ri', content: 'PRE-EDIT' },  // stale length 8
      { path: 'other.ri',   content: 'OTHER' },      // store-derived length 5
    ];
    stores.editor.state.activeFile = 'bracket.ri';
    // editorView has 'POST-EDIT live' (length 14, not 8)
    const liveView = { state: { doc: { toString: () => 'POST-EDIT live', length: 14 } } } as any;
    const result = await dispatchEditorContentWithView(stores, liveView);
    const activeEntry = result.openFiles.find((f: any) => f.path === 'bracket.ri');
    const otherEntry  = result.openFiles.find((f: any) => f.path === 'other.ri');
    // active entry must use live length
    expect(activeEntry.length).toBe('POST-EDIT live'.length);  // 14, not 8
    // non-active entry must stay store-derived
    expect(otherEntry.length).toBe('OTHER'.length);            // 5
  });

  it('empty live buffer (\'\') is returned verbatim, not replaced by store snapshot', async () => {
    // Guards against a regression where `??` is simplified to `||` or the
    // liveContent !== undefined guard is changed to a truthiness check:
    // an empty live doc ('') is a valid post-edit state and must NOT fall
    // back to the stale store content.
    const stores = makeStores();
    stores.editor.state.openFiles = [{ path: 'bracket.ri', content: 'PRE-EDIT' }];
    stores.editor.state.activeFile = 'bracket.ri';
    const emptyView = { state: { doc: { toString: () => '', length: 0 } } } as any;
    const result = await dispatchEditorContentWithView(stores, emptyView);
    // content must be '' (live), not 'PRE-EDIT' (stale store)
    expect(result.content).toBe('');
    // active openFiles entry length must be 0 (live), not 8 (stale store)
    const activeEntry = result.openFiles.find((f: any) => f.path === 'bracket.ri');
    expect(activeEntry.length).toBe(0);
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
  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

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

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

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

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

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

// ---------------------------------------------------------------------------
// debug bridge open_menu (step-1 RED → step-2 GREEN)
// ---------------------------------------------------------------------------

describe('debug bridge open_menu', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(async () => {
    cleanup();
    delete window.__REIFY_DEBUG__;
  });

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

  it('(a) open_menu({name:"file"}) returns {ok:true, open:"file"} and openMenu()==="file"', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    render(() => <MenuBar />);

    const result = await dispatchCmd(3000, 'open_menu', { name: 'file' });
    expect(result.ok).toBe(true);
    expect(result.open).toBe('file');
    expect(window.__REIFY_DEBUG__!.menuBar!.openMenu()).toBe('file');
  });

  it('(b) calling open_menu({name:"file"}) again is idempotent — menu stays open, not toggled', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    render(() => <MenuBar />);

    await dispatchCmd(3001, 'open_menu', { name: 'file' });
    const result2 = await dispatchCmd(3002, 'open_menu', { name: 'file' });
    expect(result2.ok).toBe(true);
    expect(result2.open).toBe('file');
    // Must still be open — not toggled closed
    expect(window.__REIFY_DEBUG__!.menuBar!.openMenu()).toBe('file');
  });

  it('(c) with file open, open_menu({name:"view"}) switches to view', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    render(() => <MenuBar />);

    await dispatchCmd(3003, 'open_menu', { name: 'file' });
    const result = await dispatchCmd(3004, 'open_menu', { name: 'view' });
    expect(result.ok).toBe(true);
    expect(result.open).toBe('view');
    expect(window.__REIFY_DEBUG__!.menuBar!.openMenu()).toBe('view');
  });

  it('(d) open_menu({name:"nope"}) returns {error} and does not change open state', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    render(() => <MenuBar />);

    await dispatchCmd(3005, 'open_menu', { name: 'file' });
    const result = await dispatchCmd(3006, 'open_menu', { name: 'nope' });
    expect(result).toHaveProperty('error');
    // State unchanged — still 'file'
    expect(window.__REIFY_DEBUG__!.menuBar!.openMenu()).toBe('file');
  });
});

// ---------------------------------------------------------------------------
// debug bridge menu_state (step-3 RED → step-4 GREEN)
// ---------------------------------------------------------------------------

describe('debug bridge menu_state', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(async () => {
    cleanup();
    delete window.__REIFY_DEBUG__;
  });

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

  it('(a) with no menu open, menu_state returns {open:null, items:[]}', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    render(() => <MenuBar />);

    const result = await dispatchCmd(4000, 'menu_state', {});
    expect(result.open).toBeNull();
    expect(result.items).toEqual([]);
  });

  it('(b) after opening file menu, menu_state returns open:"file" and items for new/open/save/export', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    render(() => <MenuBar />);

    await dispatchCmd(4001, 'open_menu', { name: 'file' });
    const result = await dispatchCmd(4002, 'menu_state', {});

    expect(result.open).toBe('file');
    expect(Array.isArray(result.items)).toBe(true);
    expect(result.items.length).toBeGreaterThan(0);

    // File menu has new/open/save/export
    const testIds = result.items.map((i: any) => i.testId);
    expect(testIds).toContain('menu-item-new');
    expect(testIds).toContain('menu-item-open');
    expect(testIds).toContain('menu-item-save');
    expect(testIds).toContain('menu-item-export');

    // Each item has label and enabled fields
    const openItem = result.items.find((i: any) => i.testId === 'menu-item-open');
    expect(openItem).toBeDefined();
    expect(typeof openItem.label).toBe('string');
    expect(openItem.label.length).toBeGreaterThan(0);
    expect(typeof openItem.enabled).toBe('boolean');
    expect(openItem.enabled).toBe(true);
  });

  it('(c) edit menu items undo/redo report enabled:false (registry-disabled)', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    render(() => <MenuBar />);

    await dispatchCmd(4003, 'open_menu', { name: 'edit' });
    const result = await dispatchCmd(4004, 'menu_state', {});

    expect(result.open).toBe('edit');
    const undoItem = result.items.find((i: any) => i.testId === 'menu-item-undo');
    const redoItem = result.items.find((i: any) => i.testId === 'menu-item-redo');
    expect(undoItem).toBeDefined();
    expect(redoItem).toBeDefined();
    expect(undoItem.enabled).toBe(false);
    expect(redoItem.enabled).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// debug bridge press_tab (step-5 RED → step-6 GREEN)
// ---------------------------------------------------------------------------

describe('debug bridge press_tab', () => {
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
    cleanup();
    document.body.innerHTML = '';
    delete window.__REIFY_DEBUG__;
  });

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

  it('(a) from body (no focus), press_tab focuses first tabbable and returns its descriptor', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    document.body.innerHTML = `
      <button data-testid="a">A</button>
      <button data-testid="b">B</button>
      <button data-testid="c">C</button>
    `;
    document.body.focus();

    const result = await dispatchCmd(5000, 'press_tab', {});
    expect(result.active_element).toBeDefined();
    expect(result.active_element.testId).toBe('a');
    expect(result.active_element.tagName).toBe('button');
    expect(document.activeElement?.getAttribute('data-testid')).toBe('a');
  });

  it('(b) pressing tab again advances to next tabbable', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    document.body.innerHTML = `
      <button data-testid="a">A</button>
      <button data-testid="b">B</button>
      <button data-testid="c">C</button>
    `;
    document.body.focus();

    await dispatchCmd(5001, 'press_tab', {});
    const result = await dispatchCmd(5002, 'press_tab', {});
    expect(result.active_element.testId).toBe('b');
  });

  it('(c) from last tabbable, press_tab wraps to first', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    document.body.innerHTML = `
      <button data-testid="a">A</button>
      <button data-testid="b">B</button>
      <button data-testid="c">C</button>
    `;

    // Focus the last element manually
    (document.querySelector('[data-testid="c"]') as HTMLElement).focus();

    const result = await dispatchCmd(5003, 'press_tab', {});
    expect(result.active_element.testId).toBe('a');
  });

  it('(d) with no tabbable elements, press_tab returns {active_element:null}', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    // Empty body has no focusable elements
    document.body.innerHTML = '<div>no buttons</div>';

    const result = await dispatchCmd(5004, 'press_tab', {});
    expect(result.active_element).toBeNull();
  });

  it('(e) disabled buttons and tabindex="-1" elements are skipped', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    document.body.innerHTML = `
      <button data-testid="skip-disabled" disabled>Disabled</button>
      <button data-testid="skip-neg-tabindex" tabindex="-1">Neg</button>
      <button data-testid="valid">Valid</button>
    `;
    document.body.focus();

    const result = await dispatchCmd(5005, 'press_tab', {});
    expect(result.active_element.testId).toBe('valid');
  });
});

// ---------------------------------------------------------------------------
// debug bridge tab_order (step-7 RED → step-8 GREEN)
// ---------------------------------------------------------------------------

describe('debug bridge tab_order', () => {
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
    cleanup();
    document.body.innerHTML = '';
    delete window.__REIFY_DEBUG__;
  });

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

  it('(a) returns order array matching document order for a/b/c buttons', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    document.body.innerHTML = `
      <button data-testid="a">A</button>
      <button data-testid="b">B</button>
      <button data-testid="c">C</button>
    `;

    const result = await dispatchCmd(6000, 'tab_order', {});
    expect(result.order).toEqual([
      { testId: 'a', tagName: 'button' },
      { testId: 'b', tagName: 'button' },
      { testId: 'c', tagName: 'button' },
    ]);
  });

  it('(b) disabled and tabindex="-1" elements excluded from order', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    document.body.innerHTML = `
      <button data-testid="a">A</button>
      <button data-testid="skip-disabled" disabled>Skip</button>
      <button data-testid="skip-neg" tabindex="-1">NegIdx</button>
      <button data-testid="b">B</button>
    `;

    const result = await dispatchCmd(6001, 'tab_order', {});
    const testIds = result.order.map((e: any) => e.testId);
    expect(testIds).toContain('a');
    expect(testIds).toContain('b');
    expect(testIds).not.toContain('skip-disabled');
    expect(testIds).not.toContain('skip-neg');
  });

  it('(c) empty body returns {order:[]}', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    document.body.innerHTML = '<div>no buttons</div>';
    const result = await dispatchCmd(6002, 'tab_order', {});
    expect(result.order).toEqual([]);
  });

  it('(d) rendered MenuBar yields chrome order starting with menu-trigger-file/edit/view/help', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    render(() => <MenuBar />);

    const result = await dispatchCmd(6003, 'tab_order', {});
    const testIds = result.order.map((e: any) => e.testId);
    const chromeOrder = ['menu-trigger-file', 'menu-trigger-edit', 'menu-trigger-view', 'menu-trigger-help'];
    // The four menu triggers must appear in MENU_DEFS order
    const indices = chromeOrder.map((id) => testIds.indexOf(id));
    expect(indices.every((i) => i !== -1)).toBe(true);
    for (let i = 1; i < indices.length; i++) {
      expect(indices[i]).toBeGreaterThan(indices[i - 1]);
    }
  });
});

// ---------------------------------------------------------------------------
// debug bridge resize_panes (step-1 RED → step-2 GREEN)
// ---------------------------------------------------------------------------

describe('debug bridge resize_panes', () => {
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

  async function dispatch(stores: ReturnType<typeof makeStores>, id: number, params: Record<string, unknown>) {
    await initDebugBridge(stores);
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id, command: 'resize_panes', params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  it('(a) single dimension: editorWidth calls setEditorWidth with value and returns { ok:true }', async () => {
    const stores = makeStores();
    const result = await dispatch(stores, 7000, { editorWidth: 450 });
    expect(result.ok).toBe(true);
    expect(stores.layout.setEditorWidth).toHaveBeenCalledWith(450);
    expect(stores.layout.setSideWidth).not.toHaveBeenCalled();
    expect(stores.layout.setDesignTreeHeight).not.toHaveBeenCalled();
    expect(stores.layout.setPropertyHeight).not.toHaveBeenCalled();
    expect(stores.layout.setConstraintHeight).not.toHaveBeenCalled();
  });

  it('(b) multi-dimension call invokes exactly those setters', async () => {
    const stores = makeStores();
    const result = await dispatch(stores, 7001, { sideWidth: 380, designTreeHeight: 220 });
    expect(result.ok).toBe(true);
    expect(stores.layout.setSideWidth).toHaveBeenCalledWith(380);
    expect(stores.layout.setDesignTreeHeight).toHaveBeenCalledWith(220);
    expect(stores.layout.setEditorWidth).not.toHaveBeenCalled();
    expect(stores.layout.setPropertyHeight).not.toHaveBeenCalled();
    expect(stores.layout.setConstraintHeight).not.toHaveBeenCalled();
  });

  it('(c) non-number value for a dimension returns error and calls no setter', async () => {
    const stores = makeStores();
    const result = await dispatch(stores, 7002, { editorWidth: 'bad' });
    expect(result).toHaveProperty('error');
    expect(stores.layout.setEditorWidth).not.toHaveBeenCalled();
  });

  it('(c) negative value for a dimension returns error and calls no setter', async () => {
    const stores = makeStores();
    const result = await dispatch(stores, 7003, { editorWidth: -1 });
    expect(result).toHaveProperty('error');
    expect(stores.layout.setEditorWidth).not.toHaveBeenCalled();
  });

  it('(c) NaN for a dimension returns error and calls no setter', async () => {
    const stores = makeStores();
    const result = await dispatch(stores, 7004, { editorWidth: NaN });
    expect(result).toHaveProperty('error');
    expect(stores.layout.setEditorWidth).not.toHaveBeenCalled();
  });

  it('(c) Infinity for a dimension returns error and calls no setter', async () => {
    const stores = makeStores();
    const result = await dispatch(stores, 7005, { editorWidth: Infinity });
    expect(result).toHaveProperty('error');
    expect(stores.layout.setEditorWidth).not.toHaveBeenCalled();
  });

  it('(d) empty params {} returns an error', async () => {
    const stores = makeStores();
    const result = await dispatch(stores, 7006, {});
    expect(result).toHaveProperty('error');
    expect(stores.layout.setEditorWidth).not.toHaveBeenCalled();
    expect(stores.layout.setSideWidth).not.toHaveBeenCalled();
    expect(stores.layout.setDesignTreeHeight).not.toHaveBeenCalled();
    expect(stores.layout.setPropertyHeight).not.toHaveBeenCalled();
    expect(stores.layout.setConstraintHeight).not.toHaveBeenCalled();
  });

  it('(e) returned layout snapshot has all 5 pane dimension keys with current store values', async () => {
    // Regression: resize_panes returns { ok, layout: {...ctx.stores.layout.state} }.
    // This test locks in the returned layout snapshot contract so a regression that
    // removes or reshapes the field is caught. The setter is mocked (vi.fn()) so the
    // state does not change; the snapshot reflects the initial makeStores() values.
    const stores = makeStores();
    const result = await dispatch(stores, 7007, { editorWidth: 450 });
    expect(result.ok).toBe(true);
    // layout snapshot must exist and carry all 5 dimension keys.
    expect(result.layout).toBeDefined();
    expect(result.layout).toHaveProperty('editorWidth');
    expect(result.layout).toHaveProperty('sideWidth');
    expect(result.layout).toHaveProperty('designTreeHeight');
    expect(result.layout).toHaveProperty('propertyHeight');
    expect(result.layout).toHaveProperty('constraintHeight');
    // Values reflect the initial makeStores() state (setter is mocked, state unchanged).
    expect(result.layout).toEqual({
      editorWidth: 300,
      sideWidth: 300,
      designTreeHeight: 160,
      propertyHeight: 200,
      constraintHeight: 140,
    });
  });
});

// ---------------------------------------------------------------------------
// debug bridge set_window_size (step-3 RED → step-4 GREEN)
// ---------------------------------------------------------------------------

describe('debug bridge set_window_size', () => {
  let capturedHandler: DebugRequestHandler | undefined;
  let setSizeSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandler = undefined;
    setSizeSpy = vi.fn().mockResolvedValue(undefined);
    vi.mocked(getCurrentWindow).mockReturnValue({ setSize: setSizeSpy } as any);
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      capturedHandler = handler as DebugRequestHandler;
      return () => {};
    });
  });

  afterEach(() => {
    delete window.__REIFY_DEBUG__;
  });

  async function dispatch(id: number, params: Record<string, unknown>) {
    const stores = makeStores();
    await initDebugBridge(stores);
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id, command: 'set_window_size', params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  it('(a) valid dimensions call setSize once and return { ok:true, width, height }', async () => {
    const result = await dispatch(8000, { width: 1024, height: 768 });
    expect(result).toEqual({ ok: true, width: 1024, height: 768 });
    expect(setSizeSpy).toHaveBeenCalledTimes(1);
    expect(setSizeSpy.mock.calls[0][0]).toMatchObject({ width: 1024, height: 768 });
  });

  it('(b) non-number width returns error, setSize not called', async () => {
    const result = await dispatch(8001, { width: 'bad', height: 768 });
    expect(result).toHaveProperty('error');
    expect(setSizeSpy).not.toHaveBeenCalled();
  });

  it('(b) width === 0 returns error', async () => {
    const result = await dispatch(8002, { width: 0, height: 768 });
    expect(result).toHaveProperty('error');
    expect(setSizeSpy).not.toHaveBeenCalled();
  });

  it('(b) negative width returns error', async () => {
    const result = await dispatch(8003, { width: -100, height: 768 });
    expect(result).toHaveProperty('error');
    expect(setSizeSpy).not.toHaveBeenCalled();
  });

  it('(b) NaN width returns error', async () => {
    const result = await dispatch(8004, { width: NaN, height: 768 });
    expect(result).toHaveProperty('error');
    expect(setSizeSpy).not.toHaveBeenCalled();
  });

  it('(b) Infinity width returns error', async () => {
    const result = await dispatch(8005, { width: Infinity, height: 768 });
    expect(result).toHaveProperty('error');
    expect(setSizeSpy).not.toHaveBeenCalled();
  });

  it('(b) non-number height returns error', async () => {
    const result = await dispatch(8006, { width: 1024, height: 'bad' });
    expect(result).toHaveProperty('error');
    expect(setSizeSpy).not.toHaveBeenCalled();
  });

  it('(b) height === 0 returns error', async () => {
    const result = await dispatch(8007, { width: 1024, height: 0 });
    expect(result).toHaveProperty('error');
    expect(setSizeSpy).not.toHaveBeenCalled();
  });

  it('(b) NaN height returns error', async () => {
    const result = await dispatch(8008, { width: 1024, height: NaN });
    expect(result).toHaveProperty('error');
    expect(setSizeSpy).not.toHaveBeenCalled();
  });

  it('(b) Infinity height returns error', async () => {
    const result = await dispatch(8009, { width: 1024, height: Infinity });
    expect(result).toHaveProperty('error');
    expect(setSizeSpy).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// debug bridge tree-node expand/collapse (step-5 RED → step-6 GREEN)
// ---------------------------------------------------------------------------

describe('debug bridge tree-node expand/collapse', () => {
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
    document.body.innerHTML = '';
  });

  async function dispatch(id: number, command: string, params: Record<string, unknown>) {
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id, command, params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  /** Inject a chevron button that toggles the design-panel expandedSet on click. */
  function setupDesignPanel(path: string, initialExpanded = false) {
    const expandedSet = new Set<string>();
    if (initialExpanded) expandedSet.add(path);
    const btn = document.createElement('button');
    btn.setAttribute('data-testid', `chevron-${path}`);
    btn.addEventListener('click', () => {
      if (expandedSet.has(path)) expandedSet.delete(path);
      else expandedSet.add(path);
    });
    document.body.appendChild(btn);
    window.__REIFY_DEBUG__!.designTree = { expanded: () => expandedSet };
    return { expandedSet, btn };
  }

  /** Inject a constraint-row button that toggles the constraint-panel expandedSet on click. */
  function setupConstraintPanel(path: string, initialExpanded = false) {
    const expandedSet = new Set<string>();
    if (initialExpanded) expandedSet.add(path);
    const btn = document.createElement('button');
    btn.setAttribute('data-testid', `constraint-row-${path}`);
    btn.addEventListener('click', () => {
      if (expandedSet.has(path)) expandedSet.delete(path);
      else expandedSet.add(path);
    });
    document.body.appendChild(btn);
    window.__REIFY_DEBUG__!.constraintPanel = { expandedNodes: () => expandedSet };
    return { expandedSet, btn };
  }

  it('(a) expand_tree_node: node NOT expanded → clicks chevron once, returns { ok:true, path, expanded:true }', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const { btn } = setupDesignPanel('Bracket.body', false);
    const clickSpy = vi.fn();
    btn.addEventListener('click', clickSpy);

    const result = await dispatch(9000, 'expand_tree_node', { path: 'Bracket.body' });
    expect(result.ok).toBe(true);
    expect(result.path).toBe('Bracket.body');
    expect(result.expanded).toBe(true);
    expect(clickSpy).toHaveBeenCalledTimes(1);
  });

  it('(b) expand_tree_node idempotent: node already expanded → NO click, returns expanded:true', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const { btn } = setupDesignPanel('Bracket.body', true);
    const clickSpy = vi.fn();
    btn.addEventListener('click', clickSpy);

    const result = await dispatch(9001, 'expand_tree_node', { path: 'Bracket.body' });
    expect(result.ok).toBe(true);
    expect(result.expanded).toBe(true);
    expect(clickSpy).not.toHaveBeenCalled();
  });

  it('(c) collapse_tree_node: node expanded → clicks once, returns expanded:false', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const { btn } = setupDesignPanel('Bracket.body', true);
    const clickSpy = vi.fn();
    btn.addEventListener('click', clickSpy);

    const result = await dispatch(9002, 'collapse_tree_node', { path: 'Bracket.body' });
    expect(result.ok).toBe(true);
    expect(result.expanded).toBe(false);
    expect(clickSpy).toHaveBeenCalledTimes(1);
  });

  it('(d) collapse_tree_node idempotent: not expanded → NO click, returns expanded:false', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const { btn } = setupDesignPanel('Bracket.body', false);
    const clickSpy = vi.fn();
    btn.addEventListener('click', clickSpy);

    const result = await dispatch(9003, 'collapse_tree_node', { path: 'Bracket.body' });
    expect(result.ok).toBe(true);
    expect(result.expanded).toBe(false);
    expect(clickSpy).not.toHaveBeenCalled();
  });

  it('(e) expand_tree_node: missing path returns { error }', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    setupDesignPanel('Bracket.body', false);

    const result = await dispatch(9004, 'expand_tree_node', {});
    expect(result).toHaveProperty('error');
  });

  it('(f) expand_tree_node: control element absent returns { error } with path in message', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    // Register accessor but do NOT inject a button
    const expandedSet = new Set<string>();
    window.__REIFY_DEBUG__!.designTree = { expanded: () => expandedSet };

    const result = await dispatch(9005, 'expand_tree_node', { path: 'Missing.node' });
    expect(result).toHaveProperty('error');
    expect(result.error).toContain('Missing.node');
  });

  it('(g) panel:constraint drives constraint-row testid and reads constraintPanel.expandedNodes', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const { btn } = setupConstraintPanel('constraint-1', false);
    const clickSpy = vi.fn();
    btn.addEventListener('click', clickSpy);

    const result = await dispatch(9006, 'expand_tree_node', { path: 'constraint-1', panel: 'constraint' });
    expect(result.ok).toBe(true);
    expect(result.expanded).toBe(true);
    expect(clickSpy).toHaveBeenCalledTimes(1);
  });

  it('(h) designTree panel not registered returns { error }', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    // designTree not registered on ctx (default state after initDebugBridge)

    const result = await dispatch(9007, 'expand_tree_node', { path: 'Bracket.body' });
    expect(result).toHaveProperty('error');
  });

  it('(i) invalid panel value returns { error } mentioning the unknown panel name', async () => {
    // Locks in the panel validation branch: unknown panel values (anything other than
    // 'design' or 'constraint') return an error that names the bad value.
    const stores = makeStores();
    await initDebugBridge(stores);
    setupDesignPanel('Bracket.body', false);

    const result = await dispatch(9008, 'expand_tree_node', { path: 'Bracket.body', panel: 'foo' });
    expect(result).toHaveProperty('error');
    expect(result.error).toContain('foo');
    expect(result.error).toContain('design');
    expect(result.error).toContain('constraint');
  });
});

// ─── F2 LSP probe handlers (steps 7-14) ─────────────────────────────────────

describe('debug bridge hover_at', () => {
  // F2 step-7 RED → step-8 GREEN: hover_at handler returns structured hover result.
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

  it('returns { markdown, markdownLength, range } and calls lsp_request with correct method/uri/position', async () => {
    const stores = makeStores();
    stores.editor.state.activeFile = '/tmp/cube.ri';
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    // Configure invoke: lsp_request → hover JSON; debug_response → undefined
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'lsp_request') {
        return JSON.stringify({
          contents: { kind: 'markdown', value: '**size**: Scalar' },
          range: { start: { line: 9, character: 15 }, end: { line: 9, character: 19 } },
        });
      }
      return undefined;
    });
    vi.mocked(invoke).mockClear();

    await capturedHandler!({ payload: { id: 1001, command: 'hover_at', params: { line: 9, col: 19 } } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as { result: string }).result);

    // Structured hover result
    expect(result.markdown).toBe('**size**: Scalar');
    expect(result.markdownLength).toBeGreaterThanOrEqual(1);
    expect(result.range).toBeDefined();

    // lsp_request called with correct method / uri / position
    const lspCall = calls.find((c) => c[0] === 'lsp_request');
    expect(lspCall).toBeDefined();
    expect((lspCall![1] as { method: string }).method).toBe('textDocument/hover');
    const lspParams = JSON.parse((lspCall![1] as { params: string }).params);
    expect(lspParams.textDocument.uri).toBe('file:///tmp/cube.ri');
    expect(lspParams.position).toEqual({ line: 9, character: 19 });
  });

  it('returns null-hover shape when lsp_request returns null (no hover at position)', async () => {
    // Covers the hover_at null branch: { markdown:'', markdownLength:0, contents:null, range:null }
    const stores = makeStores();
    stores.editor.state.activeFile = '/tmp/cube.ri';
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'lsp_request') return JSON.stringify(null);
      return undefined;
    });
    vi.mocked(invoke).mockClear();

    await capturedHandler!({ payload: { id: 1002, command: 'hover_at', params: { line: 0, col: 0 } } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as { result: string }).result);

    expect(result.markdown).toBe('');
    expect(result.markdownLength).toBe(0);
    expect(result.contents).toBeNull();
    expect(result.range).toBeNull();
  });
});

// ─── F2 definition_at handler (step-11 RED → step-12 GREEN) ─────────────────

describe('debug bridge definition_at', () => {
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

  async function dispatchDefinition(stores: ReturnType<typeof makeStores>, params: Record<string, unknown>) {
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id: 3001, command: 'definition_at', params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    return {
      result: JSON.parse((responseCall![1] as { result: string }).result),
      calls,
    };
  }

  it('returns { found:true, uri, range } and calls lsp_request with correct method/uri/position', async () => {
    const stores = makeStores();
    stores.editor.state.activeFile = '/tmp/cube.ri';

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'lsp_request') {
        return JSON.stringify({
          uri: 'file:///tmp/cube.ri',
          range: { start: { line: 7, character: 10 }, end: { line: 7, character: 14 } },
        });
      }
      return undefined;
    });

    const { result, calls } = await dispatchDefinition(stores, { line: 9, col: 19 });

    expect(result.found).toBe(true);
    expect(result.uri).toBe('file:///tmp/cube.ri');
    expect(result.range.start.line).toBe(7);

    const lspCall = calls.find((c) => c[0] === 'lsp_request');
    expect(lspCall).toBeDefined();
    expect((lspCall![1] as { method: string }).method).toBe('textDocument/definition');
    const lspParams = JSON.parse((lspCall![1] as { params: string }).params);
    expect(lspParams.textDocument.uri).toBe('file:///tmp/cube.ri');
    expect(lspParams.position).toEqual({ line: 9, character: 19 });
  });

  it('returns { found:false, uri:null, range:null } when LSP returns null', async () => {
    const stores = makeStores();
    stores.editor.state.activeFile = '/tmp/cube.ri';

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'lsp_request') return JSON.stringify(null);
      return undefined;
    });

    const { result } = await dispatchDefinition(stores, { line: 0, col: 0 });

    expect(result.found).toBe(false);
    expect(result.uri).toBeNull();
    expect(result.range).toBeNull();
  });
});

// ─── F2 completion_at handler (step-9 RED → step-10 GREEN) ──────────────────

describe('debug bridge completion_at', () => {
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

  async function dispatchCompletion(stores: ReturnType<typeof makeStores>, params: Record<string, unknown>) {
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id: 2001, command: 'completion_at', params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    return {
      result: JSON.parse((responseCall![1] as { result: string }).result),
      calls,
    };
  }

  it('bare array response: returns { items, itemCount } and calls lsp_request with correct method/uri/position', async () => {
    const stores = makeStores();
    stores.editor.state.activeFile = '/tmp/cube.ri';

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'lsp_request') {
        return JSON.stringify([{ label: 'box' }, { label: 'size' }]);
      }
      return undefined;
    });

    const { result, calls } = await dispatchCompletion(stores, { line: 9, col: 21 });

    expect(result.itemCount).toBeGreaterThanOrEqual(1);
    expect(Array.isArray(result.items)).toBe(true);
    expect(result.items.map((i: { label: string }) => i.label)).toContain('box');
    expect(result.items.map((i: { label: string }) => i.label)).toContain('size');

    const lspCall = calls.find((c) => c[0] === 'lsp_request');
    expect(lspCall).toBeDefined();
    expect((lspCall![1] as { method: string }).method).toBe('textDocument/completion');
    const lspParams = JSON.parse((lspCall![1] as { params: string }).params);
    expect(lspParams.textDocument.uri).toBe('file:///tmp/cube.ri');
    expect(lspParams.position).toEqual({ line: 9, character: 21 });
  });

  it('CompletionList response: normalizes items via lspClient.completion', async () => {
    const stores = makeStores();
    stores.editor.state.activeFile = '/tmp/cube.ri';

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'lsp_request') {
        return JSON.stringify({ items: [{ label: 'box' }, { label: 'sphere' }] });
      }
      return undefined;
    });

    const { result } = await dispatchCompletion(stores, { line: 9, col: 21 });

    expect(result.itemCount).toBe(2);
    expect(result.items.map((i: { label: string }) => i.label)).toContain('box');
    expect(result.items.map((i: { label: string }) => i.label)).toContain('sphere');
  });
});

// ─── F2 input-guard suite (step-13 RED → step-14 GREEN) ─────────────────────
// resolveActiveProbeTarget was included in step-8 so these are immediately GREEN.

describe('debug bridge LSP probe input guards', () => {
  const PROBES = ['hover_at', 'completion_at', 'definition_at'] as const;

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

  async function dispatchProbe(
    stores: ReturnType<typeof makeStores>,
    command: string,
    params: Record<string, unknown>,
  ) {
    await initDebugBridge(stores);
    vi.mocked(invoke).mockClear();
    await capturedHandler!({ payload: { id: 4001, command, params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    return {
      result: JSON.parse((responseCall![1] as { result: string }).result),
      calls,
    };
  }

  for (const probe of PROBES) {
    it(`${probe}: activeFile=null returns {error} and does NOT call lsp_request`, async () => {
      const stores = makeStores();
      // activeFile stays null (default)
      const { result, calls } = await dispatchProbe(stores, probe, { line: 0, col: 0 });
      expect(result).toHaveProperty('error');
      expect(calls.find((c) => c[0] === 'lsp_request')).toBeUndefined();
    });

    it(`${probe}: line=-1 returns {error} and does NOT call lsp_request`, async () => {
      const stores = makeStores();
      stores.editor.state.activeFile = '/tmp/cube.ri';
      const { result, calls } = await dispatchProbe(stores, probe, { line: -1, col: 0 });
      expect(result).toHaveProperty('error');
      expect(calls.find((c) => c[0] === 'lsp_request')).toBeUndefined();
    });

    it(`${probe}: missing col returns {error} and does NOT call lsp_request`, async () => {
      const stores = makeStores();
      stores.editor.state.activeFile = '/tmp/cube.ri';
      const { result, calls } = await dispatchProbe(stores, probe, { line: 0 });
      expect(result).toHaveProperty('error');
      expect(calls.find((c) => c[0] === 'lsp_request')).toBeUndefined();
    });
  }
});

// ---------------------------------------------------------------------------
// task-4299 steps 3–12: I1 synthetic pointer/scroll/focus tools
// Scaffold mirrors 'debug bridge R1 inspection tools' at :1714-1742.
// ---------------------------------------------------------------------------

describe('debug bridge click_at', () => {
  // step-3 RED → step-4 GREEN
  let capturedHandler: DebugRequestHandler | undefined;

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

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
    vi.restoreAllMocks();
  });

  it('happy: dispatches click with correct clientX/clientY', async () => {
    const button = document.createElement('button');
    button.setAttribute('data-testid', 'test-btn');
    document.body.appendChild(button);
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(button);

    let firedEvent: MouseEvent | undefined;
    button.addEventListener('click', (e) => { firedEvent = e as MouseEvent; });

    const result = await dispatchCmd(5001, 'click_at', { x: 140, y: 70 });
    expect(result.ok).toBe(true);
    expect(firedEvent).toBeDefined();
    expect(firedEvent!.clientX).toBe(140);
    expect(firedEvent!.clientY).toBe(70);
    // Assert target payload shape (suggestion 4: regression-guard on element resolution)
    expect(result.target.tagName).toBe('button');
    expect(result.target.testId).toBe('test-btn');
  });

  it('no element at point returns {error}', async () => {
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(null);
    const result = await dispatchCmd(5002, 'click_at', { x: 5, y: 5 });
    expect(typeof result.error).toBe('string');
  });

  it('invalid coords (string) returns {error}', async () => {
    const result = await dispatchCmd(5003, 'click_at', { x: 'a', y: 1 });
    expect(typeof result.error).toBe('string');
  });

  it('invalid coords (NaN) returns {error}', async () => {
    const result = await dispatchCmd(5004, 'click_at', { x: NaN, y: 1 });
    expect(typeof result.error).toBe('string');
  });
});

describe('debug bridge hover', () => {
  // step-5 RED → step-6 GREEN
  let capturedHandler: DebugRequestHandler | undefined;

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

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
    vi.restoreAllMocks();
  });

  it('happy: dispatches move event with correct clientX/clientY', async () => {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'test-div');
    document.body.appendChild(el);
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(el);

    let firedEvent: MouseEvent | undefined;
    // pointermove or mousemove — check whichever fires
    el.addEventListener('pointermove', (e) => { firedEvent = e as MouseEvent; });
    el.addEventListener('mousemove', (e) => { if (!firedEvent) firedEvent = e as MouseEvent; });

    const result = await dispatchCmd(5101, 'hover', { x: 50, y: 60 });
    expect(result.ok).toBe(true);
    expect(firedEvent).toBeDefined();
    expect(firedEvent!.clientX).toBe(50);
    expect(firedEvent!.clientY).toBe(60);
    // Assert target payload shape (suggestion 4: regression-guard on element resolution)
    expect(result.target.tagName).toBe('div');
    expect(result.target.testId).toBe('test-div');
  });

  it('no element at point returns {error}', async () => {
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(null);
    const result = await dispatchCmd(5102, 'hover', { x: 5, y: 5 });
    expect(typeof result.error).toBe('string');
  });

  it('invalid coords returns {error}', async () => {
    const result = await dispatchCmd(5103, 'hover', { x: 'bad', y: 1 });
    expect(typeof result.error).toBe('string');
  });
});

describe('debug bridge drag', () => {
  // step-7 RED → step-8 GREEN
  let capturedHandler: DebugRequestHandler | undefined;

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

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
    vi.restoreAllMocks();
  });

  it('happy: dispatches pointerdown at from and pointerup at to', async () => {
    const el = document.createElement('div');
    document.body.appendChild(el);
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(el);

    let downEvent: MouseEvent | undefined;
    let upEvent: MouseEvent | undefined;
    el.addEventListener('pointerdown', (e) => { downEvent = e as MouseEvent; });
    el.addEventListener('mousedown', (e) => { if (!downEvent) downEvent = e as MouseEvent; });
    el.addEventListener('pointerup', (e) => { upEvent = e as MouseEvent; });
    el.addEventListener('mouseup', (e) => { if (!upEvent) upEvent = e as MouseEvent; });

    const result = await dispatchCmd(5201, 'drag', { from: { x: 10, y: 20 }, to: { x: 80, y: 90 } });
    expect(result.ok).toBe(true);
    expect(downEvent).toBeDefined();
    expect(downEvent!.clientX).toBe(10);
    expect(downEvent!.clientY).toBe(20);
    expect(upEvent).toBeDefined();
    expect(upEvent!.clientX).toBe(80);
    expect(upEvent!.clientY).toBe(90);
  });

  it('no element at from returns {error}', async () => {
    vi.spyOn(document, 'elementFromPoint').mockReturnValue(null);
    const result = await dispatchCmd(5202, 'drag', { from: { x: 0, y: 0 }, to: { x: 10, y: 10 } });
    expect(typeof result.error).toBe('string');
  });

  it('invalid from (missing x) returns {error}', async () => {
    const result = await dispatchCmd(5203, 'drag', { from: { y: 5 }, to: { x: 10, y: 10 } });
    expect(typeof result.error).toBe('string');
  });

  it('invalid from (NaN) returns {error}', async () => {
    const result = await dispatchCmd(5204, 'drag', { from: { x: NaN, y: 5 }, to: { x: 10, y: 10 } });
    expect(typeof result.error).toBe('string');
  });

  it('null to destination falls back to from element; move/up events fire at to coords', async () => {
    // Covers the `elTo = document.elementFromPoint(to.x, to.y) ?? elFrom` fallback
    // when the destination point resolves to null (e.g. off-canvas).
    const el = document.createElement('div');
    document.body.appendChild(el);
    const spy = vi.spyOn(document, 'elementFromPoint');
    spy.mockReturnValueOnce(el);   // first call: from resolves
    spy.mockReturnValueOnce(null); // second call: to resolves to null → falls back to el

    let moveEvent: MouseEvent | undefined;
    let upEvent: MouseEvent | undefined;
    el.addEventListener('pointermove', (e) => { moveEvent = e as MouseEvent; });
    el.addEventListener('mousemove', (e) => { if (!moveEvent) moveEvent = e as MouseEvent; });
    el.addEventListener('pointerup', (e) => { upEvent = e as MouseEvent; });
    el.addEventListener('mouseup', (e) => { if (!upEvent) upEvent = e as MouseEvent; });

    const result = await dispatchCmd(5205, 'drag', { from: { x: 10, y: 20 }, to: { x: 80, y: 90 } });
    expect(result.ok).toBe(true);
    // Events fired on the fallback element (elFrom) but carry the to coordinates.
    expect(moveEvent).toBeDefined();
    expect(moveEvent!.clientX).toBe(80);
    expect(moveEvent!.clientY).toBe(90);
    expect(upEvent).toBeDefined();
    expect(upEvent!.clientX).toBe(80);
    expect(upEvent!.clientY).toBe(90);
  });
});

describe('debug bridge focus_element', () => {
  // step-9 RED → step-10 GREEN
  let capturedHandler: DebugRequestHandler | undefined;

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

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
    vi.restoreAllMocks();
  });

  it('happy: focuses element by testId', async () => {
    const input = document.createElement('input');
    input.setAttribute('data-testid', 'fld');
    document.body.appendChild(input);

    const result = await dispatchCmd(5301, 'focus_element', { testId: 'fld' });
    expect(result.ok).toBe(true);
    expect(document.activeElement).toBe(input);
  });

  it('missing testId returns {error}', async () => {
    const result = await dispatchCmd(5302, 'focus_element', {});
    expect(result.error).toBe('testId is required');
  });

  it('element not found returns {error}', async () => {
    const result = await dispatchCmd(5303, 'focus_element', { testId: 'no-such-element' });
    expect(typeof result.error).toBe('string');
  });

  // --- viewport scoping (#5891) ---
  // Plain DOM is enough here: the resolveByTestId block above already proves the
  // scoping works against a real mounted FeaModeToolbar, so these cases only
  // need to establish that focus_element routes through the shared resolver.

  /** Two panes, each holding an input under the same testId. design-main is first. */
  function twoPanesWithInput() {
    document.body.innerHTML = `
      <div data-viewport-id="design-main"><input data-testid="fld" /></div>
      <div data-viewport-id="pane-1"><input data-testid="fld" /></div>
    `;
    return {
      designMain: document.querySelector('[data-viewport-id="design-main"] [data-testid="fld"]')!,
      pane1: document.querySelector('[data-viewport-id="pane-1"] [data-testid="fld"]')!,
    };
  }

  it('#5891 scoped: focuses the named pane\'s element, not the first match', async () => {
    const { designMain, pane1 } = twoPanesWithInput();

    const result = await dispatchCmd(5310, 'focus_element', { testId: 'fld', viewportId: 'pane-1' });

    expect(result).toEqual({ ok: true });
    expect(document.activeElement).toBe(pane1);
    expect(document.activeElement).not.toBe(designMain);
  });

  it('#5891 unknown viewportId returns notFoundForViewport', async () => {
    twoPanesWithInput();

    const result = await dispatchCmd(5311, 'focus_element', { testId: 'fld', viewportId: 'nope' });

    expect(result).toEqual({ error: RESOLVE_BY_TESTID_ERRORS.notFoundForViewport('fld', 'nope') });
  });

  it('#5891 non-string viewportId returns viewportIdNotString', async () => {
    twoPanesWithInput();

    const result = await dispatchCmd(5312, 'focus_element', { testId: 'fld', viewportId: 5 });

    expect(result).toEqual({ error: RESOLVE_BY_TESTID_ERRORS.viewportIdNotString });
  });

  it('#5891 unscoped multi-match focuses the first and reports the guessed pane', async () => {
    const { designMain } = twoPanesWithInput();

    const result = await dispatchCmd(5313, 'focus_element', { testId: 'fld' });

    expect(document.activeElement).toBe(designMain);
    expect(result).toEqual({ ok: true, viewportId: 'design-main', matchCount: 2 });
  });

  it('#5891 single match keeps today\'s exact {ok:true} payload', async () => {
    const input = document.createElement('input');
    input.setAttribute('data-testid', 'lonely');
    document.body.appendChild(input);

    const result = await dispatchCmd(5314, 'focus_element', { testId: 'lonely' });

    expect(result).toEqual({ ok: true });
  });
});

describe('debug bridge scroll', () => {
  // step-11 RED → step-12 GREEN
  let capturedHandler: DebugRequestHandler | undefined;

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

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
    vi.restoreAllMocks();
  });

  it('DOM mode: sets scrollTop/scrollLeft on element by testId', async () => {
    const panel = document.createElement('div');
    panel.setAttribute('data-testid', 'panel');
    document.body.appendChild(panel);
    // jsdom's native scrollTop is a no-op — make it writable
    Object.defineProperty(panel, 'scrollTop', { configurable: true, writable: true, value: 0 });
    Object.defineProperty(panel, 'scrollLeft', { configurable: true, writable: true, value: 0 });

    const result = await dispatchCmd(5401, 'scroll', { testId: 'panel', top: 120, left: 30 });
    expect(result.ok).toBe(true);
    expect(result.scrollTop).toBe(120);
    expect(result.scrollLeft).toBe(30);
    expect((panel as any).scrollTop).toBe(120);
  });

  it('editor mode: scrolls CodeMirror scrollDOM', async () => {
    const scrollDOM = document.createElement('div');
    Object.defineProperty(scrollDOM, 'scrollTop', { configurable: true, writable: true, value: 0 });
    Object.defineProperty(scrollDOM, 'scrollLeft', { configurable: true, writable: true, value: 0 });
    // Wire editorView into the debug context
    (window as any).__REIFY_DEBUG__.editorView = { scrollDOM } as any;

    const result = await dispatchCmd(5402, 'scroll', { target: 'editor', top: 80 });
    expect(result.ok).toBe(true);
    expect(result.scrollTop).toBe(80);
    expect((scrollDOM as any).scrollTop).toBe(80);
  });

  it('editor mode with no editorView returns {error}', async () => {
    // editorView is not set (default makeStores() has none)
    const result = await dispatchCmd(5403, 'scroll', { target: 'editor', top: 50 });
    expect(typeof result.error).toBe('string');
  });

  it('neither testId nor target returns {error}', async () => {
    const result = await dispatchCmd(5404, 'scroll', { top: 100 });
    expect(typeof result.error).toBe('string');
  });

  it('DOM mode: testId not found returns {error}', async () => {
    const result = await dispatchCmd(5405, 'scroll', { testId: 'no-such-panel', top: 50 });
    expect(typeof result.error).toBe('string');
  });

  it('DOM mode: NaN top returns {error}', async () => {
    // isFiniteNumber guard: NaN silently coerces to 0 without this check.
    const panel = document.createElement('div');
    panel.setAttribute('data-testid', 'panel-nan');
    document.body.appendChild(panel);
    Object.defineProperty(panel, 'scrollTop', { configurable: true, writable: true, value: 0 });
    const result = await dispatchCmd(5406, 'scroll', { testId: 'panel-nan', top: NaN });
    expect(typeof result.error).toBe('string');
  });

  it('editor mode: Infinity left returns {error}', async () => {
    // isFiniteNumber guard: ±Infinity silently coerces to 0 without this check.
    const scrollDOM = document.createElement('div');
    Object.defineProperty(scrollDOM, 'scrollTop', { configurable: true, writable: true, value: 0 });
    Object.defineProperty(scrollDOM, 'scrollLeft', { configurable: true, writable: true, value: 0 });
    (window as any).__REIFY_DEBUG__.editorView = { scrollDOM } as any;
    const result = await dispatchCmd(5407, 'scroll', { target: 'editor', left: Infinity });
    expect(typeof result.error).toBe('string');
  });

  // --- viewport scoping (#5891), DOM arm only ---

  /** Two panes, each holding a scrollable panel under the same testId. */
  function twoPanesWithPanel() {
    document.body.innerHTML = `
      <div data-viewport-id="design-main"><div data-testid="panel"></div></div>
      <div data-viewport-id="pane-1"><div data-testid="panel"></div></div>
    `;
    const panels = ['design-main', 'pane-1'].map((id) => {
      const el = document.querySelector(`[data-viewport-id="${id}"] [data-testid="panel"]`)!;
      // jsdom's native scrollTop/scrollLeft are no-ops — make them writable.
      Object.defineProperty(el, 'scrollTop', { configurable: true, writable: true, value: 0 });
      Object.defineProperty(el, 'scrollLeft', { configurable: true, writable: true, value: 0 });
      return el as HTMLElement;
    });
    return { designMain: panels[0], pane1: panels[1] };
  }

  it('#5891 scoped: scrolls the named pane\'s panel, leaving the other at 0', async () => {
    const { designMain, pane1 } = twoPanesWithPanel();

    const result = await dispatchCmd(5410, 'scroll', {
      testId: 'panel', viewportId: 'pane-1', top: 120, left: 30,
    });

    expect(result).toEqual({ ok: true, scrollTop: 120, scrollLeft: 30 });
    expect(pane1.scrollTop).toBe(120);
    expect(designMain.scrollTop).toBe(0);
  });

  it('#5891 unknown viewportId returns notFoundForViewport', async () => {
    twoPanesWithPanel();

    const result = await dispatchCmd(5411, 'scroll', { testId: 'panel', viewportId: 'nope', top: 10 });

    expect(result).toEqual({ error: RESOLVE_BY_TESTID_ERRORS.notFoundForViewport('panel', 'nope') });
  });

  it('#5891 non-string viewportId returns viewportIdNotString', async () => {
    twoPanesWithPanel();

    const result = await dispatchCmd(5412, 'scroll', { testId: 'panel', viewportId: 5, top: 10 });

    expect(result).toEqual({ error: RESOLVE_BY_TESTID_ERRORS.viewportIdNotString });
  });

  it('#5891 unscoped multi-match scrolls the first and reports the guessed pane', async () => {
    const { designMain, pane1 } = twoPanesWithPanel();

    const result = await dispatchCmd(5413, 'scroll', { testId: 'panel', top: 45 });

    expect(designMain.scrollTop).toBe(45);
    expect(pane1.scrollTop).toBe(0);
    expect(result).toEqual({
      ok: true, scrollTop: 45, scrollLeft: 0, viewportId: 'design-main', matchCount: 2,
    });
  });

  it('#5891 single match keeps today\'s exact three-key payload', async () => {
    const panel = document.createElement('div');
    panel.setAttribute('data-testid', 'lonely-panel');
    document.body.appendChild(panel);
    Object.defineProperty(panel, 'scrollTop', { configurable: true, writable: true, value: 0 });
    Object.defineProperty(panel, 'scrollLeft', { configurable: true, writable: true, value: 0 });

    const result = await dispatchCmd(5414, 'scroll', { testId: 'lonely-panel', top: 7 });

    expect(result).toEqual({ ok: true, scrollTop: 7, scrollLeft: 0 });
  });

  it('#5891 editor mode IGNORES viewportId entirely — no error, no echoed fields', async () => {
    // The editor arm resolves no testid, so a viewportId is meaningless there and
    // must not become a spurious rejection: a caller that threads viewportId
    // through generically would otherwise break every editor scroll.
    const scrollDOM = document.createElement('div');
    Object.defineProperty(scrollDOM, 'scrollTop', { configurable: true, writable: true, value: 0 });
    Object.defineProperty(scrollDOM, 'scrollLeft', { configurable: true, writable: true, value: 0 });
    (window as any).__REIFY_DEBUG__.editorView = { scrollDOM } as any;

    const result = await dispatchCmd(5415, 'scroll', {
      target: 'editor', top: 80, viewportId: 'pane-1',
    });

    expect(result).toEqual({ ok: true, scrollTop: 80, scrollLeft: 0 });
  });
});

// ---------------------------------------------------------------------------
// debug bridge dom_query viewport scoping (#5891 step-7 RED → step-8 GREEN)
//
// dom_query had NO dedicated unit test before this task, so this block is its
// first coverage — the pre-#5891 payload shape is pinned here for the first time
// rather than merely preserved.
//
// dom_query's contract differs deliberately from the driving tools above: it is
// an EXISTENCE PROBE, so "no match in that pane" is `{exists:false}`, not an
// error. Harnesses poll it while waiting for a pane to appear; turning a
// not-yet-there pane into an error would make every such poll a failure instead
// of a `false`. A malformed request — a non-string `viewportId` — is still a
// loud error, because that is a caller bug rather than an observation.
// ---------------------------------------------------------------------------
describe('debug bridge dom_query viewport scoping', () => {
  let capturedHandler: DebugRequestHandler | undefined;

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

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
    vi.restoreAllMocks();
  });

  /**
   * Two panes each holding a same-testid element with DISTINGUISHABLE text, so
   * asserting on `text` proves WHICH element was described — not merely that
   * something existed. jsdom implements no layout and no innerText, so `text` is
   * stubbed per element; `bounds` stays all-zero (and therefore `visible` false)
   * exactly as it already does for every other jsdom-hosted dom_query.
   */
  function twoPanesWithBadge() {
    document.body.innerHTML = `
      <div data-viewport-id="design-main"><div data-testid="badge"></div></div>
      <div data-viewport-id="pane-1"><div data-testid="badge"></div></div>
    `;
    const [designMain, pane1] = ['design-main', 'pane-1'].map((id) => {
      const el = document.querySelector(
        `[data-viewport-id="${id}"] [data-testid="badge"]`,
      ) as HTMLElement;
      Object.defineProperty(el, 'innerText', { configurable: true, value: `${id} body` });
      return el;
    });
    return { designMain, pane1 };
  }

  /** The complete pre-#5891 five-key payload — asserted with toEqual so an extra key fails. */
  const description = (text: string) => ({
    exists: true,
    visible: false, // jsdom reports a zero-width rect for every element
    text,
    tagName: 'div',
    bounds: { x: 0, y: 0, width: 0, height: 0 },
  });

  it('#5891 scoped: describes the named pane\'s element, not the first match', async () => {
    twoPanesWithBadge();

    const result = await dispatchCmd(5420, 'dom_query', { testId: 'badge', viewportId: 'pane-1' });

    expect(result).toEqual(description('pane-1 body'));
  });

  it('#5891 scoped to the FIRST pane still describes that pane — not merely the last match', async () => {
    // The mirror of the case above. Without it, a resolver that always returned
    // the LAST match would pass the 'pane-1' case for the wrong reason.
    twoPanesWithBadge();

    const result = await dispatchCmd(5421, 'dom_query', {
      testId: 'badge', viewportId: 'design-main',
    });

    expect(result).toEqual(description('design-main body'));
  });

  it('#5891 unknown viewportId returns {exists:false} — the deliberate existence-probe contract', async () => {
    // NOT an error, unlike click_element/focus_element/scroll: dom_query is how a
    // harness ASKS whether a pane's element is there yet. "That pane has no such
    // element" is a legitimate observation and must stay pollable as `false`.
    twoPanesWithBadge();

    const result = await dispatchCmd(5422, 'dom_query', { testId: 'badge', viewportId: 'nope' });

    expect(result).toEqual({ exists: false });
  });

  it('#5891 non-string viewportId returns viewportIdNotString', async () => {
    // A malformed param is a CALLER bug, not an observation, so it stays loud even
    // though a missing element does not.
    twoPanesWithBadge();

    const result = await dispatchCmd(5423, 'dom_query', { testId: 'badge', viewportId: 5 });

    expect(result).toEqual({ error: RESOLVE_BY_TESTID_ERRORS.viewportIdNotString });
  });

  it('#5891 unscoped multi-match describes pane 0 and reports the guessed pane', async () => {
    twoPanesWithBadge();

    const result = await dispatchCmd(5424, 'dom_query', { testId: 'badge' });

    expect(result).toEqual({
      ...description('design-main body'),
      viewportId: 'design-main',
      matchCount: 2,
    });
  });

  it('#5891 single match keeps today\'s exact five-key payload — no diagnostic keys leak', async () => {
    document.body.innerHTML = '<div data-testid="lonely-badge"></div>';
    const el = document.querySelector('[data-testid="lonely-badge"]') as HTMLElement;
    Object.defineProperty(el, 'innerText', { configurable: true, value: 'only one' });

    const result = await dispatchCmd(5425, 'dom_query', { testId: 'lonely-badge' });

    expect(result).toEqual(description('only one'));
  });

  it('#5891 zero-match unscoped query still returns {exists:false}', async () => {
    twoPanesWithBadge();

    const result = await dispatchCmd(5426, 'dom_query', { testId: 'no-such-testid' });

    expect(result).toEqual({ exists: false });
  });
});

// ---------------------------------------------------------------------------
// debug bridge apply_gui_state (task-3026 step-24 RED → step-25 GREEN)
//
// The Rust `handle_set_fea_case` pushes a rebuilt GuiState to the frontend
// via `query_frontend("apply_gui_state", { guiState, case })`.  This handler
// applies the GuiState WITHOUT resetting the view (geometry is shared across
// cases; only the contour changes), so the camera stays fixed and per-case
// screenshots differ only in the scalar-channel colours.
// ---------------------------------------------------------------------------
describe('debug bridge apply_gui_state', () => {
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
    await handler({ payload: { id, command: 'apply_gui_state', params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  /** Minimal RawGuiState fixture with one mesh carrying a known vonMises channel. */
  const rawGuiStateWithVonMises = {
    meshes: [
      {
        entity_path: 'body/bracket',
        vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
        indices: [0, 1, 2],
        normals: null,
        scalar_channels: { vonMises: [200.0, 200.0, 200.0] },
      },
    ],
    values: [],
    constraints: [],
    files: [],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
  };

  it('(a) returns { ok: true, case: "overload" } on success', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    expect(capturedHandler).toBeDefined();

    const result = await dispatch(capturedHandler!, 6000, {
      guiState: rawGuiStateWithVonMises,
      case: 'overload',
    });

    expect(result).toEqual({ ok: true, case: 'overload' });
  });

  it('(b) calls initFromState exactly once with converted GuiState carrying the vonMises channel', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    await dispatch(capturedHandler!, 6001, {
      guiState: rawGuiStateWithVonMises,
      case: 'overload',
    });

    expect(stores.engine.initFromState).toHaveBeenCalledTimes(1);
    const passed = vi.mocked(stores.engine.initFromState).mock.calls[0][0];
    // Mesh must be present and carry the vonMises channel converted to Float32Array
    expect(passed.meshes).toHaveLength(1);
    expect(passed.meshes[0].scalar_channels).toBeDefined();
    expect(passed.meshes[0].scalar_channels!['vonMises']).toEqual(new Float32Array([200.0, 200.0, 200.0]));
  });

  it('(c) does NOT call resetToDefaultView — camera must be preserved across case switches', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    await dispatch(capturedHandler!, 6002, {
      guiState: rawGuiStateWithVonMises,
      case: 'overload',
    });

    expect(stores.viewState.resetToDefaultView).not.toHaveBeenCalled();
  });

  it('(d) omitting guiState returns { error } and does not call initFromState', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const result = await dispatch(capturedHandler!, 6003, { case: 'overload' });

    expect(result).toHaveProperty('error');
    expect(stores.engine.initFromState).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// debug bridge resize_panes problemsHeight + get_local_storage (task-4404 step-3 RED)
// RED: (a) problemsHeight is not in resize_panes DIMS so setProblemsHeight is never called;
//      (b-d) get_local_storage has no handler (returns "unknown command").
// ---------------------------------------------------------------------------

describe('debug bridge resize_panes problemsHeight + get_local_storage', () => {
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
    window.localStorage.clear();
  });

  /** makeStores extended with problems-panel layout fields (task-4404).
   *  Setters update state so that the layout-echo in resize_panes reflects the new value. */
  function makeStoresWithProblems() {
    const s = makeStores();
    (s.layout.state as any).problemsHeight = 160;
    (s.layout.state as any).problemsCollapsed = true;
    (s.layout as any).setProblemsHeight = vi.fn((v: number) => {
      (s.layout.state as any).problemsHeight = v;
    });
    (s.layout as any).setProblemsCollapsed = vi.fn((v: boolean) => {
      (s.layout.state as any).problemsCollapsed = v;
    });
    return s;
  }

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

  // (a) resize_panes({problemsHeight:240}) must call setProblemsHeight(240) and return
  //     a layout echo that includes problemsHeight:240.
  //     RED: not in DIMS → handler returns {error:"no pane dimensions provided"},
  //     setProblemsHeight never called, result.layout undefined.
  it('resize_panes({problemsHeight:240}) calls setProblemsHeight(240) and layout echo includes problemsHeight:240', async () => {
    const stores = makeStoresWithProblems();
    await initDebugBridge(stores);

    const result = await dispatchCmd(9000, 'resize_panes', { problemsHeight: 240 });

    expect(result.ok).toBe(true);
    expect((stores.layout as any).setProblemsHeight).toHaveBeenCalledWith(240);
    expect(result.layout.problemsHeight).toBe(240);
  });

  // (b) get_local_storage: present key returns {key, value (string), present:true}.
  //     RED: unknown command → result has no key/present fields.
  it('get_local_storage: seeded key returns {key, value, present:true}', async () => {
    window.localStorage.setItem(
      'reify-panel-layout',
      JSON.stringify({ problemsHeight: 200, problemsCollapsed: false }),
    );
    const stores = makeStores();
    await initDebugBridge(stores);

    const result = await dispatchCmd(9001, 'get_local_storage', { key: 'reify-panel-layout' });

    expect(result.key).toBe('reify-panel-layout');
    expect(result.present).toBe(true);
    expect(typeof result.value).toBe('string');
    const parsed = JSON.parse(result.value as string);
    expect(parsed.problemsHeight).toBe(200);
    expect(parsed.problemsCollapsed).toBe(false);
  });

  // (c) get_local_storage: absent key returns {key, value:null, present:false}.
  //     RED: unknown command → result has no key/present fields.
  it('get_local_storage: absent key returns {key, value:null, present:false}', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const result = await dispatchCmd(9002, 'get_local_storage', { key: 'absent-key-xyz-4404' });

    expect(result.key).toBe('absent-key-xyz-4404');
    expect(result.present).toBe(false);
    expect(result.value).toBeNull();
  });

  // (d) get_local_storage: missing key arg returns {error: "key is required"}.
  //     RED: unknown command → error is "unknown command: get_local_storage", not "key is required".
  it('get_local_storage: missing key arg returns {error: "key is required"}', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const result = await dispatchCmd(9003, 'get_local_storage', {});

    expect(result.error).toBe('key is required');
  });
});

// ---------------------------------------------------------------------------
// store_state includes viewports (task-4764 step-7 RED)
// RED: store_state handler currently returns only engine/editor/selection/claude;
//      no `viewports` field is present in the result.
// ---------------------------------------------------------------------------

describe('debug bridge store_state includes viewports', () => {
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

  /** Minimal DebugViewport stub: getMeshes returns an empty Map (meshCount === 0). */
  function makeEmptyStub() {
    return {
      scene: {} as any,
      camera: {
        position: { set: vi.fn(), x: 0, y: 0, z: 0 },
        up: { set: vi.fn(), x: 0, y: 1, z: 0 },
        zoom: 1,
        lookAt: vi.fn(),
        updateProjectionMatrix: vi.fn(),
        updateMatrixWorld: vi.fn(),
      } as any,
      renderer: { render: vi.fn(), domElement: { toDataURL: vi.fn() } } as any,
      getMeshes: vi.fn().mockReturnValue(new Map()),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
      controls: { target: { set: vi.fn(), x: 0, y: 0, z: 0 }, update: vi.fn() } as any,
    };
  }

  /** Minimal DebugViewport stub: getMeshes returns a Map with 1 entry (meshCount === 1). */
  function makePopulatedStub() {
    const stub = makeEmptyStub();
    const meshMap = new Map<string, unknown>([['entity-path-1', {}]]);
    stub.getMeshes = vi.fn().mockReturnValue(meshMap);
    return stub;
  }

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

  it('store_state exposes viewports map with meshCount per registered viewport', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    // Register three viewports: design-main (populated), def-preview (empty), pane-1 (populated)
    window.__REIFY_DEBUG__!.viewports = {
      'design-main': makePopulatedStub() as any,
      'def-preview': makeEmptyStub() as any,
      'pane-1': makePopulatedStub() as any,
    };

    const result = await dispatchCmd(10001, 'store_state', {});

    // `viewports` must be present and be an object
    expect(result.viewports).toBeDefined();
    expect(typeof result.viewports).toBe('object');

    // Keys must include all three registered viewports
    const keys = Object.keys(result.viewports);
    expect(keys).toHaveLength(3);
    expect(keys).toContain('design-main');
    expect(keys).toContain('def-preview');
    expect(keys).toContain('pane-1');

    // meshCount reports from getMeshes().size
    expect(result.viewports['design-main'].meshCount).toBe(1);
    expect(result.viewports['def-preview'].meshCount).toBe(0);
    expect(result.viewports['pane-1'].meshCount).toBe(1);
  });

  it('store_state returns viewports as {} when no viewports are registered', async () => {
    const stores = makeStores(['A', 'B']);
    await initDebugBridge(stores);
    // No viewports injected — window.__REIFY_DEBUG__.viewports is undefined

    const result = await dispatchCmd(10002, 'store_state', {});

    // viewports must be present and empty (not undefined, no throw)
    expect(result.viewports).toBeDefined();
    expect(result.viewports).toEqual({});

    // Existing selection assertions must still hold
    expect(result.selection.selectedEntities).toEqual(['A', 'B']);
  });
});

// ── PRD-2 ε step-3: material-state probe ─────────────────────────────────────
//
// RED: viewport_state meshInfo currently contains ONLY {entityPath, vertexCount,
// faceCount}. Step-4 additively extends bridge.ts to include a per-mesh `material`
// sub-record {color:[r,g,b], opacity, metalness, roughness, wireframe, type}.
//
// This describe block asserts that shape — it goes RED until step-4 lands.
describe('debug bridge viewport_state material-state probe', () => {
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

  /** Build a mock THREE.js Color object with r/g/b component floats. */
  function makeColor(r: number, g: number, b: number) {
    return { r, g, b };
  }

  /** Minimal viewport stub whose getMeshes returns a Map built from meshEntries. */
  function makeViewportWithMeshes(meshEntries: Array<[string, object]>) {
    const meshMap = new Map<string, unknown>(meshEntries);
    return {
      scene: {} as any,
      camera: {
        position: { set: vi.fn(), x: 0, y: 0, z: 0 },
        up: { set: vi.fn(), x: 0, y: 1, z: 0 },
        zoom: 1,
        lookAt: vi.fn(),
        updateProjectionMatrix: vi.fn(),
        updateMatrixWorld: vi.fn(),
        fov: 45, near: 0.1, far: 1000,
        rotation: { x: 0, y: 0, z: 0 },
      } as any,
      renderer: { render: vi.fn(), domElement: { toDataURL: vi.fn() } } as any,
      getMeshes: vi.fn().mockReturnValue(meshMap),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
      controls: { target: { set: vi.fn(), x: 0, y: 0, z: 0 }, update: vi.fn() } as any,
    };
  }

  /** Dispatch viewport_state and return parsed result. */
  async function dispatchViewportState(viewportId?: string) {
    vi.mocked(invoke).mockClear();
    const params: Record<string, unknown> = {};
    if (viewportId !== undefined) params.viewportId = viewportId;
    await capturedHandler!({ payload: { id: 20000, command: 'viewport_state', params } });
    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const payload = responseCall![1] as { id: number; result: string };
    return JSON.parse(payload.result);
  }

  it('meshInfo entries carry per-mesh material state for MeshStandardMaterial', async () => {
    // Build a mock mesh with a MeshStandardMaterial (steel editorial values)
    const stdMaterial = {
      type: 'MeshStandardMaterial',
      color: makeColor(0.5, 0.5, 0.52),
      opacity: 1.0,
      metalness: 0.9,
      roughness: 0.4,
      wireframe: false,
    };
    const mockGeometry = {
      getAttribute: vi.fn().mockReturnValue({ count: 12 }),
      getIndex: vi.fn().mockReturnValue({ count: 12 }),
    };
    const steelMesh = { geometry: mockGeometry, material: stdMaterial };

    // Build a second mesh with a MeshPhongMaterial (hash-fallback path)
    const phongMaterial = {
      type: 'MeshPhongMaterial',
      color: makeColor(0.8, 0.3, 0.1),
      opacity: 1.0,
      wireframe: false,
    };
    const rawMesh = { geometry: mockGeometry, material: phongMaterial };

    const stores = makeStores();
    await initDebugBridge(stores);
    window.__REIFY_DEBUG__!.viewports = {
      'design-main': makeViewportWithMeshes([
        // Entity paths match the canonical fixture (appearance_viewport_egress.ri):
        //   steel body is a DIRECT member → 'AppearanceViewportEgress#realization[0]'
        //   raw box is the '.raw' sub      → 'AppearanceViewportEgress.raw#realization[0]'
        ['AppearanceViewportEgress#realization[0]', steelMesh],
        ['AppearanceViewportEgress.raw#realization[0]', rawMesh],
      ]) as any,
    };

    const result = await dispatchViewportState('design-main');

    // meshInfo must have 2 entries
    expect(result.meshInfo).toBeDefined();
    expect(result.meshInfo).toHaveLength(2);

    // Find the steel mesh entry
    const steelInfo = result.meshInfo.find(
      (m: any) => m.entityPath === 'AppearanceViewportEgress#realization[0]',
    );
    expect(steelInfo).toBeDefined();

    // RED assertion: meshInfo entry must carry a `material` sub-record.
    // This will fail until step-4 extends bridge.ts viewport_state.
    expect(steelInfo.material).toBeDefined();
    expect(steelInfo.material.type).toBe('MeshStandardMaterial');
    expect(steelInfo.material.color).toEqual([0.5, 0.5, 0.52]);
    expect(steelInfo.material.opacity).toBe(1.0);
    expect(steelInfo.material.metalness).toBe(0.9);
    expect(steelInfo.material.roughness).toBe(0.4);
    expect(steelInfo.material.wireframe).toBe(false);

    // Find the raw/phong mesh entry
    const rawInfo = result.meshInfo.find(
      (m: any) => m.entityPath === 'AppearanceViewportEgress.raw#realization[0]',
    );
    expect(rawInfo).toBeDefined();
    expect(rawInfo.material).toBeDefined();
    expect(rawInfo.material.type).toBe('MeshPhongMaterial');
    // Phong does not have metalness/roughness — may be undefined
    expect(rawInfo.material.color).toEqual([0.8, 0.3, 0.1]);
    expect(rawInfo.material.opacity).toBe(1.0);
    expect(rawInfo.material.wireframe).toBe(false);
  });

  it('meshInfo entry material is null/undefined when mesh has no material', async () => {
    // A mesh with no material property (or material === null)
    const noMaterialMesh = {
      geometry: {
        getAttribute: vi.fn().mockReturnValue({ count: 6 }),
        getIndex: vi.fn().mockReturnValue({ count: 6 }),
      },
      // no material property
    };

    const stores = makeStores();
    await initDebugBridge(stores);
    window.__REIFY_DEBUG__!.viewports = {
      'design-main': makeViewportWithMeshes([
        ['entity/no-material', noMaterialMesh],
      ]) as any,
    };

    const result = await dispatchViewportState('design-main');
    expect(result.meshInfo).toHaveLength(1);

    const info = result.meshInfo[0];
    // When the mesh has no material, the probe must emit null or omit material entirely.
    // Either is acceptable; this test documents the contract (null | undefined).
    const mat = info.material;
    expect(mat == null || mat === undefined).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// debug bridge set_fea_channel (task 4906 step-5 RED → step-6 GREEN)
//
// set_fea_channel is a FRONTEND-ONLY debug command (no Rust dispatch arm —
// the default `_ =>` arm in debug_server.rs routes it to query_frontend). It
// drives the native <select data-testid="fea-mode-channel-select"> rendered
// by FeaModeToolbar: sets .value to the requested channel and dispatches a
// bubbling 'change' event so the component's own onChange (-> store.setChannel)
// fires, exactly as a real user selection would. These tests FAIL until
// step-6 adds the `set_fea_channel` handler to buildHandlers() in bridge.ts.
// ---------------------------------------------------------------------------

describe('debug bridge set_fea_channel', () => {
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
    cleanup();
    // (i) below appends a raw <select> directly (bypassing Solid's render(),
    // deliberately — see that test) which `cleanup()` does not know about;
    // clear it here too so a failed assertion can't leak a stale
    // data-testid="fea-mode-channel-select" into a later test in this block.
    document.body.innerHTML = '';
    delete window.__REIFY_DEBUG__;
  });

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

  /**
   * Render the toolbar enabled, with errorIndicator among the available channels.
   *
   * Returns a FRESH `FeaModeStore`, independent from the `stores` object
   * passed to `initDebugBridge()` in each test below — the `set_fea_channel`
   * handler still never reads/writes the DebugStores `stores` object
   * directly. It DOES read the debug context's FEA slots, so this helper
   * registers the store there, exactly as App registers its keyed registry via
   * `registerDebugPanel`. Do NOT skip this registration — the store-based
   * propagation check (does the store's channel actually match after dispatch?)
   * is meaningless unless the harness threads the SAME store the toolbar
   * updates onto the debug context, mirroring the real App wiring.
   *
   * `viewportId` (#5670) selects which wiring is mirrored:
   *  - omitted → no `data-viewport-id` on the toolbar, store registered on the
   *    LEGACY scalar `ctx.feaMode` slot. Cases (a)-(k) below use this form, so
   *    they exercise the same fallback path a pre-#5670 caller would.
   *  - given → the toolbar stamps `data-viewport-id`, and the store lands in
   *    the keyed `ctx.feaModes` map under that id, as App's registry does.
   *    `'design-main'` ALSO populates the scalar `ctx.feaMode` slot, because
   *    App registers both (`registerDebugPanel('feaMode', registry.get(
   *    'design-main'))` beside the keyed record). Mirroring that matters: it is
   *    what makes (p) discriminating — without a populated scalar slot there is
   *    nothing for a keyed lookup to wrongly fall back TO, so the test would
   *    pass on absence rather than on the handler's refusal.
   */
  function renderToolbarWithErrorIndicator(viewportId?: string) {
    const store = createFeaModeStore();
    store.setEnabled(true);
    render(() => (
      <FeaModeToolbar
        store={store}
        availableChannels={['vonMises', 'displacement_magnitude', 'errorIndicator']}
        viewportId={viewportId}
      />
    ));
    const ctx = window.__REIFY_DEBUG__!;
    if (viewportId === undefined) {
      ctx.feaMode = store;
    } else {
      (ctx.feaModes ??= {})[viewportId] = store;
      // App keeps the legacy scalar slot as a mirror of design-main's entry
      // (exactly as `viewport` is kept beside `viewports`). Reproduce that, so
      // the keyed cases run against the real two-slot production context.
      if (viewportId === 'design-main') ctx.feaMode = store;
    }
    return store;
  }

  /** The channel select belonging to one pane's toolbar. */
  function selectFor(viewportId: string) {
    return document.querySelector(
      `[data-testid="fea-mode-channel-select"][data-viewport-id="${viewportId}"]`,
    ) as HTMLSelectElement;
  }

  it('(a) {channel:"errorIndicator"} sets select.value, fires store.setChannel via change, and returns {ok:true}', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const store = renderToolbarWithErrorIndicator();
    expect(capturedHandler).toBeDefined();

    const result = await dispatchCmd(4100, 'set_fea_channel', { channel: 'errorIndicator' });

    expect(result).toEqual({ ok: true });
    const select = document.querySelector('[data-testid="fea-mode-channel-select"]') as HTMLSelectElement;
    expect(select.value).toBe('errorIndicator');
    expect(store.state.channel).toBe('errorIndicator');
    // Genuine propagation check: the debug-context handle (what the handler
    // actually reads) reflects the change, not just the DOM value it wrote.
    expect(window.__REIFY_DEBUG__!.feaMode!.state.channel).toBe('errorIndicator');
  });

  it('(b) {channel:"notAChannel"} returns "channel not available" and does not change the select value', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    const store = renderToolbarWithErrorIndicator();

    const result = await dispatchCmd(4101, 'set_fea_channel', { channel: 'notAChannel' });

    // Asserted against the shared SET_FEA_CHANNEL_ERRORS constant (task 4906
    // amendment) rather than a duplicated string literal — bridge.ts and this
    // test now read the same value, so a generic {error:...} shape from the
    // pre-GREEN placeholder still cannot match it, and a future wording tweak
    // only needs to change in one place.
    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.channelNotAvailable });
    const select = document.querySelector('[data-testid="fea-mode-channel-select"]') as HTMLSelectElement;
    expect(select.value).toBe('vonMises');
    expect(store.state.channel).toBe('vonMises');
  });

  it('(c) missing channel param returns "channel is required"', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    renderToolbarWithErrorIndicator();

    const result = await dispatchCmd(4102, 'set_fea_channel', {});

    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.channelRequired });
  });

  it('(d) select absent (toolbar not rendered) returns a "not found" error', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    // No FeaModeToolbar rendered — no channel select in the DOM at all.
    expect(document.querySelector('[data-testid="fea-mode-channel-select"]')).toBeNull();

    const result = await dispatchCmd(4103, 'set_fea_channel', { channel: 'errorIndicator' });

    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.selectNotFound });
  });

  it('(e) {channel:""} returns "channel not available" and does not change the select value', async () => {
    // Empty string passes the `typeof channel !== 'string'` guard (it IS a
    // string) so it falls through to the options-membership check, same as
    // any other non-matching value. Pinned separately from (b) so a future
    // refactor that special-cases empty/falsy strings is caught here.
    const stores = makeStores();
    await initDebugBridge(stores);
    const store = renderToolbarWithErrorIndicator();

    const result = await dispatchCmd(4104, 'set_fea_channel', { channel: '' });

    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.channelNotAvailable });
    const select = document.querySelector('[data-testid="fea-mode-channel-select"]') as HTMLSelectElement;
    expect(select.value).toBe('vonMises');
    expect(store.state.channel).toBe('vonMises');
  });

  it('(f) {channel:5} (present but non-string) returns "channel must be a string", distinct from missing-param (c)', async () => {
    // A present-but-wrong-type value is a caller bug distinct from simply
    // omitting the param; pinning a separate message keeps (c)'s "channel is
    // required" reserved for genuine absence rather than masking type bugs.
    const stores = makeStores();
    await initDebugBridge(stores);
    const store = renderToolbarWithErrorIndicator();

    const result = await dispatchCmd(4105, 'set_fea_channel', { channel: 5 });

    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.channelNotString });
    const select = document.querySelector('[data-testid="fea-mode-channel-select"]') as HTMLSelectElement;
    expect(select.value).toBe('vonMises');
    expect(store.state.channel).toBe('vonMises');
  });

  it('(g) disabled select returns "channel select is disabled" and does not change the value', async () => {
    // Currently unreachable via the toolbar's own render conditions (the
    // select only mounts when FEA mode is enabled), but pins the defense-in-
    // depth guard so a dispatched 'change' on a disabled control can never
    // silently report {ok:true} without the store actually updating.
    const stores = makeStores();
    await initDebugBridge(stores);
    const store = renderToolbarWithErrorIndicator();
    const select = document.querySelector('[data-testid="fea-mode-channel-select"]') as HTMLSelectElement;
    select.disabled = true;

    const result = await dispatchCmd(4106, 'set_fea_channel', { channel: 'errorIndicator' });

    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.selectDisabled });
    expect(select.value).toBe('vonMises');
    expect(store.state.channel).toBe('vonMises');
  });

  it('(h) reports an error instead of a false {ok:true} if the select does not settle on the requested channel after dispatch', async () => {
    // Simulates a non-propagating change (e.g. an event-delegation difference
    // in the real browser vs jsdom, or some other handler interfering) by
    // attaching a second 'change' listener that forces the value back to the
    // toolbar's original channel. Whether this runs before or after the
    // component's own onChange (delegated vs direct-attached listeners
    // resolve differently), the net effect is that the select does NOT
    // settle on the requested channel — exactly the failure mode the
    // read-back guard exists to catch. (An earlier version of this test
    // tried to simulate the same failure by redefining the `.value` accessor
    // via Object.defineProperty, but jsdom/webidl2js wraps HTMLSelectElement
    // in a Proxy — since it supports indexed properties — so redefining an
    // own accessor throws "trap returned falsish" instead of behaving like a
    // plain object; a genuine second listener avoids that entirely.)
    const stores = makeStores();
    await initDebugBridge(stores);
    renderToolbarWithErrorIndicator();
    const select = document.querySelector('[data-testid="fea-mode-channel-select"]') as HTMLSelectElement;
    select.addEventListener('change', () => {
      select.value = 'vonMises';
    });

    const result = await dispatchCmd(4107, 'set_fea_channel', { channel: 'errorIndicator' });

    expect(result).toEqual({
      error: SET_FEA_CHANNEL_ERRORS.didNotPropagate('vonMises', 'errorIndicator'),
    });
  });

  it('(i) propagation-verified: a select with no wired onChange now returns didNotReachStore — the store guard catches the silently-inert-onChange blind spot the DOM read-back guard alone could not', async () => {
    // Deliberately does NOT use renderToolbarWithErrorIndicator() / the real
    // FeaModeToolbar component: this is a hand-built <select> that matches
    // the toolbar's markup (same testid, same options) but has no onChange
    // handler wired to any store at all — the failure mode the DOM read-back
    // guard alone cannot catch (the handler sets `select.value = channel`
    // *before* dispatching 'change', so the DOM trivially reads back as
    // `channel` whether or not anything downstream ever observed the event).
    //
    // A fresh, unwired FeaModeStore is registered as ctx.feaMode — exactly as
    // DualViewport would register a real mounted toolbar's store — so the
    // handler has a store to read back from. Because nothing here ever calls
    // store.setChannel, it stays at its default 'vonMises', proving the store
    // guard now catches this exact blind spot instead of silently reporting
    // {ok:true} (the pre-4981 behavior this test used to pin).
    const stores = makeStores();
    await initDebugBridge(stores);
    const select = document.createElement('select');
    select.setAttribute('data-testid', 'fea-mode-channel-select');
    for (const ch of ['vonMises', 'errorIndicator']) {
      const opt = document.createElement('option');
      opt.value = ch;
      select.appendChild(opt);
    }
    select.value = 'vonMises';
    document.body.appendChild(select);
    const unwiredStore = createFeaModeStore();
    window.__REIFY_DEBUG__!.feaMode = unwiredStore;

    const result = await dispatchCmd(4108, 'set_fea_channel', { channel: 'errorIndicator' });

    expect(result).toEqual({
      error: SET_FEA_CHANNEL_ERRORS.didNotReachStore('vonMises', 'errorIndicator'),
    });
    // The DOM alone still (falsely) reads back as settled — exactly why the
    // store guard is necessary; select.value is no longer sufficient evidence.
    expect(select.value).toBe('errorIndicator');
    expect(unwiredStore.state.channel).toBe('vonMises');
  });

  it('(k) ctx.feaMode not registered while the select is present returns storeUnavailable instead of a false {ok:true}', async () => {
    // A rendered fea-mode-channel-select is only produced by FeaModeToolbar,
    // which only renders where a feaModeStore is wired — and DualViewport
    // always registers that store on ctx.feaMode. So select-present implies
    // store-registered; the store being absent here simulates a real wiring
    // anomaly (or a future FEA-capable pane that failed to register). The
    // handler must fail loudly rather than silently falling back to the
    // DOM-only read-back, which would reintroduce the exact blind spot task
    // 4981 closes.
    const stores = makeStores();
    await initDebugBridge(stores);
    renderToolbarWithErrorIndicator();
    delete window.__REIFY_DEBUG__!.feaMode;

    const result = await dispatchCmd(4110, 'set_fea_channel', { channel: 'errorIndicator' });

    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.storeUnavailable });
  });

  it('(j)/(o) two fea-mode-channel-select elements and NO viewportId still returns an ambiguous error and changes neither select', async () => {
    // Since #5670 every pane owns a keyed FeaModeStore, so two mounted
    // toolbars is the ordinary N-pane case rather than a hypothetical. An
    // unscoped request is still genuinely ambiguous: the handler must fail
    // loudly rather than silently target "whichever select happens to be first
    // in DOM order" (task 4906 amendment). Resolution is by DOM count, so an
    // id-less pair takes this identical branch.
    const stores = makeStores();
    await initDebugBridge(stores);
    const storeA = renderToolbarWithErrorIndicator('design-main');
    const storeB = renderToolbarWithErrorIndicator('pane-1');
    const selects = document.querySelectorAll('[data-testid="fea-mode-channel-select"]');
    expect(selects).toHaveLength(2);

    const result = await dispatchCmd(4109, 'set_fea_channel', { channel: 'errorIndicator' });

    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.selectAmbiguous(2) });
    selects.forEach((el) => expect((el as HTMLSelectElement).value).toBe('vonMises'));
    expect(storeA.state.channel).toBe('vonMises');
    expect(storeB.state.channel).toBe('vonMises');
  });

  it('(l) viewportId targets exactly that pane’s toolbar and store, with no cross-pane bleed', async () => {
    // The point of keying (#5670): with two panes mounted, a scoped request
    // drives one toolbar and one store. Asserting the OTHER pane is untouched —
    // both its DOM value and its store — is what proves the handler resolved
    // the store from the element it actually drove rather than from some
    // global slot that happens to hold a different pane's store.
    const stores = makeStores();
    await initDebugBridge(stores);
    const designMain = renderToolbarWithErrorIndicator('design-main');
    const pane1 = renderToolbarWithErrorIndicator('pane-1');

    const result = await dispatchCmd(4111, 'set_fea_channel', {
      channel: 'errorIndicator',
      viewportId: 'pane-1',
    });

    expect(result).toEqual({ ok: true });
    expect(selectFor('pane-1').value).toBe('errorIndicator');
    expect(pane1.state.channel).toBe('errorIndicator');
    expect(selectFor('design-main').value).toBe('vonMises');
    expect(designMain.state.channel).toBe('vonMises');
  });

  it('(m) an unknown viewportId returns selectNotFoundForViewport even though other toolbars are mounted', async () => {
    // Distinct from selectNotFound (no toolbar anywhere): here the DOM is full
    // of selects, just none for the requested pane. Collapsing the two would
    // let a typo'd id read as "FEA UI is not up yet".
    const stores = makeStores();
    await initDebugBridge(stores);
    const designMain = renderToolbarWithErrorIndicator('design-main');
    const pane1 = renderToolbarWithErrorIndicator('pane-1');

    const result = await dispatchCmd(4112, 'set_fea_channel', {
      channel: 'errorIndicator',
      viewportId: 'nope',
    });

    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.selectNotFoundForViewport('nope') });
    expect(designMain.state.channel).toBe('vonMises');
    expect(pane1.state.channel).toBe('vonMises');
  });

  it('(n) a non-string viewportId returns a schema-violation error, matching pickViewport', async () => {
    // Same guard, same wording as the movement/screenshot commands' viewportId
    // ladder — a wrongly-typed id is a caller bug distinct from an unknown one.
    const stores = makeStores();
    await initDebugBridge(stores);
    renderToolbarWithErrorIndicator('design-main');

    const result = await dispatchCmd(4113, 'set_fea_channel', {
      channel: 'errorIndicator',
      viewportId: 5,
    });

    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.viewportIdNotString });
  });

  it('(p) a keyed toolbar whose ctx.feaModes entry was never registered returns storeUnavailable', async () => {
    // The keyed lookup must fail loudly rather than silently falling back to
    // some other pane's store — which is precisely the "silently read the wrong
    // pane's store" hole keying exists to close. Mirrors (k), one level down:
    // there the legacy scalar slot is missing, here the keyed entry is.
    //
    // The fallback is genuinely AVAILABLE here and must still be refused: the
    // harness populates ctx.feaMode with design-main's store (as App does), so
    // a handler that fell back would find a store, see its channel already
    // equals 'errorIndicator'... or not, and either way would be answering
    // about the wrong pane. That is what the design-main assertion below pins.
    const stores = makeStores();
    await initDebugBridge(stores);
    renderToolbarWithErrorIndicator('design-main');
    const pane1 = renderToolbarWithErrorIndicator('pane-1');
    expect(window.__REIFY_DEBUG__!.feaMode).toBeDefined();
    delete window.__REIFY_DEBUG__!.feaModes!['pane-1'];

    const result = await dispatchCmd(4114, 'set_fea_channel', {
      channel: 'errorIndicator',
      viewportId: 'pane-1',
    });

    expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.storeUnavailable });
    // The handler drives the element before it verifies (same ordering (h),
    // (i) and (k) pin), so pane-1's own store DID receive the change — what
    // failed is the verification, because the debug context cannot reach that
    // store. design-main is the load-bearing assertion: it is still registered
    // and still untouched, proving the handler did NOT resolve through it.
    // Silently reporting {ok:true} off another pane's store is the wrong-pane
    // hole keying exists to close.
    expect(pane1.state.channel).toBe('errorIndicator');
    expect(window.__REIFY_DEBUG__!.feaModes!['design-main'].state.channel).toBe('vonMises');
  });

  it('(q) a single KEYED toolbar with no viewportId resolves through the keyed map — the production single-pane wiring', async () => {
    // The combination cases (a)-(k) and (l)-(p) between them miss: exactly one
    // mounted toolbar that IS stamped, driven by a request that omits
    // viewportId. That is the DualViewport branch the visual-regression harness
    // exercises most often, and it takes a path neither group covers —
    // pickFeaChannelSelect falls through to the document-wide single-match
    // branch, then the store resolves through ctx.feaModes['design-main']
    // rather than the scalar slot, because the element it drove carries an id.
    const stores = makeStores();
    await initDebugBridge(stores);
    const designMain = renderToolbarWithErrorIndicator('design-main');
    expect(document.querySelectorAll('[data-testid="fea-mode-channel-select"]')).toHaveLength(1);

    const result = await dispatchCmd(4115, 'set_fea_channel', { channel: 'errorIndicator' });

    expect(result).toEqual({ ok: true });
    expect(selectFor('design-main').value).toBe('errorIndicator');
    expect(designMain.state.channel).toBe('errorIndicator');
  });

  it('(r) that single-keyed-toolbar path reads the keyed entry, not the legacy scalar slot', async () => {
    // Same shape as (q), with the scalar slot removed. In production both slots
    // hold the SAME store for design-main, so (q) alone cannot tell which one
    // the handler read. Dropping ctx.feaMode makes the two paths observably
    // different: {ok:true} here can only mean the keyed entry was used, since
    // the fallback no longer exists. (k) pins the converse — an id-LESS select
    // with no scalar slot is storeUnavailable — so together they fix the
    // element's own data-viewport-id as what selects between the two slots.
    const stores = makeStores();
    await initDebugBridge(stores);
    const designMain = renderToolbarWithErrorIndicator('design-main');
    delete window.__REIFY_DEBUG__!.feaMode;

    const result = await dispatchCmd(4116, 'set_fea_channel', { channel: 'errorIndicator' });

    expect(result).toEqual({ ok: true });
    expect(designMain.state.channel).toBe('errorIndicator');
  });

  it('(s) a viewportId containing selector metacharacters returns selectNotFoundForViewport, not a CSS-parser throw', async () => {
    // The id is interpolated into an attribute selector, so an unescaped quote
    // or backslash makes document.querySelector THROW a DOMException, which the
    // dispatcher converts into an opaque `{error: '<parser message>'}` — an
    // unknown id reading as a bridge malfunction. Escaping keeps a hostile or
    // simply typo'd id on the same "no toolbar for that pane" branch as (m).
    const stores = makeStores();
    await initDebugBridge(stores);
    const designMain = renderToolbarWithErrorIndicator('design-main');

    for (const [i, badId] of ['pane-"1"', 'pane-\\1', 'pane-1"]'].entries()) {
      const result = await dispatchCmd(4117 + i, 'set_fea_channel', {
        channel: 'errorIndicator',
        viewportId: badId,
      });

      expect(result).toEqual({ error: SET_FEA_CHANNEL_ERRORS.selectNotFoundForViewport(badId) });
    }
    expect(designMain.state.channel).toBe('vonMises');
  });
});

describe('debug bridge resolveByTestId viewport scoping', () => {
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
    cleanup();
    document.body.innerHTML = '';
    delete window.__REIFY_DEBUG__;
  });

  const dispatchCmd = makeCmdDispatcher(() => capturedHandler);

  /**
   * Mount one pane's REAL FeaModeToolbar under `viewportId`, enabled.
   *
   * Copied from the `set_fea_channel` suite's helper above (#5670) so these
   * scoping tests run against the genuine `data-viewport-id` substrate the
   * production component stamps — a hand-built DOM stub could drift from it
   * and would not prove that the toolbar's own markup is addressable.
   * `setEnabled(true)` is what makes `fea-mode-enable-toggle` a discriminating
   * target: a click flips that pane's store true → false, so "which pane was
   * driven" is directly readable off the two returned stores.
   */
  function mountPane(viewportId: string) {
    const store = createFeaModeStore();
    store.setEnabled(true);
    render(() => (
      <FeaModeToolbar
        store={store}
        availableChannels={['vonMises', 'displacement_magnitude', 'errorIndicator']}
        viewportId={viewportId}
      />
    ));
    ((window.__REIFY_DEBUG__!).feaModes ??= {})[viewportId] = store;
    return store;
  }

  /** design-main mounts FIRST, so it is the document-wide first match. */
  async function mountTwoPanes() {
    const stores = makeStores();
    await initDebugBridge(stores);
    const designMain = mountPane('design-main');
    const pane1 = mountPane('pane-1');
    return { designMain, pane1 };
  }

  it('(a) a viewportId-scoped click_element drives that pane only — no cross-pane bleed', async () => {
    const { designMain, pane1 } = await mountTwoPanes();

    const result = await dispatchCmd(5100, 'click_element', {
      testId: 'fea-mode-enable-toggle',
      viewportId: 'pane-1',
    });

    expect(result).toEqual({ ok: true });
    // pane-1 is NOT the first match, so an unscoped resolver would have driven
    // design-main here. This pair of assertions is the whole point of #5891.
    expect(pane1.state.enabled).toBe(false);
    expect(designMain.state.enabled).toBe(true);
  });

  it('(b) the scope is symmetric — viewportId:"design-main" drives design-main only', async () => {
    // (a) alone would still pass if the resolver were accidentally hardcoded to
    // the LAST match rather than reading viewportId. Driving the other pane by
    // name rules that out.
    const { designMain, pane1 } = await mountTwoPanes();

    const result = await dispatchCmd(5101, 'click_element', {
      testId: 'fea-mode-enable-toggle',
      viewportId: 'design-main',
    });

    expect(result).toEqual({ ok: true });
    expect(designMain.state.enabled).toBe(false);
    expect(pane1.state.enabled).toBe(true);
  });

  it('(c) descendant-OR-self: a scoped fea-mode-toolbar resolves the root that carries BOTH attributes', async () => {
    // FeaModeToolbar puts data-testid="fea-mode-toolbar" and data-viewport-id on
    // the SAME element, so a descendant-only selector would fail to resolve the
    // root by its own testid while succeeding for all nine sibling controls.
    await mountTwoPanes();

    const result = await dispatchCmd(5102, 'click_element', {
      testId: 'fea-mode-toolbar',
      viewportId: 'pane-1',
    });

    expect(result).toEqual({ ok: true });
  });

  it('(d) an unknown viewportId returns notFoundForViewport and drives neither pane', async () => {
    const { designMain, pane1 } = await mountTwoPanes();

    const result = await dispatchCmd(5103, 'click_element', {
      testId: 'fea-mode-enable-toggle',
      viewportId: 'nope',
    });

    expect(result).toEqual({
      error: RESOLVE_BY_TESTID_ERRORS.notFoundForViewport('fea-mode-enable-toggle', 'nope'),
    });
    // Distinct from a silent fallback to the document-wide first match: the
    // caller named a pane that does not exist, so nothing may be driven.
    expect(designMain.state.enabled).toBe(true);
    expect(pane1.state.enabled).toBe(true);
  });

  it('(e) a non-string viewportId is a schema violation, matching pickViewport/pickFeaChannelSelect', async () => {
    const { designMain, pane1 } = await mountTwoPanes();

    const result = await dispatchCmd(5104, 'click_element', {
      testId: 'fea-mode-enable-toggle',
      viewportId: 5,
    });

    expect(result).toEqual({ error: RESOLVE_BY_TESTID_ERRORS.viewportIdNotString });
    expect(designMain.state.enabled).toBe(true);
    expect(pane1.state.enabled).toBe(true);
  });

  it('(f) a viewportId carrying selector metacharacters returns notFoundForViewport, not a CSS-parser throw', async () => {
    // Mirrors set_fea_channel case (s): the id is interpolated into an attribute
    // selector, so an unescaped quote or backslash makes querySelector THROW a
    // DOMException that the dispatcher surfaces as an opaque parser message —
    // an unknown pane reading as a bridge malfunction.
    const { designMain, pane1 } = await mountTwoPanes();

    for (const [i, badId] of ['pane-"1"', 'pane-\\1', 'pane-1"]'].entries()) {
      const result = await dispatchCmd(5105 + i, 'click_element', {
        testId: 'fea-mode-enable-toggle',
        viewportId: badId,
      });

      expect(result).toEqual({
        error: RESOLVE_BY_TESTID_ERRORS.notFoundForViewport('fea-mode-enable-toggle', badId),
      });
    }
    expect(designMain.state.enabled).toBe(true);
    expect(pane1.state.enabled).toBe(true);
  });

  it('(g) an UNSCOPED multi-match still clicks the first match, but now reports which pane it guessed', async () => {
    const { designMain, pane1 } = await mountTwoPanes();

    const result = await dispatchCmd(5108, 'click_element', {
      testId: 'fea-mode-enable-toggle',
    });

    // Back-compat: first match wins, exactly as before #5891.
    expect(designMain.state.enabled).toBe(false);
    expect(pane1.state.enabled).toBe(true);
    // ...but the guess is no longer silent. This is the whole reason unscoped
    // ambiguity stays first-match instead of becoming a hard error.
    expect(result).toEqual({ ok: true, viewportId: 'design-main', matchCount: 2 });
  });

  it('(h) a SINGLE match returns a byte-identical {ok:true} — no diagnostic keys leak into the common case', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    mountPane('design-main');

    const result = await dispatchCmd(5109, 'click_element', {
      testId: 'fea-mode-enable-toggle',
    });

    // toEqual, not a subset match: asserting the EXACT shape is what pins that
    // the overwhelmingly common single-match response is unchanged, so no
    // existing test or harness step comparing with toEqual starts failing.
    expect(result).toEqual({ ok: true });
  });

  it('(i) a SCOPED success also stays exactly {ok:true} — the caller already named the pane', async () => {
    await mountTwoPanes();

    const result = await dispatchCmd(5110, 'click_element', {
      testId: 'fea-mode-enable-toggle',
      viewportId: 'pane-1',
    });

    expect(result).toEqual({ ok: true });
  });

  it('(j) zero matches still yields the unchanged notFound wording', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    const result = await dispatchCmd(5111, 'click_element', { testId: 'no-such-thing' });

    expect(result).toEqual({ error: RESOLVE_BY_TESTID_ERRORS.notFound('no-such-thing') });
  });

  it('(k) a multi-match with NO data-viewport-id ancestor reports viewportId:null — pane unknown, ambiguity still surfaced', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);
    // Two bare divs sharing a testid, outside any pane wrapper: the resolver
    // cannot name a pane, but must not hide that it picked between two.
    document.body.innerHTML =
      '<div data-testid="bare-dup"></div><div data-testid="bare-dup"></div>';

    const result = await dispatchCmd(5112, 'click_element', { testId: 'bare-dup' });

    expect(result).toEqual({ ok: true, viewportId: null, matchCount: 2 });
  });

  it('(l) a SCOPED request that matches TWICE INSIDE the named pane also reports the guess', async () => {
    // Naming a pane narrows the candidate set; it does not guarantee it to one.
    // Gating the diagnostic on "was the request scoped?" instead of on
    // matchCount would re-create, one level down, exactly the silent
    // wrong-target failure #5891 exists to remove — the caller named a pane and
    // still got an arbitrary one of two elements, with nothing in the payload
    // saying so.
    const stores = makeStores();
    await initDebugBridge(stores);
    document.body.innerHTML =
      '<div data-viewport-id="pane-1">' +
      '<div data-testid="intra-dup"></div><div data-testid="intra-dup"></div>' +
      '</div>' +
      '<div data-viewport-id="design-main"><div data-testid="intra-dup"></div></div>';

    const result = await dispatchCmd(5113, 'click_element', {
      testId: 'intra-dup',
      viewportId: 'pane-1',
    });

    // matchCount is 2, not 3: the scoping DID exclude design-main's copy, so the
    // count reports the ambiguity that actually remained after scoping.
    expect(result).toEqual({ ok: true, viewportId: 'pane-1', matchCount: 2 });
  });

  it('(m) the pane ROOT matching both arms of the scoped selector counts ONCE, so it stays a bare {ok:true}', async () => {
    // The scoped selector is a two-arm list (`el-with-both-attrs, pane el`).
    // FeaModeToolbar stamps data-testid and data-viewport-id on the SAME node, so
    // the root matches arm 1; querySelectorAll de-duplicates, keeping matchCount
    // at 1. Without that de-duplication every scoped root lookup would report a
    // phantom matchCount:2 — this pins the distinction against case (l), where
    // the two matches are genuinely different elements.
    await mountTwoPanes();

    const result = await dispatchCmd(5114, 'click_element', {
      testId: 'fea-mode-toolbar',
      viewportId: 'pane-1',
    });

    expect(result).toEqual({ ok: true });
  });
});
