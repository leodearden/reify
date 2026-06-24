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
});
