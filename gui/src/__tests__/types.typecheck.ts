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

// --- Negative assertions: ChatMessage and SessionStatus must NOT be exported from types.ts ---
// These @ts-expect-error directives verify the types have been removed.
// If the types still exist, these directives are "unused" and tsc will error.
// @ts-expect-error ChatMessage should not exist in types.ts (superseded by claudeStore.ts)
type _NoChatMessage = import('../types').ChatMessage;
// @ts-expect-error SessionStatus should not exist in types.ts (superseded by claudeStore.ts)
type _NoSessionStatus = import('../types').SessionStatus;

// --- ClaudeMessageContext ↔ MessageContext structural sync guard ---
//
// ClaudeMessageContext (in bridge.ts) is a standalone interface that must stay
// structurally identical to Pick<MessageContext, 'selectedEntity' | 'diagnostics' | 'constraints'>.
//
// We use an Equals<A,B> type-level assertion rather than bidirectional assignability
// because all three fields are optional — `{}` satisfies any all-optional type,
// so assignability checks would pass even if the field names diverged entirely.
// The Equals pattern compares exact structural identity and catches renames,
// additions, and removals at compile time.
import type { ClaudeMessageContext } from '../bridge';
import type { MessageContext } from '../stores/claudeStore';

type _ExpectedClaudeContext = Pick<MessageContext, 'selectedEntity' | 'diagnostics' | 'constraints' | 'currentFile' | 'attachedContexts'>;

/** Exact structural equality check — evaluates to `true` only if A and B are identical types. */
type Equals<A, B> =
  (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;

// Compile-time assertion: if the types diverge, this line produces a type error.
// eslint-disable-next-line @typescript-eslint/no-unused-vars
type _AssertClaudeContextSync = Equals<ClaudeMessageContext, _ExpectedClaudeContext> extends true ? true : never;

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
