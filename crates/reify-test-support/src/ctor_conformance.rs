//! The canonical struct-ctor field-conformance test helpers.
//!
//! Integration tests are separate binaries and cannot share a private helper
//! without a support-crate hop. Before task #6323 that hop had not been taken,
//! so this admission set and its matcher existed as four hand-kept copies
//! across `harness_compilation_surface`, `harness_structure_declarations`,
//! `harness_mechanics` and `reify-eval-fea-tests`. They were lock-step by
//! convention only, and the drift they invited is SILENT: a stale copy simply
//! stops matching rather than failing to build, so a pin that narrows through
//! it goes quietly vacuous instead of red.
//!
//! This module is that single definition. Its own behaviour is pinned by
//! `tests/ctor_conformance.rs`, which the copies never had.

use reify_core::diagnostics::DiagnosticCode;

/// The diagnostic codes emitted by the struct-ctor field-conformance surface
/// (tasks 5302 / 5303 / 4584 / 4598 / 4622 / 4444).
///
/// The first five are the α type codes; ε (task 5303) adds the two structural
/// codes `CtorUnknownField` / `CtorArity`, for the two lenient `__arg{i}` sites
/// in the `StructureInstanceCtor` by-name binder. They belong in one set
/// because both families are emitted at the same `CTOR_FIELD_CONFORMANCE_SEVERITY`
/// knob (the const of that name in `reify-compiler/src/conformance/mod.rs` —
/// cited by SYMBOL, never by line, so the cite cannot rot), and δ's planned
/// Warning→Error flip moves them together.
///
/// Membership is therefore SEVERITY-AGNOSTIC on purpose: the δ flip must not
/// move any pin that filters through here.
pub const CTOR_CONFORMANCE_CODES: &[DiagnosticCode] = &[
    DiagnosticCode::ArgTypeMismatch,
    DiagnosticCode::SelectorKindMismatch,
    DiagnosticCode::TypeNotConformingToTrait,
    DiagnosticCode::TypeNotConformingToStructureRef,
    DiagnosticCode::TypeNotConformingToVector,
    DiagnosticCode::CtorUnknownField,
    DiagnosticCode::CtorArity,
];

/// True when `code` is one of [`CTOR_CONFORMANCE_CODES`].
///
/// Filtering to this set is what keeps a probe's "exactly N diagnostics" count
/// from being polluted by an unrelated diagnostic — an incidental `W_*`
/// warning, a downstream note. A module compiled through
/// `compile_source_with_stdlib` carries the diagnostics of the probe source AND
/// of the whole stdlib prelude, so without the narrowing a future stdlib
/// diagnostic flips unrelated pins red.
///
/// `None` (a codeless diagnostic) is NOT admitted. That is a live shape on this
/// surface, not a hypothetical: the duplicate-named-arg diagnostic in the same
/// binder is built with a bare `Diagnostic::error` and carries no code at all.
/// A pin that must also see a codeless emission has to narrow on the offending
/// SOURCE IDENTIFIER instead, not on this set.
pub fn is_ctor_conformance_code(code: Option<DiagnosticCode>) -> bool {
    code.is_some_and(|c| CTOR_CONFORMANCE_CODES.contains(&c))
}

/// The prefix `emit_arg_type_mismatch` puts before the offending param label in
/// every ctor-conformance message it words
/// (`crates/reify-compiler/src/conformance/mod.rs`; full shape
/// `argument 'X' has type 'A' but param 'X' requires type 'B'`).
///
/// THIS IS A REAL COUPLING TO DIAGNOSTIC PROSE — the single place it is
/// recorded. A `Diagnostic` carries no structured param field, so the wording
/// is the only handle a test has on which param a ctor diagnostic is about.
/// Change the emitter's wording and this const must move with it.
///
/// `pub` rather than private because three failure messages across the test
/// corpus interpolate it BY NAME, which is the quality that makes those
/// messages survive a wording change instead of silently describing the old
/// shape.
pub const CTOR_DIAGNOSTIC_ARG_PREFIX: &str = "argument '";

/// True when `message` is a ctor-conformance diagnostic naming exactly `param`.
///
/// Matches the QUOTED label (`argument 'beta'`), NEVER a bare
/// `contains(param)`. Two independent reasons, both load-bearing:
///
///   - a module compiled through `compile_source_with_stdlib` carries the whole
///     stdlib prelude's diagnostics too, so an unquoted match also catches any
///     prelude message that merely contains the word;
///   - the bare form matches the substring inside ordinary English words —
///     "o(beta)in" is the obvious one — which is an unrelated-axis false red.
///
/// Both are pinned in `tests/ctor_conformance.rs` rather than left as prose.
pub fn ctor_diagnostic_names_arg(message: &str, param: &str) -> bool {
    message.contains(&format!("{CTOR_DIAGNOSTIC_ARG_PREFIX}{param}'"))
}
