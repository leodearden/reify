//! Trampoline for `solver::tensegrity_load` — tensegrity load analysis with a
//! tension-only active set (PRD `docs/prds/v0_6/tensegrity-structures.md` §6 /
//! Tier-3 leaf T3b).
//!
//! # Contract
//!
//! Receives the six `value_inputs` matching the future `tensegrity_load`
//! signature:
//!
//! ```text
//! [0] structure       : Tensegrity              (Value::StructureInstance)
//! [1] prestress       : List<Force>             (List of Scalar{FORCE}) — one
//!                                                 per member, struts-then-cables
//! [2] youngs_modulus  : Scalar                  (broadcast E, shared section)
//! [3] area            : Scalar                  (broadcast A, shared section)
//! [4] loads           : List<Vector3<Force>>    (per-node external force)
//! [5] supports        : List<Int>               (fixed node indices)
//! ```
//!
//! It cracks the Tensegrity into node coordinates + member connectivity (struts
//! then cables, so `prestress` indexing is unambiguous — the same ordering the
//! form-find trampoline emits), broadcasts the shared `(E, A)` across members
//! (PRD §11 v1 decision), calls the pure kernel
//! [`reify_solver_elastic::tensegrity_load_analysis`], and rebuilds a
//! `TensegrityLoadResult` `Value::StructureInstance`.
//!
//! # Failure → diagnostic (PRD §8.1)
//!
//! Infeasible input returns [`ComputeOutcome::Failed`] carrying a single
//! `E_TensegrityLoadInfeasible` `Diagnostic::error` (the mnemonic lives in the
//! message text, mirroring the form-find trampoline). The trampoline never
//! panics and never returns a silently-wrong result.
//!
//! # StructureTypeId sentinel
//!
//! The trampoline has no `StructureRegistry` access, so the returned
//! `TensegrityLoadResult` uses `StructureTypeId(u32::MAX)` as a synthetic
//! sentinel — the same convention as the form-find / elastic-static trampolines.

use reify_core::{Diagnostic, DimensionVector};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_solver_elastic::{
    BarMember, BarSection, MemberKind, TensegrityLoadError, TensegrityLoadOptions,
    TensegrityLoadSolve, tensegrity_load_analysis,
};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Trampoline for `solver::tensegrity_load`. See the module doc for the
/// input/output contract.
pub fn solve_tensegrity_load_trampoline(
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
/// already-prefixed `E_TensegrityLoadInfeasible: …` message.
fn run(value_inputs: &[Value]) -> Result<Value, String> {
    if value_inputs.len() < 6 {
        return Err(format!(
            "E_TensegrityLoadInfeasible: tensegrity_load expects 6 inputs \
             (structure, prestress, youngs_modulus, area, loads, supports); got {}. \
             Let-bind all six call arguments so the ComputeNode captures them.",
            value_inputs.len()
        ));
    }

    let (nodes, member_pairs, kinds) = crack_tensegrity(&value_inputs[0])?;
    let prestress = crack_forces(&value_inputs[1], "prestress")?;
    let youngs_modulus = crack_scalar(&value_inputs[2], "youngs_modulus")?;
    let area = crack_scalar(&value_inputs[3], "area")?;
    let loads = crack_loads(&value_inputs[4])?;
    let supports = crack_supports(&value_inputs[5]);

    // Broadcast the shared (E, A) section across every member (v1 decision). A
    // fresh BarSection per member keeps this independent of BarSection's Copy/
    // Clone surface.
    let members: Vec<BarMember> = member_pairs
        .iter()
        .zip(kinds.iter())
        .zip(prestress.iter())
        .map(|((&pair, &kind), &pre)| BarMember {
            nodes: pair,
            kind,
            section: BarSection {
                youngs_modulus,
                area,
            },
            prestress: pre,
        })
        .collect();

    let options = TensegrityLoadOptions::default();
    let solve = tensegrity_load_analysis(&nodes, &members, &loads, &supports, &options)
        .map_err(|e| format!("E_TensegrityLoadInfeasible: {}", describe(e)))?;

    Ok(build_result(&solve))
}

// ── input cracking (mirrors form_find.rs Tensegrity field shape) ──────────────

/// Cracked Tensegrity topology: node coordinates, member index pairs in
/// struts-then-cables order, and the matching per-member [`MemberKind`] tags.
type CrackedTopology = (Vec<[f64; 3]>, Vec<(usize, usize)>, Vec<MemberKind>);

/// Crack the Tensegrity StructureInstance into node coordinates plus members in
/// struts-then-cables order with their matching [`MemberKind`] tags.
fn crack_tensegrity(v: &Value) -> Result<CrackedTopology, String> {
    let fields = match v {
        Value::StructureInstance(d) if d.type_name == "Tensegrity" => &d.fields,
        other => {
            return Err(format!(
                "E_TensegrityLoadInfeasible: tensegrity_load expected a Tensegrity structure, got {other:?}"
            ));
        }
    };

    let nodes = crack_nodes(fields.get(&"nodes".to_string()))?;
    let struts = crack_index_pairs(fields.get(&"struts".to_string()), "struts")?;
    let cables = crack_index_pairs(fields.get(&"cables".to_string()), "cables")?;

    // Struts first, then cables — `prestress[i]` aligns with this order.
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
                "E_TensegrityLoadInfeasible: Tensegrity.nodes must be a list of points, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, node) in list.iter().enumerate() {
        match node {
            Value::Point(c) | Value::Vector(c) if c.len() == 3 => {
                let bad = || {
                    format!(
                        "E_TensegrityLoadInfeasible: Tensegrity.nodes[{i}] has a non-numeric coordinate"
                    )
                };
                out.push([
                    scalar_f64(&c[0]).ok_or_else(bad)?,
                    scalar_f64(&c[1]).ok_or_else(bad)?,
                    scalar_f64(&c[2]).ok_or_else(bad)?,
                ]);
            }
            other => {
                return Err(format!(
                    "E_TensegrityLoadInfeasible: Tensegrity.nodes[{i}] must be a 3-component point, got {other:?}"
                ));
            }
        }
    }
    Ok(out)
}

/// Crack a `List<List<Int>>` connectivity field into index pairs. Range-checking
/// against the node count is left to the kernel (step-12 adds a located
/// trampoline-level guard).
fn crack_index_pairs(v: Option<&Value>, field: &str) -> Result<Vec<(usize, usize)>, String> {
    let list = match v {
        Some(Value::List(pairs)) => pairs,
        other => {
            return Err(format!(
                "E_TensegrityLoadInfeasible: Tensegrity.{field} must be a list of index pairs, got {other:?}"
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
                        "E_TensegrityLoadInfeasible: Tensegrity.{field}[{i}] must be two integer indices"
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "E_TensegrityLoadInfeasible: Tensegrity.{field}[{i}] must be a 2-element index list"
                ));
            }
        };
        out.push((from as usize, to as usize));
    }
    Ok(out)
}

/// Crack a `List<Force>` (accepting bare Real or any dimensioned Scalar entries).
fn crack_forces(v: &Value, what: &str) -> Result<Vec<f64>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "E_TensegrityLoadInfeasible: {what} must be a list of forces, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        match scalar_f64(item) {
            Some(x) => out.push(x),
            None => {
                return Err(format!(
                    "E_TensegrityLoadInfeasible: {what}[{i}] must be a force scalar, got {item:?}"
                ));
            }
        }
    }
    Ok(out)
}

/// Crack a single dimensioned `Scalar` (or bare `Real`) into an f64.
fn crack_scalar(v: &Value, what: &str) -> Result<f64, String> {
    scalar_f64(v).ok_or_else(|| {
        format!("E_TensegrityLoadInfeasible: {what} must be a scalar, got {v:?}")
    })
}

/// Crack `loads` (a `List<Vector3<Force>>`) into per-node `[f64; 3]` force
/// vectors. Length-checking against the node count is left to the kernel.
fn crack_loads(v: &Value) -> Result<Vec<[f64; 3]>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "E_TensegrityLoadInfeasible: loads must be a list of 3-component force vectors, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        match item {
            Value::Vector(c) | Value::Point(c) if c.len() == 3 => {
                let bad = || {
                    format!("E_TensegrityLoadInfeasible: loads[{i}] has a non-numeric component")
                };
                out.push([
                    scalar_f64(&c[0]).ok_or_else(bad)?,
                    scalar_f64(&c[1]).ok_or_else(bad)?,
                    scalar_f64(&c[2]).ok_or_else(bad)?,
                ]);
            }
            other => {
                return Err(format!(
                    "E_TensegrityLoadInfeasible: loads[{i}] must be a 3-component force vector, got {other:?}"
                ));
            }
        }
    }
    Ok(out)
}

/// Crack a `List<Int>` of support node indices. Range-checking is left to the
/// kernel (step-12 adds a located trampoline-level guard).
fn crack_supports(v: &Value) -> Vec<usize> {
    match v {
        Value::List(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::Int(a) => Some(*a as usize),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract an f64 from a Scalar (any dimension) or a bare Real.
fn scalar_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, .. } => Some(*si_value),
        Value::Real(r) => Some(*r),
        _ => None,
    }
}

// ── result construction ──────────────────────────────────────────────────────

/// Build the `TensegrityLoadResult` `Value::StructureInstance` from the kernel
/// solve. Routed through the shared `compute_targets` builders so a future
/// dimension/encoding change is a single-point edit.
fn build_result(solve: &TensegrityLoadSolve) -> Value {
    let displacements: Vec<Value> = solve
        .displacements
        .iter()
        .map(|&u| super::vec3_length(u))
        .collect();
    let member_forces = super::scalar_list(&solve.member_forces, DimensionVector::FORCE);
    let member_force_deltas =
        super::scalar_list(&solve.member_force_deltas, DimensionVector::FORCE);
    // step-10 stub: the slack mask is emitted all-`false` here (correct for the
    // no-slack happy path). step-12 wires the real `solve.slack` mask so the
    // slackening case flags its dropped cable.
    let slack: Vec<Value> = solve.slack.iter().map(|_| Value::Bool(false)).collect();

    let fields: PersistentMap<String, Value> = [
        ("displacements".to_string(), Value::List(displacements)),
        ("member_forces".to_string(), Value::List(member_forces)),
        (
            "member_force_deltas".to_string(),
            Value::List(member_force_deltas),
        ),
        ("slack".to_string(), Value::List(slack)),
        ("converged".to_string(), Value::Bool(solve.converged)),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "TensegrityLoadResult".to_string(),
        version: 1,
        fields,
    }))
}

/// Human-readable cause for a kernel [`TensegrityLoadError`] (appended after the
/// `E_TensegrityLoadInfeasible:` prefix).
///
/// step-10 placeholder: a `Debug` rendering of the variant. step-12 replaces
/// this with friendly per-variant phrases (the `form_find::describe()`
/// discipline).
fn describe(e: TensegrityLoadError) -> String {
    format!("{e:?}")
}
