// IPC types for GUI ↔ Engine communication

use std::collections::HashMap;

use serde::ser::Error as SerError;
use serde::{Deserialize, Serialize};

use reify_core::DiagnosticInfo;
use reify_ir::{DeterminacyState, Freshness, Value};

/// Custom serializer for `Vec<f32>` that rejects non-finite values.
///
/// `serde_json::to_value` silently converts `f32::NAN` and `f32::INFINITY` to
/// `null` by default.  This serializer makes degenerate geometry an explicit
/// error so that `delta_to_events` can log a warning and emit a
/// `"serialization-error"` event instead of silently producing null vertices
/// on the frontend.
///
/// # Note
///
/// The single-pass loop begins the JSON sequence before validating all
/// elements.  If a non-finite value appears at position > 0, earlier
/// elements have already been written to the serializer.  With in-memory
/// serializers like `serde_json::to_value` (the current sole caller via
/// `delta_to_events`), the partial `Value` is simply dropped on `Err`.
/// Callers using streaming serializers (e.g. `serde_json::to_writer`)
/// must discard partial output on error.
fn serialize_finite_f32_vec<S>(values: &[f32], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(values.len()))?;
    for &v in values {
        if !v.is_finite() {
            return Err(S::Error::custom(format!(
                "non-finite f32 value ({v}) in mesh geometry"
            )));
        }
        seq.serialize_element(&v)?;
    }
    seq.end()
}

/// Custom serializer for `HashMap<String, Vec<f32>>` that rejects non-finite values.
///
/// Mirrors [`serialize_finite_f32_vec`] but operates on a map of named float
/// channels.  Each channel's values are validated element-by-element; the
/// channel key and the `field_label` (e.g. `"scalar channel"` or
/// `"vector channel"`) are included in the error message for diagnostics.
///
/// The `field_label` parameter lets callers produce accurate error messages
/// regardless of which struct field is being serialized — for example,
/// `"scalar channel"` for `scalar_channels` and `"vector channel"` for
/// `vector_channels`.  Without this parameter both would produce the same
/// hard-coded label, which confuses operators reading wire-error logs.
///
/// # Note
///
/// Same partial-output caveat as [`serialize_finite_f32_vec`] — with in-memory
/// serializers like `serde_json::to_value` the partial map is simply dropped on
/// `Err`.  Callers using streaming serializers must discard partial output on
/// error.
fn serialize_finite_f32_map<S>(
    map: &HashMap<String, Vec<f32>>,
    field_label: &str,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeMap;
    let mut smap = serializer.serialize_map(Some(map.len()))?;
    for (key, values) in map {
        // Validate all values in this channel before writing the entry.
        for &v in values {
            if !v.is_finite() {
                return Err(S::Error::custom(format!(
                    "non-finite f32 value ({v}) in {field_label} '{key}'"
                )));
            }
        }
        smap.serialize_entry(key, values)?;
    }
    smap.end()
}

/// Custom serializer for `Option<Vec<f32>>` that rejects non-finite values.
///
/// Mirrors [`serialize_finite_f32_vec`] but handles the `None` case
/// (serializes as JSON `null`).
fn serialize_finite_f32_vec_opt<S>(
    values: &Option<Vec<f32>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match values {
        None => serializer.serialize_none(),
        Some(v) => serialize_finite_f32_vec(v, serializer),
    }
}

/// Full GUI state snapshot sent to the frontend after each operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuiState {
    pub meshes: Vec<MeshData>,
    pub values: Vec<ValueData>,
    pub constraints: Vec<ConstraintData>,
    pub files: Vec<FileData>,
    /// Diagnostics produced during the most recent tessellation pass.
    ///
    /// Non-empty when `tessellate_snapshot` encounters geometry errors (e.g.
    /// OCCT kernel failures). Empty on preview snapshots and after a clean eval.
    /// Distinct from `compile_diagnostics` — both streams are disjoint.
    pub tessellation_diagnostics: Vec<DiagnosticInfo>,
    /// Compile diagnostics (errors, warnings, info) from the most recently compiled module.
    ///
    /// Non-empty when the compiler emits any diagnostic — including recoverable
    /// parse/compile errors, warnings (e.g. unresolved imports, shadowing,
    /// unknown port types), or info messages. Empty after a clean compile with
    /// no diagnostics. Distinct from `tessellation_diagnostics` — compile
    /// diagnostics are produced before tessellation runs.
    ///
    /// Two additional sources populate this field beyond the normal compile output:
    ///
    /// 1. **Cold-start failure** (no prior successful compile): surfaced via
    ///    `EngineSession::last_compile_diagnostics` on the early-return branch of
    ///    `build_gui_state` (when `compiled` is `None`).  Frontends should show
    ///    these even when the viewport is empty (`meshes`, `values`, etc. are empty).
    ///
    /// 2. **Live-edit failure** (prior good compile exists): surfaced via
    ///    `EngineSession::live_compile_diagnostics` on the non-early-return branch
    ///    of `build_gui_state` (appended after `get_diagnostics()` output so
    ///    warnings from the prior good state remain visible alongside the error).
    pub compile_diagnostics: Vec<DiagnosticInfo>,
}

// ---------------------------------------------------------------------------
// Newtype wrappers used by the manual `Serialize` impl for `MeshData` below.
// These delegate to the private finite-value serialization helpers without
// requiring those helpers to be public.
// ---------------------------------------------------------------------------

struct FiniteF32Slice<'a>(&'a [f32]);
struct FiniteF32SliceOpt<'a>(&'a Option<Vec<f32>>);
/// Newtype wrapper for `serialize_finite_f32_map`.
///
/// The second field is the human-readable field label included in NaN/Inf error
/// messages (e.g. `"scalar channel"` or `"vector channel"`).  This lets
/// operators immediately identify which struct field on the wire produced a
/// non-finite value without inspecting a stack trace.
struct FiniteF32MapRef<'a>(&'a HashMap<String, Vec<f32>>, &'a str);

impl<'a> serde::Serialize for FiniteF32Slice<'a> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serialize_finite_f32_vec(self.0, s)
    }
}

impl<'a> serde::Serialize for FiniteF32SliceOpt<'a> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serialize_finite_f32_vec_opt(self.0, s)
    }
}

impl<'a> serde::Serialize for FiniteF32MapRef<'a> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serialize_finite_f32_map(self.0, self.1, s)
    }
}

/// Channel name suffix that marks a `vector_channels` entry as per-face.
///
/// Names ending in this suffix must have length `3 * face_count`; all other
/// names are treated as per-vertex and must have length `3 * vertex_count`.
/// This is the single source of truth for the OQ-4 naming convention (PRD §11).
pub const PER_FACE_CHANNEL_SUFFIX: &str = "_per_face";

/// Tessellated mesh for 3D display.
///
/// # Serialization
///
/// `Serialize` is implemented manually (not derived) so that the following
/// contracts can be enforced at serialization time with a `S::Error::custom`
/// error — before any partial output is written to the wire:
///
/// - Each entry in `scalar_channels` must have exactly `vertices.len() / 3`
///   values (one per vertex).  A mismatched entry is a programming error in
///   the kernel-side FEA sourcing task (G2) that would produce silent
///   wrong-length colour buffers on the frontend.
/// - `displaced_positions`, when `Some`, must have the same length as
///   `vertices` (same flat XYZ layout).
///
/// See project convention: *"contract in production code rather than relying
/// on test coverage"* (task 2544).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MeshData {
    pub entity_path: String,
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
    pub normals: Option<Vec<f32>>,
    /// Per-vertex scalar attribute channels (e.g. `"vonMises"` stress).
    ///
    /// Each entry maps a channel name to a flat `Vec<f32>` of length
    /// `vertex_count` (`vertices.len() / 3`).  Omitted from the wire when
    /// empty so non-FEA meshes stay compact.  Populated by the kernel-side
    /// FEA sourcing task (G2); plumbed here so the IPC contract is established
    /// without churning the wire format later.
    ///
    /// **Contract:** `values.len() == vertices.len() / 3` for each entry —
    /// enforced at serialization time.
    #[serde(default)]
    pub scalar_channels: HashMap<String, Vec<f32>>,
    /// Packed displaced vertex positions produced by the FEA deformation field.
    ///
    /// Same layout as `vertices` (`[x0, y0, z0, x1, y1, z1, ...]`).  Omitted
    /// from the wire when `None` so non-FEA meshes stay compact.  Wiring into
    /// the rendered position buffer is deferred to task G3 (FEA-mode UI);
    /// pinning the field now lets G3 land without re-touching the IPC contract.
    ///
    /// **Contract:** `len() == vertices.len()` when `Some` — enforced at
    /// serialization time.
    #[serde(default)]
    pub displaced_positions: Option<Vec<f32>>,
    /// Per-face element-kind byte values.
    ///
    /// When `Some`, each byte classifies the corresponding triangle face:
    /// - `0` — tet face (from a tetrahedral solid element)
    /// - `1` — shell triangle (from a shell/surface element)
    ///
    /// Future kernel variants (hex, wedge, beam) will add further byte values;
    /// the byte-value encoding leaves room up to `u8::MAX` before a wider type
    /// is needed.  The OQ-3 resolution (PRD §11) chose `u8` for wire compactness
    /// (1 byte per face vs 4 for `u32`).
    ///
    /// **Length contract:** `len() == indices.len() / 3` (face count) when `Some`
    /// — enforced at serialization time.  Omitted from the wire when `None` so
    /// tet-only meshes stay compact.
    #[serde(default)]
    pub element_kind: Option<Vec<u8>>,
    /// Per-face region-label tags.
    ///
    /// When `Some`, each `u32` assigns the corresponding triangle face to a
    /// named region (e.g. flange vs web).  Region label values are stable within
    /// a given mesh key — downstream consumers may cache colour mappings by label.
    ///
    /// **Length contract:** `len() == indices.len() / 3` (face count) when `Some`
    /// — enforced at serialization time.  Omitted from the wire when `None`.
    #[serde(default)]
    pub region_tags: Option<Vec<u32>>,
    /// Named per-vertex or per-face float vector channels.
    ///
    /// Each entry maps a channel name to a flat `Vec<f32>`.  The per-vertex vs
    /// per-face mode is encoded in the channel name: names ending in `_per_face`
    /// are per-face (`len == 3 * face_count`); all others are per-vertex
    /// (`len == 3 * vertex_count`).  Example: `"shell_normal_per_face"`.
    ///
    /// OQ-4 resolution (PRD §11): a single `HashMap` (not two) keeps the wire
    /// schema flat.  The channel name is the single source of truth for layout:
    ///
    /// - Names ending in `_per_face` → per-face channel; `len == 3 * face_count`
    ///   (one XYZ triple per triangle face).
    /// - All other names → per-vertex channel; `len == 3 * vertex_count`
    ///   (one XYZ triple per vertex).
    ///
    /// Both contracts are enforced at serialization time.  This covers the
    /// degenerate case where `vertex_count == face_count` (the two valid
    /// lengths collapse and layout cannot be recovered from length alone).
    ///
    /// Non-finite f32 values are rejected at serialization time (reuses the
    /// `FiniteF32MapRef` guard).  Omitted from the wire when empty.
    #[serde(default)]
    pub vector_channels: HashMap<String, Vec<f32>>,
}

impl serde::Serialize for MeshData {
    /// Validate length contracts then serialize fields using finite-value guards.
    ///
    /// Returns `Err` (via `S::Error::custom`) if:
    /// - any `scalar_channels` entry length ≠ `vertices.len() / 3`, or
    /// - `displaced_positions` length ≠ `vertices.len()` (when `Some`), or
    /// - `element_kind` length ≠ `indices.len() / 3` (when `Some`), or
    /// - `region_tags` length ≠ `indices.len() / 3` (when `Some`), or
    /// - any `vector_channels` entry with `_per_face` suffix has length ≠ `3*face_count`, or
    /// - any `vector_channels` entry without `_per_face` suffix has length ≠ `3*vertex_count`, or
    /// - any f32 value is non-finite (NaN / ±Inf).
    ///
    /// The `_per_face` suffix convention for `vector_channels` is enforced here
    /// (not just documented) so that mis-named or mis-sized channels are caught
    /// before reaching the wire — including the degenerate-mesh case where
    /// `vertex_count == face_count` and the layout cannot be recovered from
    /// length alone.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let vertex_count = self.vertices.len() / 3;
        let face_count = self.indices.len() / 3;

        // Contract: each scalar channel must be exactly vertex_count values long.
        for (channel, values) in &self.scalar_channels {
            if values.len() != vertex_count {
                return Err(S::Error::custom(format!(
                    "scalar channel '{channel}' has length {} but vertex count is {vertex_count}",
                    values.len()
                )));
            }
        }

        // Contract: displaced_positions must have the same length as vertices.
        if let Some(displaced) = &self.displaced_positions
            && displaced.len() != self.vertices.len()
        {
            return Err(S::Error::custom(format!(
                "displaced_positions has length {} but expected {} (vertices.len())",
                displaced.len(),
                self.vertices.len()
            )));
        }

        // Contract: element_kind length must equal face_count when Some.
        if let Some(ek) = &self.element_kind
            && ek.len() != face_count
        {
            return Err(S::Error::custom(format!(
                "element_kind has length {} but face count is {face_count}",
                ek.len()
            )));
        }

        // Contract: region_tags length must equal face_count when Some.
        if let Some(rt) = &self.region_tags
            && rt.len() != face_count
        {
            return Err(S::Error::custom(format!(
                "region_tags has length {} but face count is {face_count}",
                rt.len()
            )));
        }

        // Contract: vector_channels length is determined by the channel name suffix.
        // Names ending in `_per_face` must have length 3*face_count; all others
        // must have length 3*vertex_count.  This enforces the OQ-4 naming convention
        // (PRD §11) at the single enforcement chokepoint (the manual Serialize impl)
        // so that mis-named or mis-sized channels are caught before reaching the wire
        // — including the degenerate-mesh case (vertex_count == face_count) where
        // the two valid lengths collapse and the layout cannot be recovered from
        // length alone.
        for (channel, values) in &self.vector_channels {
            if channel.ends_with(PER_FACE_CHANNEL_SUFFIX) {
                if values.len() != 3 * face_count {
                    return Err(S::Error::custom(format!(
                        "vector channel '{channel}' has suffix '_per_face' so expected \
                         length {} (3*face_count) but got {}",
                        3 * face_count,
                        values.len()
                    )));
                }
            } else if values.len() != 3 * vertex_count {
                return Err(S::Error::custom(format!(
                    "vector channel '{channel}' expected length {} (3*vertex_count) but got {}",
                    3 * vertex_count,
                    values.len()
                )));
            }
        }

        // entity_path, vertices, indices, normals are always serialized.
        // scalar_channels, displaced_positions, element_kind, region_tags,
        // and vector_channels are omitted when absent/empty.
        let mut field_count = 4usize;
        if !self.scalar_channels.is_empty() {
            field_count += 1;
        }
        if self.displaced_positions.is_some() {
            field_count += 1;
        }
        if self.element_kind.is_some() {
            field_count += 1;
        }
        if self.region_tags.is_some() {
            field_count += 1;
        }
        if !self.vector_channels.is_empty() {
            field_count += 1;
        }

        let mut s = serializer.serialize_struct("MeshData", field_count)?;
        s.serialize_field("entity_path", &self.entity_path)?;
        s.serialize_field("vertices", &FiniteF32Slice(&self.vertices))?;
        s.serialize_field("indices", &self.indices)?;
        s.serialize_field("normals", &FiniteF32SliceOpt(&self.normals))?;
        if !self.scalar_channels.is_empty() {
            s.serialize_field("scalar_channels", &FiniteF32MapRef(&self.scalar_channels, "scalar channel"))?;
        }
        if self.displaced_positions.is_some() {
            s.serialize_field(
                "displaced_positions",
                &FiniteF32SliceOpt(&self.displaced_positions),
            )?;
        }
        if let Some(ek) = &self.element_kind {
            s.serialize_field("element_kind", ek)?;
        }
        if let Some(rt) = &self.region_tags {
            s.serialize_field("region_tags", rt)?;
        }
        if !self.vector_channels.is_empty() {
            s.serialize_field("vector_channels", &FiniteF32MapRef(&self.vector_channels, "vector channel"))?;
        }
        s.end()
    }
}

/// A value cell (param, let, or auto) for the property editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueData {
    pub cell_id: String,
    pub name: String,
    pub value: String,
    pub unit: String,
    pub determinacy: String,
    pub entity_path: String,
    pub kind: String,
    /// Computation freshness tag — one of `"final"`, `"intermediate"`,
    /// `"pending"`, or `"failed"`. Defaults to `"final"` (matching
    /// `Freshness::default() = Final`). See arch §7.1 lines 716-728 and
    /// the task #2337 design decision on tag-only wire format.
    pub freshness: String,
}

/// A constraint with its check status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintData {
    pub node_id: String,
    pub expression: String,
    pub status: String,
    pub label: Option<String>,
    pub parameter_ids: Vec<String>,
}

/// A source file in the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileData {
    pub path: String,
    pub content: String,
}

/// Wire-format descriptor for a single mechanism (a `Value::Map` with `kind="mechanism"`
/// and no `error` key).  Sent by the `get_mechanism_descriptors` Tauri command.
///
/// One descriptor per mechanism cell in the loaded module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MechanismDescriptor {
    /// Stringified `ValueCellId` (e.g. `"Kinematic.m"`).
    pub cell_id: String,
    /// Entity / structure name (e.g. `"Kinematic"`).
    pub entity_path: String,
    /// Member name of the mechanism cell (e.g. `"m"`).
    pub name: String,
    /// Number of body records in this mechanism.
    pub bodies_count: usize,
    /// One descriptor per unique joint appearing in `bodies`.
    pub joints: Vec<JointDescriptor>,
}

/// Wire-format descriptor for a single joint within a mechanism.
///
/// Joints are identified by their zero-based index in the order they were
/// first encountered while walking `bodies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointDescriptor {
    /// Zero-based index of this joint in the deduplicated joint list.
    pub joint_index: usize,
    /// Joint kind string: `"prismatic"`, `"revolute"`, `"coupling"`, or `"fixed"`.
    pub kind: String,
    /// Physical dimension of the motion variable: `"length"`, `"angle"`, or `"dimensionless"`.
    pub dimension: String,
    /// Lower bound of the joint range in SI units (m for length, rad for angle).
    /// `None` for coupling and fixed joints (no independent range).
    pub range_lower_si: Option<f64>,
    /// Upper bound of the joint range in SI units.
    /// `None` for coupling and fixed joints.
    pub range_upper_si: Option<f64>,
    /// Unit axis direction as `[x, y, z]`.
    /// `None` for coupling and fixed joints (no translational/rotational axis).
    pub axis: Option<[f64; 3]>,
    /// Stringified `ValueCellId` of the `param` cell that drives this joint via
    /// `bind(joint, param_ref)` inside a `snapshot()` call.
    /// `None` when the bind expression is a literal rather than a param reference,
    /// or when no `snapshot()` / `bind()` call references this joint.
    ///
    /// **Backward-compat:** kept for parity with the downstream η-frontend consumer
    /// which has not yet migrated to the `binding` field. The `binding` field is
    /// the authoritative source going forward.
    pub driving_param_cell_id: Option<String>,
    /// Current evaluated SI value of the `driving_param_cell_id` cell.
    /// `None` when `driving_param_cell_id` is `None` or the value is `Undef`.
    ///
    /// **Backward-compat:** mirrors the `current_value_si` inside `binding` for the
    /// `ParamBound` case. See `driving_param_cell_id` note above.
    pub current_value_si: Option<f64>,
    /// Structured description of how this joint is driven (introduced in task 3783).
    ///
    /// This is the authoritative field. The legacy `driving_param_cell_id` /
    /// `current_value_si` flat fields carry the same data for the `ParamBound` case
    /// and are kept for backward compatibility with the η-frontend (separate task).
    pub binding: JointBinding,
}

/// Describes how a kinematic joint is driven within a `snapshot()` call.
///
/// # Variant mapping (PRD §8.1)
///
/// | Variant | bind() form | Description |
/// |---------|-------------|-------------|
/// | `ParamBound` | `bind(j, param_ref)` | Joint driven by a named `param` cell; the param slider controls the joint position. |
/// | `LiteralBound` | `bind(j, 100mm)` | Joint driven by a literal constant; surfaced as a scrub-virtual-param slider in the GUI. |
/// | `CouplingDerived` | coupling joint (no bind) | Joint position is geometrically derived from another driving joint. `source_joint` detection is deferred to ζ work. |
/// | `FixedNoMotion` | fixed joint / default | Joint has no independent motion variable; position is fully constrained. |
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JointBinding {
    /// Joint is driven by a named `param` cell via `bind(joint, param_ref)`.
    /// `current_value_si` is the current evaluated SI value of the param cell.
    ParamBound {
        param_cell_id: String,
        current_value_si: Option<f64>,
    },
    /// Joint is driven by a literal constant via `bind(joint, <literal>)`.
    /// Surfaced as a scrubbable synth-virtual-param slider in the GUI.
    /// `synth_param_name` is the virtual param name used internally (e.g. `__joint_y_axis_v`).
    /// `initial_value_si` is the SI value of the literal (e.g. `0.1` for `100mm`).
    LiteralBound {
        synth_param_name: String,
        initial_value_si: Option<f64>,
        scrubbable: bool,
    },
    /// Joint position is derived from another driving joint (coupling joint).
    /// `source_joint` is the cell name of the driving joint; empty string until ζ detection is implemented.
    CouplingDerived { source_joint: String },
    /// Joint has no independent motion variable (fixed joint or conservative default).
    FixedNoMotion,
}

/// Current phase of the evaluation engine (mirrors frontend EvaluationStatus interface).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationStatus {
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
}

/// A node in the hierarchical entity tree emitted by `get_entity_tree`.
///
/// Root nodes correspond to top-level topology templates (structures/occurrences).
/// Children represent value cells (param, let, auto), sub-components, and ports.
///
/// # Kind values
/// - `"structure"` / `"occurrence"` — top-level template root
/// - `"param"` — a `ValueCellKind::Param` cell
/// - `"let"` — a `ValueCellKind::Let` cell
/// - `"auto"` — a `ValueCellKind::Auto` cell
/// - `"sub"` — a sub-component (`SubComponentDecl`)
/// - `"port"` — a port (`CompiledPort`)
/// - `"realization"` — a geometry-producing realization (`RealizationDecl`),
///   keyed by its mesh key (e.g. `"Bracket#realization[0]"`) so visibility
///   toggles match `engineStore.meshes`. Display name comes from
///   `display_name` (the original `let`/`param` binding name).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityTreeNode {
    /// Dot-separated path identifying this entity (e.g. `"Bracket"`, `"Bracket.width"`, `"Bracket.bolt"`).
    /// For `"realization"` nodes this is the mesh key (`Entity#realization[N]`).
    pub entity_path: String,
    /// Entity kind string — one of `"structure"`, `"occurrence"`, `"param"`, `"let"`, `"auto"`, `"sub"`, `"port"`, `"realization"`.
    pub kind: String,
    /// Type name for value cells (`cell_type.to_string()`) and sub-components (`structure_name`).
    /// `None` for template root nodes.
    pub type_name: Option<String>,
    /// Optional display label override. When `Some`, the UI uses this instead
    /// of deriving a name from `entity_path`. Set for `"realization"` nodes
    /// (carrying the binding name like `"body"`) so the outline shows the
    /// user-friendly name while `entity_path` keeps the mesh-key form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Whether this entity has at least one realization (tessellatable geometry).
    pub has_mesh: bool,
    /// Heuristic: member is named `"geometry"` AND the parent template has `"Physical"` in `trait_bounds`.
    pub trait_geometry: bool,
    /// Child nodes (value cells, sub-components, ports of this template).
    pub children: Vec<EntityTreeNode>,
    /// Computation freshness tag — one of `"final"`, `"intermediate"`,
    /// `"pending"`, or `"failed"`. Defaults to `"final"` (matching
    /// `Freshness::default() = Final`). See arch §7.1 lines 716-728 and
    /// the task #2337 design decision on tag-only wire format.
    pub freshness: String,
}

/// Source span (byte offsets) for an entity in the source file.
///
/// Emitted as part of `EntityIdentity` for value cells and sub-components.
/// Both offsets are byte positions within the source text of the containing module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceSpanInfo {
    /// Inclusive start byte offset of the entity's declaration.
    pub start: u32,
    /// Exclusive end byte offset of the entity's declaration.
    pub end: u32,
}

/// Identity record for a single entity (template root or value cell) in the compiled module.
///
/// Returned as the value type in the `HashMap<String, EntityIdentity>` from
/// `get_entity_identity_map`. The map key is the entity's `entity_path`
/// (e.g. `"Bracket"`, `"Bracket.width"`).
///
/// # Fields
/// - `content_hash`: 32-character lowercase hex string from `ContentHash::to_string()`.
///   **Semantics differ by entry kind** — the field name is preserved for API stability:
///   - *Template roots*: the compiler-produced content hash over the template's full
///     structure (params, constraints, sub-components, etc.) — a true content hash.
///   - *Value cells*: `ContentHash::of_str(cell.id.to_string())` — an **identity hash**
///     derived from the cell's path string (e.g. `"Bracket.width"`), NOT a hash of the
///     cell's content (type, default_expr, kind, etc.).  Callers needing a true
///     cell-content hash must derive it separately.
/// - `structural_fingerprint`: `"{type}:{parent}:{child_count}:{hash}"` format.
///   - `type` — entity kind (`"structure"`, `"occurrence"`, `"param"`, `"let"`, `"auto"`)
///   - `parent` — parent template name, literal `"<root>"` sentinel for root templates
///     (angle-bracket form cannot be a valid template identifier, preventing
///     collision with user-defined templates named "root")
///   - `child_count` — number of sub-components (0 for value cells)
///   - `hash` — hex hash combining sub-component content hashes
/// - `source_span`: byte span of the entity's declaration; `None` for template roots
///   (which have no span in the compiled representation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityIdentity {
    /// 32-character lowercase hex string from `ContentHash::to_string()`.
    ///
    /// **Semantic note**: for template-root entries this is a true content hash (over
    /// the template's structure, params, constraints, etc., produced by the compiler).
    /// For value-cell entries this is an *identity hash* — `ContentHash::of_str(cell.id)`
    /// — derived from the cell's path string, not from its content or type.
    /// The field name is preserved for API/JSON stability despite the inconsistency;
    /// see the `EntityIdentity` struct doc for details.
    pub content_hash: String,
    /// Structural fingerprint: `"{type}:{parent}:{child_count}:{hash}"`.
    pub structural_fingerprint: String,
    /// Source byte span; present for value cells and sub-components, absent for template roots.
    pub source_span: Option<SourceSpanInfo>,
}

/// A definition (structure or occurrence) found at a given source position.
///
/// Returned by `get_containing_definition(line, col)`.
/// `span` uses byte offsets matching the source text stored in the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefInfo {
    /// Name of the structure or occurrence (e.g. `"Bracket"`).
    pub name: String,
    /// Kind: `"structure"` or `"occurrence"`.
    pub kind: String,
    /// Byte span of the declaration in the source file.
    pub span: SourceSpanInfo,
}

/// Format a Value for GUI display, returning (formatted_value, unit_string).
///
/// Delegates to [`Value::format_display_pair()`] — the canonical implementation
/// lives on Value itself so that adding a new variant only requires editing value.rs.
pub fn format_value(v: &Value) -> (String, String) {
    v.format_display_pair()
}

/// Format a DeterminacyState as a string.
pub fn format_determinacy(d: DeterminacyState) -> String {
    match d {
        DeterminacyState::Determined => "determined".to_string(),
        DeterminacyState::Undetermined => "undetermined".to_string(),
        DeterminacyState::Provisional => "provisional".to_string(),
        DeterminacyState::Auto => "auto".to_string(),
    }
}

/// Format a [`Freshness`] variant as a lowercase tag string for the GUI wire
/// protocol.
///
/// Returns one of `"final"`, `"intermediate"`, `"pending"`, or `"failed"`.
/// Payload fields (`generation`, `last_substantive`, `error`) are deliberately
/// collapsed — the UI surface only needs the variant tag for badge selection.
/// Human-readable detail for Failed/Pending is carried by the LSP diagnostic
/// channel per the task #2337 design decision ("Wire format is a single
/// lowercase tag string").
///
/// Mirrors [`format_determinacy`] in naming convention, return type, and
/// call-site idiom.
pub fn format_freshness(f: &Freshness) -> &'static str {
    match f {
        Freshness::Final => "final",
        Freshness::Intermediate { .. } => "intermediate",
        Freshness::Pending { .. } => "pending",
        Freshness::Failed { .. } => "failed",
    }
}

// ---------------------------------------------------------------------------
// View persistence types (Task 1749)
// ---------------------------------------------------------------------------

/// Camera state serialised as plain arrays (Three.js-independent, JSON-safe).
///
/// Mirrors the TypeScript `CameraState` interface in `gui/src/stores/viewportStore.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraStateData {
    pub position: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    pub zoom: f64,
}

/// A view definition (user-created or auto-generated).
///
/// Mirrors the TypeScript `ViewDefinition` interface in
/// `gui/src/stores/autoViewGenerator.ts`.  Only user views (`auto: false`)
/// are written to the sidecar; auto views are regenerated from the entity
/// tree on every file open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewDefinitionData {
    pub id: String,
    pub name: String,
    pub auto: bool,
    /// Explicit per-node visibility state keyed by entity path.
    pub visibility: HashMap<String, String>,
    /// Set to `true` on copy-on-write user views (auto→user on first edit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<bool>,
}

/// Serialised view state stored in localStorage and the sidecar
/// `{filename}.ri.views.json` file.
///
/// Only user views are persisted; auto views are regenerated from the entity
/// tree on every file open.  Schema version is stamped at `"2"`.
///
/// Mirrors the TypeScript `PersistentViewState` interface in
/// `gui/src/types.ts`.  JSON keys use camelCase to match the TypeScript
/// wire format (`#[serde(rename_all = "camelCase")]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistentViewState {
    /// Schema version — always `"2"` in this generation.
    pub version: String,
    /// Id of the active view at persist time.
    pub active_view_id: String,
    /// Snapshot of user-created views (auto views excluded).
    pub user_views: Vec<ViewDefinitionData>,
    /// Explicit visibility overrides keyed by entity path.
    /// Preserves stale entries for undo/branch-switch restoration.
    pub explicit: HashMap<String, String>,
    /// Per-viewport camera state keyed by viewport id.
    pub viewport_cameras: HashMap<String, CameraStateData>,
    /// ISO 8601 timestamp of last write.
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Auto-resolve loop event payload types (Task 3479)
// ---------------------------------------------------------------------------

/// A single resolved auto-parameter value with display-unit conversion applied.
///
/// Mirrors the TypeScript `AutoResolveParameterValue` interface in
/// `gui/src/types.ts`.  `value` carries the display-unit numeric (e.g., 4.2
/// for 4.2 mm), not the SI numeric.  `display` is the formatted string used
/// for the trajectory label in the auto-resolve panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoResolveParameterValue {
    pub value: f64,
    pub unit: String,
    pub display: String,
}

/// Per-constraint progress within an auto-resolve iteration.
///
/// Mirrors the TypeScript `AutoResolveConstraintProgress` interface in
/// `gui/src/types.ts`.  Optional fields (`value`, `unit`, `target_lower`,
/// `target_upper`) are omitted from the JSON wire when absent — the GUI panel
/// renders gracefully without them, using `satisfied` + `name` for the
/// indicator row.
///
/// `value` is `None` until the kernel exposes per-constraint observed scalars
/// at the CheckResult boundary; emitting `0.0` would be a wire-level lie
/// (indistinguishable from a genuine zero observation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoResolveConstraintProgress {
    pub name: String,
    /// Observed scalar value for this constraint (display-unit).
    /// `None` when the kernel does not yet expose the observed value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_lower: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_upper: Option<f64>,
    pub satisfied: bool,
}

/// Payload for the `auto-resolve-iteration` Tauri event.
///
/// Emitted once per Engine::check call that produces non-empty `resolved_params`.
/// `iteration` is 0-indexed; in the current single-iteration-per-pass model
/// it is always 0.  `driving_metric` and `driving_metric_value` are omitted
/// when no primary metric is declared (the GUI treats absence as no-metric).
///
/// Mirrors the TypeScript `AutoResolveIteration` interface in `gui/src/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoResolveIteration {
    pub iteration: u32,
    pub parameters: HashMap<String, AutoResolveParameterValue>,
    pub constraints: HashMap<String, AutoResolveConstraintProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driving_metric: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driving_metric_value: Option<f64>,
}

/// IPC payload for the `warm-pool-event` Tauri channel.
///
/// Wire format per PRD §2.2: `{"kind":"evicted"|"donated","size_bytes":<u64>,"node_id":<string>}`.
/// Field names match the TS interface in `gui/src/types.ts` exactly — no `serde(rename_all)`.
///
/// Constructed from the engine-internal
/// [`reify_eval::warm_pool::WarmPoolEvent`] enum via [`WarmPoolEvent::from_engine_event`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WarmPoolEvent {
    /// `"evicted"` or `"donated"`.
    pub kind: String,
    /// Warm-state size involved in the event, in bytes.
    pub size_bytes: u64,
    /// Stringified [`reify_eval::cache::NodeId`] of the victim (evicted) or donor (donated) node.
    pub node_id: String,
}

impl WarmPoolEvent {
    /// Translate an engine-internal [`reify_eval::warm_pool::WarmPoolEvent`] to the
    /// flat IPC shape required by the `warm-pool-event` channel wire format.
    ///
    /// Preserves the victim/donor `node_id` contract documented in `journal.rs:53-62`:
    /// `WarmPoolEvent::Evicted.node_id` is the **victim** node; `Donated.node_id` is
    /// the **donor** node.  The `to_string()` call uses `NodeId`'s `Display` impl
    /// (`cache.rs:57`), which is stable across variant additions.
    pub fn from_engine_event(ev: &reify_eval::warm_pool::WarmPoolEvent) -> Self {
        use reify_eval::warm_pool::WarmPoolEvent as EngineEvent;
        match ev {
            EngineEvent::Evicted { node_id, size_bytes } => Self {
                kind: "evicted".to_string(),
                size_bytes: *size_bytes as u64,
                node_id: node_id.to_string(),
            },
            EngineEvent::Donated { node_id, size_bytes } => Self {
                kind: "donated".to_string(),
                size_bytes: *size_bytes as u64,
                node_id: node_id.to_string(),
            },
        }
    }
}

/// IPC payload for the `fea-case-changed` Tauri channel per PRD §2.2 task η.
///
/// Emitted once per check that observes a `MultiCaseResult`-shaped value in
/// `CheckResult.values`. Mirrors fire-every-commit semantics of `emit_auto_resolve_if_any`
/// (no engine-side dedup).
///
/// Field names match the TypeScript `FeaCaseChanged` interface in `gui/src/types.ts`
/// exactly — no `serde(rename_all)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaCaseChanged {
    /// Lexicographically-smallest case name (deterministic BTreeMap key order).
    pub active_case_id: String,
    /// All available case names, sorted (BTreeMap iteration order from the inner map).
    pub available_cases: Vec<String>,
}

#[cfg(test)]
mod format_value_range_tests {
    use super::*;
    use reify_ir::Value;

    #[test]
    fn both_bounds_exclusive() {
        let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), false, false);
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "(1..10)");
        assert_eq!(unit, "");
    }

    #[test]
    fn both_bounds_inclusive() {
        let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "[1..10]");
        assert_eq!(unit, "");
    }

    #[test]
    fn none_lower_inclusive_via_factory() {
        // Factory normalizes inclusive=false for None bound
        let range = Value::range(None, Some(Value::Int(10)), true, true);
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "(-\u{221e}..10]");
        assert_eq!(unit, "");
    }

    #[test]
    fn none_lower_inclusive_via_direct_struct() {
        // Bypass factory: directly construct with inclusive=true + None lower
        let range = Value::Range {
            lower: None,
            upper: Some(Box::new(Value::Int(10))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "(-\u{221e}..10]");
        assert_eq!(unit, "");
    }

    #[test]
    fn none_upper_inclusive_via_factory() {
        // Factory normalizes inclusive=false for None bound
        let range = Value::range(Some(Value::Int(1)), None, true, true);
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "[1..+\u{221e})");
        assert_eq!(unit, "");
    }

    #[test]
    fn none_upper_inclusive_via_direct_struct() {
        // Bypass factory: directly construct with inclusive=true + None upper
        let range = Value::Range {
            lower: Some(Box::new(Value::Int(1))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "[1..+\u{221e})");
        assert_eq!(unit, "");
    }

    #[test]
    fn both_bounds_none_inclusive_normalizes_to_parentheses() {
        // Both None + both inclusive=true: defensive re-normalization must fix both brackets
        let range = Value::Range {
            lower: None,
            upper: None,
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "(-\u{221e}..+\u{221e})");
        assert_eq!(unit, "");
    }

    #[test]
    fn mixed_inclusive_exclusive() {
        // Lower inclusive, upper exclusive: half-open interval [0..5)
        let range = Value::range(Some(Value::Int(0)), Some(Value::Int(5)), true, false);
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "[0..5)");
        assert_eq!(unit, "");
    }

    #[test]
    fn range_unit_always_empty_even_with_scalar_bounds() {
        // Range with Scalar bounds (LENGTH dimension): unit must still be empty
        // because Range display does not propagate unit info from its bounds.
        let lower = Value::Scalar {
            si_value: 0.001,
            dimension: reify_core::DimensionVector::LENGTH,
        };
        let upper = Value::Scalar {
            si_value: 0.01,
            dimension: reify_core::DimensionVector::LENGTH,
        };
        let range = Value::range(Some(lower), Some(upper), true, false);
        let (formatted, unit) = format_value(&range);
        // Scalars inside the range are formatted individually (SI→mm conversion),
        // but the range itself carries no unit.
        assert_eq!(formatted, "[1..10)");
        assert_eq!(unit, "", "Range unit string must always be empty");
    }
}
