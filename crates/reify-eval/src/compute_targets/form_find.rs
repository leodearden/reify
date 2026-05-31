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
use reify_solver_elastic::{FormFindError, FormFindSolve, MemberKind, form_find_anchored};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

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

// ── input cracking (reuses the tensegrity.rs Tensegrity field shape) ──────────

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

    let nodes = crack_nodes(fields.get(&"nodes".to_string()))?;
    let n = nodes.len();
    let struts = crack_index_pairs(fields.get(&"struts".to_string()), "struts", n)?;
    let cables = crack_index_pairs(fields.get(&"cables".to_string()), "cables", n)?;

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

/// Crack `Tensegrity.nodes` (a `List<Point>`) into `[f64; 3]` SI coordinates.
fn crack_nodes(v: Option<&Value>) -> Result<Vec<[f64; 3]>, String> {
    let list = match v {
        Some(Value::List(ns)) => ns,
        other => {
            return Err(format!(
                "E_FormFindInfeasible: Tensegrity.nodes must be a list of points, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, node) in list.iter().enumerate() {
        match node {
            Value::Point(c) if c.len() == 3 => {
                let bad = || {
                    format!("E_FormFindInfeasible: Tensegrity.nodes[{i}] has a non-numeric coordinate")
                };
                out.push([
                    scalar_f64(&c[0]).ok_or_else(bad)?,
                    scalar_f64(&c[1]).ok_or_else(bad)?,
                    scalar_f64(&c[2]).ok_or_else(bad)?,
                ]);
            }
            other => {
                return Err(format!(
                    "E_FormFindInfeasible: Tensegrity.nodes[{i}] must be a 3-component point, got {other:?}"
                ));
            }
        }
    }
    Ok(out)
}

/// Crack a `List<List<Int>>` connectivity field into range-checked index pairs.
fn crack_index_pairs(
    v: Option<&Value>,
    field: &str,
    n: usize,
) -> Result<Vec<(usize, usize)>, String> {
    let list = match v {
        Some(Value::List(pairs)) => pairs,
        other => {
            return Err(format!(
                "E_FormFindInfeasible: Tensegrity.{field} must be a list of index pairs, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, pair) in list.iter().enumerate() {
        let (from, to) = match pair {
            Value::List(idx) if idx.len() == 2 => match (&idx[0], &idx[1]) {
                (Value::Int(a), Value::Int(b)) => (*a, *b),
                _ => {
                    return Err(format!(
                        "E_FormFindInfeasible: Tensegrity.{field}[{i}] must be two integer indices"
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "E_FormFindInfeasible: Tensegrity.{field}[{i}] must be a 2-element index list"
                ));
            }
        };
        out.push((
            check_index(from, n, &format!("Tensegrity.{field}[{i}] start"))?,
            check_index(to, n, &format!("Tensegrity.{field}[{i}] end"))?,
        ));
    }
    Ok(out)
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
            Value::Int(a) => out.push(check_index(*a, n, &format!("anchors[{i}]"))?),
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

/// Extract an f64 from a Scalar (any dimension) or a bare Real. `point3(1m, …)`
/// lowers each component to `Scalar{LENGTH}`; `[1.0, …]` lowers to `Real`.
fn scalar_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, .. } => Some(*si_value),
        Value::Real(r) => Some(*r),
        _ => None,
    }
}

/// Range-check a signed node index against `0..n`, with a located error.
fn check_index(idx: i64, n: usize, ctx: &str) -> Result<usize, String> {
    if idx < 0 || idx as usize >= n {
        return Err(format!(
            "E_FormFindInfeasible: {ctx} index {idx} is out of range 0..{n}"
        ));
    }
    Ok(idx as usize)
}

// ── result construction ──────────────────────────────────────────────────────

/// Build the `FormFindResult` `Value::StructureInstance` from the kernel solve.
fn build_result(solve: &FormFindSolve) -> Value {
    let nodes: Vec<Value> = solve
        .nodes
        .iter()
        .map(|p| Value::Point(vec![length(p[0]), length(p[1]), length(p[2])]))
        .collect();
    // member_forces Nᵢ = qᵢ·Lᵢ are forces (N/m · m), so FORCE-dimensioned.
    let member_forces: Vec<Value> = solve
        .member_forces
        .iter()
        .map(|f| Value::Scalar { si_value: *f, dimension: DimensionVector::FORCE })
        .collect();
    let force_densities: Vec<Value> = solve.force_densities.iter().map(|q| Value::Real(*q)).collect();

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

/// A Length-dimensioned coordinate Scalar (SI metres).
fn length(m: f64) -> Value {
    Value::Scalar { si_value: m, dimension: DimensionVector::LENGTH }
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
    }
}
