/**
 * IPC types for communication between the Tauri backend and the SolidJS frontend.
 * These mirror the Rust serialized types defined in the backend (Task 83).
 */

/** Tessellated mesh data for 3D rendering. */
export interface MeshData {
  entity_path: string;
  vertices: number[];
  indices: number[];
  normals: number[] | null;
}

/** A parameter or computed value from the evaluation engine. */
export interface ValueData {
  cell_id: string;
  name: string;
  value: string;
  unit: string;
  determinacy: string;
  entity_path: string;
}

/** Status and details of a constraint node. */
export interface ConstraintData {
  node_id: string;
  expression: string;
  status: string;
  details: string | null;
  parameter_ids: string[];
}

/** A location span in source code. */
export interface SourceLocation {
  file: string;
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

/** Full GUI state snapshot from the backend. */
export interface GuiState {
  meshes: MeshData[];
  values: ValueData[];
  constraints: ConstraintData[];
  files: FileData[];
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
