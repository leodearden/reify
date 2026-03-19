import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
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
}));

import {
  onMeshUpdate,
  onValueUpdate,
  onConstraintUpdate,
  onEvaluationStatus,
} from '../bridge';
import { createEngineStore } from '../stores/engineStore';

const mockOnMeshUpdate = vi.mocked(onMeshUpdate);
const mockOnValueUpdate = vi.mocked(onValueUpdate);
const mockOnConstraintUpdate = vi.mocked(onConstraintUpdate);
const mockOnEvaluationStatus = vi.mocked(onEvaluationStatus);

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
};

const sampleConstraint: ConstraintData = {
  node_id: 'constraint_001',
  expression: 'width > 10',
  status: 'satisfied',
  details: null,
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
        details: 'too large',
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

      mockOnMeshUpdate.mockResolvedValue(unlistenMesh);
      mockOnValueUpdate.mockResolvedValue(unlistenValue);
      mockOnConstraintUpdate.mockResolvedValue(unlistenConstraint);
      mockOnEvaluationStatus.mockResolvedValue(unlistenStatus);

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

      dispose();
    });
  });
});
