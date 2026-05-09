import { batch } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import type {
  MeshData,
  ValueData,
  ConstraintData,
  EvaluationStatus,
  GuiState,
  DiagnosticInfo,
} from '../types';
import {
  onMeshUpdate,
  onValueUpdate,
  onConstraintUpdate,
  onEvaluationStatus,
  onMeshRemoved,
  onValueRemoved,
  onConstraintRemoved,
  onTessellationDiagnostics,
} from '../bridge';
import type { KernelStatus } from '../bridge';

export interface EngineState {
  meshes: Record<string, MeshData>;
  values: Record<string, ValueData>;
  constraints: Record<string, ConstraintData>;
  evalStatus: EvaluationStatus;
  tessellationDiagnostics: DiagnosticInfo[];
  kernelStatus: KernelStatus | null;
}

export interface EngineStoreOptions {
  onEntityRemoved?: (id: string) => void;
  // Fires after `initFromState` writes new state. Needed because `initFromState`
  // does not move `evalStatus.phase`, so phase-transition listeners do not
  // observe a file load — derived data (entity tree, mechanisms) would go stale.
  onEngineReinitialized?: () => void;
}

export function createEngineStore(options?: EngineStoreOptions) {
  const [state, setState] = createStore<EngineState>({
    meshes: {},
    values: {},
    constraints: {},
    evalStatus: { phase: 'idle' },
    tessellationDiagnostics: [],
    kernelStatus: null,
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

    setState({ meshes, values, constraints, tessellationDiagnostics: guiState.tessellation_diagnostics });
    options?.onEngineReinitialized?.();
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
    setState(produce((s) => { delete s.meshes[entityPath]; }));
    options?.onEntityRemoved?.(entityPath);
  }

  function removeValue(cellId: string) {
    setState(produce((s) => { delete s.values[cellId]; }));
    options?.onEntityRemoved?.(cellId);
  }

  function removeConstraint(nodeId: string) {
    setState(produce((s) => { delete s.constraints[nodeId]; }));
    options?.onEntityRemoved?.(nodeId);
  }

  function setEvalStatus(status: EvaluationStatus) {
    setState('evalStatus', status);
  }

  function setTessellationDiagnostics(diags: DiagnosticInfo[]) {
    setState('tessellationDiagnostics', diags);
  }

  function setKernelStatus(status: KernelStatus | null) {
    setState('kernelStatus', status);
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
      onTessellationDiagnostics(setTessellationDiagnostics),
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
    setTessellationDiagnostics,
    setKernelStatus,
    subscribeToEvents,
  };
}
