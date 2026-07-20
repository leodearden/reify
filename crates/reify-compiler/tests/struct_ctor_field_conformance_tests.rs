//! Struct-constructor field-conformance tests (task 5302, struct-ctor-conformance α).
//!
//! Task 5302 generalizes task 4584's struct-ctor conformance chokepoint from its
//! original 4-family allowlist (`List<TraitObject>` / `StructureRef` / `Vector` /
//! `Selector`) to ALL concrete field types, at **Warning** severity behind a single
//! module const (`CTOR_FIELD_CONFORMANCE_SEVERITY`; δ later flips it to Error).
//!
//! These are inline-source integration tests (NOT on-disk `.ri` fixtures under
//! `examples/`, which would be swept by `examples_smoke.rs`). Each `const SOURCE`
//! begins with a `module test.<name>` decl so the compiler does not emit the
//! `W_MODULE_DECL_MISSING` warning — that keeps the per-fixture "exactly one
//! diagnostic" counts exact (the double-emission pin in row 4 / C2(ii) relies on
//! this).
//!
//! Convention (mirrors `param_binding_selector_coercion_tests.rs` /
//! `vec3_type_tests.rs`): inline `const SOURCE` + assertions on the *filtered*
//! diagnostics' code / severity / message. The [`ctor_conformance_diags`] /
//! [`ctor_conformance_warnings`] helpers below filter to just the ctor-conformance
//! diagnostic codes so unrelated diagnostics never pollute the counts.
//!
//! No new diagnostic codes are minted in α; no reify-core change.

use reify_compiler::CompiledModule;
use reify_core::Diagnostic;
use reify_core::diagnostics::DiagnosticCode;
// `compile_source_with_stdlib` and `Severity` are consumed by the probe fns
// added in step-1; kept out of the prerequisite commit's imports so it builds
// clean under the `-D warnings` clippy gate.
use reify_test_support::{errors_only, warnings_only};

/// True when `code` is one of the diagnostic codes emitted by the struct-ctor
/// field-conformance pass (task 5302 / 4584 / 4598 / 4622 / 4444).
///
/// Filtering to this set keeps the per-fixture "exactly one diagnostic" counts
/// from being polluted by unrelated diagnostics (an incidental `W_*` warning, a
/// downstream note, etc.). All five codes already exist in `diagnostics.rs`; α
/// mints none.
// `#[allow(dead_code)]` covers the prerequisite commit (no probe fns yet); the
// probe fns added in step-1 consume these helpers.
#[allow(dead_code)]
fn is_ctor_conformance_code(code: Option<DiagnosticCode>) -> bool {
    matches!(
        code,
        Some(
            DiagnosticCode::ArgTypeMismatch
                | DiagnosticCode::SelectorKindMismatch
                | DiagnosticCode::TypeNotConformingToTrait
                | DiagnosticCode::TypeNotConformingToStructureRef
                | DiagnosticCode::TypeNotConformingToVector
        )
    )
}

/// All ctor-conformance diagnostics in `module`, of any severity.
///
/// Used by "exactly N diagnostics" / "zero diagnostics" assertions so an
/// incidental unrelated diagnostic does not throw off the count.
#[allow(dead_code)]
fn ctor_conformance_diags(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| is_ctor_conformance_code(d.code))
        .collect()
}

/// Ctor-conformance diagnostics in `module` restricted to `Severity::Warning`.
///
/// At α the whole ctor-conformance surface emits at Warning (the knob default),
/// so most probe fixtures assert against this. Intersecting the code filter with
/// [`warnings_only`] guards against a fixture that trips an unrelated warning.
#[allow(dead_code)]
fn ctor_conformance_warnings(module: &CompiledModule) -> Vec<&Diagnostic> {
    warnings_only(module)
        .into_iter()
        .filter(|d| is_ctor_conformance_code(d.code))
        .collect()
}

/// Ctor-conformance diagnostics in `module` restricted to `Severity::Error`.
///
/// Reserved for the (few) sites that must stay Error even at α — currently only
/// the fn-call conformance path, which these ctor fixtures do not exercise; kept
/// for symmetry with [`ctor_conformance_warnings`] and future δ-flip tests.
#[allow(dead_code)]
fn ctor_conformance_errors(module: &CompiledModule) -> Vec<&Diagnostic> {
    errors_only(module)
        .into_iter()
        .filter(|d| is_ctor_conformance_code(d.code))
        .collect()
}

// Probe / boundary test functions are added by task 5302 steps 1, 3, 5, 7, 9.
