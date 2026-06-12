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
  DiagnosticInfo,
  EntityTreeNode,
  MorphStats,
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

// --- MeshData with optional scalar_channels and displaced_positions (task 2959) ---
// (a) Both new optional fields populated — asserts they are accepted as MeshData.
const meshWithScalars: MeshData = {
  entity_path: 'FEA.body',
  vertices: new Float32Array([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
  indices: new Uint32Array([0, 1, 2]),
  normals: null,
  scalar_channels: { vonMises: new Float32Array([10, 20, 30]) },
  displaced_positions: new Float32Array([0, 0, 0, 1, 1, 1, 2, 2, 2]),
};
void meshWithScalars;

// --- MeshData with shell-extract fields (task 3597) ---
// (a) All three new fields populated with their typed-array forms.
const meshWithShellFields: MeshData = {
  entity_path: 'Shell.body',
  vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
  indices: new Uint32Array([0, 1, 2]),
  normals: null,
  element_kind: new Uint8Array([1]),
  region_tags: new Uint32Array([42]),
  vector_channels: { shell_normal_per_face: new Float32Array([0, 0, 1]) },
};
void meshWithShellFields;

// (b) MeshData with new fields absent — asserts they are optional.
const meshWithoutShellFields: MeshData = {
  entity_path: 'Tet.body',
  vertices: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
  indices: new Uint32Array([0, 1, 2]),
  normals: null,
};
void meshWithoutShellFields;

// (c) Type assertions extracting each new typed-array field.
const _ek: Uint8Array | undefined = meshWithShellFields.element_kind;
void _ek;
const _rt: Uint32Array | undefined = meshWithShellFields.region_tags;
void _rt;
const _vc: Record<string, Float32Array> | undefined = meshWithShellFields.vector_channels;
void _vc;

// --- RawMeshData (wire format from Tauri IPC) ---
const rawMesh: RawMeshData = {
  entity_path: 'Bracket.body',
  vertices: [0.0, 1.0, 2.0],
  indices: [0, 1, 2],
  normals: [0.0, 0.0, 1.0],
};

// --- RawMeshData with optional scalar_channels and displaced_positions (task 2959) ---
// (b) Both new fields populated in the raw wire type.
const rawMeshWithScalars: RawMeshData = {
  entity_path: 'FEA.body',
  vertices: [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
  indices: [0, 1, 2],
  normals: null,
  scalar_channels: { vonMises: [10, 20, 30] },
  displaced_positions: [0, 0, 0, 1, 1, 1, 2, 2, 2],
};
void rawMeshWithScalars;

// (c) RawMeshData omitting both new fields — asserts they remain optional.
const rawMeshWithoutScalars: RawMeshData = {
  entity_path: 'Plain.body',
  vertices: [0.0, 1.0, 2.0],
  indices: [0, 1, 2],
  normals: null,
};
void rawMeshWithoutScalars;

// --- RawMeshData with shell-extract fields (task 3597) ---
// (d) RawMeshData with all three new fields as raw number[] / Record<string,number[]>.
const rawMeshWithShellFields: RawMeshData = {
  entity_path: 'Shell.body',
  vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
  indices: [0, 1, 2],
  normals: null,
  element_kind: [1],
  region_tags: [42],
  vector_channels: { shell_normal_per_face: [0, 0, 1] },
};
void rawMeshWithShellFields;

// (e) RawMeshData with new fields absent — asserts they remain optional.
const rawMeshWithoutShellFields: RawMeshData = {
  entity_path: 'Tet.body',
  vertices: [0, 0, 0, 1, 0, 0, 0, 1, 0],
  indices: [0, 1, 2],
  normals: null,
};
void rawMeshWithoutShellFields;

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
  freshness: 'final',
};

// --- ValueData.freshness is string typed (not unknown) ---
const _freshStr: string = value.freshness;
void _freshStr;

// --- ValueData.freshness is required (not optional) ---
// @ts-expect-error freshness is a required field; omitting it is a type error
const _valueNoFreshness: ValueData = {
  cell_id: 'cell_002',
  name: 'height',
  value: '100.0',
  unit: 'mm',
  determinacy: 'determined',
  entity_path: 'Bracket.height',
  kind: 'Param',
};
void _valueNoFreshness;

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
  file_path: 'bracket.ri',
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

// --- DiagnosticInfo ---
const diag: DiagnosticInfo = {
  file_path: 'bracket.ri',
  line: 10,
  column: 5,
  end_line: 10,
  end_column: 20,
  severity: 'Error',
  message: 'geometry error: kernel failure',
  code: null,
};

// --- EntityTreeNode ---
const node: EntityTreeNode = {
  entity_path: 'Bracket',
  kind: 'structure',
  type_name: null,
  has_mesh: false,
  trait_geometry: false,
  freshness: 'final',
  children: [],
};

// --- EntityTreeNode.freshness is string typed ---
const _nodeFreshStr: string = node.freshness;
void _nodeFreshStr;

// --- GuiState ---
const state: GuiState = {
  meshes: [mesh, meshNoNormals],
  values: [value],
  constraints: [constraint, constraintWithLabel],
  files: [file],
  tessellation_diagnostics: [diag],
  compile_diagnostics: [diag],
  tensegrity_wires: [],
  tensegrity_surfaces: [],
};

// --- GuiState.tessellation_diagnostics type assertion ---
// The field must be typed as DiagnosticInfo[], not unknown[] or any[].
const _diagField: DiagnosticInfo[] = state.tessellation_diagnostics;

// --- GuiState.compile_diagnostics type assertion ---
// The field must be typed as DiagnosticInfo[], not unknown[] or any[].
const _compileDiagField: DiagnosticInfo[] = state.compile_diagnostics;

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

// --- ClaudeMessageContext is now a re-export of MessageContext ---
//
// ClaudeMessageContext (bridge.ts) was previously a standalone interface kept in
// sync with MessageContext (claudeStore.ts) via an Equals<A,B> assertion here.
// Since bridge.ts now re-exports MessageContext directly as ClaudeMessageContext
// (`export type { MessageContext as ClaudeMessageContext }`), the structural sync
// guard is trivially satisfied and the Pick+Equals assertion has been removed.
// The compile-time assertion in claudeBridge.test.ts serves as the ongoing guard.

// --- MESSAGE_CONTEXT_FIELD_MAP exhaustiveness guard ---
//
// bridge.ts exports MESSAGE_CONTEXT_FIELD_MAP via `as const satisfies
// Record<keyof Required<MessageContext>, string>`, narrowing values to
// literal types while preserving the exhaustiveness guard. WireMessageContext
// is derived from the map via key remapping. If a field is added to
// MessageContext but not to the map, tsc will fail in bridge.ts.
// This import ensures the map is reachable from the type-check file.
// See: claudeBridge.test.ts for compile-time Equals assertions and
// runtime Object.keys/values tests.
import { MESSAGE_CONTEXT_FIELD_MAP, BUILD_CONTEXT_HANDLED_FIELDS } from '../bridge';
import type { MessageContext } from '../stores/claudeStore';
void MESSAGE_CONTEXT_FIELD_MAP;
void BUILD_CONTEXT_HANDLED_FIELDS;

// --- BUILD_CONTEXT_HANDLED_FIELDS exhaustiveness guard ---
// Compile-time assertion: the tuple must cover every key of MessageContext.
// If a field is added to MessageContext without updating BUILD_CONTEXT_HANDLED_FIELDS,
// tsc will fail here.
type Equals<A, B> =
  (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;
type AssertTrue<T extends true> = T;
type _AssertBuildContextHandledFieldsExhaustive = AssertTrue<
  Equals<(typeof BUILD_CONTEXT_HANDLED_FIELDS)[number], keyof Required<MessageContext>>
>;

// --- MorphStats (GR-016 θ morph_stats RPC response) ---
// (a) Fully-populated form.
const morphStats: MorphStats = {
  morph_count: 7,
  remesh_count: 3,
  last_rejection_reason: 'Ineligible(StructuralChange)',
};
void morphStats;

// (b) last_rejection_reason omitted (skip-serializing on Rust side → undefined here).
const morphStatsNoReason: MorphStats = {
  morph_count: 0,
  remesh_count: 0,
};
void morphStatsNoReason;

// (c) morph_count is required (omitting it is a type error).
// @ts-expect-error morph_count is a required field
const _missingMorphCount: MorphStats = {
  remesh_count: 0,
};
void _missingMorphCount;

// (d) remesh_count is required.
// @ts-expect-error remesh_count is a required field
const _missingRemeshCount: MorphStats = {
  morph_count: 0,
};
void _missingRemeshCount;

// Suppress unused variable warnings — this file is only for type checking
void diag;
void _diagField;
void mesh;
void meshNoNormals;
void rawMesh;
void converted;
void value;
void constraint;
void constraintWithLabel;
void loc;
void file;
void node;
void state;
void idle;
void evaluating;
void resolving;
void meshUpdate;
void valueUpdate;
void constraintUpdate;
