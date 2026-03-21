/**
 * Compile-time type check file for IPC types.
 * This file is NOT executed — it only needs to pass `tsc --noEmit`.
 * It verifies that all IPC types are correctly defined and exported.
 */
import type {
  MeshData,
  RawMeshData,
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
import { convertRawMesh } from '../types';

// --- MeshData (typed arrays for WebGL) ---
const mesh: MeshData = {
  entity_path: 'Bracket.body',
  vertices: new Float32Array([0.0, 1.0, 2.0]),
  indices: new Uint32Array([0, 1, 2]),
  normals: new Float32Array([0.0, 0.0, 1.0]),
};

const meshNoNormals: MeshData = {
  entity_path: 'Bracket.body',
  vertices: new Float32Array([0.0, 1.0, 2.0]),
  indices: new Uint32Array([0, 1, 2]),
  normals: null,
};

// --- RawMeshData (wire format from Tauri IPC) ---
const rawMesh: RawMeshData = {
  entity_path: 'Bracket.body',
  vertices: [0.0, 1.0, 2.0],
  indices: [0, 1, 2],
  normals: [0.0, 0.0, 1.0],
};

// --- convertRawMesh ---
const converted: MeshData = convertRawMesh(rawMesh);

// --- ValueData ---
const value: ValueData = {
  cell_id: 'cell_001',
  name: 'width',
  value: '50.0',
  unit: 'mm',
  determinacy: 'determined',
  entity_path: 'Bracket.width',
  kind: 'Param',
};

// --- ConstraintData ---
const constraint: ConstraintData = {
  node_id: 'constraint_001',
  expression: 'width > 10',
  status: 'satisfied',
  label: null,
  parameter_ids: ['cell_001', 'cell_002'],
};

const constraintWithLabel: ConstraintData = {
  node_id: 'constraint_002',
  expression: 'height < 100',
  status: 'violated',
  label: 'height is 150, exceeds maximum of 100',
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
  constraints: [constraint, constraintWithLabel],
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
void rawMesh;
void converted;
void value;
void constraint;
void constraintWithLabel;
void loc;
void file;
void state;
void idle;
void evaluating;
void resolving;
void meshUpdate;
void valueUpdate;
void constraintUpdate;
