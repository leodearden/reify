import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import type { MeshData, ValueData, ConstraintData, EntityTreeNode } from '../types';

vi.mock('../bridge', async () => (await import('./engineStoreBridgeMockFactory')).engineStoreBridgeMock());

import { primeEngineStoreBridgeMocks } from './engineStoreBridgeHandles';
import { createEngineStore } from '../stores/engineStore';

beforeEach(primeEngineStoreBridgeMocks);

// ── reconcileToTree ──────────────────────────────────────────────────────────
// Helper to build a minimal EntityTreeNode with no children.
function makeTreeNode(entity_path: string, kind = 'structure'): EntityTreeNode {
  return {
    entity_path,
    kind,
    type_name: null,
    has_mesh: true,
    trait_geometry: false,
    freshness: 'final',
    children: [],
  };
}

describe('engineStore reconcileToTree — empty-tree no-op (step-5)', () => {
  it('reconcileToTree([]) is a strict no-op: nothing pruned, onEntityRemoved never called', () => {
    createRoot((dispose) => {
      const spy = vi.fn();
      const store = createEngineStore({ onEntityRemoved: spy });

      // Seed meshes, values, and constraints
      const mesh: MeshData = {
        entity_path: 'Bracket#realization0',
        vertices: new Float32Array([0, 1, 2]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      };
      const value: ValueData = {
        cell_id: 'cell_a',
        name: 'width',
        value: '50.0',
        unit: 'mm',
        determinacy: 'determined',
        entity_path: 'Bracket',
        kind: 'Param',
        freshness: 'final',
      };
      const constraint: ConstraintData = {
        node_id: 'Bracket#constraint0',
        expression: 'width > 0',
        status: 'satisfied',
        label: null,
        parameter_ids: ['cell_a'],
      };

      store.applyMeshUpdate(mesh);
      store.applyValueUpdates([value]);
      store.applyConstraintUpdates([constraint]);

      // Call with empty tree — should be a strict no-op
      store.reconcileToTree([]);

      // Nothing pruned
      expect(store.state.meshes['Bracket#realization0']).toBeDefined();
      expect(store.state.values['cell_a']).toBeDefined();
      expect(store.state.constraints['Bracket#constraint0']).toBeDefined();
      // onEntityRemoved never called
      expect(spy).not.toHaveBeenCalled();

      dispose();
    });
  });
});

describe('engineStore reconcileToTree — value and constraint pruning (step-3)', () => {
  it('reconcileToTree prunes orphan value and orphan constraint while keeping live ones', () => {
    createRoot((dispose) => {
      const spy = vi.fn();
      const store = createEngineStore({ onEntityRemoved: spy });

      // Live value: entity_path 'CapstanDrive.shuttle.plate' is in the tree
      const liveValue: ValueData = {
        cell_id: 'cell_live',
        name: 'thickness',
        value: '3.0',
        unit: 'mm',
        determinacy: 'determined',
        entity_path: 'CapstanDrive.shuttle.plate',
        kind: 'Param',
        freshness: 'final',
      };
      // Orphan value: entity_path 'ShuttlePlates' is NOT in the tree
      const orphanValue: ValueData = {
        cell_id: 'cell_orphan',
        name: 'width',
        value: '10.0',
        unit: 'mm',
        determinacy: 'determined',
        entity_path: 'ShuttlePlates',
        kind: 'Param',
        freshness: 'final',
      };

      // Live constraint: node_id owner (before '#') = 'CapstanDrive.shuttle.plate' is in tree
      const liveConstraint: ConstraintData = {
        node_id: 'CapstanDrive.shuttle.plate#constraint0',
        expression: 'thickness > 1',
        status: 'satisfied',
        label: null,
        parameter_ids: ['cell_live'],
      };
      // Orphan constraint: node_id owner = 'ShuttlePlates' is NOT in tree
      const orphanConstraint: ConstraintData = {
        node_id: 'ShuttlePlates#constraint0',
        expression: 'width > 5',
        status: 'satisfied',
        label: null,
        parameter_ids: ['cell_orphan'],
      };

      store.applyValueUpdates([liveValue, orphanValue]);
      store.applyConstraintUpdates([liveConstraint, orphanConstraint]);
      expect(Object.keys(store.state.values)).toHaveLength(2);
      expect(Object.keys(store.state.constraints)).toHaveLength(2);

      // Tree contains only the live structure
      const tree: EntityTreeNode[] = [makeTreeNode('CapstanDrive.shuttle.plate')];
      store.reconcileToTree(tree);

      // Orphan value pruned, live value retained
      expect(store.state.values['cell_orphan']).toBeUndefined();
      expect(store.state.values['cell_live']).toBeDefined();
      // Orphan constraint pruned, live constraint retained
      expect(store.state.constraints['ShuttlePlates#constraint0']).toBeUndefined();
      expect(store.state.constraints['CapstanDrive.shuttle.plate#constraint0']).toBeDefined();
      // onEntityRemoved fired for both orphans
      expect(spy).toHaveBeenCalledWith('cell_orphan');
      expect(spy).toHaveBeenCalledWith('ShuttlePlates#constraint0');
      expect(spy).toHaveBeenCalledTimes(2);

      dispose();
    });
  });
});

describe('engineStore reconcileToTree — mesh pruning (step-1)', () => {
  it('reconcileToTree removes orphan mesh and retains live mesh, fires onEntityRemoved for orphan', () => {
    createRoot((dispose) => {
      const spy = vi.fn();
      const store = createEngineStore({ onEntityRemoved: spy });

      // Seed: one live mesh (owner = CapstanDrive.shuttle.plate), one orphan (owner = ShuttlePlates)
      const liveMeshKey = 'CapstanDrive.shuttle.plate#realization0';
      const orphanMeshKey = 'ShuttlePlates#realization0';

      const liveMesh: MeshData = {
        entity_path: liveMeshKey,
        vertices: new Float32Array([0, 1, 2]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      };
      const orphanMesh: MeshData = {
        entity_path: orphanMeshKey,
        vertices: new Float32Array([3, 4, 5]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      };

      store.applyMeshUpdate(liveMesh);
      store.applyMeshUpdate(orphanMesh);
      expect(Object.keys(store.state.meshes)).toHaveLength(2);

      // Tree contains only the live structure node (CapstanDrive.shuttle.plate)
      const tree: EntityTreeNode[] = [makeTreeNode('CapstanDrive.shuttle.plate')];

      store.reconcileToTree(tree);

      // Orphan mesh pruned
      expect(store.state.meshes[orphanMeshKey]).toBeUndefined();
      // Live mesh retained
      expect(store.state.meshes[liveMeshKey]).toBeDefined();
      // onEntityRemoved fired with orphan key
      expect(spy).toHaveBeenCalledWith(orphanMeshKey);
      expect(spy).toHaveBeenCalledTimes(1);

      dispose();
    });
  });

  it('reconcileToTree retains a mesh/value whose owner is a deep child in the tree', () => {
    // Exercises the recursive collectPaths branch — previously all test trees
    // were flat (no children), so a regression that stopped recursing would
    // silently prune all nested-entity meshes/values/constraints.
    createRoot((dispose) => {
      const spy = vi.fn();
      const store = createEngineStore({ onEntityRemoved: spy });

      // Live mesh: owner = 'CapstanDrive.shuttle.plate', a DEEP child in tree
      const liveMeshKey = 'CapstanDrive.shuttle.plate#realization0';
      const liveMesh: MeshData = {
        entity_path: liveMeshKey,
        vertices: new Float32Array([0, 1, 2]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      };

      // Orphan mesh: owner = 'ShuttlePlates', NOT present anywhere in tree
      const orphanMeshKey = 'ShuttlePlates#realization0';
      const orphanMesh: MeshData = {
        entity_path: orphanMeshKey,
        vertices: new Float32Array([3, 4, 5]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      };

      // Live value: entity_path = 'CapstanDrive.shuttle.plate', same deep child
      const liveValue: ValueData = {
        cell_id: 'cell_nested',
        name: 'thickness',
        value: '3.0',
        unit: 'mm',
        determinacy: 'determined',
        entity_path: 'CapstanDrive.shuttle.plate',
        kind: 'Param',
        freshness: 'final',
      };

      store.applyMeshUpdate(liveMesh);
      store.applyMeshUpdate(orphanMesh);
      store.applyValueUpdates([liveValue]);

      // Nested tree: CapstanDrive → CapstanDrive.shuttle → CapstanDrive.shuttle.plate
      const tree: EntityTreeNode[] = [
        {
          entity_path: 'CapstanDrive',
          kind: 'structure',
          type_name: null,
          has_mesh: false,
          trait_geometry: false,
          freshness: 'final',
          children: [
            {
              entity_path: 'CapstanDrive.shuttle',
              kind: 'structure',
              type_name: null,
              has_mesh: false,
              trait_geometry: false,
              freshness: 'final',
              children: [
                makeTreeNode('CapstanDrive.shuttle.plate'),
              ],
            },
          ],
        },
      ];

      store.reconcileToTree(tree);

      // Live entities retained (deep child path was collected via recursion)
      expect(store.state.meshes[liveMeshKey]).toBeDefined();
      expect(store.state.values['cell_nested']).toBeDefined();

      // Orphan mesh pruned
      expect(store.state.meshes[orphanMeshKey]).toBeUndefined();
      expect(spy).toHaveBeenCalledWith(orphanMeshKey);
      expect(spy).toHaveBeenCalledTimes(1);

      dispose();
    });
  });
});

// ── reconcileToTree — cross-root snapshot guard (task 4251) ──────────────────
describe('engineStore reconcileToTree — cross-root snapshot guard (task 4251)', () => {
  it('Test A: fully-disjoint snapshot from a different design root does not prune the current design meshes', () => {
    // If the store holds meshes for CapstanDrive.* but the snapshot only lists
    // PrinterFrame.* nodes, the snapshot is stale/cross-root and pruning must be skipped.
    createRoot((dispose) => {
      const spy = vi.fn();
      const store = createEngineStore({ onEntityRemoved: spy });

      const meshA: MeshData = {
        entity_path: 'CapstanDrive.capstan#realization0',
        vertices: new Float32Array([0, 1, 2]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      };
      const meshB: MeshData = {
        entity_path: 'CapstanDrive.shuttle.plate#realization0',
        vertices: new Float32Array([3, 4, 5]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      };

      store.applyMeshUpdate(meshA);
      store.applyMeshUpdate(meshB);
      expect(Object.keys(store.state.meshes)).toHaveLength(2);

      // Stale snapshot from a different design root — shares NO paths with the current design
      store.reconcileToTree([
        makeTreeNode('PrinterFrame.gantry'),
        makeTreeNode('PrinterFrame.bed'),
      ]);

      // Both current-design meshes must be retained — cross-root guard must have blocked pruning
      expect(store.state.meshes['CapstanDrive.capstan#realization0']).toBeDefined();
      expect(store.state.meshes['CapstanDrive.shuttle.plate#realization0']).toBeDefined();
      // onEntityRemoved must NOT have been called at all
      expect(spy).not.toHaveBeenCalled();

      dispose();
    });
  });

  it('Test B (boundary pin): partial-overlap snapshot still prunes the non-overlapping orphan', () => {
    // When at least ONE store entity overlaps the snapshot, the cross-root guard must NOT
    // engage, so legitimate single-orphan pruning still fires.
    createRoot((dispose) => {
      const spy = vi.fn();
      const store = createEngineStore({ onEntityRemoved: spy });

      // Live mesh: owner 'CapstanDrive.shuttle.plate' IS in the snapshot
      const liveMesh: MeshData = {
        entity_path: 'CapstanDrive.shuttle.plate#realization0',
        vertices: new Float32Array([0, 1, 2]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      };
      // Orphan mesh: owner 'ShuttlePlates' is NOT in the snapshot
      const orphanMesh: MeshData = {
        entity_path: 'ShuttlePlates#realization0',
        vertices: new Float32Array([3, 4, 5]),
        indices: new Uint32Array([0, 1, 2]),
        normals: null,
      };

      store.applyMeshUpdate(liveMesh);
      store.applyMeshUpdate(orphanMesh);
      expect(Object.keys(store.state.meshes)).toHaveLength(2);

      // Partial-overlap snapshot: live mesh owner is present, orphan owner is absent
      store.reconcileToTree([makeTreeNode('CapstanDrive.shuttle.plate')]);

      // Orphan must be pruned (overlap exists, so cross-root guard does NOT engage)
      expect(store.state.meshes['ShuttlePlates#realization0']).toBeUndefined();
      // Live mesh must be retained
      expect(store.state.meshes['CapstanDrive.shuttle.plate#realization0']).toBeDefined();
      // onEntityRemoved fired for the orphan only
      expect(spy).toHaveBeenCalledWith('ShuttlePlates#realization0');
      expect(spy).toHaveBeenCalledTimes(1);

      dispose();
    });
  });
});
