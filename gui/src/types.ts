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
  /**
   * Freshness state of the backing value cell (arch §7.1 lines 716-728).
   * Wire values: `"final"` | `"intermediate"` | `"pending"` | `"failed"`.
   * `"final"` is the success state and renders no badge.
   * See design decision: wire format is a tag-only string (payload fields are
   * carried by the LSP diagnostic channel, not this wire type).
   */
  freshness: string;
}

/** Status and label of a constraint node. */
export interface ConstraintData {
  node_id: string;
  expression: string;
  status: string;
  label: string | null;
  parameter_ids: string[];
}

/** A diagnostic produced during compilation or tessellation. */
export interface DiagnosticInfo {
  file_path: string;
  line: number;
  column: number;
  end_line: number;
  end_column: number;
  /**
   * PascalCase severity string: `"Error"`, `"Warning"`, or `"Info"`.
   * This is the canonical wire format — compare against PascalCase strings.
   * When `code === "unresolved-source"`, position data (line/column) is
   * unreliable because the backend could not resolve the source file.
   */
  severity: string;
  message: string;
  code: string | null;
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
  tessellation_diagnostics: DiagnosticInfo[];
}

/** Wire-format GUI state as received from Tauri IPC. */
export interface RawGuiState {
  meshes: RawMeshData[];
  values: ValueData[];
  constraints: ConstraintData[];
  files: FileData[];
  tessellation_diagnostics: DiagnosticInfo[];
}

/** Convert wire-format GUI state to typed arrays. */
export function convertRawGuiState(raw: RawGuiState): GuiState {
  return {
    meshes: raw.meshes.map(convertRawMesh),
    values: raw.values,
    constraints: raw.constraints,
    files: raw.files,
    tessellation_diagnostics: raw.tessellation_diagnostics,
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

/**
 * Visibility state for entities in the 3D viewport.
 * - 'show': Opaque, selectable, raycasted normally.
 * - 'ghost': Translucent, not selectable, not raycasted. Uses ghost material.
 * - 'hidden': Completely invisible, not rendered.
 */
export type VisibilityState = 'show' | 'ghost' | 'hidden';

/** An entry in the file browser tree. */
export interface FileEntry {
  path: string;
  name: string;
  isDirectory: boolean;
  children?: FileEntry[];
}

/** A single action button rendered inside a toast notification. */
export interface ToastAction {
  label: string;
  onClick: () => void;
}

/** A toast notification message. */
export interface ToastMessage {
  id: string;
  type: 'success' | 'error' | 'info';
  message: string;
  /** Optional action buttons (e.g. [Yes][No][Ignore] for the fuzzy-rebind prompt). */
  actions?: ToastAction[];
}

/** Error emitted when the backend fails to serialize a mesh, value, or constraint. */
export interface SerializationError {
  item_type: string;
  item_id: string;
  error: string;
}

/**
 * Explicit visibility override for a single node in the Design Tree.
 * `null` means "inherit from parent" (or apply default rule if at root).
 */
export type ExplicitVisibility = VisibilityState | null;

/**
 * Byte-offset span within a source file, as returned by the backend.
 * Mirrors the Rust `SourceSpanInfo` struct in `gui/src-tauri/src/types.rs`.
 */
export interface SourceSpanInfo {
  start: number;
  end: number;
}

/**
 * Information about a definition resolved from a source position.
 * `kind` is `"structure"` | `"occurrence"` (typed as `string` for forward-compat).
 * Mirrors the Rust `DefInfo` struct in `gui/src-tauri/src/types.rs`.
 */
export interface DefInfo {
  name: string;
  kind: string;
  span: SourceSpanInfo;
}

/**
 * A node in the entity tree emitted by the backend's `get_entity_tree` command.
 * Mirrors the Rust `EntityTreeNode` struct in `gui/src-tauri/src/types.rs`.
 */
export interface EntityTreeNode {
  /** Dot-separated path or mesh-key identifying this entity. For most kinds:
   *  `"Bracket"`, `"Bracket.width"`. For `"realization"` kind: the mesh key
   *  form `"Bracket#realization[N]"` so visibility maps to `engineStore.meshes`. */
  entity_path: string;
  /** Entity kind: `"structure"`, `"occurrence"`, `"param"`, `"let"`, `"auto"`, `"sub"`, `"port"`, `"realization"`. */
  kind: string;
  /** Type name for value cells and sub-components; `null` for template root nodes. */
  type_name: string | null;
  /** Optional display label override. When present, the UI renders this
   *  instead of deriving a name from `entity_path`. Used for `"realization"`
   *  nodes (carries the original binding name like `"body"`) so the outline
   *  shows the user-friendly name while `entity_path` keeps the mesh key. */
  display_name?: string | null;
  /** Whether this entity has at least one realization (tessellatable geometry). */
  has_mesh: boolean;
  /** Heuristic: member is named `"geometry"` AND parent template has `"Physical"` in `trait_bounds`. */
  trait_geometry: boolean;
  /**
   * Freshness state of the backing node (arch §7.1 lines 716-728).
   * Wire values: `"final"` | `"intermediate"` | `"pending"` | `"failed"`.
   * `"final"` is the success state and renders no badge.
   * See design decision: wire format is a tag-only string (payload fields are
   * carried by the LSP diagnostic channel, not this wire type).
   */
  freshness: string;
  /** Child nodes (value cells, sub-components, ports, realizations). */
  children: EntityTreeNode[];
}

// ---------------------------------------------------------------------------
// View persistence types (Task 1749)
// ---------------------------------------------------------------------------

/**
 * Serialised view state stored in localStorage and the sidecar `.ri.views.json`
 * file.  Only user views are persisted (auto views are regenerated from the
 * entity tree on every open).
 *
 * `version` is stamped at `"1"` for forward-compat; unknown versions fall back
 * to defaults at load time.
 *
 * Mirrors the Rust `PersistentViewState` struct in `gui/src-tauri/src/types.rs`.
 */
export interface PersistentViewState {
  /** Schema version — always `"1"` in this generation. */
  version: '1';
  /** Id of the active view at persist time (may be auto or user). */
  activeViewId: string;
  /** Snapshot of user-created views (auto views excluded). */
  userViews: import('./stores/autoViewGenerator').ViewDefinition[];
  /**
   * Explicit visibility overrides keyed by entity path.
   * Preserves stale entries for undo/branch-switch restoration.
   */
  explicit: Record<string, VisibilityState>;
  /** Per-viewport camera state keyed by viewport id. */
  viewportCameras: Record<string, import('./stores/viewportStore').CameraState>;
  /** ISO 8601 timestamp of last write. */
  timestamp: string;
}
