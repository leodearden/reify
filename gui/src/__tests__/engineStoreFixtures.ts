// Pure-data sample payloads shared by the engineStore test files.
import type { MeshData, ValueData, ConstraintData } from '../types';

export const sampleMesh: MeshData = {
  entity_path: 'Bracket.body',
  vertices: new Float32Array([0, 1, 2, 3, 4, 5]),
  indices: new Uint32Array([0, 1, 2]),
  normals: new Float32Array([0, 0, 1, 0, 0, 1]),
};

export const sampleValue: ValueData = {
  cell_id: 'cell_001',
  name: 'width',
  value: '50.0',
  unit: 'mm',
  determinacy: 'determined',
  entity_path: 'Bracket.width',
  kind: 'Param',
  freshness: 'final',
};

export const sampleConstraint: ConstraintData = {
  node_id: 'constraint_001',
  expression: 'width > 10',
  status: 'satisfied',
  label: null,
  parameter_ids: ['cell_001'],
};

// One canonical auto-resolve iteration fixture, shared by the autoResolve and
// publish-generation suites. Previously each block re-declared this same 14-line literal;
// drift between the copies would have silently stopped the deferred-reset and
// driving-metric tests from exercising the canonical-metric interaction they
// are named for.
//
// Treat as READ-ONLY: tests spread it (`{ ...sampleIteration, iteration: 2 }`)
// rather than mutating it, so the single instance is safe to share across
// stores. Do not `Object.freeze` it — Solid's `createStore` defines a `$PROXY`
// property on any object it wraps, which throws on a frozen target.
export const sampleIteration = {
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
