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
  TensegritySurfaceData,
  SolverProgress,
  EntityTreeNode,
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
  /** Tensegrity surface facets with member-type tags (β). Empty when none present. */
  tensegritySurfaces: TensegritySurfaceData[];
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
    tensegritySurfaces: [],
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

    setState({ meshes, values, constraints, tessellationDiagnostics: guiState.tessellation_diagnostics, compileDiagnostics: guiState.compile_diagnostics, tensegrityWires: guiState.tensegrity_wires, tensegritySurfaces: guiState.tensegrity_surfaces });
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

  /**
   * Reconcile engineStore entity set against the authoritative entity tree.
   *
   * Collects all live entity paths from the tree (including children at every
   * depth), then prunes stale meshes, values, and constraints.
   *
   * Mesh pruning uses a dual check: prune a mesh key if the key itself is not
   * in livePaths AND its owner path (entity_path up to the first `#`) is not
   * in livePaths either. The direct-key check is needed because realization
   * nodes carry the full mesh key (e.g. `"Bracket#realization0"`) as their
   * entity_path (types.ts:357-360), so a mesh that is directly present in the
   * tree is never pruned even if owner-path matching would otherwise remove it.
   * Pruning is intentionally owner-granular for the cross-structure case: a
   * mesh is retained as long as its parent structure node is still live,
   * regardless of whether the specific realization node is present.
   *
   * Guards:
   *
   * 1. Empty-tree guard: if the tree yields zero live paths, returns immediately
   *    without pruning. This conflates two cases — tree not yet loaded (do
   *    nothing) and a genuinely empty design (all parts deleted). In the
   *    genuinely-empty case stale meshes/values/constraints persist until the
   *    next non-empty refresh. This is a known limitation: a reliable "tree
   *    loaded" signal would let us distinguish the two cases and reconcile the
   *    empty-design path correctly.
   *
   * 2. Cross-root / disjoint-snapshot guard: if the store currently tracks
   *    entities but NONE of them (mesh key/owner, value entity_path, or
   *    constraint owner) appear in the snapshot's live paths, the snapshot is
   *    from a different design root (stale/pre-switch) and pruning would wipe
   *    the freshly-loaded design. Return without pruning in this case.
   *    Extends the empty-tree known-limitation: a genuine full replacement to
   *    a wholly-unrelated root will not prune until the next overlapping
   *    refresh (which should arrive shortly after the new engine load).
   */
  function reconcileToTree(tree: EntityTreeNode[]): void {
    // Collect all live entity paths from the tree recursively.
    const livePaths = new Set<string>();
    function collectPaths(nodes: EntityTreeNode[]): void {
      for (const node of nodes) {
        livePaths.add(node.entity_path);
        if (node.children.length > 0) collectPaths(node.children);
      }
    }
    collectPaths(tree);

    // Guard 1: if no live paths collected, the tree is not loaded yet (or the
    // design is genuinely empty — see known limitation in the JSDoc above).
    // Either way, return without pruning to avoid a stale-tree wipe.
    if (livePaths.size === 0) return;

    // Guard 2: cross-root / disjoint-snapshot guard.  Check whether the store
    // currently has ANY entities and, if so, whether at least one of them
    // overlaps with the snapshot's live paths.  Checking across all entity
    // kinds (meshes, values, constraints) avoids a false-positive when the
    // store has only values or constraints (no meshes) — basing disjointness on
    // meshes alone would wrongly skip pruning in value/constraint-only stores.
    const hasMeshEntities = Object.keys(state.meshes).length > 0;
    const hasValueEntities = Object.keys(state.values).length > 0;
    const hasConstraintEntities = Object.keys(state.constraints).length > 0;
    if (hasMeshEntities || hasValueEntities || hasConstraintEntities) {
      let hasOverlap = false;
      // Short-circuit scan: first hit wins.
      meshScan:
      for (const key of Object.keys(state.meshes)) {
        const owner = key.includes('#') ? key.slice(0, key.indexOf('#')) : key;
        if (livePaths.has(key) || livePaths.has(owner)) { hasOverlap = true; break meshScan; }
      }
      if (!hasOverlap) {
        for (const cellId of Object.keys(state.values)) {
          const ep = state.values[cellId]?.entity_path;
          if (ep !== undefined && livePaths.has(ep)) { hasOverlap = true; break; }
        }
      }
      if (!hasOverlap) {
        for (const nodeId of Object.keys(state.constraints)) {
          const owner = nodeId.includes('#') ? nodeId.slice(0, nodeId.indexOf('#')) : nodeId;
          if (livePaths.has(owner)) { hasOverlap = true; break; }
        }
      }
      // Snapshot is from a different design root (stale/pre-switch) — skip pruning.
      // Note: this guard is defense-in-depth. The primary staleness defense is the
      // App-layer epoch guard in App.tsx (refreshEntityTree), which drops any tree
      // snapshot fetched before the latest engine (re)init regardless of overlap.
      // This guard complements it by catching fully-disjoint snapshots that reach
      // reconcileToTree through any path; it cannot handle partial-overlap staleness
      // (same-root partial snapshot), which the epoch guard handles exclusively.
      if (!hasOverlap) return;
    }

    // Prune orphan meshes.  Check the exact key first (realization nodes carry
    // it) and fall back to owner-path matching so a mesh whose parent structure
    // is still live is never incorrectly pruned.
    for (const key of Object.keys(state.meshes)) {
      const owner = key.includes('#') ? key.slice(0, key.indexOf('#')) : key;
      if (!livePaths.has(key) && !livePaths.has(owner)) {
        removeMesh(key);
      }
    }

    // Prune orphan values whose entity_path is not live.
    for (const cellId of Object.keys(state.values)) {
      const entityPath = state.values[cellId]?.entity_path;
      if (entityPath !== undefined && !livePaths.has(entityPath)) {
        removeValue(cellId);
      }
    }

    // Prune orphan constraints whose owner (node_id before the first '#') is not live.
    for (const nodeId of Object.keys(state.constraints)) {
      const owner = nodeId.includes('#') ? nodeId.slice(0, nodeId.indexOf('#')) : nodeId;
      if (!livePaths.has(owner)) {
        removeConstraint(nodeId);
      }
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
    reconcileToTree,
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
