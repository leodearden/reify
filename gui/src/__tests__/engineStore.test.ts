import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot, createComputed } from 'solid-js';
import type {
  GuiState,
  MeshData,
  ValueData,
  ConstraintData,
  EvaluationStatus,
} from '../types';

// Mock the bridge module
vi.mock('../bridge', () => ({
  onMeshUpdate: vi.fn(),
  onValueUpdate: vi.fn(),
  onConstraintUpdate: vi.fn(),
  onEvaluationStatus: vi.fn(),
  onMeshRemoved: vi.fn(),
  onValueRemoved: vi.fn(),
  onConstraintRemoved: vi.fn(),
}));

import {
  onMeshUpdate,
  onValueUpdate,
  onConstraintUpdate,
  onEvaluationStatus,
  onMeshRemoved,
  onValueRemoved,
  onConstraintRemoved,
} from '../bridge';
import { createEngineStore } from '../stores/engineStore';

const mockOnMeshUpdate = vi.mocked(onMeshUpdate);
const mockOnValueUpdate = vi.mocked(onValueUpdate);
const mockOnConstraintUpdate = vi.mocked(onConstraintUpdate);
const mockOnEvaluationStatus = vi.mocked(onEvaluationStatus);
const mockOnMeshRemoved = vi.mocked(onMeshRemoved);
const mockOnValueRemoved = vi.mocked(onValueRemoved);
const mockOnConstraintRemoved = vi.mocked(onConstraintRemoved);

beforeEach(() => {
  vi.clearAllMocks();
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
        { cell_id: 'a', name: 'a', value: '1', unit: 'mm', determinacy: 'determined', entity_path: 'X.a', kind: 'Param' },
        { cell_id: 'b', name: 'b', value: '2', unit: 'mm', determinacy: 'determined', entity_path: 'X.b', kind: 'Param' },
        { cell_id: 'c', name: 'c', value: '3', unit: 'mm', determinacy: 'determined', entity_path: 'X.c', kind: 'Param' },
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
});
