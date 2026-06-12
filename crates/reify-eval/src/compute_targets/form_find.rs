//! Trampoline for `solver::form_find` — anchored Force-Density form-finding
//! (PRD `docs/prds/v0_6/tensegrity-structures.md` §4 / Tier-1 leaf T1a).
//!
//! # Contract
//!
//! Receives the three `value_inputs` matching the `form_find` signature
//! (`tensegrity.ri`):
//!
//! ```text
//! [0] structure        : Tensegrity      (Value::StructureInstance)
//! [1] force_densities   : List<Real>      (Value::List of Value::Real)
//! [2] anchors           : List<Int>       (Value::List of Value::Int)
//! ```
//!
//! It cracks the Tensegrity into node coordinates + member connectivity (struts
//! then cables, so `force_densities` indexing is unambiguous — the same ordering
//! `tensegrity_wires` emits), calls the pure FD kernel
//! [`reify_solver_elastic::form_find_anchored`], and rebuilds a `FormFindResult`
//! `Value::StructureInstance`.
//!
//! # Failure → diagnostic (PRD §8.1)
//!
//! Infeasible input — a malformed `Value`, an out-of-range member/anchor index,
//! a per-member sign violation, a singular reduced system, or an empty free set
//! — returns [`ComputeOutcome::Failed`] carrying a single `E_FormFindInfeasible`
//! `Diagnostic::error`. The mnemonic lives in the message text (not a
//! `reify-core` `DiagnosticCode` variant — that enum is a closed compiler-origin
//! set, out of scope here). The trampoline never panics and never returns a
//! silently-wrong result.
//!
//! # StructureTypeId sentinel
//!
//! The trampoline has no `StructureRegistry` access, so the returned
//! `FormFindResult` uses `StructureTypeId(u32::MAX)` as a synthetic sentinel —
//! the same convention as the elastic-static / buckling trampolines.
//!
//! # Placement rationale
//!
//! See `compute_targets/mod.rs` for why trampolines live in `reify-eval` rather
//! than the PRD-preferred `reify-stdlib` (the `reify-eval → reify-expr →
//! reify-stdlib` dep chain rules the latter out).

use reify_core::{Diagnostic, DimensionVector};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_solver_elastic::{
    FormFindError, FormFindSolve, ForceDensitySpec, FreeFormError, MemberKind, form_find_anchored,
    form_find_free,
};

use super::tensegrity_crack::{check_index, crack_index_pairs, crack_nodes, scalar_f64};
use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Diagnostic mnemonic for this trampoline, threaded into the shared
/// `tensegrity_crack` helpers so their located errors carry the same prefix as
/// the inline guards below.
const CODE: &str = "E_FormFindInfeasible";

/// Trampoline for `solver::form_find`. See the module doc for the input/output
/// contract.
pub fn solve_form_find_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    match run(value_inputs) {
        Ok(result) => ComputeOutcome::Completed {
            result,
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        },
        Err(message) => ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(message)],
        },
    }
}

/// Crack the inputs, run the kernel, and build the result. Every failure is an
/// already-prefixed `E_FormFindInfeasible: …` message.
fn run(value_inputs: &[Value]) -> Result<Value, String> {
    // Shallow-walk capture (engine_eval.rs) only keeps direct ValueRef(let-cell)
    // args, so a caller that fails to let-bind one of the three arguments shows
    // up here as a short input slice rather than mis-indexed data.
    if value_inputs.len() < 3 {
        return Err(format!(
            "E_FormFindInfeasible: form_find expects 3 inputs \
             (structure, force_densities, anchors); got {}. Let-bind all three \
             call arguments so the ComputeNode captures them.",
            value_inputs.len()
        ));
    }

    let (nodes, members, kinds) = crack_tensegrity(&value_inputs[0])?;
    let q = crack_reals(&value_inputs[1], "force_densities")?;
    let anchors = crack_anchors(&value_inputs[2], nodes.len())?;

    let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
        .map_err(|e| format!("E_FormFindInfeasible: {}", describe(e)))?;

    Ok(build_result(&solve))
}

// ── input cracking ────────────────────────────────────────────────────────────
//
// Topology cracking (nodes, struts/cables index pairs, scalar/index validation)
// is shared with the tensegrity-load trampoline via `super::tensegrity_crack`.
// The crackers below are specific to this trampoline's form-find inputs
// (anchors, force-density reals, group ids / reference group).

/// Cracked Tensegrity topology: node coordinates, member index pairs in
/// struts-then-cables order, and the matching per-member [`MemberKind`] tags.
type CrackedTopology = (Vec<[f64; 3]>, Vec<(usize, usize)>, Vec<MemberKind>);

/// Crack the Tensegrity StructureInstance into node coordinates plus members in
/// struts-then-cables order with their matching [`MemberKind`] tags. Member
/// indices are range-checked here so the kernel never indexes out of bounds.
fn crack_tensegrity(v: &Value) -> Result<CrackedTopology, String> {
    let fields = match v {
        Value::StructureInstance(d) if d.type_name == "Tensegrity" => &d.fields,
        other => {
            return Err(format!(
                "E_FormFindInfeasible: form_find expected a Tensegrity structure, got {other:?}"
            ));
        }
    };

    let nodes = crack_nodes(fields.get(&"nodes".to_string()), CODE)?;
    let n = nodes.len();
    let struts = crack_index_pairs(fields.get(&"struts".to_string()), "struts", n, CODE)?;
    let cables = crack_index_pairs(fields.get(&"cables".to_string()), "cables", n, CODE)?;

    // Struts first, then cables — `force_densities[i]` aligns with this order.
    let mut members = Vec::with_capacity(struts.len() + cables.len());
    let mut kinds = Vec::with_capacity(struts.len() + cables.len());
    for pair in struts {
        members.push(pair);
        kinds.push(MemberKind::Strut);
    }
    for pair in cables {
        members.push(pair);
        kinds.push(MemberKind::Cable);
    }
    Ok((nodes, members, kinds))
}

/// Crack a `List<Int>` of node indices (the anchors), range-checked against `n`.
fn crack_anchors(v: &Value, n: usize) -> Result<Vec<usize>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "E_FormFindInfeasible: anchors must be a list of integer node indices, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        match item {
            Value::Int(a) => out.push(check_index(*a, n, &format!("anchors[{i}]"), CODE)?),
            other => {
                return Err(format!(
                    "E_FormFindInfeasible: anchors[{i}] must be an integer node index, got {other:?}"
                ));
            }
        }
    }
    Ok(out)
}

/// Crack a `List<Real>` (accepting bare Real or dimensionless Scalar entries).
fn crack_reals(v: &Value, what: &str) -> Result<Vec<f64>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "E_FormFindInfeasible: {what} must be a list of reals, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        match scalar_f64(item) {
            Some(x) => out.push(x),
            None => {
                return Err(format!(
                    "E_FormFindInfeasible: {what}[{i}] must be a real, got {item:?}"
                ));
            }
        }
    }
    Ok(out)
}

// ── result construction ──────────────────────────────────────────────────────

/// Build the `FormFindResult` `Value::StructureInstance` from the kernel solve.
///
/// The Scalar/Point/List encoding is routed through the shared
/// `compute_targets` builders (`super::point3_length`, `super::scalar_list`) so
/// a future dimension/encoding change is a single-point edit — the same
/// discipline the elastic-static / buckling trampolines use for their field
/// helpers.
fn build_result(solve: &FormFindSolve) -> Value {
    let nodes: Vec<Value> = solve
        .nodes
        .iter()
        .map(|&p| super::point3_length(p))
        .collect();
    // member_forces Nᵢ = qᵢ·Lᵢ are forces (N/m · m), so FORCE-dimensioned.
    let member_forces = super::scalar_list(&solve.member_forces, DimensionVector::FORCE);
    let force_densities: Vec<Value> = solve
        .force_densities
        .iter()
        .map(|&q| Value::Real(q))
        .collect();

    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(), Value::List(nodes)),
        ("member_forces".to_string(), Value::List(member_forces)),
        ("force_densities".to_string(), Value::List(force_densities)),
        ("converged".to_string(), Value::Bool(solve.converged)),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "FormFindResult".to_string(),
        version: 1,
        fields,
    }))
}

/// Human-readable cause for a kernel [`FormFindError`] (appended after the
/// `E_FormFindInfeasible:` prefix).
fn describe(e: FormFindError) -> &'static str {
    match e {
        FormFindError::SignViolation => {
            "sign violation — every cable requires q > 0 (tension) and every strut requires q < 0 (compression)"
        }
        FormFindError::SingularReducedStiffness => {
            "singular reduced stiffness — the free-node system is rank-deficient \
             (a free node with no member path to an anchor, or a disconnected component)"
        }
        FormFindError::EmptyFreeSet => {
            "every node is anchored — there is no free node to solve for"
        }
        FormFindError::DimensionMismatch => {
            "force_densities length does not match the member count (struts + cables)"
        }
        FormFindError::DegenerateTriangle => {
            "degenerate surface triangle — collinear or zero-area corners make the \
             cotangent weights diverge (2·Area → 0); check the surfaces connectivity \
             and node coordinates"
        }
        FormFindError::NonTensionSurfaceStress => {
            "non-tension surface stress — every membrane surface requires σ > 0 \
             (tension); a slack or compressed surface (σ ≤ 0) is infeasible prestress"
        }
        FormFindError::SurfaceCountMismatch => {
            "surface_stresses length does not match the surface count — each triangle \
             in `surfaces` needs exactly one isotropic σ"
        }
    }
}

// ── Free-standing (T1b) trampoline ────────────────────────────────────────────
//
// Receives 4 value_inputs matching the `form_find_free` signature:
//   [0] structure        : Tensegrity      (Value::StructureInstance)
//   [1] group_ids        : List<Int>       (Value::List of Value::Int)
//   [2] seed_ratios      : List<Real>      (Value::List of Value::Real)
//   [3] reference_group  : Int             (Value::Int)
//
// Cracks the inputs into a ForceDensitySpec::GroupRatios and calls the
// free-standing FD kernel (reify_solver_elastic::form_find_free). On success,
// builds a FormFindResult (dropping the kernel's nullity field, which is not
// part of the PRD §4 DSL-facing output). On failure, maps FreeFormError to an
// E_FormFindInfeasible diagnostic.

/// Trampoline for `solver::form_find_free` — free-standing (T1b) form-finding.
pub fn solve_form_find_free_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    match run_free(value_inputs) {
        Ok(result) => ComputeOutcome::Completed {
            result,
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![],
        },
        Err(message) => ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(message)],
        },
    }
}

/// Crack the free-standing inputs, run the kernel, and build the result.
fn run_free(value_inputs: &[Value]) -> Result<Value, String> {
    if value_inputs.len() < 4 {
        return Err(format!(
            "E_FormFindInfeasible: form_find_free expects 4 inputs \
             (structure, group_ids, seed_ratios, reference_group); got {}. \
             Let-bind all four call arguments so the ComputeNode captures them.",
            value_inputs.len()
        ));
    }

    let (nodes, members, kinds) = crack_tensegrity(&value_inputs[0])?;
    let raw_group_ids = crack_group_ids(&value_inputs[1])?;
    let seed_ratios = crack_reals(&value_inputs[2], "seed_ratios")?;
    let reference_group = crack_usize(&value_inputs[3], "reference_group")?;

    let spec = ForceDensitySpec::GroupRatios {
        group_ids: raw_group_ids,
        seed_ratios,
        reference_group,
    };

    let result =
        form_find_free(&nodes, &members, &kinds, &spec).map_err(|e| {
            format!("E_FormFindInfeasible: {}", describe_free(e))
        })?;

    Ok(build_result_free(&result.nodes, &result.member_forces, &result.force_densities, result.converged))
}

/// Crack a `List<Int>` of group ids into a `Vec<usize>` (all non-negative).
fn crack_group_ids(v: &Value) -> Result<Vec<usize>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "E_FormFindInfeasible: group_ids must be a list of integer group ids, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        match item {
            Value::Int(a) if *a >= 0 => out.push(*a as usize),
            Value::Int(a) => {
                return Err(format!(
                    "E_FormFindInfeasible: group_ids[{i}] must be non-negative, got {a}"
                ));
            }
            other => {
                return Err(format!(
                    "E_FormFindInfeasible: group_ids[{i}] must be an integer, got {other:?}"
                ));
            }
        }
    }
    Ok(out)
}

/// Crack an `Int` value into a `usize` (non-negative).
fn crack_usize(v: &Value, what: &str) -> Result<usize, String> {
    match v {
        Value::Int(a) if *a >= 0 => Ok(*a as usize),
        Value::Int(a) => Err(format!(
            "E_FormFindInfeasible: {what} must be non-negative, got {a}"
        )),
        other => Err(format!(
            "E_FormFindInfeasible: {what} must be an integer, got {other:?}"
        )),
    }
}

/// Build the `FormFindResult` `Value::StructureInstance` from the free solve.
/// Mirrors `build_result` but takes pre-extracted fields (nullity is dropped —
/// it is not part of the PRD §4 DSL-facing FormFindResult shape).
fn build_result_free(
    nodes: &[[f64; 3]],
    member_forces: &[f64],
    force_densities: &[f64],
    converged: bool,
) -> Value {
    let nodes_val: Vec<Value> = nodes.iter().map(|&p| super::point3_length(p)).collect();
    let forces_val = super::scalar_list(member_forces, DimensionVector::FORCE);
    let fds_val: Vec<Value> = force_densities.iter().map(|&q| Value::Real(q)).collect();

    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(), Value::List(nodes_val)),
        ("member_forces".to_string(), Value::List(forces_val)),
        ("force_densities".to_string(), Value::List(fds_val)),
        ("converged".to_string(), Value::Bool(converged)),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "FormFindResult".to_string(),
        version: 1,
        fields,
    }))
}

/// Human-readable cause for a kernel [`FreeFormError`] (appended after the
/// `E_FormFindInfeasible:` prefix).
fn describe_free(e: FreeFormError) -> &'static str {
    match e {
        FreeFormError::SignViolation => {
            "sign violation — cable groups require positive seed ratios (q > 0) \
             and strut groups require negative seed ratios (q < 0)"
        }
        FreeFormError::NullityMismatch { .. } => {
            "nullity mismatch — the force-density matrix D must be rank-deficient by \
             exactly d+1 = 4 for a valid 3-D free-standing form; the supplied q does not \
             achieve this"
        }
        FreeFormError::DimensionMismatch => {
            "dimension mismatch — group_ids, seed_ratios, or reference_group length \
             disagrees with the member count or contains out-of-range ids"
        }
        FreeFormError::SearchDidNotConverge => {
            "search did not converge — the adaptive GroupRatios search exhausted its \
             iteration budget without reaching a nullity-4 configuration; verify that \
             the seed signs are consistent (struts compressive, cables tensile)"
        }
        FreeFormError::SingularRecovery => {
            "singular recovery — null-space coordinate recovery failed to produce a \
             3-D realisation; try a less degenerate initial node placement"
        }
    }
}
