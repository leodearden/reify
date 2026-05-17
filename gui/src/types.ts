/**
 * IPC types for communication between the Tauri backend and the SolidJS frontend.
 * These mirror the Rust serialized types defined in the backend (Task 83).
 */

/** Tessellated mesh data for 3D rendering (typed arrays for WebGL). */
export interface MeshData {
  entity_path: string;
  /**
   * Packed vertex positions (`[x0, y0, z0, x1, y1, z1, ...]`).
   *
   * The renderer (MeshManager) always copies this buffer on ingest, so callers
   * may freely retain or mutate the `Float32Array` after passing it to
   * `sync()` — the position attribute will not alias it.
   */
  vertices: Float32Array;
  /**
   * Triangle connectivity indices (flat list of vertex index triples).
   * The renderer (MeshManager) aliases this buffer directly into a
   * `BufferAttribute` — callers must not mutate the `Uint32Array` after
   * passing it to `sync()`.
   */
  indices: Uint32Array;
  /**
   * Per-vertex normals (`[nx0, ny0, nz0, nx1, ny1, nz1, ...]`), or `null` when
   * the backend supplies none (MeshManager calls `computeVertexNormals()` in
   * that case).  The renderer aliases this buffer directly — callers must not
   * mutate the `Float32Array` after passing it to `sync()`.
   */
  normals: Float32Array | null;
  /**
   * Per-vertex scalar attribute channels (e.g. `"vonMises"` stress).
   * Mirrors `scalar_channels: HashMap<String, Vec<f32>>` in the Rust
   * `MeshData` struct (task 2959). Absent for non-FEA meshes (field omitted
   * from the wire when the map is empty).
   */
  scalar_channels?: Record<string, Float32Array>;
  /**
   * Packed displaced vertex positions produced by the FEA deformation field.
   * Same layout as `vertices` (`[x0, y0, z0, x1, y1, z1, ...]`). Absent for
   * non-FEA meshes; present but unused by the renderer until task G3 wires it
   * into the position buffer. Mirrors `displaced_positions: Option<Vec<f32>>`
   * in the Rust `MeshData` struct (task 2959). Absent (`undefined`) when the
   * Rust side serializes `None` — never `null` on the wire.
   *
   * The renderer aliases this buffer directly — callers must not mutate the
   * `Float32Array` after passing it to `sync()`.
   */
  displaced_positions?: Float32Array;
  /**
   * Per-face element kind for shell-extract meshes (task 3597).
   * Byte-value enum: `0` = tet face, `1` = shell triangle.
   * Length equals `indices.length / 3` (one byte per face).
   * Omitted from the wire when absent (`None` on the Rust side).
   */
  element_kind?: Uint8Array;
  /**
   * Per-face stable region labels for shell-extract meshes (task 3597).
   * One `u32` label per face; length equals `indices.length / 3`.
   * Labels are stable across incremental re-tessellations within a single
   * eval generation. Omitted from the wire when absent.
   */
  region_tags?: Uint32Array;
  /**
   * Named vector attribute channels for shell-extract meshes (task 3597).
   * Each entry is a packed `Float32Array` of 3-component vectors.
   * Entry length is either `3 * vertex_count` (per-vertex channel) or
   * `3 * face_count` (per-face channel); disambiguate using the channel name
   * convention: per-face channel names end in `_per_face`
   * (e.g. `"shell_normal_per_face"`).
   * Omitted from the wire when the map is empty.
   */
  vector_channels?: Record<string, Float32Array>;
}

/** Wire-format mesh data as received from Tauri IPC (JSON number arrays). */
export interface RawMeshData {
  entity_path: string;
  vertices: number[];
  indices: number[];
  normals: number[] | null;
  /**
   * Per-vertex scalar attribute channels as raw number arrays from the IPC wire.
   * Absent when the Rust backend serializes an empty map (`skip_serializing_if`).
   */
  scalar_channels?: Record<string, number[]>;
  /**
   * Packed displaced vertex positions as raw number array from the IPC wire.
   * Absent when `displaced_positions` is `None` on the Rust side.
   * The field is never sent as JSON `null`; it is either present (array) or absent.
   */
  displaced_positions?: number[];
  /**
   * Per-face element kind as raw number array from the IPC wire (task 3597).
   * Byte-value enum: `0` = tet face, `1` = shell triangle.
   * Absent when not present in the Rust payload.
   */
  element_kind?: number[];
  /**
   * Per-face stable region labels as raw number array from the IPC wire (task 3597).
   * Absent when not present in the Rust payload.
   */
  region_tags?: number[];
  /**
   * Named vector attribute channels as raw number arrays from the IPC wire (task 3597).
   * Absent when the Rust backend serializes an empty map.
   */
  vector_channels?: Record<string, number[]>;
}

/** Convert wire-format mesh data to typed arrays for WebGL consumption. */
export function convertRawMesh(raw: RawMeshData): MeshData {
  const result: MeshData = {
    entity_path: raw.entity_path,
    vertices: new Float32Array(raw.vertices),
    indices: new Uint32Array(raw.indices),
    normals: raw.normals ? new Float32Array(raw.normals) : null,
  };
  if (raw.scalar_channels !== undefined) {
    const converted: Record<string, Float32Array> = {};
    for (const [key, values] of Object.entries(raw.scalar_channels)) {
      converted[key] = new Float32Array(values);
    }
    result.scalar_channels = converted;
  }
  if (raw.displaced_positions) {
    result.displaced_positions = new Float32Array(raw.displaced_positions);
  }
  if (raw.vector_channels !== undefined) {
    const converted: Record<string, Float32Array> = {};
    for (const [key, values] of Object.entries(raw.vector_channels)) {
      converted[key] = new Float32Array(values);
    }
    result.vector_channels = converted;
  }
  if (raw.element_kind !== undefined) {
    result.element_kind = new Uint8Array(raw.element_kind);
  }
  if (raw.region_tags !== undefined) {
    result.region_tags = new Uint32Array(raw.region_tags);
  }
  return result;
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
  /** Compile-time diagnostics (warnings, errors) from the Reify compiler. */
  compile_diagnostics: DiagnosticInfo[];
}

/** Wire-format GUI state as received from Tauri IPC. */
export interface RawGuiState {
  meshes: RawMeshData[];
  values: ValueData[];
  constraints: ConstraintData[];
  files: FileData[];
  tessellation_diagnostics: DiagnosticInfo[];
  /** Compile-time diagnostics (warnings, errors) from the Reify compiler. */
  compile_diagnostics: DiagnosticInfo[];
}

/** Convert wire-format GUI state to typed arrays. */
export function convertRawGuiState(raw: RawGuiState): GuiState {
  return {
    meshes: raw.meshes.map(convertRawMesh),
    values: raw.values,
    constraints: raw.constraints,
    files: raw.files,
    tessellation_diagnostics: raw.tessellation_diagnostics,
    compile_diagnostics: raw.compile_diagnostics,
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
   * Sub-component container nodes (`kind === "sub"`) emit `"aggregate"` —
   * they have no individual freshness; inspect `children` instead.
   * Both `"final"` and `"aggregate"` suppress badge rendering.
   * See design decision: wire format is a tag-only string (payload fields are
   * carried by the LSP diagnostic channel, not this wire type).
   */
  freshness: string;
  /** Child nodes (value cells, sub-components, ports, realizations). */
  children: EntityTreeNode[];
}

// ---------------------------------------------------------------------------
// Mechanism descriptor types (Task 2536)
// ---------------------------------------------------------------------------

/**
 * Describes a single joint motion variable within a mechanism.
 * Mirrors the Rust `JointDescriptor` struct in `gui/src-tauri/src/types.rs`.
 */
export interface JointDescriptor {
  /** Zero-based index in the mechanism's body list (stable within one eval generation). */
  joint_index: number;
  /** Joint kind: `"prismatic"` | `"revolute"` | `"coupling"` | `"fixed"`. */
  kind: string;
  /** Physical dimension: `"length"` for prismatic, `"angle"` for revolute, `"dimensionless"` for coupling/fixed. */
  dimension: string;
  /** Lower bound of the joint's range in SI units (metres or radians), or null if none. */
  range_lower_si: number | null;
  /** Upper bound of the joint's range in SI units (metres or radians), or null if none. */
  range_upper_si: number | null;
  /** Unit axis vector `[x, y, z]` for prismatic/revolute joints, or null for coupling/fixed. */
  axis: [number, number, number] | null;
  /**
   * The `ValueCellId` string of the `param` cell driving this joint via `bind(joint, param_ref)`
   * inside a `snapshot()` call. Null when the binding expression is a literal (not a param ref).
   */
  driving_param_cell_id: string | null;
  /** Current evaluated value of the driving param cell in SI units, or null if unresolved. */
  current_value_si: number | null;
}

/**
 * Describes a Mechanism value cell and its joints.
 * Mirrors the Rust `MechanismDescriptor` struct in `gui/src-tauri/src/types.rs`.
 */
export interface MechanismDescriptor {
  /** The `ValueCellId` string of this mechanism cell (e.g. `"Kinematic.m"`). */
  cell_id: string;
  /** Dot-separated entity path of the structure/template containing this mechanism. */
  entity_path: string;
  /** Short name of the mechanism cell (last segment of `cell_id`). */
  name: string;
  /** Number of bodies in this mechanism. */
  bodies_count: number;
  /** Joint descriptors, one per unique joint in body-order (deduplicated by structural equality). */
  joints: JointDescriptor[];
}

// ---------------------------------------------------------------------------
// View persistence types (Task 1749)
// ---------------------------------------------------------------------------

/**
 * Serialised view state stored in localStorage and the sidecar `.ri.views.json`
 * file.  Only user views are persisted (auto views are regenerated from the
 * entity tree on every open).
 *
 * `version` is stamped at `"2"` for forward-compat; unknown versions fall back
 * to defaults at load time.
 *
 * Mirrors the Rust `PersistentViewState` struct in `gui/src-tauri/src/types.rs`.
 */
export interface PersistentViewState {
  /** Schema version — always `"2"` in this generation. */
  version: '2';
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

// ---------------------------------------------------------------------------
// Auto-resolve loop progress types (Task 2967)
// ---------------------------------------------------------------------------

/**
 * A single parameter value snapshot from one auto-resolve iteration.
 * Mirrors the engine's wire type for `param x = auto` iteration progress.
 *
 * `value` is `null` when the Rust side resolved this auto-parameter to a
 * non-Scalar value (NaN sentinel; `serde_json` maps `f64::NAN` to JSON
 * `null`). Wire contract pinned by
 * `auto_resolve_parameter_value_nan_sentinel_serializes_value_field_as_null`
 * in `gui/src-tauri/src/tests/types_tests.rs`. Mirrors the `number | null`
 * convention used by `JointDescriptor.range_lower_si` and sibling fields.
 */
export interface AutoResolveParameterValue {
  value: number | null;
  unit: string;
  display: string;
}

/**
 * Progress for one FEA-derived constraint at a given iteration.
 * `satisfied` is true when the constraint bound is met.
 * `value` is optional: the Rust side serialises it as `Option<f64>` with
 * `skip_serializing_if`, so it is absent from the wire payload whenever the
 * kernel has no observed scalar for the constraint (the common case).
 */
export interface AutoResolveConstraintProgress {
  name: string;
  value?: number;
  unit?: string;
  target_lower?: number;
  target_upper?: number;
  satisfied: boolean;
}

/**
 * A single iteration snapshot emitted by the auto-resolve loop.
 * `driving_metric` names the primary metric being optimised;
 * `driving_metric_value` is its scalar value at this iteration.
 *
 * **Invariant:** `driving_metric` MUST be the same value for every iteration
 * within a single auto-resolve loop. Iterations whose `driving_metric` conflicts
 * with the canonical metric (the first iteration in the array that declares one)
 * are dropped by `engineStore.applyAutoResolveIteration` with a `console.warn`.
 * Omitting `driving_metric` on an iteration is always permitted — those iterations
 * are accepted without affecting the canonical.
 *
 * Empty-string `driving_metric` (`""`) is treated by the GUI as "no metric
 * declared" (same as omission) and emits a `console.warn` so the upstream
 * malformation is visible — producers SHOULD omit the field rather than emit `""`.
 */
export interface AutoResolveIteration {
  iteration: number;
  parameters: Record<string, AutoResolveParameterValue>;
  constraints: Record<string, AutoResolveConstraintProgress>;
  driving_metric?: string;
  driving_metric_value?: number;
}

/**
 * Mesh-morph runtime statistics — response shape for the
 * `morph_stats` debug-MCP RPC (GR-016 / docs/prds/v0_3/gui-event-channel-inventory.md §2.3).
 *
 * The fields mirror `reify_mesh_morph::stats::MorphStats` (Rust). Per PRD §3.2
 * the field names match exactly (no `#[serde(rename_all)]`). `last_rejection_reason`
 * is `Option<String>` on Rust and serialized `skip_serializing_if = "Option::is_none"`,
 * so it arrives as `undefined` (absent key) when no rejection has been recorded.
 *
 * Consumer: MCP debug session (claude-debug). No frontend listener — this is
 * an RPC response shape, not a Tauri event payload.
 */
export interface MorphStats {
  morph_count: number;
  remesh_count: number;
  last_rejection_reason?: string;
}

/**
 * Payload for the `warm-pool-event` Tauri channel (GR-016 ε).
 *
 * Wire format per PRD §2.2: field names match the Rust IPC struct in
 * `gui/src-tauri/src/types.rs::WarmPoolEvent` exactly — no `serde(rename_all)`.
 *
 * Emitted by `EngineSession::drain_and_emit_warm_pool_events` after each engine
 * call boundary. Consumer: `WarmPoolDebugPanel` (debug-mode only, PRD §11 Q6).
 */
export interface WarmPoolEvent {
  /** `'evicted'` when a warm state was evicted; `'donated'` when one was donated. */
  kind: 'evicted' | 'donated';
  /** Warm-state size involved in the event, in bytes. */
  size_bytes: number;
  /** Stringified `NodeId` of the victim (evicted) or donor (donated) node. */
  node_id: string;
}

/**
 * Placeholder type for a multi-load-case FEA result.
 *
 * The `unknown` case-value is a deliberate placeholder per PRD §11 Q5 —
 * the multi-load-case-fea PRD (M-016 / task 3026) owns the fully-typed
 * Rust IPC type; this task only references it as a prerequisite.
 * Narrowing `unknown` to the real type is a localized lockstep edit when 3026 lands.
 */
export interface MultiCaseResult {
  cases: Record<string, unknown>;
}

/**
 * Payload for the `fea-case-changed` Tauri event channel per PRD §2.2 task η.
 *
 * Wire format: field names match the Rust IPC struct in
 * `gui/src-tauri/src/types.rs::FeaCaseChanged` exactly — no `serde(rename_all)`.
 *
 * Emitted by `EngineSession::emit_fea_case_if_any` once per check that observes
 * a MultiCaseResult-shaped value in `CheckResult.values`.
 * Consumer: `FeaCasePickerDropdown`.
 */
export interface FeaCaseChanged {
  /** The currently-active case name (lexicographically smallest when first emitted). */
  active_case_id: string;
  /** All available case names, sorted lexicographically. */
  available_cases: string[];
}
