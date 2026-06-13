import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createRoot } from 'solid-js';
import type { MeshData, ValueData, ConstraintData, EntityTreeNode } from '../types';

// Mock the bridge module. engineStore imports several event subscribers plus the
// new `syncObservedDemand` invoke wrapper; all must be present on the mock.
vi.mock('../bridge', () => ({
  onMeshUpdate: vi.fn(() => Promise.resolve(() => {})),
  onValueUpdate: vi.fn(() => Promise.resolve(() => {})),
  onConstraintUpdate: vi.fn(() => Promise.resolve(() => {})),
  onEvaluationStatus: vi.fn(() => Promise.resolve(() => {})),
  onMeshRemoved: vi.fn(() => Promise.resolve(() => {})),
  onValueRemoved: vi.fn(() => Promise.resolve(() => {})),
  onConstraintRemoved: vi.fn(() => Promise.resolve(() => {})),
  onTessellationDiagnostics: vi.fn(() => Promise.resolve(() => {})),
  onCompileDiagnostics: vi.fn(() => Promise.resolve(() => {})),
  onAutoResolveStart: vi.fn(() => Promise.resolve(() => {})),
  onAutoResolveIteration: vi.fn(() => Promise.resolve(() => {})),
  onAutoResolveComplete: vi.fn(() => Promise.resolve(() => {})),
  onSolverProgress: vi.fn(() => Promise.resolve(() => {})),
  cancelSolve: vi.fn(() => Promise.resolve()),
  // Selective-demand precondition (task 4532): passive observed-demand sync.
  syncObservedDemand: vi.fn(() => Promise.resolve()),
}));

import { syncObservedDemand as bridgeSyncObservedDemand } from '../bridge';
import { createEngineStore } from '../stores/engineStore';
import { createViewStateStore } from '../stores/viewStateStore';

const mockBridgeSync = vi.mocked(bridgeSyncObservedDemand);

// ── Fixtures ────────────────────────────────────────────────────────────────

function makeMesh(entity_path: string): MeshData {
  return {
    entity_path,
    vertices: new Float32Array([0, 1, 2]),
    indices: new Uint32Array([0, 0, 0]),
    normals: null,
  };
}

function makeValue(cell_id: string, entity_path: string): ValueData {
  return {
    cell_id,
    name: cell_id,
    value: '1',
    unit: 'mm',
    determinacy: 'determined',
    entity_path,
    kind: 'Param',
    freshness: 'final',
  };
}

function makeConstraint(node_id: string): ConstraintData {
  return { node_id, expression: 't > 0', status: 'satisfied', label: null, parameter_ids: [] };
}

function realizationNode(entity_path: string): EntityTreeNode {
  return {
    entity_path,
    kind: 'realization',
    type_name: null,
    has_mesh: true,
    // trait_geometry forces defaultVisibilityFor → 'show', so an un-overridden
    // realization is "visible" by default (deterministic baseline for part a).
    trait_geometry: true,
    freshness: 'final',
    children: [],
  };
}

function structureNode(entity_path: string, children: EntityTreeNode[]): EntityTreeNode {
  return {
    entity_path,
    kind: 'structure',
    type_name: null,
    has_mesh: true,
    trait_geometry: false,
    freshness: 'final',
    children,
  };
}

// Two geometry-producing realizations in distinct entities, each with the mesh
// key form `Entity#realization[N]` (RealizationNodeId Display, identity.rs:180).
const BODY_A_REAL = 'BodyA#realization[0]';
const BODY_B_REAL = 'BodyB#realization[0]';

function makeTree(): EntityTreeNode[] {
  return [
    structureNode('BodyA', [realizationNode(BODY_A_REAL)]),
    structureNode('BodyB', [realizationNode(BODY_B_REAL)]),
  ];
}

/** Sorted copy for order-independent array comparison. */
function sorted(xs: readonly string[]): string[] {
  return [...xs].sort();
}

beforeEach(() => {
  vi.clearAllMocks();
  mockBridgeSync.mockResolvedValue(undefined);
});

describe('engineStore.syncObservedDemand (selective-demand precondition, task 4532)', () => {
  it('(a) gathers effective-visible realization keys + displayed cells + panel constraints and invokes the command once', async () => {
    await createRoot(async (dispose) => {
      const engine = createEngineStore();
      const view = createViewStateStore();
      view.setTree(makeTree());

      // Two realization meshes + one NON-realization mesh (proves the action
      // filters mesh keys down to realization-kind entries only — the bare
      // structure key must never be sent as a realization).
      engine.applyMeshUpdate(makeMesh(BODY_A_REAL));
      engine.applyMeshUpdate(makeMesh(BODY_B_REAL));
      engine.applyMeshUpdate(makeMesh('BodyA')); // non-realization — excluded

      engine.applyValueUpdates([
        makeValue('BodyA.thickness', 'BodyA'),
        makeValue('BodyB.thickness', 'BodyB'),
      ]);
      engine.applyConstraintUpdates([makeConstraint('BodyA#constraint[0]')]);

      await engine.syncObservedDemand(view.getEffectiveVisibility);

      expect(mockBridgeSync).toHaveBeenCalledTimes(1);
      const [visibleRealizations, displayedCells, panelConstraints] = mockBridgeSync.mock.calls[0];

      // Both realizations default to 'show' → both visible. The non-realization
      // 'BodyA' mesh key is filtered out.
      expect(sorted(visibleRealizations)).toEqual(sorted([BODY_A_REAL, BODY_B_REAL]));
      expect(visibleRealizations).not.toContain('BodyA');

      expect(sorted(displayedCells)).toEqual(sorted(['BodyA.thickness', 'BodyB.thickness']));
      expect(sorted(panelConstraints)).toEqual(['BodyA#constraint[0]']);

      dispose();
    });
  });

  it("(b) excludes a 'hidden' realization while a 'ghost' realization stays included", async () => {
    await createRoot(async (dispose) => {
      const engine = createEngineStore();
      const view = createViewStateStore();
      view.setTree(makeTree());

      engine.applyMeshUpdate(makeMesh(BODY_A_REAL));
      engine.applyMeshUpdate(makeMesh(BODY_B_REAL));

      // Tri-state visibility (NOT binary 'hide'): hide A, ghost B.
      view.setVisibility(BODY_A_REAL, 'hidden');
      view.setVisibility(BODY_B_REAL, 'ghost');

      await engine.syncObservedDemand(view.getEffectiveVisibility);

      expect(mockBridgeSync).toHaveBeenCalledTimes(1);
      const visibleRealizations = mockBridgeSync.mock.calls[0][0];

      // 'hidden' → excluded; 'ghost' → still a viewport demand source (effective !== 'hidden').
      expect(visibleRealizations).not.toContain(BODY_A_REAL);
      expect(visibleRealizations).toContain(BODY_B_REAL);

      dispose();
    });
  });

  it('is best-effort: a rejected invoke is swallowed, not thrown into the caller', async () => {
    await createRoot(async (dispose) => {
      const engine = createEngineStore();
      const view = createViewStateStore();
      view.setTree(makeTree());
      engine.applyMeshUpdate(makeMesh(BODY_A_REAL));

      mockBridgeSync.mockRejectedValueOnce(new Error('ipc down'));

      // Must resolve (not reject) — observational sync never breaks the UI.
      await expect(engine.syncObservedDemand(view.getEffectiveVisibility)).resolves.toBeUndefined();

      dispose();
    });
  });
});
