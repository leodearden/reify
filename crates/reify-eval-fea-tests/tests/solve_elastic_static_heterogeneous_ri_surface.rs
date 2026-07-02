// SPDX-License-Identifier: AGPL-3.0-or-later

//! .ri call-surface conformance test for heterogeneous `solve_elastic_static`
//! (task #4757 step-5 RED → step-6 GREEN).
//!
//! Compiles (type-checks only — no evaluation, no OCCT required) a small .ri
//! module that binds:
//! ```ri
//! let body = box(40mm, 40mm, 10mm)
//! let mat  = as_printed_material(body, FDMProcess())
//! let result = solve_elastic_static(mat, 40mm, 40mm, 10mm, [PointLoad()], [FixedSupport()])
//! ```
//!
//! ## RED state
//!
//! Before step-6 adds the `solve_elastic_static(material: Field<…>, …)` overload,
//! `check_leaf_trait_conformance` emits:
//! ```
//! type 'Field<Point3<Length>, AnisotropicMaterial>' does not conform to trait 'ConstitutiveLaw'
//! ```
//! — an Error-severity diagnostic.  The test assertion `errors.is_empty()` is therefore
//! UNSATISFIED → RED.
//!
//! ## GREEN state (after step-6)
//!
//! After step-6 adds `@optimized("solver::elastic_static") pub fn solve_elastic_static(
//!     material: Field<Point3<Length>, AnisotropicMaterial>, …)` to `solver_elastic.ri`,
//! the overload resolver prefers the exact Field match over the `ConstitutiveLaw` wildcard.
//! No conformance error is emitted → `errors.is_empty()` is satisfied → GREEN.
//!
//! ## Note on escalation
//!
//! If step-6 reveals the stdlib loader/overload resolver cannot accept a same-`@optimized`-
//! target overload as a pure-.ri change, the step-6 implementer must escalate
//! (dependency_discovered — reify-compiler/src is out of task #4757's listed scope) rather
//! than weakening this assertion.

use reify_core::{ModulePath, Severity};

/// Parse and compile a .ri source with the stdlib prelude, asserting zero
/// Error-severity diagnostics.  Returns the compiled module for optional
/// further inspection.
///
/// Mirrors `conformance_runtime.rs::compile_no_errors`.
fn compile_no_errors(source: &str, module_name: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in '{module_name}': {:#?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "'{module_name}' should compile with zero Error diagnostics, got: {:#?}",
        errors
    );
    compiled
}

/// Step-5 RED → step-6 GREEN: the canonical heterogeneous FEA surface.
///
/// Binds `mat = as_printed_material(body, FDMProcess())` and passes it directly
/// to `solve_elastic_static(mat, …)`.  This is REJECTED today because the only
/// `solve_elastic_static` overload accepts `material: ConstitutiveLaw`, and
/// `Field<Point3<Length>, AnisotropicMaterial>` does not conform to that trait.
///
/// After step-6 adds the exact-Field overload, the overload resolver prefers it
/// and no conformance error is emitted.
#[test]
fn field_arg_to_solve_elastic_static_compiles_without_conformance_error() {
    let source = r#"
structure def HeteroElasticSmoke {
    let body   = box(40mm, 40mm, 10mm)
    let mat    = as_printed_material(body, FDMProcess())
    let result = solve_elastic_static(mat, 40mm, 40mm, 10mm, [PointLoad()], [FixedSupport()])
}
"#;

    // `compile_no_errors` will PANIC with the conformance diagnostic until
    // step-6 adds the Field overload — that panic is the RED signal.
    compile_no_errors(source, "hetero_elastic_ri_surface");
}
