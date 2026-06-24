import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, cleanup } from '@solidjs/testing-library';
import type { MeshData } from '../../types';
import { createViewportStore } from '../../stores/viewportStore';
import type { ViewportState } from '../../stores/viewportStore';

// Local PaneConfig interface — mirrors the type MultiViewport.tsx will export (step-2).
// Defined here so the scaffold compiles before MultiViewport.tsx exists.
interface PaneConfig {
  viewportId: string;
  meshes: Record<string, MeshData>;
  onSelect?: (path: string | null, modifiers?: { ctrl: boolean; shift: boolean }) => void;
  onHover?: (path: string | null) => void;
  hoveredEntity?: string | null;
  selectedEntity?: string | null;
  selectedEntities?: readonly string[];
  evalStatus?: any;
  entityVisibility?: Record<string, any>;
  tensegrityWires?: any[];
  tensegritySurfaces?: any[];
  fitToViewRef?: (fn: () => void) => void;
  flyToEntityRef?: (fn: (entityPath: string) => void) => void;
}

// ── Mock Viewport ────────────────────────────────────────────────────────────
// Capture rendered instances by viewportId so tests can assert prop threading.
const capturedViewportPropsByid: Record<string, any> = {};
// Capture inner ref fns by viewportId for ref-forwarding tests.
const capturedInnerFnsByViewportId: Record<string, { fitToView: ReturnType<typeof vi.fn>; flyToEntity: ReturnType<typeof vi.fn> }> = {};

vi.mock('../../viewport/Viewport', () => ({
  Viewport: (props: any) => {
    capturedViewportPropsByid[props.viewportId] = props;
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

function makeViewportStore(initialViewports?: Record<string, ViewportState>) {
  const real = createViewportStore(initialViewports);
  // Mock wraps the real store so drift between test double and production is impossible by construction.
  return {
    state: real.state,
    getViewport: vi.fn((...a: Parameters<typeof real.getViewport>) => real.getViewport(...a)),
    setActiveViewport: vi.fn((...a: Parameters<typeof real.setActiveViewport>) => real.setActiveViewport(...a)),
    assignView: vi.fn((...a: Parameters<typeof real.assignView>) => real.assignView(...a)),
    updateCamera: vi.fn((...a: Parameters<typeof real.updateCamera>) => real.updateCamera(...a)),
    setDefPath: vi.fn((...a: Parameters<typeof real.setDefPath>) => real.setDefPath(...a)),
    setForceExpanded: vi.fn((...a: Parameters<typeof real.setForceExpanded>) => real.setForceExpanded(...a)),
    setSplitRatio: vi.fn((...a: Parameters<typeof real.setSplitRatio>) => real.setSplitRatio(...a)),
    addPane: vi.fn((...a: Parameters<typeof real.addPane>) => real.addPane(...a)),
    removePane: vi.fn((...a: Parameters<typeof real.removePane>) => real.removePane(...a)),
    setSizeWeight: vi.fn((...a: Parameters<typeof real.setSizeWeight>) => real.setSizeWeight(...a)),
  };
}

function makePaneConfig(
  viewportId: string,
  meshPaths: string[] = [],
  overrides?: Partial<Omit<PaneConfig, 'viewportId' | 'meshes'>>,
): PaneConfig {
  const meshes: Record<string, MeshData> = {};
  for (const p of meshPaths) meshes[p] = makeMesh(p);
  return { viewportId, meshes, ...overrides };
}

afterEach(() => {
  cleanup();
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

// Lazy import so vi.mock hoisting is already in place when the module loads.
async function importMultiViewport() {
  return import('../../viewport/MultiViewport');
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('MultiViewport', () => {
  it('(empty) empty panes array renders placeholder, no Viewport', async () => {
    const { MultiViewport } = await importMultiViewport();
    const viewportStore = makeViewportStore();

    render(() => <MultiViewport panes={[]} viewportStore={viewportStore} />);

    expect(screen.getByTestId('multi-viewport-empty')).toBeTruthy();
    expect(screen.queryByTestId('viewport-design-main')).toBeNull();
  });

  it('(render-n) renders one Viewport per pane config entry', async () => {
    const { MultiViewport } = await importMultiViewport();
    const viewportStore = makeViewportStore();
    const panes = [
      makePaneConfig('design-main'),
      makePaneConfig('pane-1'),
      makePaneConfig('pane-2'),
    ];

    render(() => <MultiViewport panes={panes} viewportStore={viewportStore} />);

    // Root container
    expect(screen.getByTestId('multi-viewport')).toBeTruthy();

    // Three Viewport mocks — one per pane
    expect(screen.getByTestId('viewport-design-main')).toBeTruthy();
    expect(screen.getByTestId('viewport-pane-1')).toBeTruthy();
    expect(screen.getByTestId('viewport-pane-2')).toBeTruthy();

    // Each wrapped in a per-pane element
    expect(screen.getByTestId('multi-viewport-pane-design-main')).toBeTruthy();
    expect(screen.getByTestId('multi-viewport-pane-pane-1')).toBeTruthy();
    expect(screen.getByTestId('multi-viewport-pane-pane-2')).toBeTruthy();
  });

  it('(grid-cols) column count follows ceil(sqrt(N)) heuristic', async () => {
    const { MultiViewport } = await importMultiViewport();

    // N → expected_cols: 1→1, 2→2, 4→2, 5→3
    const cases: Array<[number, number]> = [
      [1, 1],
      [2, 2],
      [4, 2],
      [5, 3],
    ];

    for (const [n, expectedCols] of cases) {
      const viewportStore = makeViewportStore();
      const panes = Array.from({ length: n }, (_, i) =>
        makePaneConfig(i === 0 ? 'design-main' : `pane-${i}`),
      );

      render(() => <MultiViewport panes={panes} viewportStore={viewportStore} />);

      const root = screen.getByTestId('multi-viewport') as HTMLElement;
      const gridCols = root.style.gridTemplateColumns;
      const tracks = gridCols.trim().split(/\s+/).filter(Boolean);
      expect(tracks, `N=${n} should have ${expectedCols} column tracks`).toHaveLength(expectedCols);

      cleanup();
      for (const key of Object.keys(capturedViewportPropsByid)) delete capturedViewportPropsByid[key];
      for (const key of Object.keys(capturedInnerFnsByViewportId)) delete capturedInnerFnsByViewportId[key];
      for (const key of Object.keys(capturedSplitterPropsByTestId)) delete capturedSplitterPropsByTestId[key];
      vi.clearAllMocks();
    }
  });

  it('(passthrough) each Viewport receives its pane meshes, passthrough props, refs, and the shared store', async () => {
    const { MultiViewport } = await importMultiViewport();
    const viewportStore = makeViewportStore();

    // Build strongly-typed passthrough values so reference equality is unambiguous.
    const onSelect = vi.fn();
    const onHover = vi.fn();
    const fitToViewRef = vi.fn();
    const flyToEntityRef = vi.fn();
    const evalStatus = { tag: 'ok' };
    const entityVisibility: Record<string, any> = { 'design/A': true };
    const tensegrityWires = [{ p: 0, q: 1 }];
    const tensegritySurfaces = [{ tri: [0, 1, 2] }];

    const panes = [
      makePaneConfig('design-main', ['design/A'], {
        onSelect,
        onHover,
        hoveredEntity: 'design/A',
        selectedEntity: 'design/A',
        selectedEntities: ['design/A'],
        evalStatus,
        entityVisibility,
        tensegrityWires,
        tensegritySurfaces,
        fitToViewRef,
        flyToEntityRef,
      }),
      makePaneConfig('pane-1'),
    ];

    render(() => <MultiViewport panes={panes} viewportStore={viewportStore} />);

    const captured = capturedViewportPropsByid['design-main'];
    expect(captured, 'design-main Viewport should have been captured').toBeDefined();

    // Meshes for this pane
    expect(Object.keys(captured.meshes)).toEqual(['design/A']);

    // Shared store is the same reference
    expect(captured.viewportStore).toBe(viewportStore);

    // All passthrough props are the exact same references as those passed into the pane config.
    expect(captured.onSelect).toBe(onSelect);
    expect(captured.onHover).toBe(onHover);
    expect(captured.hoveredEntity).toBe('design/A');
    expect(captured.selectedEntity).toBe('design/A');
    expect(captured.selectedEntities).toEqual(['design/A']);
    expect(captured.evalStatus).toBe(evalStatus);
    expect(captured.entityVisibility).toBe(entityVisibility);
    expect(captured.tensegrityWires).toBe(tensegrityWires);
    expect(captured.tensegritySurfaces).toBe(tensegritySurfaces);
    expect(captured.fitToViewRef).toBe(fitToViewRef);
    expect(captured.flyToEntityRef).toBe(flyToEntityRef);
  });

  it('(size-fr) column tracks are weighted by per-pane sizeWeight from the store', async () => {
    const { MultiViewport } = await importMultiViewport();

    const DEFAULT_CAMERA_STATE = {
      position: [5, 5, 5] as [number, number, number],
      target: [0, 0, 0] as [number, number, number],
      up: [0, 0, 1] as [number, number, number],
      zoom: 1,
    };

    // Store where design-main has sizeWeight=2, pane-1 has sizeWeight=1.
    const viewportStore = makeViewportStore({
      'design-main': {
        id: 'design-main',
        type: 'design',
        viewId: null,
        defPath: null,
        active: true,
        forceExpanded: false,
        camera: { ...DEFAULT_CAMERA_STATE },
        sizeWeight: 2,
      },
      'pane-1': {
        id: 'pane-1',
        type: 'pane',
        viewId: null,
        defPath: null,
        active: false,
        forceExpanded: false,
        camera: { ...DEFAULT_CAMERA_STATE },
        sizeWeight: 1,
        paneIndex: 1,
      },
    });

    const panes = [makePaneConfig('design-main'), makePaneConfig('pane-1')];
    render(() => <MultiViewport panes={panes} viewportStore={viewportStore} />);

    const root = screen.getByTestId('multi-viewport') as HTMLElement;
    // design-main has sizeWeight=2, pane-1 has sizeWeight=1 → tracks are '2fr 1fr'.
    expect(root.style.gridTemplateColumns).toBe('2fr 1fr');

    cleanup();
    for (const key of Object.keys(capturedViewportPropsByid)) delete capturedViewportPropsByid[key];
    for (const key of Object.keys(capturedSplitterPropsByTestId)) delete capturedSplitterPropsByTestId[key];
    vi.clearAllMocks();

    // A pane whose store entry is missing/undefined sizeWeight falls back to '1fr'.
    const vs2 = makeViewportStore(); // only has 'design-main' (sizeWeight=1) and 'def-preview'
    const panes2 = [makePaneConfig('design-main'), makePaneConfig('pane-extra')];
    render(() => <MultiViewport panes={panes2} viewportStore={vs2} />);

    const root2 = screen.getByTestId('multi-viewport') as HTMLElement;
    // design-main has sizeWeight=1 (default), pane-extra is unknown → both fall back to 1fr.
    expect(root2.style.gridTemplateColumns).toBe('1fr 1fr');
  });

  it('(splitters) C-1 vertical splitters; dragging calls setSizeWeight with current+delta/width', async () => {
    const { MultiViewport } = await importMultiViewport();

    // ── (i) N=1 → no splitter ─────────────────────────────────────────────
    {
      const viewportStore = makeViewportStore();
      render(() => <MultiViewport panes={[makePaneConfig('design-main')]} viewportStore={viewportStore} />);
      expect(screen.queryByTestId('multi-viewport-splitter-0')).toBeNull();
      cleanup();
      for (const key of Object.keys(capturedSplitterPropsByTestId)) delete capturedSplitterPropsByTestId[key];
      vi.clearAllMocks();
    }

    // ── (i) N=2 → exactly one splitter-0 with orientation=vertical ────────
    {
      const viewportStore = makeViewportStore();
      const panes = [makePaneConfig('design-main'), makePaneConfig('pane-1')];
      render(() => <MultiViewport panes={panes} viewportStore={viewportStore} />);
      const splitter = screen.getByTestId('multi-viewport-splitter-0');
      expect(splitter).toBeTruthy();
      expect(splitter.getAttribute('data-orientation')).toBe('vertical');
      expect(screen.queryByTestId('multi-viewport-splitter-1')).toBeNull();
      cleanup();
      for (const key of Object.keys(capturedViewportPropsByid)) delete capturedViewportPropsByid[key];
      for (const key of Object.keys(capturedSplitterPropsByTestId)) delete capturedSplitterPropsByTestId[key];
      vi.clearAllMocks();
    }

    // ── (i) N=5 → C=ceil(sqrt(5))=3 → 2 splitters ────────────────────────
    {
      const viewportStore = makeViewportStore();
      const panes = Array.from({ length: 5 }, (_, i) =>
        makePaneConfig(i === 0 ? 'design-main' : `pane-${i}`),
      );
      render(() => <MultiViewport panes={panes} viewportStore={viewportStore} />);
      expect(screen.getByTestId('multi-viewport-splitter-0')).toBeTruthy();
      expect(screen.getByTestId('multi-viewport-splitter-1')).toBeTruthy();
      expect(screen.queryByTestId('multi-viewport-splitter-2')).toBeNull();
      cleanup();
      for (const key of Object.keys(capturedViewportPropsByid)) delete capturedViewportPropsByid[key];
      for (const key of Object.keys(capturedSplitterPropsByTestId)) delete capturedSplitterPropsByTestId[key];
      vi.clearAllMocks();
    }

    // ── (ii) resize: setSizeWeight('design-main', 1.2) for delta=80, width=400 ─
    {
      const viewportStore = makeViewportStore();
      const panes = [makePaneConfig('design-main'), makePaneConfig('pane-1')];
      render(() => <MultiViewport panes={panes} viewportStore={viewportStore} />);

      const root = screen.getByTestId('multi-viewport') as HTMLElement;
      Object.defineProperty(root, 'clientWidth', { get: () => 400, configurable: true });

      // Clear spy call records from render (getViewport may have been called).
      vi.clearAllMocks();

      capturedSplitterPropsByTestId['multi-viewport-splitter-0'].onResize(80);

      // design-main sizeWeight defaults to 1; new weight = 1 + 80/400 = 1.2
      expect(viewportStore.setSizeWeight).toHaveBeenCalledOnce();
      expect(viewportStore.setSizeWeight).toHaveBeenCalledWith('design-main', 1.2);

      cleanup();
      for (const key of Object.keys(capturedViewportPropsByid)) delete capturedViewportPropsByid[key];
      for (const key of Object.keys(capturedSplitterPropsByTestId)) delete capturedSplitterPropsByTestId[key];
      vi.clearAllMocks();
    }

    // ── (iii) clientWidth=0 → no setSizeWeight call (width guard) ─────────
    {
      const viewportStore = makeViewportStore();
      const panes = [makePaneConfig('design-main'), makePaneConfig('pane-1')];
      render(() => <MultiViewport panes={panes} viewportStore={viewportStore} />);

      const root = screen.getByTestId('multi-viewport') as HTMLElement;
      Object.defineProperty(root, 'clientWidth', { get: () => 0, configurable: true });

      vi.clearAllMocks();
      capturedSplitterPropsByTestId['multi-viewport-splitter-0'].onResize(80);

      expect(viewportStore.setSizeWeight).not.toHaveBeenCalled();
    }
  });
});
