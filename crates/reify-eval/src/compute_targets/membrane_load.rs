//! Trampoline for `solver::membrane_load` — combined membrane + bar/cable load
//! analysis with a tension-only active set (PRD
//! `docs/prds/v0_6/tensegrity-membrane.md` §5 / §10 / §11, task η, layer M2).
//!
//! # Contract
//!
//! Receives the ten `value_inputs` matching the future `membrane_load` signature
//! (the surface analogue of T3b's six-input `tensegrity_load`, broadcasting a
//! single shared membrane section because `Tensegrity.surfaces` is a bare
//! `List<List<Int>>` with no per-triangle `Membrane` binding):
//!
//! ```text
//! [0] structure         : Tensegrity              (Value::StructureInstance)
//! [1] prestress         : List<Force>             — one per line member,
//!                                                   struts-then-cables
//! [2] youngs_modulus    : Pressure                (broadcast line-member E)
//! [3] area              : Area                    (broadcast line-member A)
//! [4] loads             : List<Vector3<Force>>    (per-node external force)
//! [5] supports          : List<Int>              (fixed node indices)
//! [6] surface_prestress : List<Pressure>          — one σ₀ per triangle,
//!                                                   surfaces order
//! [7] membrane_thickness: Length                  (broadcast patch thickness)
//! [8] membrane_youngs   : Pressure                (broadcast patch E)
//! [9] membrane_poisson  : Real                    (broadcast patch ν)
//! ```
//!
//! It cracks the Tensegrity into node coordinates + line connectivity (struts
//! then cables) + surface triangle triples, broadcasts the shared line section
//! `(E, A)` and the shared membrane section `(t, E, ν)` across patches, calls the
//! pure kernel [`reify_solver_elastic::membrane_load_analysis`], and rebuilds a
//! `MembraneLoadResult` `Value::StructureInstance`.
//!
//! # Failure → diagnostic
//!
//! Infeasible input returns [`ComputeOutcome::Failed`] carrying a single
//! `E_MembraneLoadInfeasible` `Diagnostic::error` (the mnemonic lives in the
//! message text, mirroring the `tensegrity_load` trampoline). The trampoline
//! never panics and never returns a silently-wrong (`converged: false`) result.
//!
//! # StructureTypeId sentinel
//!
//! The trampoline has no `StructureRegistry` access, so the returned
//! `MembraneLoadResult` uses `StructureTypeId(u32::MAX)` as a synthetic sentinel —
//! the same convention as the form-find / tensegrity-load trampolines.

use reify_core::{Diagnostic, DimensionVector};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_solver_elastic::{
    BarMember, BarSection, IsotropicElastic, MemberKind, MembraneLoadError, MembraneLoadOptions,
    MembraneLoadSolve, MembranePatch, membrane_load_analysis,
};

use super::tensegrity_crack::{check_index, crack_index_pairs, crack_index_triples, crack_nodes};
use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Diagnostic mnemonic for this trampoline, threaded into the shared
/// `tensegrity_crack` helpers so their located errors carry the same prefix as
/// the inline guards in [`run`].
const CODE: &str = "E_MembraneLoadInfeasible";

/// Trampoline for `solver::membrane_load`. See the module doc for the
/// input/output contract.
pub fn solve_membrane_load_trampoline(
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
/// already-prefixed `E_MembraneLoadInfeasible: …` message.
fn run(value_inputs: &[Value]) -> Result<Value, String> {
    if value_inputs.len() < 10 {
        return Err(format!(
            "E_MembraneLoadInfeasible: membrane_load expects 10 inputs (structure, \
             prestress, youngs_modulus, area, loads, supports, surface_prestress, \
             membrane_thickness, membrane_youngs, membrane_poisson); got {}. Let-bind \
             all ten call arguments so the ComputeNode captures them.",
            value_inputs.len()
        ));
    }

    let (nodes, member_pairs, kinds, surface_triples) = crack_tensegrity(&value_inputs[0])?;
    let prestress = crack_forces(&value_inputs[1], "prestress")?;
    let youngs_modulus = crack_dimensioned_scalar(
        &value_inputs[2],
        "youngs_modulus",
        DimensionVector::PRESSURE,
        "Pressure",
    )?;
    let area = crack_dimensioned_scalar(&value_inputs[3], "area", DimensionVector::AREA, "Area")?;
    let loads = crack_loads(&value_inputs[4])?;
    let supports = crack_supports(&value_inputs[5], nodes.len())?;
    let surface_prestress = crack_pressures(&value_inputs[6], "surface_prestress")?;
    let membrane_thickness = crack_dimensioned_scalar(
        &value_inputs[7],
        "membrane_thickness",
        DimensionVector::LENGTH,
        "Length",
    )?;
    let membrane_youngs = crack_dimensioned_scalar(
        &value_inputs[8],
        "membrane_youngs",
        DimensionVector::PRESSURE,
        "Pressure",
    )?;
    let membrane_poisson = crack_real(&value_inputs[9], "membrane_poisson")?;

    // Length guards — reject silently-wrong inputs before building elements. The
    // broadcast zips below would otherwise truncate to the shortest, quietly
    // solving a smaller problem than the caller described.
    if prestress.len() != member_pairs.len() {
        return Err(format!(
            "E_MembraneLoadInfeasible: prestress length {} does not match the line-member \
             count {} (struts + cables); supply one prestress force per line member.",
            prestress.len(),
            member_pairs.len()
        ));
    }
    if surface_prestress.len() != surface_triples.len() {
        return Err(format!(
            "E_MembraneLoadInfeasible: surface_prestress length {} does not match the \
             surface (patch) count {}; supply one prestress σ₀ per surface triangle.",
            surface_prestress.len(),
            surface_triples.len()
        ));
    }
    if loads.len() != nodes.len() {
        return Err(format!(
            "E_MembraneLoadInfeasible: loads length {} does not match the node count {}; \
             supply one force vector per node.",
            loads.len(),
            nodes.len()
        ));
    }

    // Broadcast the shared line section (E, A) across every bar/cable member (v1
    // decision). A fresh BarSection per member keeps this independent of
    // BarSection's Copy/Clone surface.
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

    // Broadcast the shared membrane section (thickness, E, ν) across every patch;
    // the per-triangle σ₀ is the form-found state, carried per-patch from
    // `surface_prestress` (the surface analogue of T3b's per-member prestress).
    let patches: Vec<MembranePatch> = surface_triples
        .iter()
        .zip(surface_prestress.iter())
        .map(|(&tri, &sigma)| MembranePatch {
            nodes: tri,
            thickness: membrane_thickness,
            material: IsotropicElastic {
                youngs_modulus: membrane_youngs,
                poisson_ratio: membrane_poisson,
            },
            prestress: sigma,
        })
        .collect();

    let options = MembraneLoadOptions::default();
    let solve = membrane_load_analysis(&nodes, &members, &patches, &loads, &supports, &options)
        .map_err(|e| format!("E_MembraneLoadInfeasible: {}", describe(e)))?;

    Ok(build_result(&solve))
}

// ── input cracking ────────────────────────────────────────────────────────────
//
// Topology cracking (nodes, struts/cables index pairs, surface triples, scalar/
// index validation) is shared with the form-find / tensegrity-load trampolines via
// `super::tensegrity_crack`. The crackers below are specific to this trampoline's
// load-analysis inputs (dimensioned section scalars, per-node force vectors,
// per-triangle pressures, support indices), mirroring `tensegrity_load`.

/// Cracked Tensegrity topology: node coordinates, member index pairs in
/// struts-then-cables order, the matching per-member [`MemberKind`] tags, and the
/// surface triangle corner triples.
type CrackedTopology = (
    Vec<[f64; 3]>,
    Vec<(usize, usize)>,
    Vec<MemberKind>,
    Vec<(usize, usize, usize)>,
);

/// Crack the Tensegrity StructureInstance into node coordinates, members in
/// struts-then-cables order with their [`MemberKind`] tags, and surface triples.
fn crack_tensegrity(v: &Value) -> Result<CrackedTopology, String> {
    let fields = match v {
        Value::StructureInstance(d) if d.type_name == "Tensegrity" => &d.fields,
        other => {
            return Err(format!(
                "E_MembraneLoadInfeasible: membrane_load expected a Tensegrity structure, got {other:?}"
            ));
        }
    };

    let nodes = crack_nodes(fields.get(&"nodes".to_string()), CODE)?;
    let n = nodes.len();
    let struts = crack_index_pairs(fields.get(&"struts".to_string()), "struts", n, CODE)?;
    let cables = crack_index_pairs(fields.get(&"cables".to_string()), "cables", n, CODE)?;
    let surfaces = crack_index_triples(fields.get(&"surfaces".to_string()), "surfaces", n, CODE)?;

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
    Ok((nodes, members, kinds, surfaces))
}

/// Crack a `List<Force>` into f64 newtons (one per line member).
fn crack_forces(v: &Value, what: &str) -> Result<Vec<f64>, String> {
    crack_scalar_list(v, what, DimensionVector::FORCE, "Force")
}

/// Crack a `List<Pressure>` into f64 pascals (one per surface triangle).
fn crack_pressures(v: &Value, what: &str) -> Result<Vec<f64>, String> {
    crack_scalar_list(v, what, DimensionVector::PRESSURE, "Pressure")
}

/// Crack a `List<Scalar>` requiring each entry to carry `expected` units (a bare
/// `Real` is still accepted for ergonomics). Shared by [`crack_forces`] and
/// [`crack_pressures`].
fn crack_scalar_list(
    v: &Value,
    what: &str,
    expected: DimensionVector,
    label: &str,
) -> Result<Vec<f64>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "E_MembraneLoadInfeasible: {what} must be a list of {label} scalars, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        out.push(crack_dimensioned_scalar(
            item,
            &format!("{what}[{i}]"),
            expected,
            label,
        )?);
    }
    Ok(out)
}

/// Crack a single dimensioned `Scalar` into an f64, requiring its unit to equal
/// `expected`. A bare `Real` is still accepted (the dimensionless ergonomic escape
/// hatch), but a *dimensioned* `Scalar` whose unit disagrees (e.g. an Area passed
/// where a Pressure is expected) is rejected with a located error rather than
/// silently solving a physically wrong problem. `label` is the human unit name.
fn crack_dimensioned_scalar(
    v: &Value,
    what: &str,
    expected: DimensionVector,
    label: &str,
) -> Result<f64, String> {
    match v {
        Value::Real(r) => Ok(*r),
        Value::Scalar {
            si_value,
            dimension,
        } if *dimension == expected => Ok(*si_value),
        Value::Scalar { .. } => Err(format!(
            "E_MembraneLoadInfeasible: {what} has the wrong unit — expected a {label}; \
             check the call argument order (youngs_modulus / membrane_youngs are Pressures, \
             area is an Area, membrane_thickness is a Length, and prestress / loads are Forces)"
        )),
        other => Err(format!(
            "E_MembraneLoadInfeasible: {what} must be a scalar, got {other:?}"
        )),
    }
}

/// Crack a bare real number (e.g. Poisson's ratio) — a `Value::Real` or any
/// `Value::Scalar` (a dimensionless ratio may lower either way).
fn crack_real(v: &Value, what: &str) -> Result<f64, String> {
    match v {
        Value::Real(r) => Ok(*r),
        Value::Scalar { si_value, .. } => Ok(*si_value),
        other => Err(format!(
            "E_MembraneLoadInfeasible: {what} must be a real number, got {other:?}"
        )),
    }
}

/// Crack `loads` (a `List<Vector3<Force>>`) into per-node `[f64; 3]` force vectors.
/// The loads-vs-nodes length check is performed in [`run`]; this cracker only
/// validates per-entry shape (3-component, FORCE-dimensioned).
fn crack_loads(v: &Value) -> Result<Vec<[f64; 3]>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "E_MembraneLoadInfeasible: loads must be a list of 3-component force vectors, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        match item {
            Value::Vector(c) | Value::Point(c) if c.len() == 3 => {
                out.push([
                    crack_dimensioned_scalar(
                        &c[0],
                        &format!("loads[{i}].x"),
                        DimensionVector::FORCE,
                        "Force",
                    )?,
                    crack_dimensioned_scalar(
                        &c[1],
                        &format!("loads[{i}].y"),
                        DimensionVector::FORCE,
                        "Force",
                    )?,
                    crack_dimensioned_scalar(
                        &c[2],
                        &format!("loads[{i}].z"),
                        DimensionVector::FORCE,
                        "Force",
                    )?,
                ]);
            }
            other => {
                return Err(format!(
                    "E_MembraneLoadInfeasible: loads[{i}] must be a 3-component force vector, got {other:?}"
                ));
            }
        }
    }
    Ok(out)
}

/// Crack a `List<Int>` of support node indices, range-checking each against the
/// node count `n` (an out-of-range index surfaces "… is out of range 0..n").
fn crack_supports(v: &Value, n: usize) -> Result<Vec<usize>, String> {
    let list = match v {
        Value::List(items) => items,
        other => {
            return Err(format!(
                "E_MembraneLoadInfeasible: supports must be a list of integer node indices, got {other:?}"
            ));
        }
    };
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        match item {
            Value::Int(a) => out.push(check_index(*a, n, &format!("supports[{i}]"), CODE)?),
            other => {
                return Err(format!(
                    "E_MembraneLoadInfeasible: supports[{i}] must be an integer node index, got {other:?}"
                ));
            }
        }
    }
    Ok(out)
}

// ── result construction ──────────────────────────────────────────────────────

/// Build the `MembraneLoadResult` `Value::StructureInstance` from the kernel
/// solve. Routed through the shared `compute_targets` builders so a future
/// dimension/encoding change is a single-point edit. Every field is a REAL
/// (non-`Undef`) value by construction — the G6 field-population invariant.
fn build_result(solve: &MembraneLoadSolve) -> Value {
    let displacements: Vec<Value> = solve
        .displacements
        .iter()
        .map(|&u| super::vec3_length(u))
        .collect();
    let member_forces = super::scalar_list(&solve.member_forces, DimensionVector::FORCE);
    let member_force_deltas =
        super::scalar_list(&solve.member_force_deltas, DimensionVector::FORCE);
    let member_slack: Vec<Value> = solve.member_slack.iter().map(|&s| Value::Bool(s)).collect();

    // Each patch Δσ encodes its three independent Voigt components
    // [Δσxx, Δσyy, Δσxy] (the symmetric 2×2 [[σxx, σxy], [σxy, σyy]]).
    let surface_stress_deltas: Vec<Value> = solve
        .surface_stress_deltas
        .iter()
        .map(|m| {
            Value::List(super::scalar_list(
                &[m[0][0], m[1][1], m[0][1]],
                DimensionVector::PRESSURE,
            ))
        })
        .collect();
    // Each patch principal pair is [min, max] of the total stress σ₀·I + Δσ.
    let surface_principal_stresses: Vec<Value> = solve
        .surface_principal_stresses
        .iter()
        .map(|p| Value::List(super::scalar_list(&[p[0], p[1]], DimensionVector::PRESSURE)))
        .collect();
    let surface_slack: Vec<Value> = solve.surface_slack.iter().map(|&s| Value::Bool(s)).collect();

    let fields: PersistentMap<String, Value> = [
        ("displacements".to_string(), Value::List(displacements)),
        ("member_forces".to_string(), Value::List(member_forces)),
        (
            "member_force_deltas".to_string(),
            Value::List(member_force_deltas),
        ),
        ("member_slack".to_string(), Value::List(member_slack)),
        (
            "surface_stress_deltas".to_string(),
            Value::List(surface_stress_deltas),
        ),
        (
            "surface_principal_stresses".to_string(),
            Value::List(surface_principal_stresses),
        ),
        ("surface_slack".to_string(), Value::List(surface_slack)),
        ("converged".to_string(), Value::Bool(solve.converged)),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "MembraneLoadResult".to_string(),
        version: 1,
        fields,
    }))
}

/// Human-readable cause for a kernel [`MembraneLoadError`] (appended after the
/// `E_MembraneLoadInfeasible:` prefix), mirroring `tensegrity_load::describe()`.
/// Most arms are pre-empted by the located trampoline guards above; they remain so
/// every kernel-side variant maps to a friendly phrase instead of a `Debug` dump.
fn describe(e: MembraneLoadError) -> String {
    match e {
        MembraneLoadError::DimensionMismatch => {
            "input dimensions disagree — loads must supply one force vector per node \
             and every bar endpoint / patch corner / support index must lie within the \
             node set"
                .to_string()
        }
        MembraneLoadError::EmptyFreeSet => {
            "every node is anchored — there is no free node to solve for".to_string()
        }
        MembraneLoadError::SingularSystem => {
            "singular tangent system — the inner CG solve did not converge (a free node \
             with no taut load path to a support, or an ill-conditioned reduced stiffness \
             once slack cables / patches were dropped)"
                .to_string()
        }
        MembraneLoadError::ActiveSetDidNotConverge { iterations } => format!(
            "tension-only active set did not reach a fixed point within {iterations} \
             passes (the PRD §11 Q5 cap) — drop-only monotonicity should converge in at \
             most #cables + #patches passes, so this signals a non-monotone active-set policy"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one `describe()` arm that interpolates a runtime value
    /// (`ActiveSetDidNotConverge`'s `{iterations}` count) — assert the count is
    /// actually substituted so a decoupled format string fails here rather than
    /// shipping a literal `{iterations}` or a stale count.
    #[test]
    fn describe_active_set_did_not_converge_interpolates_iteration_count() {
        let msg7 = describe(MembraneLoadError::ActiveSetDidNotConverge { iterations: 7 });
        assert!(
            msg7.contains("tension-only active set did not reach a fixed point"),
            "ActiveSetDidNotConverge describe() phrase changed: {msg7:?}",
        );
        assert!(
            msg7.contains("within 7 passes"),
            "must interpolate the iteration count (expected 'within 7 passes'): {msg7:?}",
        );
        let msg42 = describe(MembraneLoadError::ActiveSetDidNotConverge { iterations: 42 });
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
