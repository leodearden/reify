import { batch } from 'solid-js';
import { createStore } from 'solid-js/store';
import type {
  MeshData,
  ValueData,
  ConstraintData,
  EvaluationStatus,
  GuiState,
} from '../types';
import {
  onMeshUpdate,
  onValueUpdate,
  onConstraintUpdate,
  onEvaluationStatus,
  onMeshRemoved,
  onValueRemoved,
  onConstraintRemoved,
} from '../bridge';

export interface EngineState {
  meshes: Record<string, MeshData>;
  values: Record<string, ValueData>;
  constraints: Record<string, ConstraintData>;
  evalStatus: EvaluationStatus;
}

export function createEngineStore() {
  const [state, setState] = createStore<EngineState>({
    meshes: {},
    values: {},
    constraints: {},
    evalStatus: { phase: 'idle' },
  });

  function initFromState(guiState: GuiState) {
    const meshes: Record<string, MeshData> = {};
    for (const m of guiState.meshes) {
      meshes[m.entity_path] = m;
    }

    const values: Record<string, ValueData> = {};
    for (const v of guiState.values) {
      values[v.cell_id] = v;
    }

    const constraints: Record<string, ConstraintData> = {};
    for (const c of guiState.constraints) {
      constraints[c.node_id] = c;
    }

    setState({ meshes, values, constraints });
  }

  function applyMeshUpdate(mesh: MeshData) {
    setState('meshes', mesh.entity_path, mesh);
  }

  function applyValueUpdates(updates: ValueData[]) {
    batch(() => {
      for (const v of updates) {
        setState('values', v.cell_id, v);
      }
    });
  }

  function applyConstraintUpdates(updates: ConstraintData[]) {
    batch(() => {
      for (const c of updates) {
        setState('constraints', c.node_id, c);
      }
    });
  }

  function removeMesh(entityPath: string) {
    setState('meshes', entityPath, undefined!);
  }

  function removeValue(cellId: string) {
    setState('values', cellId, undefined!);
  }

  function removeConstraint(nodeId: string) {
    setState('constraints', nodeId, undefined!);
  }

  function setEvalStatus(status: EvaluationStatus) {
    setState('evalStatus', status);
  }

  async function subscribeToEvents(): Promise<() => void> {
    const results = await Promise.allSettled([
      onMeshUpdate(applyMeshUpdate),
      onValueUpdate((v) => applyValueUpdates([v])),
      onConstraintUpdate((c) => applyConstraintUpdates([c])),
      onEvaluationStatus(setEvalStatus),
      onMeshRemoved(removeMesh),
      onValueRemoved(removeValue),
      onConstraintRemoved(removeConstraint),
    ]);

    const unlisteners: (() => void)[] = [];
    for (const result of results) {
      if (result.status === 'fulfilled') {
        unlisteners.push(result.value);
      } else {
        console.warn('Failed to subscribe to event:', result.reason);
      }
    }

    return () => {
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }

  return {
    state,
    initFromState,
    applyMeshUpdate,
    applyValueUpdates,
    applyConstraintUpdates,
    removeMesh,
    removeValue,
    removeConstraint,
    setEvalStatus,
    subscribeToEvents,
  };
}
