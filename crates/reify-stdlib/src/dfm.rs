//! Design-for-Manufacturing (DFM) builtins (PRD v0_6 process-dfm-completion, task α).
//!
//! Two surfaces, mirroring the stackup / flexure modules:
//!
//! - [`eval_dfm`] — the pure builtin dispatcher (sibling of `stackup::eval_stackup`),
//!   wired into `crate::eval_builtin`'s fall-through chain in `lib.rs`. It evaluates
//!   `fits_build_volume(part_bbox, envelope_bbox[, severity_or_rule])`, a pure
//!   bbox-vs-bbox extent comparator (no kernel / `EvalContext` access). The two
//!   `Value::BoundingBox` inputs are resolved from Solids UPSTREAM by the existing
//!   kernel-aware `bounding_box(solid)` builtin, so `fits_build_volume` itself stays
//!   unit-testable and dependency-free (PRD §2.1 / §4 decision 4).
//!
//! - [`diagnose`] — the `DFMSeverity` → diagnostic-severity bridge (sibling of
//!   `flexures::flexure_diagnose`). It is re-exported as `crate::dfm_diagnose` and
//!   called from reify-expr's builtin fall-through on BOTH the success and the
//!   `Value::Undef` paths: a successfully-evaluated `fits_build_volume` that returns
//!   `Bool(false)` is a build-volume VIOLATION whose severity comes from the optional
//!   rule argument; a `Value::Undef` result is a usage error.

use reify_ir::Value;
use reify_core::{Diagnostic, DimensionVector, Severity};

use crate::helpers::tensor_components_f64;

/// Evaluate a DFM builtin by name.
///
/// Returns `Some(value)` if `name` is a recognised DFM function, `None` otherwise
/// (so the dispatch chain in `lib.rs` can fall through). Mirrors
/// [`crate::stackup::eval_stackup`]'s `Option<Value>` fall-through convention.
pub(crate) fn eval_dfm(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "fits_build_volume" => fits_build_volume(args),
        _ => return None,
    })
}

// --- fits_build_volume ---

/// `fits_build_volume(part_bbox, envelope_bbox[, severity_or_rule]) -> Bool`.
///
/// Pure component-wise extent comparator: returns `Value::Bool(true)` iff the
/// part's per-axis extent `<=` the envelope's on every axis (EXACT `<=`, no
/// tolerance — PRD §3 G6, so equal extents fit). Both inputs must be
/// `Value::BoundingBox` whose corners are finite 3-component LENGTH Point3s; the
/// optional 3rd argument carries the rule's `DFMSeverity` for [`diagnose`] and is
/// ignored by the boolean compute. Any malformed input yields `Value::Undef`.
fn fits_build_volume(args: &[Value]) -> Value {
    if !matches!(args.len(), 2 | 3) {
        return Value::Undef;
    }
    let part = match parse_bbox_extents(&args[0]) {
        Some(e) => e,
        None => return Value::Undef,
    };
    let envelope = match parse_bbox_extents(&args[1]) {
        Some(e) => e,
        None => return Value::Undef,
    };
    // The optional 3rd arg tags the violation severity for `diagnose` (a bare
    // DFMSeverity enum or a DFMRule structure-instance). It does NOT affect the
    // fit, but a malformed tag is a usage error → Undef.
    if args.len() == 3 && parse_dfm_severity(&args[2]).is_none() {
        return Value::Undef;
    }
    let fits = (0..3).all(|i| part[i] <= envelope[i]);
    Value::Bool(fits)
}

/// Extract the per-axis extents `[x, y, z] = max - min` from a `Value::BoundingBox`.
///
/// Returns `None` (→ the caller yields `Value::Undef`) unless `v` is a
/// `Value::BoundingBox` whose `min` and `max` corners are each a finite,
/// 3-component LENGTH `Point`/`Vector`/`Tensor` with `max >= min` on every axis.
/// [`tensor_components_f64`] already rejects non-container, empty, non-numeric, and
/// internally-mixed-dimension corners; on top of that we require exactly 3
/// components, the LENGTH dimension on BOTH corners (which also rejects a min/max
/// dimension mismatch), finite coordinates (so a NaN/inf corner cannot corrupt the
/// extent compare), and a non-inverted box (see the `max >= min` guard below).
fn parse_bbox_extents(v: &Value) -> Option<[f64; 3]> {
    let (min, max) = match v {
        Value::BoundingBox { min, max } => (min, max),
        _ => return None,
    };
    let (min_vals, min_dim) = tensor_components_f64(min)?;
    let (max_vals, max_dim) = tensor_components_f64(max)?;
    if min_vals.len() != 3 || max_vals.len() != 3 {
        return None;
    }
    if min_dim != DimensionVector::LENGTH || max_dim != DimensionVector::LENGTH {
        return None;
    }
    if !min_vals.iter().all(|x| x.is_finite()) || !max_vals.iter().all(|x| x.is_finite()) {
        return None;
    }
    // Reject an inverted / degenerate box (`max < min` on any axis). Such a box
    // yields a NEGATIVE extent, which would always satisfy `part <= envelope` and
    // silently report the part as "fitting" — a false-positive that hides the
    // malformed input. The upstream `bounding_box(...)` query always emits
    // `max >= min`, so this only fires on a hand-constructed bbox and surfaces it
    // as a usage error (→ Undef) rather than a spurious fit. (Values are finite
    // here, so the comparison is total — no NaN edge case.)
    if (0..3).any(|i| max_vals[i] < min_vals[i]) {
        return None;
    }
    Some([
        max_vals[0] - min_vals[0],
        max_vals[1] - min_vals[1],
        max_vals[2] - min_vals[2],
    ])
}

/// Parse the rule's `DFMSeverity` from a single `fits_build_volume` 3rd argument.
///
/// Accepts either a bare `Value::Enum { type_name: "DFMSeverity", variant }` or a
/// DFMRule `Value::StructureInstance` carrying a `severity` field of that enum, and
/// maps the `Info` / `Warning` / `Error` variant to the matching [`Severity`].
/// Returns `None` if `v` is neither shape, or carries an unrecognized variant. This
/// both rejects a malformed 3rd arg in [`fits_build_volume`] and is the shape-reader
/// the [`diagnose`] severity bridge reuses (where absence falls back to a default).
///
/// DFMRule `severity` is GUARANTEED populated, so the `None` branch never
/// false-rejects a valid rule: the stdlib `DFMRule` trait
/// (`crates/reify-compiler/stdlib/process.ri`) declares `param severity : DFMSeverity`
/// with NO default, making it a required member every conforming structure must
/// supply (the same no-default → required-member convention `Process` uses for
/// `duration`/`cost`). A `DFMRule` built without an explicit severity is therefore
/// unconstructable — it would fail structure type-checking upstream, never reaching
/// `fits_build_volume`. Note conformers keep their OWN concrete `type_name` (a
/// `structure Foo : DFMRule` is `Foo`, not `"DFMRule"`), so this reader duck-types on
/// the `severity` field rather than on `type_name == "DFMRule"`; a `StructureInstance`
/// lacking a `DFMSeverity` `severity` field is consequently not a conforming rule and
/// is correctly treated as a malformed 3rd arg, rather than silently defaulting (which
/// would weaken the validation `fits_build_volume`'s tests rely on).
fn parse_dfm_severity(v: &Value) -> Option<Severity> {
    let variant = match v {
        Value::Enum { type_name, variant } if type_name == "DFMSeverity" => variant.as_str(),
        Value::StructureInstance(data) => match data.fields.get("severity") {
            Some(Value::Enum { type_name, variant }) if type_name == "DFMSeverity" => {
                variant.as_str()
            }
            _ => return None,
        },
        _ => return None,
    };
    match variant {
        "Info" => Some(Severity::Info),
        "Warning" => Some(Severity::Warning),
        "Error" => Some(Severity::Error),
        _ => None,
    }
}

/// Resolve the `DFMSeverity` carried by a `fits_build_volume` call for the
/// success-path violation diagnostic.
///
/// Scans the optional 3rd argument via [`parse_dfm_severity`] (a bare
/// `DFMSeverity` enum or a DFMRule structure-instance), DEFAULTING to
/// [`Severity::Warning`] when the tag is absent or unrecognized — a build-volume
/// violation is a Warning unless the rule declares otherwise.
fn dfm_severity(args: &[Value]) -> Severity {
    args.get(2)
        .and_then(parse_dfm_severity)
        .unwrap_or(Severity::Warning)
}

/// Build the code-less build-volume VIOLATION diagnostic at `severity`.
///
/// Mirrors [`crate::geometry::diagnose`]'s code-less convention (no
/// `DiagnosticCode` — adding one would touch reify-core, out of this task's
/// scope); the `{I,W,E}_DFM_BUILD_VOLUME` message prefix preserves the PRD's
/// diagnostic-code naming. The severity-specific constructor
/// (`Diagnostic::info` / `warning` / `error`) sets `Diagnostic.severity`, which is
/// the asserted contract.
fn build_volume_violation(severity: Severity) -> Diagnostic {
    let msg = |prefix: char| {
        format!(
            "{prefix}_DFM_BUILD_VOLUME: part does not fit the build volume — its \
             bounding-box extent exceeds the envelope on at least one axis; shrink or \
             reorient the part, or select a larger build envelope"
        )
    };
    match severity {
        Severity::Info => Diagnostic::info(msg('I')),
        Severity::Warning => Diagnostic::warning(msg('W')),
        Severity::Error => Diagnostic::error(msg('E')),
    }
}

/// Resolve the rule's declared severity from `args` for the new geometry-check arms.
///
/// Scans `args` position-independently via [`parse_dfm_severity`]
/// (a bare `DFMSeverity` enum or a DFMRule structure-instance), defaulting to
/// [`Severity::Warning`] when no tag is present. Distinct from the fits-specific
/// positional [`dfm_severity`] (which reads `args.get(2)`) — the new arms receive
/// only the rule tag in `args` without a positional [part, env, tag] prefix.
fn rule_severity(args: &[Value]) -> Severity {
    args.iter().find_map(parse_dfm_severity).unwrap_or(Severity::Warning)
}

/// Build the code-less overhang VIOLATION diagnostic at `severity`.
///
/// Mirrors [`build_volume_violation`]: code-less message-prefix convention;
/// `{I,W,E}_DFM_OVERHANG` names the PRD's diagnostic code. Emitted when
/// `unsupported_overhang_faces` returns `Bool(true)` (faces dip below the build plane).
fn overhang_violation(severity: Severity) -> Diagnostic {
    let msg = |prefix: char| {
        format!(
            "{prefix}_DFM_OVERHANG: face dips below the build plane — \
             the part has unsupported overhanging geometry that exceeds the \
             process self-support angle; add support structures or redesign the \
             overhanging features"
        )
    };
    match severity {
        Severity::Info => Diagnostic::info(msg('I')),
        Severity::Warning => Diagnostic::warning(msg('W')),
        Severity::Error => Diagnostic::error(msg('E')),
    }
}

/// Build the code-less min-wall-thickness VIOLATION diagnostic at `severity`.
///
/// Mirrors [`overhang_violation`]: code-less message-prefix convention;
/// `{I,W,E}_DFM_MIN_WALL` names the PRD's diagnostic code. Emitted when
/// `min_wall_thickness` returns `Bool(true)` (measured wall thinner than the
/// process minimum feature size). The verdict is pre-computed upstream by ζ;
/// this helper does NOT re-examine numeric thresholds.
fn min_wall_violation(severity: Severity) -> Diagnostic {
    let msg = |prefix: char| {
        format!(
            "{prefix}_DFM_MIN_WALL: wall thinner than the process minimum feature size — \
             the part has a wall section whose thickness falls below the process limit; \
             increase the wall thickness or select a process with a smaller minimum feature size"
        )
    };
    match severity {
        Severity::Info => Diagnostic::info(msg('I')),
        Severity::Warning => Diagnostic::warning(msg('W')),
        Severity::Error => Diagnostic::error(msg('E')),
    }
}

/// Build the code-less min-feature-size VIOLATION diagnostic at `severity`.
///
/// Mirrors [`min_wall_violation`]: code-less message-prefix convention;
/// `{I,W,E}_DFM_MIN_FEATURE` names the PRD's diagnostic code. Emitted when
/// `min_feature_size_measure` returns `Bool(true)` (measured feature thinner than
/// the process minimum feature size). The verdict is pre-computed upstream by ζ;
/// this helper does NOT re-examine numeric thresholds.
fn min_feature_violation(severity: Severity) -> Diagnostic {
    let msg = |prefix: char| {
        format!(
            "{prefix}_DFM_MIN_FEATURE: feature thinner than the process minimum feature size — \
             the part has a thin feature whose size falls below the process resolution limit; \
             increase the feature size or select a process with a smaller minimum feature size"
        )
    };
    match severity {
        Severity::Info => Diagnostic::info(msg('I')),
        Severity::Warning => Diagnostic::warning(msg('W')),
        Severity::Error => Diagnostic::error(msg('E')),
    }
}

/// Build the code-less draft-angle VIOLATION diagnostic at `severity`.
///
/// Mirrors [`overhang_violation`]: code-less message-prefix convention;
/// `{I,W,E}_DFM_DRAFT` names the PRD's diagnostic code. Emitted when element 0 of
/// `min_draft_angle`'s result List is `Bool(true)` (wall draft below process minimum).
fn draft_violation(severity: Severity) -> Diagnostic {
    let msg = |prefix: char| {
        format!(
            "{prefix}_DFM_DRAFT: wall draft below the process minimum — \
             the part has faces whose draft angle is too shallow for clean mold \
             release; increase the draft angle or add a taper to the affected walls"
        )
    };
    match severity {
        Severity::Info => Diagnostic::info(msg('I')),
        Severity::Warning => Diagnostic::warning(msg('W')),
        Severity::Error => Diagnostic::error(msg('E')),
    }
}

/// Build the code-less undercut diagnostic.
///
/// Always [`Severity::Error`] — an undercut means the part physically cannot release
/// from the mold (a hard manufacturability failure per PRD §2.3), so the rule's
/// declared severity does not apply. Takes no severity argument and ignores `args`.
/// Emitted when element 1 of `min_draft_angle`'s result List is `Bool(true)`.
fn undercut_violation() -> Diagnostic {
    Diagnostic::error(
        "E_DFM_UNDERCUT: re-entrant wall — the part cannot release from the mold; \
         eliminate the undercut, add a side-action or lifter, or redesign the affected \
         geometry"
            .to_string(),
    )
}

/// Build the code-less E_DFM_BUILD_VOLUME usage-error diagnostic for a
/// `fits_build_volume` that evaluated to `Value::Undef`.
///
/// Always [`Severity::Error`] (a malformed CALL, not a design violation, so the
/// rule's declared severity does not apply). The detail is PINPOINTED to the
/// argument that actually failed: `fits_build_volume` yields `Undef` on three
/// distinct misuses — wrong arity, a non-BoundingBox part/envelope, or a malformed
/// optional severity/rule tag — so a single fixed "pass two bounding boxes" message
/// would MISDIRECT the wrong-arity and bad-3rd-arg cases (where both bboxes are
/// well-formed and only the count or the tag is at fault). The branches below mirror
/// the guards in [`fits_build_volume`] in the same order, so the reported culprit
/// matches the guard that actually rejected the call.
fn build_volume_usage_error(args: &[Value]) -> Diagnostic {
    let detail = if !matches!(args.len(), 2 | 3) {
        format!(
            "expected 2 or 3 arguments — a part bounding box, an envelope bounding box, \
             and an optional DFMSeverity/DFMRule severity tag — but got {}",
            args.len()
        )
    } else if parse_bbox_extents(&args[0]).is_none() || parse_bbox_extents(&args[1]).is_none() {
        "the part and envelope must both be bounding boxes with finite, 3-component \
         LENGTH corners and max >= min on every axis; resolve a Solid to its bounding \
         box with bounding_box(...) before calling fits_build_volume"
            .to_string()
    } else {
        // 2..=3 args with both bboxes well-formed → the only remaining Undef cause is
        // a malformed optional 3rd arg (a non-DFMSeverity / non-DFMRule tag).
        "the optional third argument must be a DFMSeverity enum value or a DFMRule \
         carrying a `severity` field"
            .to_string()
    };
    Diagnostic::error(format!(
        "E_DFM_BUILD_VOLUME: invalid fits_build_volume call — {detail}"
    ))
}

/// Pure post-call DFM diagnostic classifier (the `DFMSeverity` bridge).
///
/// Mirrors [`crate::flexures::flexure_diagnose`]: returns a `Vec<Diagnostic>`, fires on
/// BOTH the success and `Value::Undef` paths, and short-circuits to an empty `Vec` for
/// any non-DFM `name` (the guard dispatches before `result` is inspected).
///
/// Dispatches on `name`:
///
/// - `"fits_build_volume"` — build-volume extent check (problem-flag polarity inverted:
///   `Bool(false)` = violation, `Bool(true)` = fits). Success path: `Bool(false)` →
///   one `{I,W,E}_DFM_BUILD_VOLUME` at the rule's declared [`dfm_severity`]; `Bool(true)`
///   → nothing. Usage-error path: `Value::Undef` → one [`Severity::Error`] diagnostic
///   pinpointed to the offending argument (see [`build_volume_usage_error`]).
///
/// - `"unsupported_overhang_faces"` — overhang check (problem-flag polarity: `Bool(true)`
///   = overhang violation present, `Bool(false)` = conforms). `Bool(true)` → one
///   `{I,W,E}_DFM_OVERHANG` at the rule's declared [`rule_severity`] (default Warning);
///   `Bool(false)` → nothing. The verdict is pre-computed upstream by γ; δ receives it
///   as a bare Bool and does NOT re-examine numeric thresholds. No `Undef` usage-error
///   path: γ guarantees a valid Bool verdict; any non-Bool result (e.g. `Undef`) emits
///   nothing (defensive) rather than a spurious error.
///
/// - `"min_draft_angle"` — draft-angle / undercut check. Result is
///   `Value::List([Bool(draft_violation), Bool(has_undercut)])`. Each element is
///   independent: `draft_violation = true` → one `{I,W,E}_DFM_DRAFT` at the rule's
///   declared [`rule_severity`]; `has_undercut = true` → one `E_DFM_UNDERCUT` (ALWAYS
///   [`Severity::Error`], independent of the rule tag — an undercut means the part
///   cannot physically release from the mold). Both true → two diagnostics; `[false,
///   false]` → nothing. No `Undef` usage-error path: γ guarantees a well-formed List;
///   a non-List result (e.g. `Undef`) emits nothing (defensive) rather than a spurious
///   error.
///
/// - `"min_wall_thickness"` — minimum wall thickness check (problem-flag polarity:
///   `Bool(true)` = violation, `Bool(false)` = conforms). ζ pre-computes the Bool
///   verdict by comparing `Engine::measure_min_wall` output against the process
///   `min_feature_size`; a `BelowResolution`/`NoMeasurement`/`None` result maps to
///   `Value::Undef` (Indeterminate — C1/D5). `Bool(true)` → one `{I,W,E}_DFM_MIN_WALL`
///   at the rule's declared [`rule_severity`] (default Warning); `Bool(false)` → nothing.
///   `Undef`/non-Bool → nothing (defensive, Indeterminate — never a false Violated).
///
/// - `"min_feature_size_measure"` — minimum feature size check (same problem-flag
///   polarity as `"min_wall_thickness"`). ζ pre-computes the Bool verdict by comparing
///   `Engine::measure_min_feature` output against `min_feature_size`; non-`Measured`
///   outcomes map to `Value::Undef` (C1/D5). `Bool(true)` → one `{I,W,E}_DFM_MIN_FEATURE`
///   at the rule's declared [`rule_severity`] (default Warning); `Bool(false)`/`Undef`/
///   non-Bool → nothing (Indeterminate — never a false Violated).
///
/// - Any other name → empty (non-DFM builtin, ignored).
///
/// NOTE — problem-flag polarity for the new arms (`Bool(true)` = violation) deliberately
/// differs from `fits_build_volume`'s predicate polarity (`Bool(true)` = fits). γ must
/// encode the correct polarity for each arm.
///
/// DUPLICATE EMISSION: this classifier is stateless and re-runs on every evaluation;
/// see the `fits_build_volume` note for the dedup rationale.
pub fn diagnose(name: &str, args: &[Value], result: &Value) -> Vec<Diagnostic> {
    match name {
        "fits_build_volume" => {
            let mut diags = Vec::new();
            match result {
                Value::Bool(false) => diags.push(build_volume_violation(dfm_severity(args))),
                Value::Undef => diags.push(build_volume_usage_error(args)),
                _ => {}
            }
            diags
        }
        "unsupported_overhang_faces" => {
            let mut diags = Vec::new();
            if let Value::Bool(true) = result {
                diags.push(overhang_violation(rule_severity(args)));
            }
            diags
        }
        "min_draft_angle" => {
            let mut diags = Vec::new();
            if let Value::List(items) = result {
                // Element 0: draft violation (honors rule severity).
                if let Some(Value::Bool(true)) = items.first() {
                    diags.push(draft_violation(rule_severity(args)));
                }
                // Element 1: undercut — always Error, independent of the rule tag.
                if let Some(Value::Bool(true)) = items.get(1) {
                    diags.push(undercut_violation());
                }
            }
            diags
        }
        "min_wall_thickness" => {
            let mut diags = Vec::new();
            // Bool(true) = wall thinner than min_feature_size (violation).
            // Bool(false) = conforms; Undef = Indeterminate (BelowResolution/NoMeasurement/None).
            // C1/D5: never emit on non-Bool — a sub-resolution or unmeasurable result
            // must never produce a false Violated verdict.
            if let Value::Bool(true) = result {
                diags.push(min_wall_violation(rule_severity(args)));
            }
            diags
        }
        "min_feature_size_measure" => {
            let mut diags = Vec::new();
            // Bool(true) = feature thinner than min_feature_size (violation).
            // Bool(false) = conforms; Undef = Indeterminate (BelowResolution/NoMeasurement/None).
            // C1/D5: never emit on non-Bool.
            if let Value::Bool(true) = result {
                diags.push(min_feature_violation(rule_severity(args)));
            }
            diags
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use reify_core::DimensionVector;
    use reify_core::identity::RealizationNodeId;
    use reify_ir::geometry::GeometryHandleId;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};

    /// A LENGTH scalar of `si` metres.
    fn len(si: f64) -> Value {
        Value::Scalar { si_value: si, dimension: DimensionVector::LENGTH }
    }

    /// A `Value::BoundingBox` from two LENGTH Point3 corners (metres).
    fn bbox(min: [f64; 3], max: [f64; 3]) -> Value {
        Value::BoundingBox {
            min: Box::new(Value::Point(vec![len(min[0]), len(min[1]), len(min[2])])),
            max: Box::new(Value::Point(vec![len(max[0]), len(max[1]), len(max[2])])),
        }
    }

    // ─── step-1: fits_build_volume happy path ──────────────────────────────

    #[test]
    fn fits_build_volume_part_inside_envelope_true() {
        // 10 mm cube part inside a 20 mm cube envelope → fits.
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Bool(true)));
    }

    #[test]
    fn fits_build_volume_part_past_one_axis_false() {
        // Part extent 30 mm on X exceeds the 20 mm envelope (Y/Z fit) → does not fit.
        let part = bbox([0.0, 0.0, 0.0], [0.030, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Bool(false)));
    }

    #[test]
    fn fits_build_volume_equal_extents_true() {
        // Inclusive `<=`: equal extents fit (PRD §3 G6, no tolerance).
        let part = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Bool(true)));
    }

    #[test]
    fn fits_build_volume_extent_is_position_invariant_true() {
        // The compare is over extents (max-min), not absolute position: a 10 mm
        // part offset far from the origin still fits a 20 mm envelope at the origin.
        let part = bbox([0.100, 0.100, 0.100], [0.110, 0.110, 0.110]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Bool(true)));
    }

    // ─── step-3 helpers: malformed-input builders ──────────────────────────

    /// A scalar of `si` in the given dimension.
    fn scalar(si: f64, dim: DimensionVector) -> Value {
        Value::Scalar { si_value: si, dimension: dim }
    }

    /// A `Value::BoundingBox` whose min/max corners are Point3s of `dim`.
    fn bbox_dim(min: [f64; 3], max: [f64; 3], dim: DimensionVector) -> Value {
        Value::BoundingBox {
            min: Box::new(Value::Point(vec![scalar(min[0], dim), scalar(min[1], dim), scalar(min[2], dim)])),
            max: Box::new(Value::Point(vec![scalar(max[0], dim), scalar(max[1], dim), scalar(max[2], dim)])),
        }
    }

    /// A `Value::BoundingBox` from explicit min/max corner values (malformed corners).
    fn bbox_pts(min: Value, max: Value) -> Value {
        Value::BoundingBox { min: Box::new(min), max: Box::new(max) }
    }

    /// A raw kernel "Solid" handle value — NOT a `Value::BoundingBox`.
    fn solid_handle() -> Value {
        Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("Part", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: GeometryHandleId(1),
        }
    }

    /// A bare `DFMSeverity` enum value.
    fn dfm_sev(variant: &str) -> Value {
        Value::Enum { type_name: "DFMSeverity".into(), variant: variant.into() }
    }

    /// A `DFMRule` structure-instance carrying a `severity` field.
    fn dfm_rule(sev_variant: &str) -> Value {
        let mut fields = PersistentMap::new();
        fields.insert("severity".to_string(), dfm_sev(sev_variant));
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "DFMRule".into(),
            version: 1,
            fields,
        }))
    }

    // ─── step-3: fits_build_volume validation → Undef ──────────────────────

    #[test]
    fn fits_build_volume_arity_zero_undef() {
        assert_eq!(eval_dfm("fits_build_volume", &[]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_arity_one_undef() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        assert_eq!(eval_dfm("fits_build_volume", &[part]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_arity_four_undef() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(
            eval_dfm("fits_build_volume", &[part, env, dfm_sev("Warning"), Value::Int(1)]),
            Some(Value::Undef)
        );
    }

    #[test]
    fn fits_build_volume_non_bbox_real_undef() {
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[Value::Real(1.0), env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_non_bbox_map_undef() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let m = Value::Map(BTreeMap::new());
        assert_eq!(eval_dfm("fits_build_volume", &[part, m]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_non_bbox_solid_handle_undef() {
        // A raw Solid (ephemeral kernel handle) is NOT a BoundingBox; it must be
        // resolved via bounding_box(...) UPSTREAM. Passing one directly is a usage error.
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[solid_handle(), env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_non_bbox_undef_arg_undef() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, Value::Undef]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_dimensionless_corners_undef() {
        // Corners must be LENGTH; a DIMENSIONLESS bbox is rejected (drives step-4).
        let part = bbox_dim([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], DimensionVector::DIMENSIONLESS);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_mass_corners_undef() {
        // A MASS-dimensioned bbox is not a spatial extent (drives step-4).
        let part = bbox_dim([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], DimensionVector::MASS);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_mismatched_min_max_dim_undef() {
        // min LENGTH, max MASS — inconsistent corner dimensions (drives step-4).
        let part = bbox_pts(
            Value::Point(vec![len(0.0), len(0.0), len(0.0)]),
            Value::Point(vec![
                scalar(1.0, DimensionVector::MASS),
                scalar(1.0, DimensionVector::MASS),
                scalar(1.0, DimensionVector::MASS),
            ]),
        );
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_two_components_undef() {
        // A 2-component corner is not a 3D extent.
        let part = bbox_pts(
            Value::Point(vec![len(0.0), len(0.0)]),
            Value::Point(vec![len(0.010), len(0.010)]),
        );
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_nan_corner_undef() {
        // A NaN corner must not silently produce a bogus comparison (drives step-4).
        let part = bbox([0.0, 0.0, 0.0], [f64::NAN, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_inf_corner_undef() {
        // An infinite corner is not a valid finite extent (drives step-4).
        let part = bbox([0.0, 0.0, 0.0], [f64::INFINITY, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(eval_dfm("fits_build_volume", &[part, env]), Some(Value::Undef));
    }

    #[test]
    fn fits_build_volume_inverted_bbox_undef() {
        // An inverted / degenerate bbox (max < min on an axis) yields a NEGATIVE
        // extent that would always satisfy `part <= envelope` and silently report
        // the part as "fitting" — a false-positive. Reject it as a usage error
        // (→ Undef). bounding_box() upstream guarantees max >= min, so this only
        // guards a malformed, hand-constructed bbox.
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let inverted_part = bbox([0.010, 0.010, 0.010], [0.0, 0.0, 0.0]);
        assert_eq!(
            eval_dfm("fits_build_volume", &[inverted_part, env]),
            Some(Value::Undef)
        );
        // Symmetric: an inverted envelope is equally malformed (negative envelope
        // extent), so it too is a usage error rather than a comparison input.
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let inverted_env = bbox([0.020, 0.020, 0.020], [0.0, 0.0, 0.0]);
        assert_eq!(
            eval_dfm("fits_build_volume", &[part, inverted_env]),
            Some(Value::Undef)
        );
    }

    #[test]
    fn fits_build_volume_invalid_third_arg_real_undef() {
        // 3rd arg must be a DFMSeverity enum or a DFMRule structure-instance (drives step-4).
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(
            eval_dfm("fits_build_volume", &[part, env, Value::Real(1.0)]),
            Some(Value::Undef)
        );
    }

    #[test]
    fn fits_build_volume_invalid_third_arg_wrong_enum_undef() {
        // An enum of the wrong type is not a DFMSeverity (drives step-4).
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let wrong = Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() };
        assert_eq!(eval_dfm("fits_build_volume", &[part, env, wrong]), Some(Value::Undef));
    }

    // Guard: a VALID 3rd-arg severity tag must NOT cause Undef — it is ignored by
    // the boolean compute, so step-4 must not over-reject it.
    #[test]
    fn fits_build_volume_valid_severity_enum_third_arg_computes() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(
            eval_dfm("fits_build_volume", &[part, env, dfm_sev("Warning")]),
            Some(Value::Bool(true))
        );
    }

    #[test]
    fn fits_build_volume_valid_rule_third_arg_computes() {
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(
            eval_dfm("fits_build_volume", &[part, env, dfm_rule("Error")]),
            Some(Value::Bool(true))
        );
    }

    // ─── step-5: anti-orphan — fits_build_volume routes through eval_builtin ─

    #[test]
    fn eval_builtin_routes_fits_build_volume_true() {
        // Routes through the PUBLIC `crate::eval_builtin` (not `eval_dfm`): proves
        // the `dfm::eval_dfm` arm is wired into the dispatch chain at runtime —
        // stronger than a grep. A fitting pair must resolve to `Bool(true)`.
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(crate::eval_builtin("fits_build_volume", &[part, env]), Value::Bool(true));
    }

    #[test]
    fn eval_builtin_routes_fits_build_volume_false() {
        let part = bbox([0.0, 0.0, 0.0], [0.030, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        assert_eq!(crate::eval_builtin("fits_build_volume", &[part, env]), Value::Bool(false));
    }

    // ─── step-7: diagnose success path (DFMSeverity → Severity bridge) ──────

    /// A non-fitting part/envelope pair: the part's X-extent (30 mm) exceeds the
    /// envelope (20 mm), so `fits_build_volume` evaluates to `Bool(false)` — a
    /// build-volume VIOLATION that `diagnose` classifies on the success path.
    fn nonfitting_pair() -> (Value, Value) {
        let part = bbox([0.0, 0.0, 0.0], [0.030, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        (part, env)
    }

    #[test]
    fn diagnose_violation_warning_severity() {
        // Bool(false) + DFMSeverity.Warning → exactly one Warning diagnostic.
        let (part, env) = nonfitting_pair();
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_sev("Warning")],
            &Value::Bool(false),
        );
        assert_eq!(diags.len(), 1, "one violation diagnostic");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("W_DFM"),
            "Warning message carries the W_DFM prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_violation_error_severity() {
        let (part, env) = nonfitting_pair();
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_sev("Error")],
            &Value::Bool(false),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM"),
            "Error message carries the E_DFM prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_violation_info_severity() {
        let (part, env) = nonfitting_pair();
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_sev("Info")],
            &Value::Bool(false),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(
            diags[0].message.contains("I_DFM"),
            "Info message carries the I_DFM prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_violation_rule_form_reads_severity_field() {
        // The DFMRule structure-instance form: severity is read from its
        // `severity` field (DFMSeverity.Error) → an Error diagnostic.
        let (part, env) = nonfitting_pair();
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_rule("Error")],
            &Value::Bool(false),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].message.contains("E_DFM"), "msg: {}", diags[0].message);
    }

    #[test]
    fn diagnose_violation_defaults_to_warning_when_severity_absent() {
        // No 3rd arg → default Warning (PRD: default Warning when absent).
        let (part, env) = nonfitting_pair();
        let diags = diagnose("fits_build_volume", &[part, env], &Value::Bool(false));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    #[test]
    fn diagnose_fits_true_emits_no_diagnostic() {
        // A fitting design (Bool(true)) is NOT a violation → empty Vec.
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_sev("Warning")],
            &Value::Bool(true),
        );
        assert!(diags.is_empty(), "a fitting design surfaces no diagnostic");
    }

    // ─── step-9: diagnose Undef path + non-DFM name guard ──────────────────

    #[test]
    fn diagnose_undef_emits_error_usage_diagnostic() {
        // A `Value::Undef` result is a usage error (non-BoundingBox args, e.g. a
        // raw Solid not resolved via bounding_box(...)): exactly one Error
        // diagnostic, independent of any severity tag (drives step-10).
        let diags = diagnose(
            "fits_build_volume",
            &[Value::Real(1.0), Value::Real(2.0)],
            &Value::Undef,
        );
        assert_eq!(diags.len(), 1, "one usage-error diagnostic");
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM"),
            "usage error carries the E_DFM prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_non_dfm_name_undef_is_empty() {
        // A non-DFM builtin name short-circuits to empty even on Undef.
        assert!(diagnose("box", &[], &Value::Undef).is_empty());
    }

    // ─── amend: usage-error message is pinpointed to the failing argument ───
    // `fits_build_volume` returns Undef on three distinct misuses; the single
    // usage-error diagnostic must name the argument actually at fault rather than
    // always blaming the bbox case. Every branch keeps the E_DFM / Error contract.

    #[test]
    fn diagnose_undef_bad_bbox_reports_bounding_box() {
        // A non-BoundingBox part → the message points at the bbox arguments.
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let diags = diagnose("fits_build_volume", &[Value::Real(1.0), env], &Value::Undef);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        let m = &diags[0].message;
        assert!(m.contains("E_DFM"), "carries E_DFM prefix: {m}");
        assert!(m.contains("bounding box"), "points at the bbox args: {m}");
    }

    #[test]
    fn diagnose_undef_wrong_arity_reports_argument_count() {
        // A 4-arg call (valid bboxes) → the message points at the argument COUNT,
        // NOT at the bounding boxes (which are well-formed here).
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, dfm_sev("Warning"), Value::Int(1)],
            &Value::Undef,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        let m = &diags[0].message;
        assert!(m.contains("E_DFM"), "carries E_DFM prefix: {m}");
        assert!(m.contains("got 4"), "reports the actual argument count: {m}");
        assert!(
            !m.contains("bounding_box(...)"),
            "does NOT misdirect to the Solid-resolution fix when bboxes are fine: {m}"
        );
    }

    #[test]
    fn diagnose_undef_bad_severity_tag_reports_third_arg() {
        // Two valid bboxes + a malformed 3rd arg → the message points at the
        // optional severity/rule tag, NOT at the bounding boxes.
        let part = bbox([0.0, 0.0, 0.0], [0.010, 0.010, 0.010]);
        let env = bbox([0.0, 0.0, 0.0], [0.020, 0.020, 0.020]);
        let diags = diagnose(
            "fits_build_volume",
            &[part, env, Value::Real(1.0)],
            &Value::Undef,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        let m = &diags[0].message;
        assert!(m.contains("E_DFM"), "carries E_DFM prefix: {m}");
        assert!(m.contains("third argument"), "points at the severity/rule tag: {m}");
        assert!(
            !m.contains("bounding_box(...)"),
            "does NOT misdirect to the Solid-resolution fix when bboxes are fine: {m}"
        );
    }

    #[test]
    fn diagnose_non_dfm_name_bool_is_empty() {
        // A non-DFM name short-circuits to empty even with a Bool(false) result —
        // the name guard dispatches before the result is inspected.
        assert!(diagnose("stackup_rss", &[], &Value::Bool(false)).is_empty());
    }

    // ─── step-1 RED: diagnose — unsupported_overhang_faces arm ───────────────

    #[test]
    fn diagnose_overhang_violation_warning_severity() {
        // Bool(true) = overhang violation present; DFMSeverity.Warning → one Warning diagnostic.
        let diags = diagnose(
            "unsupported_overhang_faces",
            &[dfm_sev("Warning")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1, "one overhang violation diagnostic");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("W_DFM_OVERHANG"),
            "Warning overhang message carries W_DFM_OVERHANG prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_overhang_violation_error_severity() {
        let diags = diagnose(
            "unsupported_overhang_faces",
            &[dfm_sev("Error")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM_OVERHANG"),
            "Error overhang message carries E_DFM_OVERHANG prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_overhang_violation_info_severity() {
        let diags = diagnose(
            "unsupported_overhang_faces",
            &[dfm_sev("Info")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(
            diags[0].message.contains("I_DFM_OVERHANG"),
            "Info overhang message carries I_DFM_OVERHANG prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_overhang_violation_rule_form_reads_severity_field() {
        // DFMRule structure-instance form: severity read from `severity` field → Error diagnostic.
        let diags = diagnose(
            "unsupported_overhang_faces",
            &[dfm_rule("Error")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM_OVERHANG"),
            "msg: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_overhang_violation_defaults_to_warning_when_tag_absent() {
        // No tag in args → default Warning.
        let diags = diagnose("unsupported_overhang_faces", &[], &Value::Bool(true));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    #[test]
    fn diagnose_overhang_conforms_emits_no_diagnostic() {
        // Bool(false) = conforms (no unsupported overhang faces) → empty Vec.
        let diags = diagnose(
            "unsupported_overhang_faces",
            &[dfm_sev("Warning")],
            &Value::Bool(false),
        );
        assert!(diags.is_empty(), "a conforming overhang result surfaces no diagnostic");
    }

    // ─── step-3 RED: diagnose — min_draft_angle arm (draft only, no undercut) ─

    /// Construct a `Value::List([Bool(draft_violation), Bool(has_undercut)])`.
    fn draft_result(violated: bool, undercut: bool) -> Value {
        Value::List(vec![Value::Bool(violated), Value::Bool(undercut)])
    }

    #[test]
    fn diagnose_draft_violation_warning_severity() {
        // List([Bool(true), Bool(false)]) + DFMSeverity.Warning → one Warning diagnostic.
        let diags = diagnose(
            "min_draft_angle",
            &[dfm_sev("Warning")],
            &draft_result(true, false),
        );
        assert_eq!(diags.len(), 1, "one draft violation diagnostic");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("W_DFM_DRAFT"),
            "Warning draft message carries W_DFM_DRAFT prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_draft_violation_error_severity() {
        let diags = diagnose(
            "min_draft_angle",
            &[dfm_sev("Error")],
            &draft_result(true, false),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM_DRAFT"),
            "Error draft message carries E_DFM_DRAFT prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_draft_violation_info_severity() {
        let diags = diagnose(
            "min_draft_angle",
            &[dfm_sev("Info")],
            &draft_result(true, false),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(
            diags[0].message.contains("I_DFM_DRAFT"),
            "Info draft message carries I_DFM_DRAFT prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_draft_violation_rule_form_reads_severity_field() {
        // DFMRule structure-instance form: severity read from `severity` field → Error diagnostic.
        let diags = diagnose(
            "min_draft_angle",
            &[dfm_rule("Error")],
            &draft_result(true, false),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM_DRAFT"),
            "msg: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_draft_violation_defaults_to_warning_when_tag_absent() {
        // No tag in args → default Warning.
        let diags = diagnose("min_draft_angle", &[], &draft_result(true, false));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    #[test]
    fn diagnose_draft_conforms_emits_no_diagnostic() {
        // List([Bool(false), Bool(false)]) → empty Vec.
        let diags = diagnose(
            "min_draft_angle",
            &[dfm_sev("Warning")],
            &draft_result(false, false),
        );
        assert!(diags.is_empty(), "a conforming draft result surfaces no diagnostic");
    }

    // ─── step-5 RED: diagnose — undercut (always Error) + combined ────────────

    #[test]
    fn diagnose_undercut_only_is_always_error_even_with_warning_tag() {
        // List([Bool(false), Bool(true)]) + DFMSeverity.Warning → ONE Error diagnostic
        // (undercut ignores the rule's severity — always Error).
        let diags = diagnose(
            "min_draft_angle",
            &[dfm_sev("Warning")],
            &draft_result(false, true),
        );
        assert_eq!(diags.len(), 1, "exactly one undercut diagnostic");
        assert_eq!(
            diags[0].severity,
            Severity::Error,
            "undercut is always Error even when tag is Warning"
        );
        assert!(
            diags[0].message.contains("E_DFM_UNDERCUT"),
            "undercut carries E_DFM_UNDERCUT prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_undercut_only_is_always_error_even_with_info_tag() {
        // Lock severity-independence: Info tag still emits Error for undercut.
        let diags = diagnose(
            "min_draft_angle",
            &[dfm_sev("Info")],
            &draft_result(false, true),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error, "undercut is always Error even when tag is Info");
        assert!(
            diags[0].message.contains("E_DFM_UNDERCUT"),
            "msg: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_draft_and_undercut_combined_emits_two_diagnostics() {
        // List([Bool(true), Bool(true)]) + DFMSeverity.Warning → TWO diagnostics:
        // one W_DFM_DRAFT (Warning) AND one E_DFM_UNDERCUT (Error).
        let diags = diagnose(
            "min_draft_angle",
            &[dfm_sev("Warning")],
            &draft_result(true, true),
        );
        assert_eq!(diags.len(), 2, "two diagnostics: draft + undercut");
        // One diagnostic must be Warning/W_DFM_DRAFT, the other Error/E_DFM_UNDERCUT.
        let has_draft = diags.iter().any(|d| {
            d.severity == Severity::Warning && d.message.contains("W_DFM_DRAFT")
        });
        let has_undercut = diags.iter().any(|d| {
            d.severity == Severity::Error && d.message.contains("E_DFM_UNDERCUT")
        });
        assert!(has_draft, "one W_DFM_DRAFT Warning diagnostic present");
        assert!(has_undercut, "one E_DFM_UNDERCUT Error diagnostic present");
    }

    // ─── amend: defensive no-emit for malformed γ verdicts ────────────────────
    // The new arms deliberately have no Undef usage-error path (γ pre-computes the
    // verdict and guarantees a valid Bool / List). These tests lock the documented
    // defensive no-emit behavior so a future refactor that accidentally emits a
    // diagnostic (or panics) on a malformed verdict is caught immediately.

    #[test]
    fn diagnose_overhang_non_bool_result_emits_nothing() {
        // Defensive: a non-Bool result (e.g. Undef — a wrong-shaped γ verdict) must
        // produce NO diagnostics rather than panic or emit a spurious error.
        // γ guarantees Bool; this guards the "impossible" branch so regressions are
        // caught rather than silently dropped.
        assert!(
            diagnose("unsupported_overhang_faces", &[], &Value::Undef).is_empty(),
            "non-Bool result emits nothing (defensive, no Undef usage-error path)"
        );
    }

    #[test]
    fn diagnose_draft_non_list_result_emits_nothing() {
        // Defensive: a non-List result (e.g. Undef — a wrong-shaped γ verdict) must
        // produce NO diagnostics rather than panic or emit a spurious error.
        // γ guarantees a well-formed List; this guards the "impossible" branch.
        assert!(
            diagnose("min_draft_angle", &[dfm_sev("Warning")], &Value::Undef).is_empty(),
            "non-List result emits nothing (defensive, no Undef usage-error path)"
        );
    }

    // ─── step-1 RED: diagnose — min_wall_thickness arm ────────────────────────
    // These tests fail until step-2 adds the arm to `diagnose`.

    #[test]
    fn diagnose_min_wall_violation_warning_severity() {
        // Bool(true) = wall thinner than min_feature_size; DFMSeverity.Warning → one Warning diagnostic.
        let diags = diagnose(
            "min_wall_thickness",
            &[dfm_sev("Warning")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1, "one min_wall violation diagnostic");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("W_DFM_MIN_WALL"),
            "Warning min_wall message carries W_DFM_MIN_WALL prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_min_wall_violation_error_severity() {
        let diags = diagnose(
            "min_wall_thickness",
            &[dfm_sev("Error")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM_MIN_WALL"),
            "Error min_wall message carries E_DFM_MIN_WALL prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_min_wall_violation_info_severity() {
        let diags = diagnose(
            "min_wall_thickness",
            &[dfm_sev("Info")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(
            diags[0].message.contains("I_DFM_MIN_WALL"),
            "Info min_wall message carries I_DFM_MIN_WALL prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_min_wall_violation_rule_form_reads_severity_field() {
        // DFMRule structure-instance form: severity read from `severity` field → Error diagnostic.
        let diags = diagnose(
            "min_wall_thickness",
            &[dfm_rule("Error")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM_MIN_WALL"),
            "msg: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_min_wall_violation_defaults_to_warning_when_tag_absent() {
        // No tag in args → default Warning.
        let diags = diagnose("min_wall_thickness", &[], &Value::Bool(true));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    #[test]
    fn diagnose_min_wall_conforms_emits_no_diagnostic() {
        // Bool(false) = wall meets the process minimum → no violation diagnostic.
        let diags = diagnose(
            "min_wall_thickness",
            &[dfm_sev("Warning")],
            &Value::Bool(false),
        );
        assert!(diags.is_empty(), "a conforming min_wall result surfaces no diagnostic");
    }

    #[test]
    fn diagnose_min_wall_undef_emits_nothing() {
        // Value::Undef (Indeterminate — BelowResolution/NoMeasurement/None) → empty.
        // C1/D5: never a false Violated on a non-Measured result.
        assert!(
            diagnose("min_wall_thickness", &[dfm_sev("Warning")], &Value::Undef).is_empty(),
            "Undef result (Indeterminate) emits nothing"
        );
    }

    // ─── step-3 RED: diagnose — min_feature_size_measure arm ─────────────────
    // These tests fail until step-4 adds the arm to `diagnose`.

    #[test]
    fn diagnose_min_feature_violation_warning_severity() {
        // Bool(true) = feature thinner than min_feature_size; DFMSeverity.Warning → one Warning diagnostic.
        let diags = diagnose(
            "min_feature_size_measure",
            &[dfm_sev("Warning")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1, "one min_feature violation diagnostic");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("W_DFM_MIN_FEATURE"),
            "Warning min_feature message carries W_DFM_MIN_FEATURE prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_min_feature_violation_error_severity() {
        let diags = diagnose(
            "min_feature_size_measure",
            &[dfm_sev("Error")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM_MIN_FEATURE"),
            "Error min_feature message carries E_DFM_MIN_FEATURE prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_min_feature_violation_info_severity() {
        let diags = diagnose(
            "min_feature_size_measure",
            &[dfm_sev("Info")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(
            diags[0].message.contains("I_DFM_MIN_FEATURE"),
            "Info min_feature message carries I_DFM_MIN_FEATURE prefix: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_min_feature_violation_rule_form_reads_severity_field() {
        // DFMRule structure-instance form: severity read from `severity` field → Error diagnostic.
        let diags = diagnose(
            "min_feature_size_measure",
            &[dfm_rule("Error")],
            &Value::Bool(true),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(
            diags[0].message.contains("E_DFM_MIN_FEATURE"),
            "msg: {}",
            diags[0].message
        );
    }

    #[test]
    fn diagnose_min_feature_violation_defaults_to_warning_when_tag_absent() {
        // No tag in args → default Warning.
        let diags = diagnose("min_feature_size_measure", &[], &Value::Bool(true));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    #[test]
    fn diagnose_min_feature_conforms_emits_no_diagnostic() {
        // Bool(false) = feature meets the process minimum → no violation diagnostic.
        let diags = diagnose(
            "min_feature_size_measure",
            &[dfm_sev("Warning")],
            &Value::Bool(false),
        );
        assert!(diags.is_empty(), "a conforming min_feature result surfaces no diagnostic");
    }

    #[test]
    fn diagnose_min_feature_undef_emits_nothing() {
        // Value::Undef (Indeterminate — BelowResolution/NoMeasurement/None) → empty.
        // C1/D5: never a false Violated on a non-Measured result.
        assert!(
            diagnose("min_feature_size_measure", &[dfm_sev("Warning")], &Value::Undef).is_empty(),
            "Undef result (Indeterminate) emits nothing"
        );
    }
}
