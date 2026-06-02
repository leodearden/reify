import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { createStore, reconcile } from 'solid-js/store';
import type { MeshData } from '../../types';
import { createViewportStore } from '../../stores/viewportStore';
import type { ViewportState } from '../../stores/viewportStore';

// ── Mock Viewport ────────────────────────────────────────────────────────────
// Capture rendered instances by viewportId so we can assert mesh sources.
const capturedViewportPropsByid: Record<string, any> = {};
// Capture inner ref fns by viewportId so ref-forwarding tests can inspect them.
const capturedInnerFnsByViewportId: Record<string, { fitToView: ReturnType<typeof vi.fn>; flyToEntity: ReturnType<typeof vi.fn> }> = {};

vi.mock('../../viewport/Viewport', () => ({
  Viewport: (props: any) => {
    capturedViewportPropsByid[props.viewportId] = props;
    // Simulate onMount ref registration (same as real Viewport calling fitToViewRef/flyToEntityRef)
    const innerFitToView = vi.fn();
    const innerFlyToEntity = vi.fn();
    props.fitToViewRef?.(innerFitToView);
    props.flyToEntityRef?.(innerFlyToEntity);
    capturedInnerFnsByViewportId[props.viewportId] = { fitToView: innerFitToView, flyToEntity: innerFlyToEntity };
    const el = document.createElement('div');
    el.setAttribute('data-testid', `viewport-${props.viewportId}`);
    el.textContent = `Viewport:${props.viewportId}`;
    return el;
  },
}));

// ── Mock Splitter ────────────────────────────────────────────────────────────
// Capture props keyed by data-testid so tests can invoke onResize etc.
const capturedSplitterPropsByTestId: Record<string, any> = {};

vi.mock('../../components/Splitter', () => ({
  Splitter: (props: any) => {
    capturedSplitterPropsByTestId[props['data-testid'] ?? 'splitter'] = props;
    const el = document.createElement('div');
    el.setAttribute('data-testid', props['data-testid'] ?? 'splitter');
    el.setAttribute('data-orientation', props.orientation);
    return el;
  },
}));

// ── Helpers ──────────────────────────────────────────────────────────────────

function makeMesh(path: string): MeshData {
  return {
    entity_path: path,
    vertices: new Float32Array([0, 0, 0]),
    indices: new Uint32Array([0]),
    normals: new Float32Array([0, 1, 0]),
  };
}

function makeEngineStore(meshPaths: string[] = []) {
  const meshes: Record<string, MeshData> = {};
  for (const p of meshPaths) meshes[p] = makeMesh(p);
  const [state] = createStore({ meshes, tensegrityWires: [] as any[] });
  return { state };
}

function makeDefPreviewStore(meshPaths: string[] = [], defName: string | null = null) {
  const meshes: Record<string, MeshData> = {};
  for (const p of meshPaths) meshes[p] = makeMesh(p);
  // Expose setState so reactive-transition tests can mutate meshes without
  // hand-rolling a parallel store double.
  const [state, setState] = createStore({ defName, meshes, isLoading: false, error: null as string | null });
  return {
    state,
    setState,
    applyPreview: vi.fn(),
    clearPreview: vi.fn(),
    setError: vi.fn(),
    setLoading: vi.fn(),
    loadPreview: vi.fn(),
  };
}

const DEFAULT_TEST_CAMERA = {
  position: [0, 0, 5] as [number, number, number],
  target: [0, 0, 0] as [number, number, number],
  // Use current Z-up convention so fixtures stay consistent with the seeded default.
  up: [0, 0, 1] as [number, number, number],
  zoom: 1,
};

function makeViewportStore(overrides?: { 'design-main'?: Partial<ViewportState>; 'def-preview'?: Partial<ViewportState>; splitRatio?: number }) {
  const initialViewports: Record<string, ViewportState> = {
    'design-main': {
      id: 'design-main',
      type: 'design',
      viewId: null,
      defPath: null,
      active: true,
      forceExpanded: false,
      camera: { ...DEFAULT_TEST_CAMERA },
      ...(overrides?.['design-main'] ?? {}),
    },
    'def-preview': {
      id: 'def-preview',
      type: 'def-preview',
      viewId: null,
      defPath: null,
      active: false,
      forceExpanded: false,
      camera: { ...DEFAULT_TEST_CAMERA },
      ...(overrides?.['def-preview'] ?? {}),
    },
  };
  const real = createViewportStore(initialViewports);
  if (overrides?.splitRatio !== undefined) real.setSplitRatio(overrides.splitRatio);
  // Mock wraps the real store so drift between test double and production is impossible by construction.
  // Arrow wrappers make the delegation explicit and guard against future this-dependent refactors.
  return {
    state: real.state,
    getViewport: vi.fn((...a: Parameters<typeof real.getViewport>) => real.getViewport(...a)),
    setActiveViewport: vi.fn((...a: Parameters<typeof real.setActiveViewport>) => real.setActiveViewport(...a)),
    assignView: vi.fn((...a: Parameters<typeof real.assignView>) => real.assignView(...a)),
    updateCamera: vi.fn((...a: Parameters<typeof real.updateCamera>) => real.updateCamera(...a)),
    setDefPath: vi.fn((...a: Parameters<typeof real.setDefPath>) => real.setDefPath(...a)),
    setForceExpanded: vi.fn((...a: Parameters<typeof real.setForceExpanded>) => real.setForceExpanded(...a)),
    setSplitRatio: vi.fn((...a: Parameters<typeof real.setSplitRatio>) => real.setSplitRatio(...a)),
  };
}

// Lazy import so vi.mock hoisting is already in place
async function importDualViewport() {
  return import('../../viewport/DualViewport');
}

afterEach(() => {
  cleanup();
  // Clear captured props between tests
  for (const key of Object.keys(capturedViewportPropsByid)) {
    delete capturedViewportPropsByid[key];
  }
  for (const key of Object.keys(capturedInnerFnsByViewportId)) {
    delete capturedInnerFnsByViewportId[key];
  }
  for (const key of Object.keys(capturedSplitterPropsByTestId)) {
    delete capturedSplitterPropsByTestId[key];
  }
  vi.clearAllMocks();
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe('DualViewport', () => {
  it('(a) both viewports active: both render, splitter present, no strips', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore(['mesh/A']);
    const defPreviewStore = makeDefPreviewStore(['mesh/B'], 'BoltFlange');
    const viewportStore = makeViewportStore();
    const onForceExpand = vi.fn();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => true}
        designViewportActive={() => true}
        defName={() => 'BoltFlange'}
        onForceExpand={onForceExpand}
      />
    ));

    // Both viewport mocks should be rendered
    expect(screen.getByTestId('viewport-design-main')).toBeTruthy();
    expect(screen.getByTestId('viewport-def-preview')).toBeTruthy();

    // Splitter between them
    expect(screen.getByTestId('splitter-dual')).toBeTruthy();

    // No minimized strips
    expect(screen.queryByTestId('strip-def-preview')).toBeNull();
    expect(screen.queryByTestId('strip-design')).toBeNull();
  });

  it('(b) only design active: def-preview strip shows, design viewport renders', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore(['mesh/A']);
    const defPreviewStore = makeDefPreviewStore([], null);
    const viewportStore = makeViewportStore();
    const onForceExpand = vi.fn();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => false}
        designViewportActive={() => true}
        defName={() => null}
        onForceExpand={onForceExpand}
      />
    ));

    // Design viewport rendered
    expect(screen.getByTestId('viewport-design-main')).toBeTruthy();
    // Def-preview strip shown (minimized)
    expect(screen.getByTestId('strip-def-preview')).toBeTruthy();
    expect(screen.getByTestId('strip-def-preview').textContent).toContain('Preview');
    // Design strip absent
    expect(screen.queryByTestId('strip-design')).toBeNull();
    // Def-preview viewport not mounted
    expect(screen.queryByTestId('viewport-def-preview')).toBeNull();
  });

  it('(b2) only design active with defName set: strip label includes defName', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore(['mesh/A']);
    const defPreviewStore = makeDefPreviewStore([], 'BoltFlange');
    const viewportStore = makeViewportStore();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => false}
        designViewportActive={() => true}
        defName={() => 'BoltFlange'}
        onForceExpand={vi.fn()}
      />
    ));

    const strip = screen.getByTestId('strip-def-preview');
    expect(strip.textContent).toContain('BoltFlange');
  });

  it('(c) only def-preview active: design strip shows, def-preview viewport renders', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore([]);
    const defPreviewStore = makeDefPreviewStore(['mesh/B'], 'BoltFlange');
    const viewportStore = makeViewportStore();
    const onForceExpand = vi.fn();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => true}
        designViewportActive={() => false}
        defName={() => 'BoltFlange'}
        onForceExpand={onForceExpand}
      />
    ));

    // Def-preview viewport rendered
    expect(screen.getByTestId('viewport-def-preview')).toBeTruthy();
    // Design strip shown
    expect(screen.getByTestId('strip-design')).toBeTruthy();
    expect(screen.getByTestId('strip-design').textContent).toContain('Design');
    // Def-preview strip absent
    expect(screen.queryByTestId('strip-def-preview')).toBeNull();
    // Design viewport not mounted
    expect(screen.queryByTestId('viewport-design-main')).toBeNull();
  });

  it('(d) neither active: placeholder renders', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore([]);
    const defPreviewStore = makeDefPreviewStore([], null);
    const viewportStore = makeViewportStore();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => false}
        designViewportActive={() => false}
        defName={() => null}
        onForceExpand={vi.fn()}
      />
    ));

    expect(screen.getByTestId('dual-viewport-empty')).toBeTruthy();
    expect(screen.queryByTestId('viewport-design-main')).toBeNull();
    expect(screen.queryByTestId('viewport-def-preview')).toBeNull();
    expect(screen.queryByTestId('strip-def-preview')).toBeNull();
    expect(screen.queryByTestId('strip-design')).toBeNull();
  });

  it('(e) clicking strip-def-preview calls onForceExpand("def-preview")', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore(['mesh/A']);
    const defPreviewStore = makeDefPreviewStore([], null);
    const viewportStore = makeViewportStore();
    const onForceExpand = vi.fn();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => false}
        designViewportActive={() => true}
        defName={() => null}
        onForceExpand={onForceExpand}
      />
    ));

    fireEvent.click(screen.getByTestId('strip-def-preview'));
    expect(onForceExpand).toHaveBeenCalledWith('def-preview');
  });

  it('(e2) clicking strip-design calls onForceExpand("design-main")', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore([]);
    const defPreviewStore = makeDefPreviewStore(['mesh/B'], 'BoltFlange');
    const viewportStore = makeViewportStore();
    const onForceExpand = vi.fn();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => true}
        designViewportActive={() => false}
        defName={() => 'BoltFlange'}
        onForceExpand={onForceExpand}
      />
    ));

    fireEvent.click(screen.getByTestId('strip-design'));
    expect(onForceExpand).toHaveBeenCalledWith('design-main');
  });

  it('(f) def-preview Viewport receives meshes from defPreviewStore', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore(['design/A']);
    const defPreviewStore = makeDefPreviewStore(['preview/B'], 'BoltFlange');
    const viewportStore = makeViewportStore();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => true}
        designViewportActive={() => true}
        defName={() => 'BoltFlange'}
        onForceExpand={vi.fn()}
      />
    ));

    // design-main gets engineStore meshes
    expect(capturedViewportPropsByid['design-main'].meshes).toBe(engineStore.state.meshes);
    // def-preview gets defPreviewStore meshes
    expect(capturedViewportPropsByid['def-preview'].meshes).toBe(defPreviewStore.state.meshes);
  });

  it('(f2) design Viewport receives meshes from engineStore', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore(['design/A', 'design/B']);
    const defPreviewStore = makeDefPreviewStore(['preview/C'], 'Widget');
    const viewportStore = makeViewportStore();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => true}
        designViewportActive={() => true}
        defName={() => 'Widget'}
        onForceExpand={vi.fn()}
      />
    ));

    expect(Object.keys(capturedViewportPropsByid['design-main'].meshes)).toContain('design/A');
    expect(Object.keys(capturedViewportPropsByid['def-preview'].meshes)).toContain('preview/C');
    // Design viewport does NOT get preview meshes
    expect(Object.keys(capturedViewportPropsByid['design-main'].meshes)).not.toContain('preview/C');
  });

  // ── Ref forwarding tests ─────────────────────────────────────────────────────

  describe('ref forwarding', () => {
    it('(g) parent fitToViewRef and flyToEntityRef spies are called once at setup even when no Viewport is mounted', async () => {
      const { DualViewport } = await importDualViewport();
      const engineStore = makeEngineStore([]);
      const defPreviewStore = makeDefPreviewStore([], null);
      const viewportStore = makeViewportStore();
      const fitToViewRefSpy = vi.fn();
      const flyToEntityRefSpy = vi.fn();

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => false}
          designViewportActive={() => false}
          defName={() => null}
          onForceExpand={vi.fn()}
          fitToViewRef={fitToViewRefSpy}
          flyToEntityRef={flyToEntityRefSpy}
        />
      ));

      // Both spies should have been called once at DualViewport setup with a function proxy
      expect(fitToViewRefSpy).toHaveBeenCalledTimes(1);
      expect(typeof fitToViewRefSpy.mock.calls[0][0]).toBe('function');
      expect(flyToEntityRefSpy).toHaveBeenCalledTimes(1);
      expect(typeof flyToEntityRefSpy.mock.calls[0][0]).toBe('function');
    });

    it('(h) proxy is a safe no-op when no inner Viewport is mounted', async () => {
      const { DualViewport } = await importDualViewport();
      const engineStore = makeEngineStore([]);
      const defPreviewStore = makeDefPreviewStore([], null);
      const viewportStore = makeViewportStore();
      const fitToViewRefSpy = vi.fn();
      const flyToEntityRefSpy = vi.fn();

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => false}
          designViewportActive={() => false}
          defName={() => null}
          onForceExpand={vi.fn()}
          fitToViewRef={fitToViewRefSpy}
          flyToEntityRef={flyToEntityRefSpy}
        />
      ));

      // Invoke the proxy — should not throw even without an inner Viewport
      const fitProxy = fitToViewRefSpy.mock.calls[0]?.[0] as (() => void) | undefined;
      const flyProxy = flyToEntityRefSpy.mock.calls[0]?.[0] as ((p: string) => void) | undefined;
      expect(() => fitProxy?.()).not.toThrow();
      expect(() => flyProxy?.('some/entity')).not.toThrow();
    });

    it('(i) after design Viewport mounts, proxy delegates to the inner fn', async () => {
      const { DualViewport } = await importDualViewport();
      const [designActive, setDesignActive] = createSignal(false);
      const engineStore = makeEngineStore([]);
      const defPreviewStore = makeDefPreviewStore([], null);
      const viewportStore = makeViewportStore();
      const fitToViewRefSpy = vi.fn();

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => false}
          designViewportActive={designActive}
          defName={() => null}
          onForceExpand={vi.fn()}
          fitToViewRef={fitToViewRefSpy}
        />
      ));

      // Spy was called once at setup with the proxy
      expect(fitToViewRefSpy).toHaveBeenCalledTimes(1);
      const parentProxy = fitToViewRefSpy.mock.calls[0][0] as () => void;

      // Flip design active → inner Viewport mounts → capture-callback called
      setDesignActive(true);

      // SolidJS reactivity is synchronous; inner Viewport mock is now mounted
      const innerFitSpy = capturedInnerFnsByViewportId['design-main']?.fitToView;
      expect(innerFitSpy).toBeDefined();

      // Invoking parent proxy now delegates to the inner fn
      parentProxy();
      expect(innerFitSpy).toHaveBeenCalledTimes(1);
    });

    it('(k) dragging splitter-dual calls viewportStore.setSplitRatio with current + delta/containerHeight', async () => {
      const { DualViewport } = await importDualViewport();
      const engineStore = makeEngineStore(['mesh/A']);
      const defPreviewStore = makeDefPreviewStore(['mesh/B'], 'BoltFlange');
      const viewportStore = makeViewportStore();

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => true}
          designViewportActive={() => true}
          defName={() => 'BoltFlange'}
          onForceExpand={vi.fn()}
        />
      ));

      // Stub the root container clientHeight to 400
      const container = screen.getByTestId('dual-viewport');
      Object.defineProperty(container, 'clientHeight', { configurable: true, value: 400 });

      // Invoke the dual splitter's onResize callback with delta=80
      capturedSplitterPropsByTestId['splitter-dual'].onResize(80);

      // Expected: 0.5 + 80/400 = 0.7
      expect(viewportStore.setSplitRatio).toHaveBeenCalledOnce();
      expect(viewportStore.setSplitRatio).toHaveBeenCalledWith(0.7);
    });

    it('(k2) no-op when container height is 0 — store not called', async () => {
      const { DualViewport } = await importDualViewport();
      const engineStore = makeEngineStore(['mesh/A']);
      const defPreviewStore = makeDefPreviewStore(['mesh/B'], 'BoltFlange');
      const viewportStore = makeViewportStore();

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => true}
          designViewportActive={() => true}
          defName={() => 'BoltFlange'}
          onForceExpand={vi.fn()}
        />
      ));

      // Explicitly stub clientHeight to 0 so the guard branch is exercised
      // regardless of any jsdom default behaviour.
      const container = screen.getByTestId('dual-viewport');
      Object.defineProperty(container, 'clientHeight', { configurable: true, value: 0 });
      capturedSplitterPropsByTestId['splitter-dual'].onResize(80);

      expect(viewportStore.setSplitRatio).not.toHaveBeenCalled();
    });

    it('(k3) keyboard arrow delta: onResize(10) with height=400 calls setSplitRatio(0.525)', async () => {
      const { DualViewport } = await importDualViewport();
      const engineStore = makeEngineStore(['mesh/A']);
      const defPreviewStore = makeDefPreviewStore(['mesh/B'], 'BoltFlange');
      const viewportStore = makeViewportStore();

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => true}
          designViewportActive={() => true}
          defName={() => 'BoltFlange'}
          onForceExpand={vi.fn()}
        />
      ));

      const container = screen.getByTestId('dual-viewport');
      Object.defineProperty(container, 'clientHeight', { configurable: true, value: 400 });

      // Arrow key sends delta=10 (Splitter.tsx keyboard path)
      capturedSplitterPropsByTestId['splitter-dual'].onResize(10);

      // 0.5 + 10/400 = 0.525
      expect(viewportStore.setSplitRatio).toHaveBeenCalledOnce();
      expect(viewportStore.setSplitRatio).toHaveBeenCalledWith(0.525);
    });

    it('(k4) sequential drags accumulate: second drag reads updated splitRatio', async () => {
      const { DualViewport } = await importDualViewport();
      const engineStore = makeEngineStore(['mesh/A']);
      const defPreviewStore = makeDefPreviewStore(['mesh/B'], 'BoltFlange');
      // makeViewportStore's setSplitRatio spy writes to state so the second drag
      // reads the post-first-drag ratio.
      const viewportStore = makeViewportStore();

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => true}
          designViewportActive={() => true}
          defName={() => 'BoltFlange'}
          onForceExpand={vi.fn()}
        />
      ));

      const container = screen.getByTestId('dual-viewport');
      Object.defineProperty(container, 'clientHeight', { configurable: true, value: 400 });

      // First drag: 0.5 + 80/400 = 0.7
      capturedSplitterPropsByTestId['splitter-dual'].onResize(80);
      // Second drag: 0.7 + 40/400 = 0.8
      capturedSplitterPropsByTestId['splitter-dual'].onResize(40);

      expect(viewportStore.setSplitRatio).toHaveBeenCalledTimes(2);
      expect(viewportStore.setSplitRatio).toHaveBeenNthCalledWith(1, 0.7);
      // 0.7 + 40/400 = 0.7999…99 in IEEE 754; use closeTo for float safety
      expect(viewportStore.setSplitRatio).toHaveBeenNthCalledWith(2, expect.closeTo(0.8, 10));
    });

    it('(k5) real createViewportStore setSplitRatio rejects NaN/Infinity — returns false, preserves state', () => {
      // Uses the REAL store (no render needed) to pin the Number.isFinite guard at
      // viewportStore.ts:188. If that guard is removed, this test catches it.
      // (The mock's guard fidelity is verified transitively via this test's behavior.)
      const store = createViewportStore();
      expect(store.state.splitRatio).toBe(0.5);

      // NaN: must return false and leave state unchanged
      const nanResult = store.setSplitRatio(NaN);
      expect(nanResult).toBe(false);
      expect(store.state.splitRatio).toBe(0.5);

      // Infinity: must return false and leave state unchanged
      const infResult = store.setSplitRatio(Infinity);
      expect(infResult).toBe(false);
      expect(store.state.splitRatio).toBe(0.5);

      // -Infinity: must return false and leave state unchanged
      const negInfResult = store.setSplitRatio(-Infinity);
      expect(negInfResult).toBe(false);
      expect(store.state.splitRatio).toBe(0.5);
    });

    it('(k6) end-to-end clamp upper bound: real store clamps raw ratio 3.0 → 0.9', async () => {
      // Uses the real createViewportStore to lock in the integration contract between
      // handleDualResize's raw arithmetic and the real store's clamp semantics.
      const { DualViewport } = await importDualViewport();
      const engineStore = makeEngineStore(['mesh/A']);
      const defPreviewStore = makeDefPreviewStore(['mesh/B'], 'BoltFlange');
      // Real store: initial splitRatio = 0.5, clamp [0.1, 0.9]
      const viewportStore = createViewportStore();

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => true}
          designViewportActive={() => true}
          defName={() => 'BoltFlange'}
          onForceExpand={vi.fn()}
        />
      ));

      // containerHeight = 400; delta = 1000 → raw = 0.5 + 1000/400 = 3.0 → clamped to 0.9
      const container = screen.getByTestId('dual-viewport');
      Object.defineProperty(container, 'clientHeight', { configurable: true, value: 400 });
      capturedSplitterPropsByTestId['splitter-dual'].onResize(1000);

      expect(viewportStore.state.splitRatio).toBe(0.9);
    });

    it('(k7) end-to-end clamp lower bound: real store clamps raw ratio -2.0 → 0.1', async () => {
      const { DualViewport } = await importDualViewport();
      const engineStore = makeEngineStore(['mesh/A']);
      const defPreviewStore = makeDefPreviewStore(['mesh/B'], 'BoltFlange');
      const viewportStore = createViewportStore();

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => true}
          designViewportActive={() => true}
          defName={() => 'BoltFlange'}
          onForceExpand={vi.fn()}
        />
      ));

      // containerHeight = 400; delta = -1000 → raw = 0.5 + (-1000)/400 = -2.0 → clamped to 0.1
      const container = screen.getByTestId('dual-viewport');
      Object.defineProperty(container, 'clientHeight', { configurable: true, value: 400 });
      capturedSplitterPropsByTestId['splitter-dual'].onResize(-1000);

      expect(viewportStore.state.splitRatio).toBe(0.1);
    });

    it('(l) both viewports active: wrapper flex styles reflect splitRatio', async () => {
      const { DualViewport } = await importDualViewport();
      const engineStore = makeEngineStore(['mesh/A']);
      const defPreviewStore = makeDefPreviewStore(['mesh/B'], 'BoltFlange');
      // splitRatio of 0.3 → def-preview gets 0.3, design gets 0.7
      const viewportStore = makeViewportStore({ splitRatio: 0.3 });

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => true}
          designViewportActive={() => true}
          defName={() => 'BoltFlange'}
          onForceExpand={vi.fn()}
        />
      ));

      const defWrapper = screen.getByTestId('dual-viewport-def-preview-wrapper');
      const designWrapper = screen.getByTestId('dual-viewport-design-wrapper');
      // Assert non-empty style attribute (dual-mode path applies inline styles)
      expect(defWrapper.getAttribute('style')).toBeTruthy();
      expect(designWrapper.getAttribute('style')).toBeTruthy();
      // Assert flexGrow longhands directly — jsdom round-trips these reliably,
      // unlike the flex shorthand which may normalize '0%' → '0px' or reorder tokens.
      expect(defWrapper.style.flexGrow).toBe('0.3');
      expect(designWrapper.style.flexGrow).toBe('0.7');
    });

    it('(l2) single-viewport mode: design wrapper has no inline flex style', async () => {
      const { DualViewport } = await importDualViewport();
      const engineStore = makeEngineStore(['mesh/A']);
      const defPreviewStore = makeDefPreviewStore([], null);
      const viewportStore = makeViewportStore({ splitRatio: 0.3 });

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => false}
          designViewportActive={() => true}
          defName={() => null}
          onForceExpand={vi.fn()}
        />
      ));

      const designWrapper = screen.getByTestId('dual-viewport-design-wrapper');
      // No inline flex override — CSS flex: 1 fallback applies
      expect(designWrapper.style.flex).toBe('');
    });

    it('(j) after design Viewport unmounts, proxy becomes a no-op (inner fn not called again)', async () => {
      const { DualViewport } = await importDualViewport();
      const [designActive, setDesignActive] = createSignal(true);
      const engineStore = makeEngineStore(['mesh/A']);
      const defPreviewStore = makeDefPreviewStore([], null);
      const viewportStore = makeViewportStore();
      const fitToViewRefSpy = vi.fn();

      render(() => (
        <DualViewport
          engineStore={engineStore}
          defPreviewStore={defPreviewStore}
          viewportStore={viewportStore}
          defPreviewActive={() => false}
          designViewportActive={designActive}
          defName={() => null}
          onForceExpand={vi.fn()}
          fitToViewRef={fitToViewRefSpy}
        />
      ));

      // Inner Viewport is mounted; capture its inner fn
      const innerFitSpy = capturedInnerFnsByViewportId['design-main']?.fitToView;
      expect(innerFitSpy).toBeDefined();

      // Parent proxy should delegate to inner fn
      const parentProxy = fitToViewRefSpy.mock.calls[0][0] as () => void;
      parentProxy();
      expect(innerFitSpy).toHaveBeenCalledTimes(1);

      // Unmount design Viewport (toggle to inactive)
      setDesignActive(false);

      // After unmount, proxy must be a no-op (inner capture cleared)
      parentProxy();
      // Inner fn should NOT have been called again
      expect(innerFitSpy).toHaveBeenCalledTimes(1);
    });
  });

  // ── Mesh-gate tests (task #4213) ─────────────────────────────────────────

  it('(n-A) NO-MESH -> STRIP: defPreviewActive true but empty meshes does not mount blank viewport', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore(['design/A']);
    // Empty def-preview store — meshes: {}
    const defPreviewStore = makeDefPreviewStore([], 'BoltFlange');
    const viewportStore = makeViewportStore();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => true}
        designViewportActive={() => true}
        defName={() => 'BoltFlange'}
        onForceExpand={vi.fn()}
      />
    ));

    // def-preview viewport must NOT mount when meshes is empty (avoids blank grid)
    expect(screen.queryByTestId('viewport-def-preview')).toBeNull();
    // Minimized strip shown instead
    expect(screen.getByTestId('strip-def-preview')).toBeTruthy();
    // Design pane still renders normally
    expect(screen.getByTestId('viewport-design-main')).toBeTruthy();
    // Splitter absent — only one effective pane
    expect(screen.queryByTestId('splitter-dual')).toBeNull();
  });

  it('(n-B) REACTIVE TRANSITION: def-preview pane expands once meshes arrive', async () => {
    const { DualViewport } = await importDualViewport();

    // makeDefPreviewStore exposes setState so we can reactively add meshes
    // without hand-rolling a parallel store double.
    const defPreviewStore = makeDefPreviewStore([], 'BoltFlange');
    const engineStore = makeEngineStore(['design/A']);
    const viewportStore = makeViewportStore();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => true}
        designViewportActive={() => true}
        defName={() => 'BoltFlange'}
        onForceExpand={vi.fn()}
      />
    ));

    // Phase 1: meshes empty → strip shown, viewport NOT mounted
    expect(screen.getByTestId('strip-def-preview')).toBeTruthy();
    expect(screen.queryByTestId('viewport-def-preview')).toBeNull();

    // Simulate applyPreview resolving: add a mesh
    defPreviewStore.setState('meshes', { 'preview/B': makeMesh('preview/B') });

    // Phase 2: mesh present → viewport mounts, strip gone, splitter appears
    expect(screen.getByTestId('viewport-def-preview')).toBeTruthy();
    expect(screen.queryByTestId('strip-def-preview')).toBeNull();
    expect(screen.getByTestId('splitter-dual')).toBeTruthy();
  });

  it('(n-C) FORCEEXPANDED GUARD: forceExpanded unconditionally mounts viewport even with empty meshes', async () => {
    const { DualViewport } = await importDualViewport();
    const engineStore = makeEngineStore(['design/A']);
    // Empty meshes — forceExpanded must override the mesh gate
    const defPreviewStore = makeDefPreviewStore([], 'BoltFlange');
    const viewportStore = makeViewportStore({ 'def-preview': { forceExpanded: true } });

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => false}
        designViewportActive={() => true}
        defName={() => 'BoltFlange'}
        onForceExpand={vi.fn()}
      />
    ));

    // Manual override must bypass the mesh gate
    expect(screen.getByTestId('viewport-def-preview')).toBeTruthy();
  });

  it('(n-D) REVERSE TRANSITION: clearing meshes collapses the expanded pane back to a strip', async () => {
    const { DualViewport } = await importDualViewport();
    // Start with meshes present — auto-expansion should mount the viewport
    const defPreviewStore = makeDefPreviewStore(['preview/B'], 'BoltFlange');
    const engineStore = makeEngineStore(['design/A']);
    const viewportStore = makeViewportStore();

    render(() => (
      <DualViewport
        engineStore={engineStore}
        defPreviewStore={defPreviewStore}
        viewportStore={viewportStore}
        defPreviewActive={() => true}
        designViewportActive={() => true}
        defName={() => 'BoltFlange'}
        onForceExpand={vi.fn()}
      />
    ));

    // Phase 1: meshes present → viewport mounted, strip absent
    expect(screen.getByTestId('viewport-def-preview')).toBeTruthy();
    expect(screen.queryByTestId('strip-def-preview')).toBeNull();

    // SolidJS store merges nested objects by default, so `setState('meshes', {})`
    // would keep existing keys. Use reconcile({}) to force key removal and truly
    // empty the record — mirroring what clearPreview does in the real store.
    defPreviewStore.setState('meshes', reconcile({}) as any);

    // Phase 2: meshes gone → viewport unmounts, strip reappears (no blank grid)
    expect(screen.queryByTestId('viewport-def-preview')).toBeNull();
    expect(screen.getByTestId('strip-def-preview')).toBeTruthy();
  });

  it('(m) makeViewportStore wraps the real createViewportStore — spies delegate to real impl', () => {
    // Characterisation test: verifies that every mock method delegates to the real store
    // rather than being a no-op stub. Covers all seven methods so any re-hand-rolled
    // bare vi.fn() will be caught here.
    const store = makeViewportStore();

    // setForceExpanded must mutate state — not be a no-op stub
    expect(store.state.viewports['design-main'].forceExpanded).toBe(false);
    store.setForceExpanded('design-main', true);
    expect(store.state.viewports['design-main'].forceExpanded).toBe(true);

    // assignView must update viewId
    expect(store.state.viewports['design-main'].viewId).toBeNull();
    store.assignView('design-main', 'v1');
    expect(store.state.viewports['design-main'].viewId).toBe('v1');

    // setActiveViewport must flip active flags across all viewports
    expect(store.state.viewports['design-main'].active).toBe(true);
    expect(store.state.viewports['def-preview'].active).toBe(false);
    store.setActiveViewport('def-preview');
    expect(store.state.viewports['def-preview'].active).toBe(true);
    expect(store.state.viewports['design-main'].active).toBe(false);

    // updateCamera must persist camera state
    store.updateCamera('design-main', { ...DEFAULT_TEST_CAMERA, zoom: 2 });
    expect(store.state.viewports['design-main'].camera.zoom).toBe(2);

    // setDefPath must update defPath
    expect(store.state.viewports['design-main'].defPath).toBeNull();
    store.setDefPath('design-main', 'some/def.ts');
    expect(store.state.viewports['design-main'].defPath).toBe('some/def.ts');

    // getViewport must return the live viewport state (reflects prior mutations)
    const vp = store.getViewport('design-main');
    expect(vp?.defPath).toBe('some/def.ts');

    // setSplitRatio must use real clamp semantics
    store.setSplitRatio(1.5);
    expect(store.state.splitRatio).toBe(0.9);
    store.setSplitRatio(-5);
    expect(store.state.splitRatio).toBe(0.1);
    // NaN is rejected — state stays at 0.1 from prior call
    store.setSplitRatio(NaN);
    expect(store.state.splitRatio).toBe(0.1);
  });
});
