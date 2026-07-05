//! Trampoline for `solver::multi_case` — the `fn solve_load_cases`
//! @optimized target (task 4088, esc-4088-231 decision A).
//!
//! # Contract
//!
//! Receives the 6 `value_inputs` matching the `fn solve_load_cases` signature:
//!
//! ```text
//! [0] material : ConstitutiveLaw   (Value::StructureInstance)
//! [1] length   : Length            (Value::Scalar { dimension: LENGTH })
//! [2] width    : Length            (Value::Scalar { dimension: LENGTH })
//! [3] height   : Length            (Value::Scalar { dimension: LENGTH })
//! [4] cases    : List<LoadCase>    (Value::List of LoadCase StructureInstances)
//! [5] options  : ElasticOptions    (Value::StructureInstance — shared default)
//! ```
//!
//! For each `LoadCase` in `cases`:
//!   1. Extracts `name` (String), `loads` (Value), `supports` (Value).
//!   2. Resolves effective options: `LoadCase.options == Option(Some(X))` → `X`;
//!      otherwise inherits `value_inputs[5]` (mirrors `resolve_load_case_options`
//!      in `crates/reify-expr/src/lib.rs`).
//!   3. Calls `elastic_static::solve_elastic_static_trampoline` directly with
//!      `[material, length, width, height, loads, supports, effective_options]`.
//!   4. Collects `Completed.result` (a real Sampled-field `ElasticResult`
//!      `Value::StructureInstance`) into an inner `BTreeMap<String→Value>`.
//!
//! Returns `ComputeOutcome::Completed` with:
//! - `result`         — `Value::Map{"cases" -> Value::Map<String, ElasticResult>}`
//!   matching the shape expected by `detect_multi_case_result`,
//!   `extract_cases_map`, `multi_case_result_value`, and all existing consumers.
//! - `new_warm_state` — `None` (cross-case warm-state reuse re-homed to task 4152,
//!   esc-4088-231 decision A).
//! - `cost_per_byte`  — `None`.
//!
//! # Per-case warm state
//!
//! Each sub-solve is cold (`prior_warm_state = None`). Cross-case warm-state
//! and realization-cache reuse (the B9 intent) are explicitly deferred to task
//! 4152 (gated on 4088 + 4091).  In v1 every case invokes the `elastic_static`
//! trampoline fresh.
//!
//! # Error propagation
//!
//! - Empty `cases` list → `Failed` with a descriptive diagnostic (best-effort
//!   parity with `eval_solve_load_cases`).
//! - A case is not a `Value::StructureInstance` → `Failed`.
//! - `LoadCase.name` is not a `Value::String` → `Failed`.
//! - `LoadCase.loads` or `LoadCase.supports` absent → `Failed`.
//! - Sub-`Cancelled` → propagate `Cancelled` immediately.
//! - Sub-`Failed` → propagate `Failed` immediately (no partial results).
//!
//! # Placement rationale
//!
//! See `compute_targets/mod.rs` for why trampolines live in `reify-eval` rather
//! than `reify-stdlib` (cycle-free dependency graph).

use std::collections::BTreeMap;

use reify_core::Diagnostic;
use reify_ir::{OpaqueState, Value};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Trampoline for `solver::multi_case`.
///
/// Iterates the runtime `List<LoadCase>` in `value_inputs[4]`, dispatches
/// the `elastic_static` trampoline per case, and collects the per-case
/// `Completed.result` into a `Value::Map{"cases"→Map}` matching the
/// `MultiCaseResult` runtime shape.
///
/// See module-level docs for the full contract.
pub fn solve_multi_case_trampoline(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // ── (1) Validate arity ────────────────────────────────────────────────────
    if value_inputs.len() < 6 {
        return ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(format!(
                "solve_load_cases (solver::multi_case): expected 6 value_inputs, \
                 got {} — possible arity mismatch at dispatch site",
                value_inputs.len()
            ))],
            structured_detail: vec![],
        };
    }

    let material = &value_inputs[0];
    let length = &value_inputs[1];
    let width = &value_inputs[2];
    let height = &value_inputs[3];
    let cases_val = &value_inputs[4];
    let shared_options = &value_inputs[5];

    // ── (2) Unwrap cases list ─────────────────────────────────────────────────
    let cases = match cases_val {
        Value::List(v) => v,
        other => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "solve_load_cases (solver::multi_case): cases argument must be \
                     Value::List, got {:?}",
                    std::mem::discriminant(other)
                ))],
                structured_detail: vec![],
            };
        }
    };

    if cases.is_empty() {
        return ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(
                "Multi-load case analysis requires at least one LoadCase. \
                 Use solve_elastic_static for single-case analysis.",
            )],
            structured_detail: vec![],
        };
    }

    // ── (3) Iterate cases, dispatch elastic_static per case ──────────────────
    let mut inner: BTreeMap<Value, Value> = BTreeMap::new();
    let mut accumulated_diagnostics: Vec<Diagnostic> = Vec::new();

    for case_val in cases.iter() {
        // Cooperative cancellation: allow long batches to be interrupted between
        // sub-solves.  `solve_elastic_static_trampoline` ignores its own
        // cancellation handle, so this loop is the only place a multi-case
        // solve can be interrupted before completion.
        if cancellation.is_cancelled() {
            return ComputeOutcome::Cancelled;
        }

        // ── Extract LoadCase fields ───────────────────────────────────────────
        let data = match case_val {
            Value::StructureInstance(d) => d,
            other => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(format!(
                        "solve_load_cases (solver::multi_case): each case must be a \
                         LoadCase Value::StructureInstance, got {:?}",
                        std::mem::discriminant(other)
                    ))],
                    structured_detail: vec![],
                };
            }
        };

        let name = match data.fields.get(&"name".to_string()) {
            Some(Value::String(s)) => s.clone(),
            other => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(format!(
                        "solve_load_cases (solver::multi_case): LoadCase.name must be \
                         Value::String, got {:?}",
                        other.map(std::mem::discriminant)
                    ))],
                    structured_detail: vec![],
                };
            }
        };

        let loads = match data.fields.get(&"loads".to_string()) {
            Some(v) => v.clone(),
            None => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(format!(
                        "solve_load_cases (solver::multi_case): LoadCase \"{name}\" \
                         is missing the \"loads\" field"
                    ))],
                    structured_detail: vec![],
                };
            }
        };

        let supports = match data.fields.get(&"supports".to_string()) {
            Some(v) => v.clone(),
            None => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(format!(
                        "solve_load_cases (solver::multi_case): LoadCase \"{name}\" \
                         is missing the \"supports\" field"
                    ))],
                    structured_detail: vec![],
                };
            }
        };

        // ── Resolve effective options (mirrors resolve_load_case_options in
        //    crates/reify-expr/src/lib.rs) ─────────────────────────────────────
        // Option(Some(x)) → per-case override; anything else → shared default.
        let effective_options = match data.fields.get(&"options".to_string()) {
            Some(Value::Option(Some(per_case_opts))) => (**per_case_opts).clone(),
            _ => shared_options.clone(),
        };

        // ── Dispatch elastic_static for this case ─────────────────────────────
        // Each case is cold (prior_warm_state = None).  Cross-case warm-state
        // and realization-cache reuse are re-homed to task 4152.
        let per_case_inputs: Vec<Value> = vec![
            material.clone(),
            length.clone(),
            width.clone(),
            height.clone(),
            loads,
            supports,
            effective_options,
        ];

        let outcome = super::elastic_static::solve_elastic_static_trampoline(
            &per_case_inputs,
            realization_inputs,
            &Value::Undef,
            None, // cold — cross-case warm-state reuse re-homed to task 4152
            cancellation,
        );

        match outcome {
            ComputeOutcome::Completed {
                result,
                diagnostics: case_diags,
                ..
            } => {
                accumulated_diagnostics.extend(case_diags);
                let key = Value::String(name.clone());
                if inner.contains_key(&key) {
                    // Duplicate case name: emit a warning so the user can
                    // correct it; we overwrite the earlier result (matching
                    // BTreeMap::insert semantics) rather than silently losing it.
                    accumulated_diagnostics.push(Diagnostic::warning(format!(
                        "solve_load_cases (solver::multi_case): duplicate LoadCase \
                         name \"{name}\" — the earlier result for this case is \
                         overwritten; this is almost certainly a user error"
                    )));
                }
                inner.insert(key, result);
            }
            ComputeOutcome::Cancelled => return ComputeOutcome::Cancelled,
            ComputeOutcome::Failed {
                diagnostics: fail_diags,
                ..
            } => {
                // Prepend any warnings collected from earlier completed cases so
                // they are not silently discarded along with the failure.
                let mut all_diags = accumulated_diagnostics;
                all_diags.extend(fail_diags);
                return ComputeOutcome::Failed {
                    diagnostics: all_diags,
                    structured_detail: vec![],
                };
            }
        }
    }

    // ── (4) Wrap as MultiCaseResult shape ─────────────────────────────────────
    //
    // Emits Value::Map{"cases" -> Map<String, ElasticResult>} — the de-facto
    // runtime MultiCaseResult contract consumed by detect_multi_case_result,
    // extract_cases_map, multi_case_result_value, and all existing consumers.
    let mut outer: BTreeMap<Value, Value> = BTreeMap::new();
    outer.insert(Value::String("cases".to_string()), Value::Map(inner));

    ComputeOutcome::Completed {
        result: Value::Map(outer),
        new_warm_state: None, // cross-case warm-state donation re-homed to task 4152
        cost_per_byte: None,
        diagnostics: accumulated_diagnostics,
        structured_detail: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{ContentHash, DimensionVector, RealizationNodeId};
    use reify_ir::{
        ElementOrderTag, FieldSourceKind, PersistentMap, StructureInstanceData, StructureTypeId,
        VolumeMesh,
    };

    // ── Realized-mesh scaffold ────────────────────────────────────────────────
    //
    // Mirrors `elastic_static.rs`'s `make_box_tet_volume_mesh` / `vm_read_handle`
    // (task 4091). Recreated here because those live in that file's private
    // `#[cfg(test)]` module and cannot be reached cross-file.

    /// A structured `rx×ry×rz` box of `dims`, each cell split into 6 P1 tets
    /// (Freudenthal). AABB is exactly `[0,dx]×[0,dy]×[0,dz]`.
    fn make_box_tet_volume_mesh(dims: [f64; 3], reps: [usize; 3]) -> VolumeMesh {
        let [dx, dy, dz] = dims;
        let [rx, ry, rz] = reps;
        let (nx1, ny1, nz1) = (rx + 1, ry + 1, rz + 1);
        let node_idx = |ix: usize, iy: usize, iz: usize| -> usize { iz * ny1 * nx1 + iy * nx1 + ix };

        let mut vertices: Vec<f32> = Vec::with_capacity(nx1 * ny1 * nz1 * 3);
        for iz in 0..nz1 {
            for iy in 0..ny1 {
                for ix in 0..nx1 {
                    vertices.push((ix as f64 * dx / rx as f64) as f32);
                    vertices.push((iy as f64 * dy / ry as f64) as f32);
                    vertices.push((iz as f64 * dz / rz as f64) as f32);
                }
            }
        }

        let mut tet_indices: Vec<u32> = Vec::with_capacity(rx * ry * rz * 6 * 4);
        for hz in 0..rz {
            for hy in 0..ry {
                for hx in 0..rx {
                    let c = [
                        node_idx(hx, hy, hz),
                        node_idx(hx + 1, hy, hz),
                        node_idx(hx + 1, hy + 1, hz),
                        node_idx(hx, hy + 1, hz),
                        node_idx(hx, hy, hz + 1),
                        node_idx(hx + 1, hy, hz + 1),
                        node_idx(hx + 1, hy + 1, hz + 1),
                        node_idx(hx, hy + 1, hz + 1),
                    ];
                    let tets: [[usize; 4]; 6] = [
                        [c[0], c[1], c[2], c[6]],
                        [c[0], c[2], c[3], c[6]],
                        [c[0], c[5], c[1], c[6]],
                        [c[0], c[3], c[7], c[6]],
                        [c[0], c[4], c[5], c[6]],
                        [c[0], c[7], c[4], c[6]],
                    ];
                    for tet in tets {
                        for n in tet {
                            tet_indices.push(n as u32);
                        }
                    }
                }
            }
        }

        VolumeMesh {
            vertices,
            connectivity: reify_ir::VolumeConnectivity::Tet {
                indices: tet_indices,
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        }
    }

    /// Wrap a `VolumeMesh` in a `RealizationReadHandle` carrying
    /// `RealizedContent::VolumeMesh` — the shape the engine projects into a
    /// geometry-consuming compute node's `realization_inputs`.
    fn vm_read_handle(vm: VolumeMesh) -> RealizationReadHandle {
        RealizationReadHandle::new(
            RealizationNodeId::new("TestBody", 0),
            ContentHash(0),
            Some(crate::RealizedContent::VolumeMesh(std::sync::Arc::new(vm))),
        )
    }

    /// The pre-hydration `body : Solid` `Value::GeometryHandle` placeholder — the
    /// discriminator at `value_inputs[1]` that selects the body layout.
    fn make_body_handle() -> Value {
        Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("TestBody", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: None,
        }
    }

    /// Steel-like isotropic material StructureInstance (youngs_modulus +
    /// poisson_ratio; `classify_material` falls through to Isotropic).
    fn make_isotropic_material(youngs: f64, poisson: f64) -> Value {
        let fields: PersistentMap<String, Value> = [
            (
                "youngs_modulus".to_string(),
                Value::Scalar { si_value: youngs, dimension: DimensionVector::PRESSURE },
            ),
            ("poisson_ratio".to_string(), Value::Real(poisson)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "IsotropicElastic".to_string(),
            version: 1,
            fields,
        }))
    }

    /// A `LoadCase` StructureInstance with one `PointLoad{force}` +
    /// one `FixedSupport` (the fields the multi_case trampoline reads: name,
    /// loads, supports).
    fn make_load_case(name: &str, force_n: f64) -> Value {
        let load = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "PointLoad".to_string(),
            version: 1,
            fields: [("force".to_string(), Value::Real(force_n))].into_iter().collect(),
        }));
        let support = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "FixedSupport".to_string(),
            version: 1,
            fields: [].into_iter().collect(),
        }));
        let fields: PersistentMap<String, Value> = [
            ("name".to_string(), Value::String(name.to_string())),
            ("loads".to_string(), Value::List(vec![load])),
            ("supports".to_string(), Value::List(vec![support])),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "LoadCase".to_string(),
            version: 1,
            fields,
        }))
    }

    /// A shared-default `ElasticOptions(shell_force: Off)`.
    fn make_options() -> Value {
        let fields: PersistentMap<String, Value> = [
            (
                "shell_force".to_string(),
                Value::Enum {
                    type_name: "ShellForce".to_string(),
                    variant: "Off".to_string(),
                    payload: vec![],
                },
            ),
            ("shell_threshold".to_string(), Value::Real(0.2)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "ElasticOptions".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Extract the per-case `ElasticResult` field map from the
    /// `Value::Map{"cases"→Map<String,ElasticResult>}` MultiCaseResult value.
    fn case_result_fields(result: &Value, name: &str) -> PersistentMap<String, Value> {
        let cases = match result {
            Value::Map(outer) => outer
                .get(&Value::String("cases".to_string()))
                .unwrap_or_else(|| panic!("MultiCaseResult must carry a \"cases\" map")),
            other => panic!("expected MultiCaseResult Value::Map, got {other:?}"),
        };
        let case = match cases {
            Value::Map(inner) => inner
                .get(&Value::String(name.to_string()))
                .unwrap_or_else(|| panic!("cases map must contain case \"{name}\"")),
            other => panic!("expected cases Value::Map, got {other:?}"),
        };
        match case {
            Value::StructureInstance(d) => {
                assert_eq!(
                    d.type_name.as_str(),
                    "ElasticResult",
                    "each case must be an ElasticResult StructureInstance"
                );
                d.fields.clone()
            }
            other => panic!("expected ElasticResult StructureInstance, got {other:?}"),
        }
    }

    /// `bounds_max` of a Sampled field in an ElasticResult field map.
    fn field_bounds_max(fields: &PersistentMap<String, Value>, name: &str) -> Vec<f64> {
        let field = fields
            .get(&name.to_string())
            .unwrap_or_else(|| panic!("ElasticResult must carry a {name} field"));
        match field {
            Value::Field { source, lambda, .. } => {
                assert!(
                    matches!(source, FieldSourceKind::Sampled),
                    "{name} must be a Sampled field, got source {source:?}"
                );
                match lambda.as_ref() {
                    Value::SampledField(sf) => sf.bounds_max.clone(),
                    other => panic!("{name} lambda must be Value::SampledField, got {other:?}"),
                }
            }
            other => panic!("{name} must be Value::Field, got {other:?}"),
        }
    }

    // ── task 4870: multi_case body-overload realized-mesh forwarding (step-5 RED) ─

    /// step-5 RED (task 4870): the `body : Solid` overload of `solve_load_cases`
    /// presents its arg list to `solve_multi_case_trampoline` as the BODY layout
    ///   `[material, body(GeometryHandle), cases, options]`
    /// — the realized solid replaces the three scalar dims, riding at
    /// `value_inputs[1]` as a `Value::GeometryHandle` while its realized tet mesh
    /// arrives via `realization_inputs`.
    ///
    /// The trampoline must detect this layout, rebuild each per-case dispatch in
    /// the elastic BODY layout `[material, body, loads, supports, options]`, and
    /// forward `realization_inputs` UNCHANGED so all cases share ONE realized
    /// mesh. The realized AABB ([0,2]×[0,0.5]×[0,0.5]) differs from any plausible
    /// scalar-dims misread, so EVERY case's resampled §7a bounds_max must equal it
    /// — proving the shared realized mesh drove each sub-solve.
    ///
    /// RED today: the trampoline validates `value_inputs.len() < 6` (the 4-element
    /// body layout fails arity) and reads `value_inputs[1..4]` as
    /// length/width/height — so it never forwards the realized mesh per case.
    #[test]
    fn multi_case_body_overload_forwards_realized_mesh_to_each_case() {
        // BODY layout: [material, body(GeometryHandle), cases, options].
        let value_inputs = [
            make_isotropic_material(200e9, 0.3),
            make_body_handle(),
            Value::List(vec![
                make_load_case("operating", 1000.0),
                make_load_case("overload", 2000.0),
            ]),
            make_options(),
        ];

        // ONE realized mesh spanning [0,2]×[0,0.5]×[0,0.5], shared across cases.
        let realization_inputs =
            [vm_read_handle(make_box_tet_volume_mesh([2.0, 0.5, 0.5], [2, 1, 1]))];

        let cancellation = CancellationHandle::new();
        let outcome = solve_multi_case_trampoline(
            &value_inputs,
            &realization_inputs,
            &Value::Undef,
            None,
            &cancellation,
        );
        let result = match outcome {
            ComputeOutcome::Completed { result, .. } => result,
            other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
        };

        // EVERY case must solve on the REALIZED body AABB [2.0, 0.5, 0.5] — the
        // shared realization forwarded to each sub-solve — NOT a scalar-dims
        // synthetic box. If forwarding were broken for any case, its bounds_max
        // would diverge (or the sub-solve would fail).
        let realized_max = [2.0_f64, 0.5, 0.5];
        for case_name in ["operating", "overload"] {
            let fields = case_result_fields(&result, case_name);
            for field_name in ["displacement", "stress"] {
                let bounds_max = field_bounds_max(&fields, field_name);
                assert_eq!(bounds_max.len(), 3, "{field_name} bounds_max must be 3D");
                for axis in 0..3 {
                    assert!(
                        (bounds_max[axis] - realized_max[axis]).abs() < 1e-6,
                        "case \"{case_name}\" field {field_name} bounds_max[{axis}] = {} must \
                         equal the REALIZED body AABB {} — the multi_case body overload must \
                         forward the shared realized mesh to every case",
                        bounds_max[axis],
                        realized_max[axis],
                    );
                }
            }
        }
    }
}
