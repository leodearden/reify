import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import type { GuiState, FeaDiagnosticInfo, FeaConvergenceInfo } from '../types';

vi.mock('../bridge', async () => (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock());

import {
  mockOnFeaDiagnosticsChanged,
  mockOnFeaConvergenceChanged,
  primeEngineStoreBridgeMocks,
} from './engineStoreBridgeHandles';
import { createEngineStore } from '../stores/engineStore';

beforeEach(primeEngineStoreBridgeMocks);

// ── #2966: feaDiagnostics store field ───────────────────────────────────────

describe('engineStore feaDiagnostics (#2966)', () => {
  it('createEngineStore initial state has feaDiagnostics: []', () => {
    // RED: EngineState.feaDiagnostics field absent until step-4.
    createRoot((dispose) => {
      const { state } = createEngineStore();
      expect((state as any).feaDiagnostics).toEqual([]);
      dispose();
    });
  });

  it('initFromState copies fea_diagnostics Unconstrained payload into state.feaDiagnostics', () => {
    // RED: initFromState does not set feaDiagnostics until step-4.
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const diag: FeaDiagnosticInfo = {
        kind: 'Unconstrained',
        rigid_body_modes: ['TranslationX', 'TranslationY', 'TranslationZ'],
      };
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
        fea_diagnostics: [diag],
      };
      initFromState(guiState);
      const feaDiagnostics: FeaDiagnosticInfo[] = (state as any).feaDiagnostics;
      expect(feaDiagnostics).toHaveLength(1);
      expect(feaDiagnostics[0].kind).toBe('Unconstrained');
      expect(
        (feaDiagnostics[0] as { kind: 'Unconstrained'; rigid_body_modes: string[] }).rigid_body_modes,
      ).toEqual(['TranslationX', 'TranslationY', 'TranslationZ']);
      dispose();
    });
  });

  it('initFromState with empty fea_diagnostics yields feaDiagnostics: []', () => {
    // RED: initFromState does not set feaDiagnostics until step-4.
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
        tensegrity_surfaces: [],
        display_panes: [],
        display_appearance: [],
        fea_diagnostics: [],
      };
      initFromState(guiState);
      expect((state as any).feaDiagnostics).toEqual([]);
      dispose();
    });
  });

  it('initFromState with no fea_diagnostics field yields feaDiagnostics: [] (forward-compat)', () => {
    // GuiState objects without fea_diagnostics (from older wire snapshots) must default to [].
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      // Cast away the required fea_diagnostics to simulate an older wire snapshot.
      const guiState = {
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
        // fea_diagnostics intentionally absent
      } as unknown as GuiState;
      initFromState(guiState);
      expect((state as any).feaDiagnostics).toEqual([]);
      dispose();
    });
  });
});

// ── s8: setFeaDiagnostics reducer + subscribeToEvents wiring ────────────────

describe('engineStore setFeaDiagnostics and subscribeToEvents wiring (step s8)', () => {
  it('setFeaDiagnostics replaces state.feaDiagnostics with provided list', () => {
    // RED: setFeaDiagnostics does not exist yet.
    createRoot((dispose) => {
      const store = createEngineStore();
      expect(store.state.feaDiagnostics).toEqual([]);

      const diag: FeaDiagnosticInfo = { kind: 'Unconstrained', rigid_body_modes: ['TranslationX'] };
      (store as any).setFeaDiagnostics([diag]);
      expect(store.state.feaDiagnostics).toEqual([diag]);

      // A subsequent call with [] clears the list (pins the clear-stale-overlay path).
      (store as any).setFeaDiagnostics([]);
      expect(store.state.feaDiagnostics).toEqual([]);
      dispose();
    });
  });

  it('subscribeToEvents wires onFeaDiagnosticsChanged and updates state', async () => {
    // RED: onFeaDiagnosticsChanged not yet wired into subscribeToEvents.
    await createRoot(async (dispose) => {
      let feaCb: ((diags: FeaDiagnosticInfo[]) => void) | undefined;
      const spyUnlisten = vi.fn();

      mockOnFeaDiagnosticsChanged.mockImplementation(async (cb) => {
        feaCb = cb as (diags: FeaDiagnosticInfo[]) => void;
        return spyUnlisten;
      });

      const store = createEngineStore();
      const cleanup = await store.subscribeToEvents();

      expect(mockOnFeaDiagnosticsChanged).toHaveBeenCalledWith(expect.any(Function));

      const diag: FeaDiagnosticInfo = {
        kind: 'Unconstrained',
        rigid_body_modes: ['TranslationX', 'TranslationY'],
      };
      feaCb!([diag]);
      expect(store.state.feaDiagnostics).toEqual([diag]);

      await cleanup();
      expect(spyUnlisten).toHaveBeenCalled();
      dispose();
    });
  });
});

// ── Task 3001 step-15: feaConvergence store field ───────────────────────────

describe('engineStore feaConvergence (task 3001)', () => {
  it('createEngineStore initial state has feaConvergence: null', () => {
    // RED: EngineState.feaConvergence field absent until step-16.
    createRoot((dispose) => {
      const { state } = createEngineStore();
      expect((state as any).feaConvergence).toBeNull();
      dispose();
    });
  });

  it('initFromState copies a NotConverged fea_convergence into state.feaConvergence', () => {
    // RED: initFromState does not map fea_convergence until step-16.
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const convergence: FeaConvergenceInfo = { converged: false, reason: 'MaxDofs' };
      const guiState: GuiState = {
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
        fea_diagnostics: [],
        fea_convergence: convergence,
      };
      initFromState(guiState);
      expect((state as any).feaConvergence).toEqual({ converged: false, reason: 'MaxDofs' });
      dispose();
    });
  });

  it('initFromState copies a Converged fea_convergence with no reason', () => {
    // RED: initFromState does not map fea_convergence until step-16.
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const guiState: GuiState = {
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
        fea_diagnostics: [],
        fea_convergence: { converged: true },
      };
      initFromState(guiState);
      expect((state as any).feaConvergence).toEqual({ converged: true });
      dispose();
    });
  });

  it('initFromState with null fea_convergence yields feaConvergence: null', () => {
    // RED: initFromState does not map fea_convergence until step-16.
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const guiState: GuiState = {
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
        fea_diagnostics: [],
        fea_convergence: null,
      };
      initFromState(guiState);
      expect((state as any).feaConvergence).toBeNull();
      dispose();
    });
  });

  it('initFromState with no fea_convergence field yields feaConvergence: null (forward-compat)', () => {
    // GuiState objects without fea_convergence (from older wire snapshots) must default to null.
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      // Cast away the required fea_convergence to simulate an older wire snapshot.
      const guiState = {
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
        fea_diagnostics: [],
        // fea_convergence intentionally absent
      } as unknown as GuiState;
      initFromState(guiState);
      expect((state as any).feaConvergence).toBeNull();
      dispose();
    });
  });
});

// ── step-8: setFeaConvergence reducer + subscribeToEvents wiring ───────────

describe('engineStore setFeaConvergence and subscribeToEvents wiring (step-8)', () => {
  it('setFeaConvergence replaces state.feaConvergence', () => {
    // RED: setFeaConvergence does not exist yet.
    createRoot((dispose) => {
      const store = createEngineStore();
      expect(store.state.feaConvergence).toBeNull();

      (store as any).setFeaConvergence({ converged: true });
      expect(store.state.feaConvergence).toEqual({ converged: true });

      // A subsequent call with null clears it (pins the clear-stale-indicator path).
      (store as any).setFeaConvergence(null);
      expect(store.state.feaConvergence).toBeNull();
      dispose();
    });
  });

  it('subscribeToEvents wires onFeaConvergenceChanged and updates state', async () => {
    // RED: onFeaConvergenceChanged not yet wired into subscribeToEvents.
    await createRoot(async (dispose) => {
      let feaConvergenceCb: ((conv: FeaConvergenceInfo | null) => void) | undefined;
      const spyUnlisten = vi.fn();

      mockOnFeaConvergenceChanged.mockImplementation(async (cb) => {
        feaConvergenceCb = cb as (conv: FeaConvergenceInfo | null) => void;
        return spyUnlisten;
      });

      const store = createEngineStore();
      const cleanup = await store.subscribeToEvents();

      expect(mockOnFeaConvergenceChanged).toHaveBeenCalledWith(expect.any(Function));

      const convergence: FeaConvergenceInfo = { converged: false, reason: 'MaxDofs' };
      feaConvergenceCb!(convergence);
      expect(store.state.feaConvergence).toEqual(convergence);

      await cleanup();
      expect(spyUnlisten).toHaveBeenCalled();
      dispose();
    });
  });
});
