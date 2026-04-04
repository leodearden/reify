/**
 * IPC types for communication between the Tauri backend and the SolidJS frontend.
 * These mirror the Rust serialized types defined in the backend (Task 83).
 */

/** Tessellated mesh data for 3D rendering (typed arrays for WebGL). */
export interface MeshData {
  entity_path: string;
  vertices: Float32Array;
  indices: Uint32Array;
  normals: Float32Array | null;
}

/** Wire-format mesh data as received from Tauri IPC (JSON number arrays). */
export interface RawMeshData {
  entity_path: string;
  vertices: number[];
  indices: number[];
  normals: number[] | null;
}

/** Convert wire-format mesh data to typed arrays for WebGL consumption. */
export function convertRawMesh(raw: RawMeshData): MeshData {
  return {
    entity_path: raw.entity_path,
    vertices: new Float32Array(raw.vertices),
    indices: new Uint32Array(raw.indices),
    normals: raw.normals ? new Float32Array(raw.normals) : null,
  };
}

/** A parameter or computed value from the evaluation engine. */
export interface ValueData {
  cell_id: string;
  name: string;
  value: string;
  unit: string;
  determinacy: string;
  entity_path: string;
  kind: string;
}

/** Status and label of a constraint node. */
export interface ConstraintData {
  node_id: string;
  expression: string;
  status: string;
  label: string | null;
  parameter_ids: string[];
}

/** A location span in source code. */
export interface SourceLocation {
  file_path: string;
  line: number;
  column: number;
  end_line: number;
  end_column: number;
}

/** Contents of an open source file. */
export interface FileData {
  path: string;
  content: string;
}

/** Full GUI state snapshot from the backend (with typed arrays). */
export interface GuiState {
  meshes: MeshData[];
  values: ValueData[];
  constraints: ConstraintData[];
  files: FileData[];
}

/** Wire-format GUI state as received from Tauri IPC. */
export interface RawGuiState {
  meshes: RawMeshData[];
  values: ValueData[];
  constraints: ConstraintData[];
  files: FileData[];
}

/** Convert wire-format GUI state to typed arrays. */
export function convertRawGuiState(raw: RawGuiState): GuiState {
  return {
    meshes: raw.meshes.map(convertRawMesh),
    values: raw.values,
    constraints: raw.constraints,
    files: raw.files,
  };
}

/** Current phase of the evaluation engine. */
export interface EvaluationStatus {
  phase: 'idle' | 'evaluating' | 'resolving';
  progress?: number;
}

/** Type aliases for event update payloads (same shape as base types). */
export type MeshUpdate = MeshData;
export type ValueUpdate = ValueData;
export type ConstraintUpdate = ConstraintData;

/** Supported export formats for geometry. */
export type ExportFormat = 'step' | 'stl';

/** An entry in the file browser tree. */
export interface FileEntry {
  path: string;
  name: string;
  isDirectory: boolean;
  children?: FileEntry[];
}

/** A toast notification message. */
export interface ToastMessage {
  id: string;
  type: 'success' | 'error' | 'info';
  message: string;
}
