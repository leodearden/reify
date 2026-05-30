import { batch } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import type {
  MeshData,
  ValueData,
  ConstraintData,
  EvaluationStatus,
  GuiState,
  DiagnosticInfo,
  AutoResolveIteration,
  TensegrityWireData,
  SolverProgress,
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
  onCompileDiagnostics,
  onAutoResolveStart,
  onAutoResolveIteration,
  onAutoResolveComplete,
  onSolverProgress,
  cancelSolve as bridgeCancelSolve,
} from '../bridge';
import type { KernelStatus } from '../bridge';

/** State for an in-flight FEA CG solver (shown as overlay while solve is active >1s). */
export interface SolverProgressState {
  latest: SolverProgress | null;
  trace: SolverProgress[];
  visible: boolean;
  coarseReached: boolean;
}

/** State for an auto-resolve loop (param x = auto optimisation). */
export interface AutoResolveLoopState {
  active: boolean;
  iterations: AutoResolveIteration[];
  /**
   * Canonical driving metric for the current loop — cached on first acceptance of
   * an iteration that declares one. Read by `applyAutoResolveIteration` in O(1)
   * rather than scanning `iterations`. Cleared by `beginAutoResolveLoop` and
   * `endAutoResolveLoop` so each loop starts with a fresh canonical.
   */
  canonicalDrivingMetric?: string;
  /**
   * Whether an empty-string `driving_metric` warning has already been emitted
   * for the current loop. Used to rate-limit the warn to once-per-loop so a
   * misconfigured producer that floods empty-string iterations does not flood
   * the dev console. Cleared by `beginAutoResolveLoop` and `endAutoResolveLoop`.
   */
  warnedEmptyMetric?: boolean;
}

export interface EngineState {
  meshes: Record<string, MeshData>;
  values: Record<string, ValueData>;
  constraints: Record<string, ConstraintData>;
  evalStatus: EvaluationStatus;
  tessellationDiagnostics: DiagnosticInfo[];
  compileDiagnostics: DiagnosticInfo[];
  kernelStatus: KernelStatus | null;
  autoResolve: AutoResolveLoopState;
  /** Tensegrity wire endpoint pairs with member-type tags (T0b). Empty when none present. */
  tensegrityWires: TensegrityWireData[];
  solverProgress: SolverProgressState;
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
    compileDiagnostics: [],
    kernelStatus: null,
    autoResolve: { active: false, iterations: [], canonicalDrivingMetric: undefined, warnedEmptyMetric: undefined },
    tensegrityWires: [],
    solverProgress: { latest: null, trace: [], visible: false, coarseReached: false },
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

    setState({ meshes, values, constraints, tessellationDiagnostics: guiState.tessellation_diagnostics, compileDiagnostics: guiState.compile_diagnostics, tensegrityWires: guiState.tensegrity_wires });
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

  function resetSolverProgress() {
    if (debounceHandle !== null) {
      clearTimeout(debounceHandle);
      debounceHandle = null;
    }
    setState('solverProgress', { latest: null, trace: [], visible: false, coarseReached: false });
  }

  function setEvalStatus(status: EvaluationStatus) {
    setState('evalStatus', status);
    // Reset solver progress on any phase that is not an active solve phase.
    // EvaluationStatus.phase is currently 'idle' | 'evaluating' | 'resolving';
    // the active-solve phases are 'evaluating' and 'resolving'. Using a negative
    // guard (rather than === 'idle') means any future terminal phase added to the
    // union will automatically trigger a reset without needing a code change here.
    if (status.phase !== 'evaluating' && status.phase !== 'resolving') {
      resetSolverProgress();
    }
  }

  function setTessellationDiagnostics(diags: DiagnosticInfo[]) {
    setState('tessellationDiagnostics', diags);
  }

  function setCompileDiagnostics(diags: DiagnosticInfo[]) {
    setState('compileDiagnostics', diags);
  }

  function setKernelStatus(status: KernelStatus | null) {
    setState('kernelStatus', status);
  }

  /** Start a new auto-resolve loop: flip active=true and clear previous iterations. */
  function beginAutoResolveLoop() {
    setState('autoResolve', { active: true, iterations: [], canonicalDrivingMetric: undefined, warnedEmptyMetric: undefined });
  }

  /**
   * Append one iteration snapshot to the accumulating iterations array.
   *
   * Enforces the AutoResolveIteration invariant: `driving_metric` must be
   * consistent across all iterations in a single loop. If the incoming
   * iteration's `driving_metric` conflicts with the canonical metric, the
   * iteration is dropped with a `console.warn` rather than corrupting the
   * chart data series.
   *
   * The canonical metric is cached in `state.autoResolve.canonicalDrivingMetric`
   * (O(1) read) rather than re-scanning `iterations` on every call (see
   * AutoResolveIteration invariant in types.ts).
   */
  function applyAutoResolveIteration(iter: AutoResolveIteration) {
    // Empty-string driving_metric is treated as "no metric declared" — same as
    // undefined — but emits a console.warn (once per loop) so the upstream
    // malformation (the wire schema permits omission, not empty-string) is
    // visible in dev without flooding the console when a misconfigured producer
    // emits many such iterations in a single loop.
    if (iter.driving_metric === '' && !state.autoResolve.warnedEmptyMetric) {
      console.warn('[auto-resolve-iteration] empty driving_metric; treating as undeclared', {
        iteration: iter.iteration,
      });
      setState('autoResolve', 'warnedEmptyMetric', true);
    }
    const metric = iter.driving_metric === '' ? undefined : iter.driving_metric;
    const canonical = state.autoResolve.canonicalDrivingMetric;
    if (canonical && metric && metric !== canonical) {
      console.warn('[auto-resolve-iteration] driving_metric mismatch; dropping iteration', {
        iteration: iter.iteration,
        canonical,
        received: iter.driving_metric,
      });
      return;
    }
    setState(produce((s) => { s.autoResolve.iterations.push(iter); }));
    // Establish canonical on first iteration that declares a driving_metric.
    if (metric && !canonical) {
      setState('autoResolve', 'canonicalDrivingMetric', metric);
    }
  }

  /**
   * Mark the loop as finished and reset iteration history.
   *
   * The panel unmounts when `active` flips to false (App.tsx uses
   * `<Show when={autoResolve.active}>`), so any preserved iterations would be
   * unreachable until the next `beginAutoResolveLoop` clears them anyway.
   * Clearing eagerly avoids holding dead state between runs.
   */
  function endAutoResolveLoop() {
    setState('autoResolve', { active: false, iterations: [], canonicalDrivingMetric: undefined, warnedEmptyMetric: undefined });
  }

  let debounceHandle: ReturnType<typeof setTimeout> | null = null;

  // Maximum trace entries stored for the convergence chart. Matches the chart
  // pixel width (200px) so we never hold more history than is renderable, and
  // per-render cost of buildPolylinePoints stays bounded even for long CG runs.
  const TRACE_CAP = 200;

  function applySolverProgress(p: SolverProgress) {
    setState(produce((s) => {
      s.solverProgress.latest = p;
      s.solverProgress.trace.push(p);
      // Evict the oldest entry once we exceed the cap (sliding window).
      if (s.solverProgress.trace.length > TRACE_CAP) {
        s.solverProgress.trace.shift();
      }
      if (!s.solverProgress.coarseReached && p.residual < 1e-2) {
        s.solverProgress.coarseReached = true;
      }
    }));
    if (debounceHandle === null && !state.solverProgress.visible) {
      debounceHandle = setTimeout(() => {
        setState('solverProgress', 'visible', true);
        debounceHandle = null;
      }, 1000);
    }
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
      onCompileDiagnostics(setCompileDiagnostics),
      onAutoResolveStart(beginAutoResolveLoop),
      onAutoResolveIteration(applyAutoResolveIteration),
      onAutoResolveComplete(endAutoResolveLoop),
      onSolverProgress(applySolverProgress),
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
      // Clear any pending debounce timer first so it cannot fire after teardown
      // and call setState on a disposed store.
      resetSolverProgress();
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
    setCompileDiagnostics,
    setKernelStatus,
    beginAutoResolveLoop,
    applyAutoResolveIteration,
    endAutoResolveLoop,
    applySolverProgress,
    resetSolverProgress,
    async cancelSolve() {
      await bridgeCancelSolve();
      resetSolverProgress();
    },
    subscribeToEvents,
  };
}
