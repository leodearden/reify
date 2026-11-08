import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import type {
  GuiState,
  TensegrityWireData,
  TensegritySurfaceData,
  DisplayDirective,
  AppearanceDirective,
} from '../types';

vi.mock('../bridge', async () => (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock());

import {
  mockOnTensegrityWiresUpdate,
  mockOnTensegritySurfacesUpdate,
  mockOnDisplayPanesUpdate,
  mockOnDisplayAppearanceUpdate,
  primeEngineStoreBridgeMocks,
} from './engineStoreBridgeHandles';
import { createEngineStore } from '../stores/engineStore';

beforeEach(primeEngineStoreBridgeMocks);

describe('engineStore tensegrity initFromState fan-out', () => {
  // ── T0b: tensegrityWires store fan-out ───────────────────────────────────

  it('initFromState writes tensegrity_wires from GuiState into state.tensegrityWires', () => {
    // RED until EngineState.tensegrityWires is added and initFromState sets it.
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const guiState: GuiState = { fea_convergence: null,
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [
          { entity_path: 'TPrism', kind: 'strut', x1: 1.0, y1: 0.0, z1: 1.0, x2: 0.866, y2: 0.5, z2: 0.0 },
          { entity_path: 'TPrism', kind: 'cable', x1: 1.0, y1: 0.0, z1: 1.0, x2: -0.5, y2: 0.866, z2: 1.0 },
        ],
        tensegrity_surfaces: [],
        display_panes: [],
        display_appearance: [],
        fea_diagnostics: []
      };
      initFromState(guiState);
      expect((state as any).tensegrityWires).toHaveLength(2);
      expect((state as any).tensegrityWires[0].kind).toBe('strut');
      expect((state as any).tensegrityWires[0].entity_path).toBe('TPrism');
      expect((state as any).tensegrityWires[1].kind).toBe('cable');
      dispose();
    });
  });

  it('initFromState leaves tensegrityWires as [] when tensegrity_wires is absent or empty', () => {
    // RED until EngineState.tensegrityWires is initialised to [] and initFromState sets it.
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      // Initial state should be []
      expect((state as any).tensegrityWires).toEqual([]);

      const guiState: GuiState = { fea_convergence: null,
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
        tensegrity_surfaces: [],
        display_panes: [],
        display_appearance: [],
        fea_diagnostics: []
      };
      initFromState(guiState);
      expect((state as any).tensegrityWires).toEqual([]);
      dispose();
    });
  });

  // ── β: tensegritySurfaces store fan-out ──────────────────────────────────

  it('initFromState writes tensegrity_surfaces from GuiState into state.tensegritySurfaces', () => {
    // RED until EngineState.tensegritySurfaces is added and initFromState sets it.
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const guiState: GuiState = { fea_convergence: null,
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
        tensegrity_surfaces: [
          {
            entity_path: 'Patch',
            kind: 'membrane',
            i0: 0, i1: 1, i2: 2,
            x0: 0.0, y0: 0.0, z0: 0.0,
            x1: 1.0, y1: 0.0, z1: 0.0,
            x2: 0.5, y2: 0.866, z2: 0.0,
          },
          {
            entity_path: 'Patch',
            kind: 'membrane',
            i0: 1, i1: 3, i2: 2,
            x0: 1.0, y0: 0.0, z0: 0.0,
            x1: 1.5, y1: 0.866, z1: 0.0,
            x2: 0.5, y2: 0.866, z2: 0.0,
          },
        ],
        display_panes: [],
        display_appearance: [],
        fea_diagnostics: []
      };
      initFromState(guiState);
      expect((state as any).tensegritySurfaces).toHaveLength(2);
      expect((state as any).tensegritySurfaces[0].kind).toBe('membrane');
      expect((state as any).tensegritySurfaces[0].entity_path).toBe('Patch');
      expect((state as any).tensegritySurfaces[0].i0).toBe(0);
      expect((state as any).tensegritySurfaces[0].x2).toBe(0.5);
      expect((state as any).tensegritySurfaces[1].i0).toBe(1);
      dispose();
    });
  });

  it('initFromState leaves tensegritySurfaces as [] when tensegrity_surfaces is empty', () => {
    // RED until EngineState.tensegritySurfaces is initialised to [] and initFromState sets it.
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      // Initial state should be []
      expect((state as any).tensegritySurfaces).toEqual([]);

      const guiState: GuiState = { fea_convergence: null,
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
        tensegrity_surfaces: [],
        display_panes: [],
        display_appearance: [],
        fea_diagnostics: []
      };
      initFromState(guiState);
      expect((state as any).tensegritySurfaces).toEqual([]);
      dispose();
    });
  });
});

// ── #5031 L2 stopgap: whole-value reducers for the 4 live-sync list fields ──

describe('engineStore live-sync reducers (#5031 step-1)', () => {
  it('setTensegrityWires replaces state.tensegrityWires with the provided list', () => {
    createRoot((dispose) => {
      const { state, setTensegrityWires } = createEngineStore();
      expect(state.tensegrityWires).toEqual([]);

      const wire: TensegrityWireData = {
        entity_path: 'TPrism', kind: 'strut', x1: 1.0, y1: 0.0, z1: 1.0, x2: 0.866, y2: 0.5, z2: 0.0,
      };
      setTensegrityWires([wire]);
      expect(state.tensegrityWires).toEqual([wire]);
      dispose();
    });
  });

  it('setTensegritySurfaces replaces state.tensegritySurfaces with the provided list', () => {
    createRoot((dispose) => {
      const { state, setTensegritySurfaces } = createEngineStore();
      expect(state.tensegritySurfaces).toEqual([]);

      const surface: TensegritySurfaceData = {
        entity_path: 'Patch', kind: 'membrane', i0: 0, i1: 1, i2: 2,
        x0: 0.0, y0: 0.0, z0: 0.0, x1: 1.0, y1: 0.0, z1: 0.0, x2: 0.5, y2: 0.866, z2: 0.0,
      };
      setTensegritySurfaces([surface]);
      expect(state.tensegritySurfaces).toEqual([surface]);
      dispose();
    });
  });

  it('setDisplayPanes replaces state.displayPanes with the provided list', () => {
    createRoot((dispose) => {
      const { state, setDisplayPanes } = createEngineStore();
      expect(state.displayPanes).toEqual([]);

      const directive: DisplayDirective = { subject: 'S#realization[0]', pane: 1 };
      setDisplayPanes([directive]);
      expect(state.displayPanes).toEqual([directive]);
      dispose();
    });
  });

  it('setDisplayAppearance replaces state.displayAppearance with the provided list', () => {
    createRoot((dispose) => {
      const { state, setDisplayAppearance } = createEngineStore();
      expect(state.displayAppearance).toEqual([]);

      const styleDirective: AppearanceDirective = {
        subject: 'S#realization[0]',
        style: { color: [0.5, 0.3, 0.1, 0.8], finish: 2, opacity: 0.8, wireframe: false },
      };
      setDisplayAppearance([styleDirective]);
      expect(state.displayAppearance).toEqual([styleDirective]);
      dispose();
    });
  });
});

describe('engineStore subscribeToEvents wiring for 4 live-sync fields (#5031 step-3)', () => {
  it('subscribeToEvents wires the 4 new listeners, routes payloads to state, and cleanup unsubscribes them', async () => {
    // RED: subscribeToEvents does not yet import or call the 4 new bridge listeners.
    await createRoot(async (dispose) => {
      const unlistenWires = vi.fn();
      const unlistenSurfaces = vi.fn();
      const unlistenPanes = vi.fn();
      const unlistenAppearance = vi.fn();

      let wiresCb: ((wires: TensegrityWireData[]) => void) | undefined;
      let surfacesCb: ((surfaces: TensegritySurfaceData[]) => void) | undefined;
      let panesCb: ((panes: DisplayDirective[]) => void) | undefined;
      let appearanceCb: ((appearance: AppearanceDirective[]) => void) | undefined;

      mockOnTensegrityWiresUpdate.mockImplementation(async (cb) => {
        wiresCb = cb as typeof wiresCb;
        return unlistenWires;
      });
      mockOnTensegritySurfacesUpdate.mockImplementation(async (cb) => {
        surfacesCb = cb as typeof surfacesCb;
        return unlistenSurfaces;
      });
      mockOnDisplayPanesUpdate.mockImplementation(async (cb) => {
        panesCb = cb as typeof panesCb;
        return unlistenPanes;
      });
      mockOnDisplayAppearanceUpdate.mockImplementation(async (cb) => {
        appearanceCb = cb as typeof appearanceCb;
        return unlistenAppearance;
      });

      const store = createEngineStore();
      const cleanup = await store.subscribeToEvents();

      expect(mockOnTensegrityWiresUpdate).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnTensegritySurfacesUpdate).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnDisplayPanesUpdate).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnDisplayAppearanceUpdate).toHaveBeenCalledWith(expect.any(Function));

      // Prove routing (not just subscription): invoking the captured callback
      // must update live store state via the reducer.
      const wire: TensegrityWireData = {
        entity_path: 'TPrism', kind: 'strut', x1: 1.0, y1: 0.0, z1: 1.0, x2: 0.866, y2: 0.5, z2: 0.0,
      };
      wiresCb!([wire]);
      expect(store.state.tensegrityWires).toEqual([wire]);

      const surface: TensegritySurfaceData = {
        entity_path: 'Patch', kind: 'membrane', i0: 0, i1: 1, i2: 2,
        x0: 0.0, y0: 0.0, z0: 0.0, x1: 1.0, y1: 0.0, z1: 0.0, x2: 0.5, y2: 0.866, z2: 0.0,
      };
      surfacesCb!([surface]);
      expect(store.state.tensegritySurfaces).toEqual([surface]);

      const directive: DisplayDirective = { subject: 'S#realization[0]', pane: 1 };
      panesCb!([directive]);
      expect(store.state.displayPanes).toEqual([directive]);

      const styleDirective: AppearanceDirective = {
        subject: 'S#realization[0]',
        style: { color: [0.5, 0.3, 0.1, 0.8], finish: 2, opacity: 0.8, wireframe: false },
      };
      appearanceCb!([styleDirective]);
      expect(store.state.displayAppearance).toEqual([styleDirective]);

      cleanup();
      expect(unlistenWires).toHaveBeenCalled();
      expect(unlistenSurfaces).toHaveBeenCalled();
      expect(unlistenPanes).toHaveBeenCalled();
      expect(unlistenAppearance).toHaveBeenCalled();

      dispose();
    });
  });
});
