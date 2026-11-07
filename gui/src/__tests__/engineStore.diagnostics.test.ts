import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import type { GuiState, DiagnosticInfo } from '../types';

vi.mock('../bridge', async () => (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock());

import {
  mockOnMeshUpdate,
  mockOnValueUpdate,
  mockOnConstraintUpdate,
  mockOnEvaluationStatus,
  mockOnMeshRemoved,
  mockOnValueRemoved,
  mockOnConstraintRemoved,
  mockOnTessellationDiagnostics,
  mockOnCompileDiagnostics,
  primeEngineStoreBridgeMocks,
} from './engineStoreBridgeHandles';
import { createEngineStore } from '../stores/engineStore';

beforeEach(primeEngineStoreBridgeMocks);

describe('engineStore tessellationDiagnostics', () => {
  it('initial state.tessellationDiagnostics is []', () => {
    createRoot((dispose) => {
      const { state } = createEngineStore();
      expect(state.tessellationDiagnostics).toEqual([]);
      dispose();
    });
  });

  it('initFromState populates tessellationDiagnostics from GuiState', () => {
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const diag: DiagnosticInfo = {
        file_path: '<unknown>',
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 1,
        severity: 'Error',
        message: 'geometry error: kernel failure',
        code: null,
      };
      const guiState: GuiState = { fea_convergence: null,
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [diag],
        compile_diagnostics: [],
        tensegrity_wires: [],
        tensegrity_surfaces: [],
        display_panes: [],
        display_appearance: [],
        fea_diagnostics: []
      };
      initFromState(guiState);
      expect(state.tessellationDiagnostics).toEqual([diag]);
      dispose();
    });
  });

  it('subscribeToEvents wires tessellation-diagnostics event and updates state', async () => {
    await createRoot(async (dispose) => {
      let tessCb: ((diags: DiagnosticInfo[]) => void) | undefined;

      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnTessellationDiagnostics.mockImplementation(async (cb) => {
        tessCb = cb as (diags: DiagnosticInfo[]) => void;
        return vi.fn();
      });

      const store = createEngineStore();
      await store.subscribeToEvents();

      expect(mockOnTessellationDiagnostics).toHaveBeenCalledWith(expect.any(Function));

      const diag: DiagnosticInfo = {
        file_path: '<unknown>',
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 1,
        severity: 'Error',
        message: 'geometry error: kernel failure',
        code: null,
      };

      tessCb!([diag]);
      expect(store.state.tessellationDiagnostics).toEqual([diag]);
      dispose();
    });
  });
});

describe('engineStore compileDiagnostics', () => {
  it('initial state.compileDiagnostics is []', () => {
    createRoot((dispose) => {
      const { state } = createEngineStore();
      expect(state.compileDiagnostics).toEqual([]);
      dispose();
    });
  });

  it('initFromState populates compileDiagnostics from GuiState.compile_diagnostics', () => {
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const diag: DiagnosticInfo = {
        file_path: 'helper.ri',
        line: 3,
        column: 1,
        end_line: 3,
        end_column: 10,
        severity: 'Warning',
        message: "unknown port type 'Foo'",
        code: null,
      };
      const guiState: GuiState = { fea_convergence: null,
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [diag],
        tensegrity_wires: [],
        tensegrity_surfaces: [],
        display_panes: [],
        display_appearance: [],
        fea_diagnostics: []
      };
      initFromState(guiState);
      expect(state.compileDiagnostics).toEqual([diag]);
      dispose();
    });
  });

  it('subscribeToEvents wires compile-diagnostics event and updates state', async () => {
    await createRoot(async (dispose) => {
      let compileCb: ((diags: DiagnosticInfo[]) => void) | undefined;

      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnTessellationDiagnostics.mockResolvedValue(vi.fn());
      mockOnCompileDiagnostics.mockImplementation(async (cb) => {
        compileCb = cb as (diags: DiagnosticInfo[]) => void;
        return vi.fn();
      });

      const store = createEngineStore();
      await store.subscribeToEvents();

      expect(mockOnCompileDiagnostics).toHaveBeenCalledWith(expect.any(Function));

      const diag: DiagnosticInfo = {
        file_path: 'helper.ri',
        line: 3,
        column: 1,
        end_line: 3,
        end_column: 10,
        severity: 'Warning',
        message: "unknown port type 'Foo'",
        code: null,
      };

      compileCb!([diag]);
      expect(store.state.compileDiagnostics).toEqual([diag]);
      dispose();
    });
  });
});
