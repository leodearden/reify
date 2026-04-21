import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { createStore } from 'solid-js/store';
import type { MeshData } from '../../types';

// ── Mock Viewport ────────────────────────────────────────────────────────────
// Capture rendered instances by viewportId so we can assert mesh sources.
const capturedViewportPropsByid: Record<string, any> = {};

vi.mock('../../viewport/Viewport', () => ({
  Viewport: (props: any) => {
    capturedViewportPropsByid[props.viewportId] = props;
    const el = document.createElement('div');
    el.setAttribute('data-testid', `viewport-${props.viewportId}`);
    el.textContent = `Viewport:${props.viewportId}`;
    return el;
  },
}));

// ── Mock Splitter ────────────────────────────────────────────────────────────
vi.mock('../../components/Splitter', () => ({
  Splitter: (props: any) => {
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
    ghost: false,
  };
}

function makeEngineStore(meshPaths: string[] = []) {
  const meshes: Record<string, MeshData> = {};
  for (const p of meshPaths) meshes[p] = makeMesh(p);
  const [state] = createStore({ meshes });
  return { state };
}

function makeDefPreviewStore(meshPaths: string[] = [], defName: string | null = null) {
  const meshes: Record<string, MeshData> = {};
  for (const p of meshPaths) meshes[p] = makeMesh(p);
  const [state] = createStore({ defName, meshes, isLoading: false, error: null });
  return { state };
}

function makeViewportStore(overrides?: { 'design-main'?: Partial<any>; 'def-preview'?: Partial<any> }) {
  const viewports = {
    'design-main': {
      id: 'design-main',
      forceExpanded: false,
      ...(overrides?.['design-main'] ?? {}),
    },
    'def-preview': {
      id: 'def-preview',
      forceExpanded: false,
      ...(overrides?.['def-preview'] ?? {}),
    },
  };
  const [state] = createStore({ viewports });
  return { state };
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
});
