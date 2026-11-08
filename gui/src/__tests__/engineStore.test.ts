import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot, createComputed } from 'solid-js';
import type {
  GuiState,
  MeshData,
  ValueData,
  ConstraintData,
  EvaluationStatus,
  DisplayDirective,
  AppearanceDirective,
} from '../types';
import type { KernelStatus } from '../bridge';

vi.mock('../bridge', async () => (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock());

import {
  mockOnMeshUpdate,
  mockOnValueUpdate,
  mockOnConstraintUpdate,
  mockOnEvaluationStatus,
  mockOnMeshRemoved,
  mockOnValueRemoved,
  mockOnConstraintRemoved,
  mockOnEvalGeneration,
  primeEngineStoreBridgeMocks,
} from './engineStoreBridgeHandles';
import { sampleMesh, sampleValue, sampleConstraint } from './engineStoreFixtures';
import { createEngineStore } from '../stores/engineStore';

beforeEach(primeEngineStoreBridgeMocks);

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
      const guiState: GuiState = { fea_convergence: null,
        meshes: [sampleMesh],
        values: [sampleValue],
        constraints: [sampleConstraint],
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

      expect(state.meshes['Bracket.body']).toEqual(sampleMesh);
      expect(state.values['cell_001']).toEqual(sampleValue);
      expect(state.constraints['constraint_001']).toEqual(sampleConstraint);
      dispose();
    });
  });

  it('initFromState plumbs display_panes into state.displayPanes', () => {
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      // (a) Fresh store has displayPanes === []
      expect(state.displayPanes).toEqual([]);

      // (b) After initFromState with a non-empty display_panes, state.displayPanes matches
      const directive: DisplayDirective = { subject: 'S#realization[0]', pane: 1 };
      const guiState: GuiState = { fea_convergence: null,
        meshes: [],
        values: [],
        constraints: [],
        files: [],
        tessellation_diagnostics: [],
        compile_diagnostics: [],
        tensegrity_wires: [],
        tensegrity_surfaces: [],
        display_panes: [directive],
        display_appearance: [],
        fea_diagnostics: []
      };
      initFromState(guiState);
      expect(state.displayPanes).toEqual([{ subject: 'S#realization[0]', pane: 1 }]);
      dispose();
    });
  });

  it('initFromState plumbs display_appearance into state.displayAppearance (step 11 RED)', () => {
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();

      // (a) Fresh store has displayAppearance === []
      expect((state as any).displayAppearance).toEqual([]);

      // (b) After initFromState with a non-empty display_appearance, state.displayAppearance matches
      const styleDirective: AppearanceDirective = {
        subject: 'S#realization[0]',
        style: { color: [0.5, 0.3, 0.1, 0.8], finish: 2, opacity: 0.8, wireframe: false },
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
        display_appearance: [styleDirective],
        fea_diagnostics: [],
      };
      initFromState(guiState);
      expect((state as any).displayAppearance).toEqual([styleDirective]);

      // (c) GuiState with display_appearance omitted yields []
      const guiStateNoAppearance: GuiState = { fea_convergence: null,
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
      initFromState(guiStateNoAppearance);
      expect((state as any).displayAppearance).toEqual([]);

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

  describe('applyMeshUpdate typed-array ownership', () => {
    // applyMeshUpdate swaps in an update's typed arrays and never writes into
    // the arrays the entity held before. meshManager.sync relies on this: it
    // skips rebuilding the edges overlay when an entity's arrays are the
    // identical objects it last built from.
    function meshWithFreshArrays(vertices: number[], displaced?: number[]): MeshData {
      return {
        entity_path: 'Bracket.body',
        vertices: new Float32Array(vertices),
        indices: new Uint32Array([0, 1, 2]),
        normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
        ...(displaced ? { displaced_positions: new Float32Array(displaced) } : {}),
      };
    }

    it('swaps in an update\'s new arrays and leaves the previous arrays untouched', () => {
      createRoot((dispose) => {
        const { state, applyMeshUpdate } = createEngineStore();
        const first = meshWithFreshArrays([0, 0, 0, 1, 0, 0, 0, 1, 0], [0, 0, 1, 1, 0, 1, 0, 1, 1]);
        applyMeshUpdate(first);
        const held = state.meshes['Bracket.body'];
        const oldVertices = held.vertices;
        const oldIndices = held.indices;
        const oldDisplaced = held.displaced_positions!;

        const second = meshWithFreshArrays([5, 5, 5, 6, 5, 5, 5, 6, 5], [5, 5, 6, 6, 5, 6, 5, 6, 6]);
        applyMeshUpdate(second);

        const now = state.meshes['Bracket.body'];
        expect(now.vertices).toBe(second.vertices);
        expect(now.indices).toBe(second.indices);
        expect(now.displaced_positions).toBe(second.displaced_positions);
        expect([...oldVertices]).toEqual([0, 0, 0, 1, 0, 0, 0, 1, 0]);
        expect([...oldIndices]).toEqual([0, 1, 2]);
        expect([...oldDisplaced]).toEqual([0, 0, 1, 1, 0, 1, 0, 1, 1]);
        dispose();
      });
    });

    it('keeps the very same arrays when an update re-sends them', () => {
      createRoot((dispose) => {
        const { state, applyMeshUpdate } = createEngineStore();
        const first = meshWithFreshArrays([0, 0, 0, 1, 0, 0, 0, 1, 0]);
        applyMeshUpdate(first);

        applyMeshUpdate({ ...first, normals: new Float32Array(9) });

        expect(state.meshes['Bracket.body'].vertices).toBe(first.vertices);
        expect(state.meshes['Bracket.body'].indices).toBe(first.indices);
        dispose();
      });
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
      const unlistenEvalGeneration = vi.fn();

      mockOnMeshUpdate.mockResolvedValue(unlistenMesh);
      mockOnValueUpdate.mockResolvedValue(unlistenValue);
      mockOnConstraintUpdate.mockResolvedValue(unlistenConstraint);
      mockOnEvaluationStatus.mockResolvedValue(unlistenStatus);
      mockOnMeshRemoved.mockResolvedValue(unlistenMeshRemoved);
      mockOnValueRemoved.mockResolvedValue(unlistenValueRemoved);
      mockOnConstraintRemoved.mockResolvedValue(unlistenConstraintRemoved);
      mockOnEvalGeneration.mockResolvedValue(unlistenEvalGeneration);

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
      expect(unlistenEvalGeneration).toHaveBeenCalled();

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
      const guiState: GuiState = { fea_convergence: null,
        meshes: [sampleMesh],
        values: [sampleValue],
        constraints: [sampleConstraint],
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
      expect(spy).toHaveBeenCalledTimes(1);
      dispose();
    });
  });

  it('onEngineReinitialized fires once per initFromState invocation across multiple loads', () => {
    createRoot((dispose) => {
      const spy = vi.fn();
      const { initFromState } = createEngineStore({ onEngineReinitialized: spy });
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
      initFromState(guiState);
      initFromState(guiState);
      expect(spy).toHaveBeenCalledTimes(3);
      dispose();
    });
  });

  it('createEngineStore works without onEngineReinitialized (option is optional)', () => {
    createRoot((dispose) => {
      const { state, initFromState } = createEngineStore();
      const guiState: GuiState = { fea_convergence: null,
        meshes: [sampleMesh],
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
      // Must not throw when the callback is omitted.
      expect(() => initFromState(guiState)).not.toThrow();
      expect(state.meshes['Bracket.body']).toEqual(sampleMesh);
      dispose();
    });
  });

  it('onEntityRemoved callback fires for event-driven removals via subscribeToEvents', async () => {
    await createRoot(async (dispose) => {
      const spy = vi.fn();

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
      const guiState: GuiState = { fea_convergence: null,
        meshes: [],
        values: [failedValue],
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
});
