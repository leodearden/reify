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

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
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
    const controlsTargetSet = vi.fn();
    const rendererRender = vi.fn();
    const camera = {
      position: { set: cameraPositionSet, x: 0, y: 0, z: 0 },
      up: { set: cameraUpSet, x: 0, y: 1, z: 0 },
      zoom: 1,
      updateProjectionMatrix: vi.fn(),
      updateMatrixWorld: vi.fn(),
    };
    const controls = {
      target: { set: controlsTargetSet, x: 0, y: 0, z: 0 },
      update: vi.fn(),
    };
    const renderer = { render: rendererRender, domElement: { toDataURL: vi.fn() } };
    const scene = {} as any;
    return { camera, controls, renderer, scene, cameraPositionSet, cameraUpSet, controlsTargetSet, rendererRender };
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

  it('set_test_mode does not throw when a viewport is wired', async () => {
    const stores = makeStores();
    await initDebugBridge(stores);

    // Wire a stub viewport onto the context after init
    window.__REIFY_DEBUG__!.viewport = {
      scene: {} as any,
      camera: {} as any,
      renderer: {
        render: vi.fn(),
        domElement: { toDataURL: vi.fn().mockReturnValue('data:image/png;base64,abc') },
      } as any,
      getMeshes: vi.fn().mockReturnValue(new Map()),
      getGhostMeshes: vi.fn().mockReturnValue(new Map()),
      fitToView: vi.fn(),
      flyToEntity: vi.fn(),
    };

    // Should complete without throwing and return the expected payload
    await capturedHandler!({ payload: { id: 12, command: 'set_test_mode', params: { enabled: true } } });

    const calls = vi.mocked(invoke).mock.calls;
    const responseCall = calls.find((c) => c[0] === 'debug_response');
    expect(responseCall).toBeDefined();
    const result = JSON.parse((responseCall![1] as { id: number; result: string }).result);
    expect(result).toEqual({ ok: true, test_mode: true });
  });
});
