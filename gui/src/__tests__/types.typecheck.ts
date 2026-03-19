/**
 * Compile-time type check file for IPC types.
 * This file is NOT executed — it only needs to pass `tsc --noEmit`.
 * It verifies that all IPC types are correctly defined and exported.
 */
import type {
  MeshData,
  ValueData,
  ConstraintData,
  SourceLocation,
  FileData,
  GuiState,
  EvaluationStatus,
  MeshUpdate,
  ValueUpdate,
  ConstraintUpdate,
} from '../types';

// --- MeshData ---
const mesh: MeshData = {
  entity_path: 'Bracket.body',
  vertices: [0.0, 1.0, 2.0],
  indices: [0, 1, 2],
  normals: [0.0, 0.0, 1.0],
};

const meshNoNormals: MeshData = {
  entity_path: 'Bracket.body',
  vertices: [0.0, 1.0, 2.0],
  indices: [0, 1, 2],
  normals: null,
};

// --- ValueData ---
const value: ValueData = {
  cell_id: 'cell_001',
  name: 'width',
  value: '50.0',
  unit: 'mm',
  determinacy: 'determined',
  entity_path: 'Bracket.width',
};

// --- ConstraintData ---
const constraint: ConstraintData = {
  node_id: 'constraint_001',
  expression: 'width > 10',
  status: 'satisfied',
  details: null,
  parameter_ids: ['cell_001', 'cell_002'],
};

const constraintWithDetails: ConstraintData = {
  node_id: 'constraint_002',
  expression: 'height < 100',
  status: 'violated',
  details: 'height is 150, exceeds maximum of 100',
  parameter_ids: ['cell_003'],
};

// --- SourceLocation ---
const loc: SourceLocation = {
  file: 'bracket.ri',
  line: 10,
  column: 5,
  end_line: 10,
  end_column: 20,
};

// --- FileData ---
const file: FileData = {
  path: 'bracket.ri',
  content: 'structure Bracket { }',
};

// --- GuiState ---
const state: GuiState = {
  meshes: [mesh, meshNoNormals],
  values: [value],
  constraints: [constraint, constraintWithDetails],
  files: [file],
};

// --- EvaluationStatus ---
const idle: EvaluationStatus = { phase: 'idle' };
const evaluating: EvaluationStatus = { phase: 'evaluating', progress: 0.5 };
const resolving: EvaluationStatus = { phase: 'resolving' };

// --- Type aliases ---
const meshUpdate: MeshUpdate = mesh;
const valueUpdate: ValueUpdate = value;
const constraintUpdate: ConstraintUpdate = constraint;

// Suppress unused variable warnings — this file is only for type checking
void mesh;
void meshNoNormals;
void value;
void constraint;
void constraintWithDetails;
void loc;
void file;
void state;
void idle;
void evaluating;
void resolving;
void meshUpdate;
void valueUpdate;
void constraintUpdate;
