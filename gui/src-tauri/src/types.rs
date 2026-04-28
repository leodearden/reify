// IPC types for GUI ↔ Engine communication

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde::ser::Error as SerError;

use reify_types::{DeterminacyState, DiagnosticInfo, Freshness, Value};

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
    pub tessellation_diagnostics: Vec<DiagnosticInfo>,
}

/// Tessellated mesh for 3D display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshData {
    pub entity_path: String,
    #[serde(serialize_with = "serialize_finite_f32_vec")]
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
    #[serde(serialize_with = "serialize_finite_f32_vec_opt")]
    pub normals: Option<Vec<f32>>,
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
/// tree on every file open.  Schema version is stamped at `"1"`.
///
/// Mirrors the TypeScript `PersistentViewState` interface in
/// `gui/src/types.ts`.  JSON keys use camelCase to match the TypeScript
/// wire format (`#[serde(rename_all = "camelCase")]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistentViewState {
    /// Schema version — always `"1"` in this generation.
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

#[cfg(test)]
mod format_value_range_tests {
    use super::*;
    use reify_types::Value;

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
            dimension: reify_types::DimensionVector::LENGTH,
        };
        let upper = Value::Scalar {
            si_value: 0.01,
            dimension: reify_types::DimensionVector::LENGTH,
        };
        let range = Value::range(Some(lower), Some(upper), true, false);
        let (formatted, unit) = format_value(&range);
        // Scalars inside the range are formatted individually (SI→mm conversion),
        // but the range itself carries no unit.
        assert_eq!(formatted, "[1..10)");
        assert_eq!(unit, "", "Range unit string must always be empty");
    }
}
