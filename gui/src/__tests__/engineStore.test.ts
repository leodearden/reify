import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot, createComputed } from 'solid-js';
import type {
  GuiState,
  MeshData,
  ValueData,
  ConstraintData,
  EvaluationStatus,
  DiagnosticInfo,
} from '../types';
import type { KernelStatus } from '../bridge';

// Mock the bridge module
vi.mock('../bridge', () => ({
  onMeshUpdate: vi.fn(),
  onValueUpdate: vi.fn(),
  onConstraintUpdate: vi.fn(),
  onEvaluationStatus: vi.fn(),
  onMeshRemoved: vi.fn(),
  onValueRemoved: vi.fn(),
  onConstraintRemoved: vi.fn(),
  onTessellationDiagnostics: vi.fn(),
  onCompileDiagnostics: vi.fn(),
  onKernelStatus: vi.fn(),
  onAutoResolveStart: vi.fn(),
  onAutoResolveIteration: vi.fn(),
  onAutoResolveComplete: vi.fn(),
}));

import {
  onMeshUpdate,
  onValueUpdate,
  onConstraintUpdate,
  onEvaluationStatus,
  onMeshRemoved,
  onValueRemoved,
  onConstraintRemoved,
  onTessellationDiagnostics,
  onCompileDiagnostics,
  onAutoResolveStart,
  onAutoResolveIteration,
  onAutoResolveComplete,
} from '../bridge';
import { createEngineStore } from '../stores/engineStore';

const mockOnMeshUpdate = vi.mocked(onMeshUpdate);
const mockOnValueUpdate = vi.mocked(onValueUpdate);
const mockOnConstraintUpdate = vi.mocked(onConstraintUpdate);
const mockOnEvaluationStatus = vi.mocked(onEvaluationStatus);
const mockOnMeshRemoved = vi.mocked(onMeshRemoved);
const mockOnValueRemoved = vi.mocked(onValueRemoved);
const mockOnConstraintRemoved = vi.mocked(onConstraintRemoved);
const mockOnTessellationDiagnostics = vi.mocked(onTessellationDiagnostics);
const mockOnCompileDiagnostics = vi.mocked(onCompileDiagnostics);
const mockOnAutoResolveStart = vi.mocked(onAutoResolveStart);
const mockOnAutoResolveIteration = vi.mocked(onAutoResolveIteration);
const mockOnAutoResolveComplete = vi.mocked(onAutoResolveComplete);

beforeEach(() => {
  vi.clearAllMocks();
  // Default: all subscriptions succeed with a no-op unlisten function.
  // Tests that need specific behaviour can override individual mocks.
  mockOnTessellationDiagnostics.mockResolvedValue(vi.fn());
  mockOnCompileDiagnostics.mockResolvedValue(vi.fn());
  mockOnAutoResolveStart.mockResolvedValue(vi.fn());
  mockOnAutoResolveIteration.mockResolvedValue(vi.fn());
  mockOnAutoResolveComplete.mockResolvedValue(vi.fn());
});

const sampleMesh: MeshData = {
  entity_path: 'Bracket.body',
  vertices: new Float32Array([0, 1, 2, 3, 4, 5]),
  indices: new Uint32Array([0, 1, 2]),
  normals: new Float32Array([0, 0, 1, 0, 0, 1]),
};

const sampleValue: ValueData = {
  cell_id: 'cell_001',
  name: 'width',
  value: '50.0',
  unit: 'mm',
  determinacy: 'determined',
  entity_path: 'Bracket.width',
  kind: 'Param',
  freshness: 'final',
};

const sampleConstraint: ConstraintData = {
  node_id: 'constraint_001',
  expression: 'width > 10',
  status: 'satisfied',
  label: null,
  parameter_ids: ['cell_001'],
};

describe('engineStore', () => {
  it('has empty initial state with idle evalStatus', () => {
    createRoot((dispose) => {
      const { state } = createEngineStore();
      expect(state.meshes).toEqual({});
      expect(state.values).toEqual({});
      expect(state.constraints).toEqual({});
      expect(state.evalStatus).toEqual({ phase: 'idle' });
      dispose();
    });
  });

  it('initFromState populates meshes/values/constraints from GuiState', () => {
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const guiState: GuiState = {
        meshes: [sampleMesh],
        values: [sampleValue],
        constraints: [sampleConstraint],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
      };
      initFromState(guiState);

      expect(state.meshes['Bracket.body']).toEqual(sampleMesh);
      expect(state.values['cell_001']).toEqual(sampleValue);
      expect(state.constraints['constraint_001']).toEqual(sampleConstraint);
      dispose();
    });
  });

  it('applyMeshUpdate upserts a mesh by entity_path', () => {
    createRoot((dispose) => {
      const { state, applyMeshUpdate } = createEngineStore();
      applyMeshUpdate(sampleMesh);
      expect(state.meshes['Bracket.body']).toEqual(sampleMesh);

      // Update existing
      const updated = { ...sampleMesh, vertices: new Float32Array([9, 8, 7]) };
      applyMeshUpdate(updated);
      expect(state.meshes['Bracket.body'].vertices).toEqual(new Float32Array([9, 8, 7]));
      dispose();
    });
  });

  it('applyValueUpdates upserts multiple values by cell_id', () => {
    createRoot((dispose) => {
      const { state, applyValueUpdates } = createEngineStore();
      const value2: ValueData = {
        cell_id: 'cell_002',
        name: 'height',
        value: '30.0',
        unit: 'mm',
        determinacy: 'determined',
        entity_path: 'Bracket.height',
        kind: 'Param',
        freshness: 'final',
      };
      applyValueUpdates([sampleValue, value2]);
      expect(state.values['cell_001']).toEqual(sampleValue);
      expect(state.values['cell_002']).toEqual(value2);
      dispose();
    });
  });

  it('applyConstraintUpdates upserts multiple constraints by node_id', () => {
    createRoot((dispose) => {
      const { state, applyConstraintUpdates } = createEngineStore();
      const c2: ConstraintData = {
        node_id: 'constraint_002',
        expression: 'height < 100',
        status: 'violated',
        label: 'too large',
        parameter_ids: ['cell_002'],
      };
      applyConstraintUpdates([sampleConstraint, c2]);
      expect(state.constraints['constraint_001']).toEqual(sampleConstraint);
      expect(state.constraints['constraint_002']).toEqual(c2);
      dispose();
    });
  });

  it('setEvalStatus updates evalStatus', () => {
    createRoot((dispose) => {
      const { state, setEvalStatus } = createEngineStore();
      const status: EvaluationStatus = { phase: 'evaluating', progress: 0.5 };
      setEvalStatus(status);
      expect(state.evalStatus).toEqual(status);
      dispose();
    });
  });

  it('subscribeToEvents wires bridge listeners and returns cleanup', async () => {
    await createRoot(async (dispose) => {
      const unlistenMesh = vi.fn();
      const unlistenValue = vi.fn();
      const unlistenConstraint = vi.fn();
      const unlistenStatus = vi.fn();
      const unlistenMeshRemoved = vi.fn();
      const unlistenValueRemoved = vi.fn();
      const unlistenConstraintRemoved = vi.fn();

      mockOnMeshUpdate.mockResolvedValue(unlistenMesh);
      mockOnValueUpdate.mockResolvedValue(unlistenValue);
      mockOnConstraintUpdate.mockResolvedValue(unlistenConstraint);
      mockOnEvaluationStatus.mockResolvedValue(unlistenStatus);
      mockOnMeshRemoved.mockResolvedValue(unlistenMeshRemoved);
      mockOnValueRemoved.mockResolvedValue(unlistenValueRemoved);
      mockOnConstraintRemoved.mockResolvedValue(unlistenConstraintRemoved);

      const { subscribeToEvents } = createEngineStore();
      const cleanup = await subscribeToEvents();

      expect(mockOnMeshUpdate).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnValueUpdate).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnConstraintUpdate).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnEvaluationStatus).toHaveBeenCalledWith(expect.any(Function));

      // Call cleanup and verify all unlisten functions are called
      cleanup();
      expect(unlistenMesh).toHaveBeenCalled();
      expect(unlistenValue).toHaveBeenCalled();
      expect(unlistenConstraint).toHaveBeenCalled();
      expect(unlistenStatus).toHaveBeenCalled();
      expect(unlistenMeshRemoved).toHaveBeenCalled();
      expect(unlistenValueRemoved).toHaveBeenCalled();
      expect(unlistenConstraintRemoved).toHaveBeenCalled();

      dispose();
    });
  });

  it('applyValueUpdates triggers exactly 1 reactive update for multiple items', () => {
    createRoot((dispose) => {
      const { state, applyValueUpdates } = createEngineStore();
      // Counter starts at -1 to account for the initial effect run
      let updateCount = -1;

      createComputed(() => {
        // Read the reactive state to establish tracking
        JSON.stringify(state.values);
        updateCount++;
      });

      // Initial effect run sets counter to 0
      expect(updateCount).toBe(0);

      const values: ValueData[] = [
        { cell_id: 'a', name: 'a', value: '1', unit: 'mm', determinacy: 'determined', entity_path: 'X.a', kind: 'Param', freshness: 'final' },
        { cell_id: 'b', name: 'b', value: '2', unit: 'mm', determinacy: 'determined', entity_path: 'X.b', kind: 'Param', freshness: 'final' },
        { cell_id: 'c', name: 'c', value: '3', unit: 'mm', determinacy: 'determined', entity_path: 'X.c', kind: 'Param', freshness: 'final' },
      ];

      applyValueUpdates(values);

      // Should be 1 batched notification, not 3 separate ones
      expect(updateCount).toBe(1);

      dispose();
    });
  });

  it('applyConstraintUpdates triggers exactly 1 reactive update for multiple items', () => {
    createRoot((dispose) => {
      const { state, applyConstraintUpdates } = createEngineStore();
      let updateCount = -1;

      createComputed(() => {
        JSON.stringify(state.constraints);
        updateCount++;
      });

      expect(updateCount).toBe(0);

      const constraints: ConstraintData[] = [
        { node_id: 'n1', expression: 'a > 0', status: 'satisfied', label: null, parameter_ids: ['a'] },
        { node_id: 'n2', expression: 'b > 0', status: 'satisfied', label: null, parameter_ids: ['b'] },
        { node_id: 'n3', expression: 'c > 0', status: 'violated', label: 'fail', parameter_ids: ['c'] },
      ];

      applyConstraintUpdates(constraints);

      // Should be 1 batched notification, not 3 separate ones
      expect(updateCount).toBe(1);

      dispose();
    });
  });

  it('removeMesh deletes a mesh entry from state.meshes by entity_path', () => {
    createRoot((dispose) => {
      const { state, applyMeshUpdate, removeMesh } = createEngineStore();
      applyMeshUpdate(sampleMesh);
      expect(state.meshes['Bracket.body']).toBeDefined();

      removeMesh('Bracket.body');
      expect(state.meshes['Bracket.body']).toBeUndefined();
      dispose();
    });
  });

  it('removeValue deletes a value entry from state.values by cell_id', () => {
    createRoot((dispose) => {
      const { state, applyValueUpdates, removeValue } = createEngineStore();
      applyValueUpdates([sampleValue]);
      expect(state.values['cell_001']).toBeDefined();

      removeValue('cell_001');
      expect(state.values['cell_001']).toBeUndefined();
      dispose();
    });
  });

  it('removeConstraint deletes a constraint entry from state.constraints by node_id', () => {
    createRoot((dispose) => {
      const { state, applyConstraintUpdates, removeConstraint } = createEngineStore();
      applyConstraintUpdates([sampleConstraint]);
      expect(state.constraints['constraint_001']).toBeDefined();

      removeConstraint('constraint_001');
      expect(state.constraints['constraint_001']).toBeUndefined();
      dispose();
    });
  });

  it('subscribeToEvents wires removal listeners and cleanup calls all seven unlisten fns', async () => {
    await createRoot(async (dispose) => {
      const unlistenMesh = vi.fn();
      const unlistenValue = vi.fn();
      const unlistenConstraint = vi.fn();
      const unlistenStatus = vi.fn();
      const unlistenMeshRemoved = vi.fn();
      const unlistenValueRemoved = vi.fn();
      const unlistenConstraintRemoved = vi.fn();

      mockOnMeshUpdate.mockResolvedValue(unlistenMesh);
      mockOnValueUpdate.mockResolvedValue(unlistenValue);
      mockOnConstraintUpdate.mockResolvedValue(unlistenConstraint);
      mockOnEvaluationStatus.mockResolvedValue(unlistenStatus);
      mockOnMeshRemoved.mockResolvedValue(unlistenMeshRemoved);
      mockOnValueRemoved.mockResolvedValue(unlistenValueRemoved);
      mockOnConstraintRemoved.mockResolvedValue(unlistenConstraintRemoved);

      const { subscribeToEvents } = createEngineStore();
      const cleanup = await subscribeToEvents();

      expect(mockOnMeshRemoved).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnValueRemoved).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnConstraintRemoved).toHaveBeenCalledWith(expect.any(Function));

      cleanup();
      expect(unlistenMeshRemoved).toHaveBeenCalled();
      expect(unlistenValueRemoved).toHaveBeenCalled();
      expect(unlistenConstraintRemoved).toHaveBeenCalled();

      dispose();
    });
  });

  it('subscribeToEvents handles partial listener failures without leaking', async () => {
    await createRoot(async (dispose) => {
      const unlistenMesh = vi.fn();
      const unlistenValue = vi.fn();
      const unlistenStatus = vi.fn();
      const unlistenMeshRemoved = vi.fn();
      const unlistenValueRemoved = vi.fn();
      const unlistenConstraintRemoved = vi.fn();

      mockOnMeshUpdate.mockResolvedValue(unlistenMesh);
      mockOnValueUpdate.mockResolvedValue(unlistenValue);
      // onConstraintUpdate rejects — simulating an unavailable event
      mockOnConstraintUpdate.mockRejectedValue(new Error('event not available'));
      mockOnEvaluationStatus.mockResolvedValue(unlistenStatus);
      mockOnMeshRemoved.mockResolvedValue(unlistenMeshRemoved);
      mockOnValueRemoved.mockResolvedValue(unlistenValueRemoved);
      mockOnConstraintRemoved.mockResolvedValue(unlistenConstraintRemoved);

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

      const { subscribeToEvents } = createEngineStore();

      // Should resolve (not reject) even with partial failure
      const cleanup = await subscribeToEvents();
      expect(typeof cleanup).toBe('function');

      // Should warn about the failed subscription
      expect(warnSpy).toHaveBeenCalled();

      // Cleanup should call all successfully-registered unlisten fns
      cleanup();
      expect(unlistenMesh).toHaveBeenCalled();
      expect(unlistenValue).toHaveBeenCalled();
      expect(unlistenStatus).toHaveBeenCalled();
      expect(unlistenMeshRemoved).toHaveBeenCalled();
      expect(unlistenValueRemoved).toHaveBeenCalled();
      expect(unlistenConstraintRemoved).toHaveBeenCalled();

      warnSpy.mockRestore();
      dispose();
    });
  });

  // S4: phantom key tests — removed keys must not linger in Object.keys
  it('removeMesh leaves no phantom key in Object.keys(state.meshes)', () => {
    createRoot((dispose) => {
      const { state, applyMeshUpdate, removeMesh } = createEngineStore();
      applyMeshUpdate(sampleMesh);
      removeMesh('Bracket.body');

      expect(Object.keys(state.meshes)).not.toContain('Bracket.body');
      expect(Object.keys(state.meshes)).toHaveLength(0);
      dispose();
    });
  });

  it('removeValue leaves no phantom key in Object.keys(state.values)', () => {
    createRoot((dispose) => {
      const { state, applyValueUpdates, removeValue } = createEngineStore();
      applyValueUpdates([sampleValue]);
      removeValue('cell_001');

      expect(Object.keys(state.values)).not.toContain('cell_001');
      expect(Object.keys(state.values)).toHaveLength(0);
      dispose();
    });
  });

  it('removeConstraint leaves no phantom key in Object.keys(state.constraints)', () => {
    createRoot((dispose) => {
      const { state, applyConstraintUpdates, removeConstraint } = createEngineStore();
      applyConstraintUpdates([sampleConstraint]);
      removeConstraint('constraint_001');

      expect(Object.keys(state.constraints)).not.toContain('constraint_001');
      expect(Object.keys(state.constraints)).toHaveLength(0);
      dispose();
    });
  });

  it('Object.values after removeMesh contains no undefined entries', () => {
    createRoot((dispose) => {
      const { state, applyMeshUpdate, removeMesh } = createEngineStore();
      applyMeshUpdate(sampleMesh);
      removeMesh('Bracket.body');

      const values = Object.values(state.meshes);
      expect(values).toHaveLength(0);
      expect(values.every((v) => v !== undefined)).toBe(true);
      dispose();
    });
  });

  it('iterating Object.values after removal does not crash on property access', () => {
    createRoot((dispose) => {
      const { state, applyMeshUpdate, removeMesh } = createEngineStore();
      const mesh2: MeshData = {
        entity_path: 'Mount.body',
        vertices: new Float32Array([1, 2, 3]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      };
      applyMeshUpdate(sampleMesh);
      applyMeshUpdate(mesh2);

      removeMesh('Bracket.body');

      // This simulates what StatusBar does: iterate values and access .indices.length
      // With phantom keys, this would crash on undefined.indices
      const totalTriangles = Object.values(state.meshes).reduce(
        (sum, mesh) => sum + mesh.indices.length / 3,
        0,
      );
      expect(totalTriangles).toBe(1); // Only Mount.body remains
      dispose();
    });
  });

  // S8 integration: onEntityRemoved callback fires on removal
  it('onEntityRemoved callback fires when removeMesh is called', () => {
    createRoot((dispose) => {
      const spy = vi.fn();
      const { applyMeshUpdate, removeMesh } = createEngineStore({ onEntityRemoved: spy });
      applyMeshUpdate(sampleMesh);
      removeMesh('Bracket.body');
      expect(spy).toHaveBeenCalledWith('Bracket.body');
      dispose();
    });
  });

  it('onEntityRemoved callback fires when removeValue is called', () => {
    createRoot((dispose) => {
      const spy = vi.fn();
      const { applyValueUpdates, removeValue } = createEngineStore({ onEntityRemoved: spy });
      applyValueUpdates([sampleValue]);
      removeValue('cell_001');
      expect(spy).toHaveBeenCalledWith('cell_001');
      dispose();
    });
  });

  it('onEntityRemoved callback fires when removeConstraint is called', () => {
    createRoot((dispose) => {
      const spy = vi.fn();
      const { applyConstraintUpdates, removeConstraint } = createEngineStore({ onEntityRemoved: spy });
      applyConstraintUpdates([sampleConstraint]);
      removeConstraint('constraint_001');
      expect(spy).toHaveBeenCalledWith('constraint_001');
      dispose();
    });
  });

  // onEngineReinitialized: fires once per initFromState call so callers
  // (App.tsx) can refresh derived data — entity tree, mechanism descriptors —
  // without depending on evalStatus.phase transitions.
  it('onEngineReinitialized callback fires when initFromState is called', () => {
    createRoot((dispose) => {
      const spy = vi.fn();
      const { initFromState } = createEngineStore({ onEngineReinitialized: spy });
      const guiState: GuiState = {
        meshes: [sampleMesh],
        values: [sampleValue],
        constraints: [sampleConstraint],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
      };
      initFromState(guiState);
      expect(spy).toHaveBeenCalledTimes(1);
      dispose();
    });
  });

  it('onEngineReinitialized fires once per initFromState invocation across multiple loads', () => {
    createRoot((dispose) => {
      const spy = vi.fn();
      const { initFromState } = createEngineStore({ onEngineReinitialized: spy });
      const guiState: GuiState = {
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
      };
      initFromState(guiState);
      initFromState(guiState);
      initFromState(guiState);
      expect(spy).toHaveBeenCalledTimes(3);
      dispose();
    });
  });

  it('createEngineStore works without onEngineReinitialized (option is optional)', () => {
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const guiState: GuiState = {
        meshes: [sampleMesh],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
      };
      // Must not throw when the callback is omitted.
      expect(() => initFromState(guiState)).not.toThrow();
      expect(state.meshes['Bracket.body']).toEqual(sampleMesh);
      dispose();
    });
  });

  it('onEntityRemoved callback fires for event-driven removals via subscribeToEvents', async () => {
    await createRoot(async (dispose) => {
      const spy = vi.fn();

      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      // Capture the removal callbacks when subscribeToEvents registers them
      let meshRemovedCb: ((entityPath: string) => void) | undefined;
      let valueRemovedCb: ((cellId: string) => void) | undefined;
      let constraintRemovedCb: ((nodeId: string) => void) | undefined;
      mockOnMeshRemoved.mockImplementation(async (cb) => { meshRemovedCb = cb; return vi.fn(); });
      mockOnValueRemoved.mockImplementation(async (cb) => { valueRemovedCb = cb; return vi.fn(); });
      mockOnConstraintRemoved.mockImplementation(async (cb) => { constraintRemovedCb = cb; return vi.fn(); });

      const store = createEngineStore({ onEntityRemoved: spy });
      await store.subscribeToEvents();

      // Simulate event-driven removals
      meshRemovedCb!('Bracket.body');
      expect(spy).toHaveBeenCalledWith('Bracket.body');

      valueRemovedCb!('cell_001');
      expect(spy).toHaveBeenCalledWith('cell_001');

      constraintRemovedCb!('constraint_001');
      expect(spy).toHaveBeenCalledWith('constraint_001');

      expect(spy).toHaveBeenCalledTimes(3);
      dispose();
    });
  });
});

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
      const guiState: GuiState = {
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [diag],
        compile_diagnostics: [],
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
      const guiState: GuiState = {
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [diag],
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

describe('engineStore freshness pass-through', () => {
  it('initFromState preserves freshness=failed round-trip through state.values', () => {
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const failedValue: ValueData = {
        cell_id: 'cell_failed',
        name: 'depth',
        value: '',
        unit: 'mm',
        determinacy: 'undef',
        entity_path: 'Bracket.depth',
        kind: 'Let',
        freshness: 'failed',
      };
      const guiState: GuiState = {
        meshes: [],
        values: [failedValue],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
      };
      initFromState(guiState);
      expect(state.values['cell_failed'].freshness).toBe('failed');
      dispose();
    });
  });

  it('applyValueUpdates reflects a Pending→Final freshness transition in state.values', () => {
    createRoot((dispose) => {
      const { state, applyValueUpdates } = createEngineStore();
      // Step 1: insert a cell with freshness 'pending'
      const pendingValue: ValueData = {
        cell_id: 'cell_p2f',
        name: 'radius',
        value: '',
        unit: 'mm',
        determinacy: 'undef',
        entity_path: 'Bracket.radius',
        kind: 'Let',
        freshness: 'pending',
      };
      applyValueUpdates([pendingValue]);
      expect(state.values['cell_p2f'].freshness).toBe('pending');

      // Step 2: update the same cell to freshness 'final'
      const finalValue: ValueData = { ...pendingValue, freshness: 'final', value: '12.5' };
      applyValueUpdates([finalValue]);
      expect(state.values['cell_p2f'].freshness).toBe('final');
      expect(state.values['cell_p2f'].value).toBe('12.5');
      dispose();
    });
  });
});

describe('engineStore autoResolve loop state', () => {
  const sampleIteration = {
    iteration: 1,
    parameters: {
      'Bracket.thickness': { value: 4.2, unit: 'mm', display: '4.2mm' },
    },
    constraints: {
      max_von_mises: {
        name: 'max_von_mises',
        value: 180,
        unit: 'MPa',
        target_upper: 200,
        satisfied: true,
      },
    },
    driving_metric: 'max_von_mises',
    driving_metric_value: 180,
  };

  it('(a) initial state.autoResolve equals { active: false, iterations: [] }', () => {
    createRoot((dispose) => {
      const { state } = createEngineStore();
      expect(state.autoResolve).toEqual({ active: false, iterations: [] });
      dispose();
    });
  });

  it('(b) beginAutoResolveLoop sets active=true and clears iterations', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      applyAutoResolveIteration(sampleIteration);
      expect(state.autoResolve.iterations).toHaveLength(1);
      beginAutoResolveLoop();
      expect(state.autoResolve.active).toBe(true);
      expect(state.autoResolve.iterations).toHaveLength(0);
      dispose();
    });
  });

  it('(c) applyAutoResolveIteration appends to iterations', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.iterations[0]).toEqual(sampleIteration);

      const iter2 = { ...sampleIteration, iteration: 2, driving_metric_value: 165 };
      applyAutoResolveIteration(iter2);
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.iterations[1].iteration).toBe(2);
      dispose();
    });
  });

  it('(d) endAutoResolveLoop sets active=false and clears iterations', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration, endAutoResolveLoop } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2 });
      expect(state.autoResolve.iterations).toHaveLength(2);

      endAutoResolveLoop();
      expect(state.autoResolve.active).toBe(false);
      // iterations are cleared — the panel unmounts on active=false so
      // preserved data would be unreachable until the next beginAutoResolveLoop
      expect(state.autoResolve.iterations).toHaveLength(0);
      dispose();
    });
  });
});

describe('engineStore autoResolve subscribeToEvents wiring', () => {
  const sampleIteration = {
    iteration: 1,
    parameters: { 'Bracket.thickness': { value: 4.2, unit: 'mm', display: '4.2mm' } },
    constraints: {
      max_von_mises: { name: 'max_von_mises', value: 180, unit: 'MPa', target_upper: 200, satisfied: true },
    },
    driving_metric: 'max_von_mises',
    driving_metric_value: 180,
  };

  it('(a) subscribeToEvents calls onAutoResolveStart, onAutoResolveIteration, onAutoResolveComplete', async () => {
    await createRoot(async (dispose) => {
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());

      const store = createEngineStore();
      await store.subscribeToEvents();

      expect(mockOnAutoResolveStart).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnAutoResolveIteration).toHaveBeenCalledWith(expect.any(Function));
      expect(mockOnAutoResolveComplete).toHaveBeenCalledWith(expect.any(Function));
      dispose();
    });
  });

  it('(b) auto-resolve-start callback flips active=true and clears iterations', async () => {
    await createRoot(async (dispose) => {
      let startCb: (() => void) | undefined;
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnAutoResolveStart.mockImplementation(async (cb) => { startCb = cb; return vi.fn(); });

      const store = createEngineStore();
      store.applyAutoResolveIteration(sampleIteration);
      await store.subscribeToEvents();

      startCb!();
      expect(store.state.autoResolve.active).toBe(true);
      expect(store.state.autoResolve.iterations).toHaveLength(0);
      dispose();
    });
  });

  it('(c) auto-resolve-iteration callback appends to iterations', async () => {
    await createRoot(async (dispose) => {
      let iterCb: ((iter: typeof sampleIteration) => void) | undefined;
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnAutoResolveIteration.mockImplementation(async (cb) => { iterCb = cb as typeof iterCb; return vi.fn(); });

      const store = createEngineStore();
      await store.subscribeToEvents();

      iterCb!(sampleIteration);
      expect(store.state.autoResolve.iterations).toHaveLength(1);
      expect(store.state.autoResolve.iterations[0]).toEqual(sampleIteration);
      dispose();
    });
  });

  it('(d) auto-resolve-complete callback sets active=false and clears iterations', async () => {
    await createRoot(async (dispose) => {
      let startCb: (() => void) | undefined;
      let iterCb: ((iter: typeof sampleIteration) => void) | undefined;
      let completeCb: (() => void) | undefined;
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnAutoResolveStart.mockImplementation(async (cb) => { startCb = cb; return vi.fn(); });
      mockOnAutoResolveIteration.mockImplementation(async (cb) => { iterCb = cb as typeof iterCb; return vi.fn(); });
      mockOnAutoResolveComplete.mockImplementation(async (cb) => { completeCb = cb; return vi.fn(); });

      const store = createEngineStore();
      await store.subscribeToEvents();

      startCb!();
      iterCb!(sampleIteration);
      completeCb!();

      expect(store.state.autoResolve.active).toBe(false);
      // iterations cleared — panel unmounts on active=false so data would be unreachable
      expect(store.state.autoResolve.iterations).toHaveLength(0);
      dispose();
    });
  });

  it('(e) cleanup() invokes the three new unlisten fns', async () => {
    await createRoot(async (dispose) => {
      const unlistenStart = vi.fn();
      const unlistenIteration = vi.fn();
      const unlistenComplete = vi.fn();
      mockOnMeshUpdate.mockResolvedValue(vi.fn());
      mockOnValueUpdate.mockResolvedValue(vi.fn());
      mockOnConstraintUpdate.mockResolvedValue(vi.fn());
      mockOnEvaluationStatus.mockResolvedValue(vi.fn());
      mockOnMeshRemoved.mockResolvedValue(vi.fn());
      mockOnValueRemoved.mockResolvedValue(vi.fn());
      mockOnConstraintRemoved.mockResolvedValue(vi.fn());
      mockOnAutoResolveStart.mockResolvedValue(unlistenStart);
      mockOnAutoResolveIteration.mockResolvedValue(unlistenIteration);
      mockOnAutoResolveComplete.mockResolvedValue(unlistenComplete);

      const store = createEngineStore();
      const cleanup = await store.subscribeToEvents();

      cleanup();
      expect(unlistenStart).toHaveBeenCalled();
      expect(unlistenIteration).toHaveBeenCalled();
      expect(unlistenComplete).toHaveBeenCalled();
      dispose();
    });
  });
});

describe('engineStore autoResolve driving_metric invariance', () => {
  const sampleIteration = {
    iteration: 1,
    parameters: {
      'Bracket.thickness': { value: 4.2, unit: 'mm', display: '4.2mm' },
    },
    constraints: {
      max_von_mises: {
        name: 'max_von_mises',
        value: 180,
        unit: 'MPa',
        target_upper: 200,
        satisfied: true,
      },
    },
    driving_metric: 'max_von_mises',
    driving_metric_value: 180,
  };

  it('(1) iteration with matching driving_metric is appended', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');
      const iter2 = { ...sampleIteration, iteration: 2, driving_metric_value: 165 };
      applyAutoResolveIteration(iter2);
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.iterations[1]).toEqual(iter2);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');
      dispose();
    });
  });

  it('(2) iteration with mismatched driving_metric is dropped', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      const mismatchedIter = { ...sampleIteration, iteration: 2, driving_metric: 'displacement', driving_metric_value: 1.5 };
      applyAutoResolveIteration(mismatchedIter);
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.iterations[0].driving_metric).toBe('max_von_mises');
      dispose();
    });
  });

  it('(3) iteration without driving_metric is appended regardless', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      // Apply an iteration without driving_metric
      const { driving_metric, driving_metric_value, ...noMetricIter } = sampleIteration;
      applyAutoResolveIteration({ ...noMetricIter, iteration: 2 });
      expect(state.autoResolve.iterations).toHaveLength(2);
      // Canonical metric is preserved: subsequent mismatched iteration is still rejected
      const mismatchedIter = { ...sampleIteration, iteration: 3, driving_metric: 'displacement', driving_metric_value: 1.5 };
      applyAutoResolveIteration(mismatchedIter);
      expect(state.autoResolve.iterations).toHaveLength(2);
      dispose();
    });
  });

  it('(4) first iteration with driving_metric establishes the canonical; no-canonical state accepts any metric', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();

      // Initially no canonical — the loop accepts ANY driving_metric value.
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();

      // First iteration has no driving_metric — accepted; canonical remains undefined.
      const { driving_metric, driving_metric_value, ...noMetricIter } = sampleIteration;
      applyAutoResolveIteration({ ...noMetricIter, iteration: 1 });
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();

      // Second iteration declares driving_metric='displacement' — accepted (no canonical
      // conflict) and ESTABLISHES the canonical for the remainder of the loop.
      const displacementIter = { ...sampleIteration, iteration: 2, driving_metric: 'displacement', driving_metric_value: 0.5 };
      applyAutoResolveIteration(displacementIter);
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('displacement');

      // Third iteration with a different metric — DROPPED (canonical is now 'displacement').
      const mismatchedIter = { ...sampleIteration, iteration: 3, driving_metric_value: 190 };
      applyAutoResolveIteration(mismatchedIter);
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('displacement');

      dispose();
    });
  });

  it('(5) console.warn fires on rejection', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const mismatchedIter = { ...sampleIteration, iteration: 2, driving_metric: 'displacement', driving_metric_value: 1.5 };
      applyAutoResolveIteration(mismatchedIter);
      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringContaining('driving_metric mismatch'),
        expect.objectContaining({
          iteration: 2,
          canonical: 'max_von_mises',
          received: 'displacement',
        }),
      );
      expect(state.autoResolve.iterations).toHaveLength(1);
      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(6) endAutoResolveLoop + beginAutoResolveLoop resets canonical so a new loop adopts a different driving_metric', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration, endAutoResolveLoop } = createEngineStore();
      beginAutoResolveLoop();
      const iterA = { ...sampleIteration, driving_metric: 'A', driving_metric_value: 1 };
      applyAutoResolveIteration(iterA);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('A');
      expect(state.autoResolve.iterations).toHaveLength(1);

      endAutoResolveLoop();
      beginAutoResolveLoop();
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();

      const iterB = { ...sampleIteration, driving_metric: 'B', driving_metric_value: 1 };
      applyAutoResolveIteration(iterB);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('B');
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.iterations[0].driving_metric).toBe('B');
      dispose();
    });
  });

  it('(7) a second beginAutoResolveLoop without endAutoResolveLoop in between still clears canonical', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      const iterA = { ...sampleIteration, driving_metric: 'A', driving_metric_value: 1 };
      applyAutoResolveIteration(iterA);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('A');

      // Fresh begin without an explicit end — must still clear canonical.
      beginAutoResolveLoop();
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();
      expect(state.autoResolve.iterations).toHaveLength(0);

      const iterB = { ...sampleIteration, driving_metric: 'B', driving_metric_value: 1 };
      applyAutoResolveIteration(iterB);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('B');
      expect(state.autoResolve.iterations).toHaveLength(1);
      expect(state.autoResolve.iterations[0].driving_metric).toBe('B');
      dispose();
    });
  });
});

describe('engineStore autoResolve empty-string driving_metric', () => {
  const sampleIteration = {
    iteration: 1,
    parameters: {
      'Bracket.thickness': { value: 4.2, unit: 'mm', display: '4.2mm' },
    },
    constraints: {
      max_von_mises: {
        name: 'max_von_mises',
        value: 180,
        unit: 'MPa',
        target_upper: 200,
        satisfied: true,
      },
    },
    driving_metric: 'max_von_mises',
    driving_metric_value: 180,
  };

  it('(a) empty-string driving_metric after canonical established: appended, canonical unchanged, dedicated warn fires', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      applyAutoResolveIteration(sampleIteration);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2, driving_metric: '' });
      // Iteration is still appended (not dropped)
      expect(state.autoResolve.iterations).toHaveLength(2);
      // Canonical is unchanged
      expect(state.autoResolve.canonicalDrivingMetric).toBe('max_von_mises');
      // Dedicated empty-string warn fires (not the mismatch warn)
      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringContaining('empty driving_metric'),
        expect.objectContaining({ iteration: 2 }),
      );
      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(b) empty-string driving_metric as first iteration: appended, canonical stays undefined, warn fires', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: '' });
      // Iteration is appended
      expect(state.autoResolve.iterations).toHaveLength(1);
      // canonical must NOT be set to ''
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();
      // empty-string warn fires
      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringContaining('empty driving_metric'),
        expect.objectContaining({ iteration: 1 }),
      );
      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(c) real-metric iteration following empty-string iteration still establishes canonical', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();
      // First: empty-string driving_metric — canonical stays undefined
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: '' });
      expect(state.autoResolve.canonicalDrivingMetric).toBeUndefined();

      // Second: real driving_metric — must be appended and establish the canonical
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2, driving_metric: 'displacement', driving_metric_value: 0.8 });
      expect(state.autoResolve.iterations).toHaveLength(2);
      expect(state.autoResolve.canonicalDrivingMetric).toBe('displacement');

      const emptyWarnCalls = warnSpy.mock.calls.filter(
        ([msg]) => typeof msg === 'string' && msg.includes('empty driving_metric'),
      );
      expect(emptyWarnCalls).toHaveLength(1);
      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(d) warn fires at most once per loop even when multiple empty-string iterations arrive', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();
      beginAutoResolveLoop();

      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      // Three consecutive empty-string iterations
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: '' });
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2, driving_metric: '' });
      applyAutoResolveIteration({ ...sampleIteration, iteration: 3, driving_metric: '' });

      expect(state.autoResolve.iterations).toHaveLength(3);
      // Warn fires exactly once, not three times
      const emptyWarnCalls = warnSpy.mock.calls.filter(([msg]) =>
        typeof msg === 'string' && msg.includes('empty driving_metric'),
      );
      expect(emptyWarnCalls).toHaveLength(1);

      warnSpy.mockRestore();
      dispose();
    });
  });

  it('(e) beginAutoResolveLoop resets warn-once flag so a new loop can warn again', () => {
    createRoot((dispose) => {
      const { state, beginAutoResolveLoop, applyAutoResolveIteration } = createEngineStore();

      // First loop: empty-string → warn fires once and flag is set
      beginAutoResolveLoop();
      const spy1 = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: '' });
      applyAutoResolveIteration({ ...sampleIteration, iteration: 2, driving_metric: '' });
      expect(spy1.mock.calls.filter(([m]) => typeof m === 'string' && m.includes('empty driving_metric'))).toHaveLength(1);
      spy1.mockRestore();

      // Second loop: beginAutoResolveLoop clears the flag — warn fires again on the first empty-string
      beginAutoResolveLoop();
      const spy2 = vi.spyOn(console, 'warn').mockImplementation(() => {});
      applyAutoResolveIteration({ ...sampleIteration, iteration: 1, driving_metric: '' });
      expect(spy2.mock.calls.filter(([m]) => typeof m === 'string' && m.includes('empty driving_metric'))).toHaveLength(1);
      spy2.mockRestore();
      dispose();
    });
  });
});

describe('engineStore kernelStatus', () => {
  it('initial state.kernelStatus is null', () => {
    createRoot((dispose) => {
      const { state } = createEngineStore();
      expect(state.kernelStatus).toBeNull();
      dispose();
    });
  });

  it('setKernelStatus updates kernelStatus and subsequent calls replace it', () => {
    createRoot((dispose) => {
      const { state, setKernelStatus } = createEngineStore();
      const degraded: KernelStatus = {
        available: false,
        message: 'Geometry kernel not available — OCCT not linked',
      };
      setKernelStatus(degraded);
      expect(state.kernelStatus).toEqual(degraded);

      const ok: KernelStatus = { available: true, message: null };
      setKernelStatus(ok);
      expect(state.kernelStatus).toEqual(ok);
      dispose();
    });
  });

  // ── T0b: tensegrityWires store fan-out ───────────────────────────────────

  it('initFromState writes tensegrity_wires from GuiState into state.tensegrityWires', () => {
    // RED until EngineState.tensegrityWires is added and initFromState sets it.
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const guiState: GuiState = {
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

      const guiState: GuiState = {
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
      };
      initFromState(guiState);
      expect((state as any).tensegrityWires).toEqual([]);
      dispose();
    });
  });
});
