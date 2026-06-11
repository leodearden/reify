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
//! [2] youngs_modulus  : Pressure                (broadcast E, shared section)
//! [3] area            : Area                    (broadcast A, shared section)
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
    let supports = crack_supports(&value_inputs[5], nodes.len())?;

    // Length guards — reject silently-wrong inputs before building members. The
    // member/kind/prestress zip below would otherwise truncate to the shortest,
    // quietly solving a smaller problem than the caller described.
    if prestress.len() != member_pairs.len() {
        return Err(format!(
            "E_TensegrityLoadInfeasible: prestress length {} does not match the member \
             count {} (struts + cables); supply one prestress force per member.",
            prestress.len(),
            member_pairs.len()
        ));
    }
    if loads.len() != nodes.len() {
        return Err(format!(
            "E_TensegrityLoadInfeasible: loads length {} does not match the node count \
             {}; supply one force vector per node.",
            loads.len(),
            nodes.len()
        ));
    }

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
    let n = nodes.len();
    let struts = crack_index_pairs(fields.get(&"struts".to_string()), n, "struts")?;
    let cables = crack_index_pairs(fields.get(&"cables".to_string()), n, "cables")?;

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

/// Crack a `List<List<Int>>` connectivity field into index pairs, range-checking
/// each endpoint against the node count `n` so an out-of-range member index is a
/// located trampoline-level error rather than a generic kernel `DimensionMismatch`.
fn crack_index_pairs(
    v: Option<&Value>,
    n: usize,
    field: &str,
) -> Result<Vec<(usize, usize)>, String> {
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
        out.push((
            check_index(from, n, &format!("Tensegrity.{field}[{i}] start"))?,
            check_index(to, n, &format!("Tensegrity.{field}[{i}] end"))?,
        ));
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
/// vectors. The loads-vs-nodes length check is performed in [`run`] (the
/// trampoline) so a mismatch surfaces as a *located*
/// `E_TensegrityLoadInfeasible` error; the kernel's own
/// `loads.len() != nodes.len()` guard is a redundant backstop. This cracker only
/// validates per-entry shape (3-component, numeric).
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

/// Crack a `List<Int>` of support node indices, range-checking each against the
/// node count `n`. A non-list, a non-integer entry, or an out-of-range index is
/// a located trampoline-level error (so e.g. a support index past the node array
/// surfaces "… index 99 is out of range 0..3" rather than a generic kernel
/// `DimensionMismatch`).
fn crack_supports(v: &Value, n: usize) -> Result<Vec<usize>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "E_TensegrityLoadInfeasible: supports must be a list of integer node indices, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        match item {
            Value::Int(a) => out.push(check_index(*a, n, &format!("supports[{i}]"))?),
            other => {
                return Err(format!(
                    "E_TensegrityLoadInfeasible: supports[{i}] must be an integer node index, got {other:?}"
                ));
            }
        }
    }
    Ok(out)
}

/// Extract an f64 from a Scalar (any dimension) or a bare Real.
fn scalar_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, .. } => Some(*si_value),
        Value::Real(r) => Some(*r),
        _ => None,
    }
}

/// Range-check a signed node index against `0..n`, returning a located
/// `… index N is out of range 0..n` error (the `form_find::check_index`
/// discipline). A negative index — or one at/after the node count — is rejected
/// here rather than wrapping to a huge `usize` and indexing out of bounds in the
/// kernel.
fn check_index(idx: i64, n: usize, ctx: &str) -> Result<usize, String> {
    if idx < 0 || idx as usize >= n {
        return Err(format!(
            "E_TensegrityLoadInfeasible: {ctx} index {idx} is out of range 0..{n}"
        ));
    }
    Ok(idx as usize)
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
    // The tension-only active set marks dropped (slack) cables in `solve.slack`;
    // surface that mask verbatim. The kernel already zeroes a slack cable's
    // `member_forces` entry (its total tension fell to 0), so the FORCE list
    // above carries the zeroed force without any extra handling here.
    let slack: Vec<Value> = solve.slack.iter().map(|&s| Value::Bool(s)).collect();

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
/// `E_TensegrityLoadInfeasible:` prefix), mirroring the `form_find::describe()`
/// discipline. Returns `String` (not `&'static str`) because
/// [`TensegrityLoadError::ActiveSetDidNotConverge`] interpolates its iteration
/// count. Most of these are pre-empted by the located trampoline guards above;
/// the arms remain so every kernel-side variant maps to a friendly phrase
/// instead of a `Debug` rendering.
fn describe(e: TensegrityLoadError) -> String {
    match e {
        TensegrityLoadError::DimensionMismatch => {
            "input dimensions disagree — loads must supply one force vector per node \
             and every member endpoint / support index must lie within the node set"
                .to_string()
        }
        TensegrityLoadError::EmptyFreeSet => {
            "every node is anchored — there is no free node to solve for".to_string()
        }
        TensegrityLoadError::SingularSystem => {
            "singular tangent system — the inner CG solve did not converge (a free \
             node with no taut load path to a support, or an ill-conditioned reduced \
             stiffness once slack cables were dropped)"
                .to_string()
        }
        TensegrityLoadError::ActiveSetDidNotConverge { iterations } => format!(
            "tension-only active set did not reach a fixed point within {iterations} \
             passes (the PRD §11 Q5 cap) — drop-only monotonicity should converge in \
             at most #cables passes, so this signals a non-monotone active-set policy"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Direct coverage for the `describe()` cause arms. Three of the four
    // (`DimensionMismatch`, `SingularSystem`, `ActiveSetDidNotConverge`) are not
    // reachable through the producer-side trampoline integration tests, so their
    // phrasing would otherwise be unprotected:
    //   * `DimensionMismatch` — the trampoline's own *located* length/range guards
    //     (`run`) pre-empt it before the kernel is called.
    //   * `SingularSystem` — a free node with no taut load path has a zero/missing
    //     stiffness diagonal, which trips the kernel's Jacobi-preconditioner
    //     `assert!` (a panic), not this variant; reaching it needs an
    //     ill-conditioned-but-diagonal-present system that exhausts the CG cap,
    //     which is impractical to construct as a fast, non-flaky golden.
    //   * `ActiveSetDidNotConverge` — `run` always passes
    //     `TensegrityLoadOptions::default()` (cap = 64), so no crafted Value input
    //     can drive the active set past the cap.
    // These unit tests pin each arm's wording — and, critically, the `{iterations}`
    // interpolation — so a format regression fails here instead of silently
    // shipping a garbled diagnostic. (`EmptyFreeSet` *is* exercised end-to-end by
    // `tests/tensegrity_t3b_load.rs::trampoline_all_anchored_is_failed_empty_free_set`;
    // it is included here too for a complete, single-glance phrase map.)

    #[test]
    fn describe_dimension_mismatch_phrase() {
        let msg = describe(TensegrityLoadError::DimensionMismatch);
        assert!(
            msg.contains("input dimensions disagree"),
            "DimensionMismatch describe() phrase changed: {msg:?}",
        );
    }

    #[test]
    fn describe_empty_free_set_phrase() {
        let msg = describe(TensegrityLoadError::EmptyFreeSet);
        assert!(
            msg.contains("every node is anchored"),
            "EmptyFreeSet describe() phrase changed: {msg:?}",
        );
    }

    #[test]
    fn describe_singular_system_phrase() {
        let msg = describe(TensegrityLoadError::SingularSystem);
        assert!(
            msg.contains("singular tangent system"),
            "SingularSystem describe() phrase changed: {msg:?}",
        );
        assert!(
            msg.contains("did not converge"),
            "SingularSystem describe() should name the CG non-convergence: {msg:?}",
        );
    }

    /// The key regression guard: `ActiveSetDidNotConverge` is the only arm that
    /// interpolates a runtime value (`iterations`). Assert the count is actually
    /// substituted — a different payload must appear verbatim and yield a distinct
    /// message — so a decoupled/broken format string fails here rather than
    /// shipping a diagnostic with a literal `{iterations}` or a stale count.
    #[test]
    fn describe_active_set_did_not_converge_interpolates_iteration_count() {
        let msg7 = describe(TensegrityLoadError::ActiveSetDidNotConverge { iterations: 7 });
        assert!(
            msg7.contains("tension-only active set did not reach a fixed point"),
            "ActiveSetDidNotConverge describe() phrase changed: {msg7:?}",
        );
        assert!(
            msg7.contains("within 7 passes"),
            "ActiveSetDidNotConverge must interpolate its iteration count \
             (expected 'within 7 passes'): {msg7:?}",
        );

        // A different count must change the message verbatim — proves the value is
        // interpolated, not a hardcoded literal that happens to read "7".
        let msg42 =
            describe(TensegrityLoadError::ActiveSetDidNotConverge { iterations: 42 });
        assert!(
            msg42.contains("within 42 passes"),
            "iteration count must track the variant payload: {msg42:?}",
        );
        assert_ne!(
            msg7, msg42,
            "distinct iteration counts must yield distinct messages",
        );
    }
}
