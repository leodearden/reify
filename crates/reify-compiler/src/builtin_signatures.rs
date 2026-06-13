//! Per-argument type signatures for geometry topology-selector builtins
//! (task 4493, type-hygiene ζ).
//!
//! Hosts the checkable argument-slot table ([`builtin_arg_slots`]) and the
//! call-site checker ([`check_builtin_arg_types`]) for the geometry
//! topology-selector family.  The mechanism is generic (name-keyed), but only
//! geometry-selector dimensioned-scalar slots are populated here; math args
//! (polymorphic, no fixed dimension) and geometry-handle arg0 (ε=4358's
//! territory, PRD §4 out-of-scope) are intentionally absent.
//!
//! # Design: sibling of `math_signatures.rs`
//!
//! Placed beside `math_signatures.rs` per PRD open-question-3 (implementer's
//! choice): the arg-slot table covers the geometry family, not the math-linalg
//! §3 family, so folding it into the frozen `math_signatures` contract would be
//! a misnomer.  The module structure mirrors `math_signatures.rs`: a public-to-
//! crate name-keyed match function + a small set of supporting types.
//!
//! # Checked vs. unchecked slots (decision-4 gradualism)
//!
//! CHECKED (definite dimension mismatch, zero false positives):
//! - `center_of_mass` / `moment_of_inertia` arg1 `density` → MASS_DENSITY ("Density")
//! - `faces_by_normal` / `edges_parallel_to` arg2 `tol` → ANGLE ("Angle")
//! - `edges_at_height` arg1 `h` → LENGTH ("Length"), arg2 `tol` → LENGTH ("Length")
//!
//! UNCHECKED (would false-positive on valid call sites or is out-of-scope):
//! - arg0 (geometry handle) — ε=4358's territory
//! - `dir` Vec3 slot — accepts list literals `[0,0,1]` that coerce
//! - Range slots (`edges_by_length` / `faces_by_area`)
//! - Names without dimensioned-scalar args (`split`, `face`, `edge`, `solid_body`, …)

use reify_core::{
    Diagnostic, DiagnosticCode, DiagnosticLabel, DimensionVector, SourceSpan, Type,
};
use reify_ir::CompiledExpr;

/// A single checkable argument slot: the zero-based index, human-readable
/// parameter name, and expected type for a builtin argument.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CheckableArg {
    /// Zero-based index of this argument in the call argument list.
    pub index: usize,
    /// Human-readable parameter name used in diagnostic messages
    /// (e.g., `"density"`, `"tol"`, `"h"`).
    pub name: &'static str,
    /// Expected type for this slot.
    pub expected: ExpectedArg,
}

/// The expected type for a checkable builtin argument slot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ExpectedArg {
    /// A dimensioned scalar with a specific dimension.
    Scalar {
        /// The required physical dimension (e.g., `DimensionVector::MASS_DENSITY`).
        dimension: DimensionVector,
        /// Human-readable type name for diagnostic messages
        /// (e.g., `"Density"`, `"Angle"`, `"Length"`).
        type_name: &'static str,
    },
}

/// Return the checkable dimensioned-scalar argument slots for a named builtin.
///
/// Returns an empty `Vec` for unrecognized names and names with no checked
/// dimensioned-scalar arg.  Step-2 populates the table; the stub returns
/// empty so step-1 RED tests can be written first.
pub(crate) fn builtin_arg_slots(_name: &str) -> Vec<CheckableArg> {
    // Stub: returns empty. Implemented in step-2.
    vec![]
}

/// Check the compiled arguments of a builtin call against its known type
/// signatures, pushing [`DiagnosticCode::ArgTypeMismatch`] errors for
/// DEFINITE static mismatches only.
///
/// No-op stub until step-4 implements the logic.
pub(crate) fn check_builtin_arg_types(
    _name: &str,
    _compiled_args: &[CompiledExpr],
    _call_span: SourceSpan,
    _diagnostics: &mut Vec<Diagnostic>,
) {
    // Stub: no-op. Implemented in step-4.
}
