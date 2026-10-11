//! Compile-side type pins for the `std.modal.analysis.fns` module
//! (`crates/reify-compiler/stdlib/modal_analysis_fns.ri`).
//!
//! Scope (#6094): `displacement_at` returns `List<Length>`, not `List<Real>`.
//! Why the reconstruction Φ·ξ is plain metres is derived at the declaration:
//! `crates/reify-compiler/stdlib/modal_analysis_fns.ri` :: `displacement_at`.
//!
//! Two independent signals:
//!   (a) both `displacement_at` overloads declare `List<Length>`;
//!   (b) at a call site the bound cell types as `List<Length>`, with zero Error
//!       diagnostics.

use reify_compiler::{CompiledModule, stdlib_loader};
use reify_core::{DimensionVector, Type, ty::SelectorKind};
use reify_ir::CompiledFunction;
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/modal/analysis/fns` CompiledModule from the production
/// stdlib loader. Exercises the same embedded + sequential-prelude compilation
/// path as production.
///
/// Panics listing every available module path on a miss, so a module rename
/// fails loudly instead of making every test below vacuous.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/modal/analysis/fns")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/modal/analysis/fns module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Collect every compiled function named `name` in `std/modal/analysis/fns`.
/// `displacement_at` has two overloads (String and FaceSelector `location`), so
/// a `.find()` would leave one of them unpinned.
fn find_overloads(name: &str) -> Vec<&'static CompiledFunction> {
    load_stdlib_module()
        .functions
        .iter()
        .filter(|f| f.name == name)
        .collect()
}

/// The expected `List<Length>` return type: `List(Scalar<LENGTH>)`.
fn list_of_length() -> Type {
    Type::List(Box::new(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    }))
}

// ─── (a) declared return type of both overloads ───────────────────────────────

/// Both `displacement_at` overloads must declare `-> List<Length>`: one Length
/// per timestep.
#[test]
fn displacement_at_overloads_return_list_of_length() {
    let overloads = find_overloads("displacement_at");

    // Non-vacuity guard 1: exactly the two known declarations were found.
    assert_eq!(
        overloads.len(),
        2,
        "expected exactly 2 `displacement_at` overloads in std/modal/analysis/fns \
         (String `location` and FaceSelector `location`, sharing one trampoline); \
         got {} with param lists: {:?}",
        overloads.len(),
        overloads.iter().map(|f| &f.params).collect::<Vec<_>>()
    );

    // Non-vacuity guard 2: the two overloads are different declarations.
    let location_types: Vec<&Type> = overloads
        .iter()
        .map(|f| {
            &f.params
                .get(1)
                .unwrap_or_else(|| {
                    panic!(
                        "displacement_at overload should have a param[1] `location`; got: {:?}",
                        f.params
                    )
                })
                .1
        })
        .collect();
    assert_ne!(
        location_types[0], location_types[1],
        "the two `displacement_at` overloads must differ in their `location` param \
         type (String vs FaceSelector) — identical types mean the pin below covers \
         one declaration twice and leaves the other unchecked; got: {location_types:?}"
    );
    assert!(
        location_types.contains(&&Type::String)
            && location_types.contains(&&Type::Selector(SelectorKind::Face)),
        "the two `displacement_at` overloads should be the String and \
         FaceSelector (= Selector(Face)) `location` declarations; got: {location_types:?}"
    );

    // The pin itself, applied to EACH overload.
    for func in &overloads {
        let location_ty = &func.params[1].1;
        assert!(
            func.is_pub,
            "`displacement_at` (location: {location_ty:?}) should be pub"
        );
        assert_eq!(
            func.return_type,
            list_of_length(),
            "`displacement_at` (location: {:?}) return type should be \
             List<Scalar<LENGTH>> (= List<Length>) — the reconstruction Φ·ξ is \
             plain metres, the √mass factors cancel (#6094); got: {:?}",
            location_ty,
            func.return_type
        );
    }
}

// ─── (b) user-observable consumer signal ──────────────────────────────────────

/// A `structure` binding `displacement_at(...)` through a `List<Length>`
/// annotation, fed a directly-constructed `DisplacementTimeHistory`. Nothing
/// here is evaluated, only type-checked, so `modes` / `boundary_conditions` are
/// empty and the matrix norms are placeholders.
const LIST_LENGTH_CONSUMER_PROBE: &str = r#"
structure DisplacementAtLengthConsumerProbe {
    let modal_result = ModalResult(
        part: Part(),
        modes: [],
        boundary_conditions: [],
        damping: RayleighDamping(alpha: 0.0Hz, beta: 0.0s),
        mass_matrix_norm: 1.0,
        stiffness_matrix_norm: 1.0
    )
    let response = DisplacementTimeHistory(
        part: Part(),
        modal_result: modal_result,
        t_samples: [0s, 0.01s],
        mode_coords: [[0.0, 0.0]]
    )

    let tip : List<Length> = displacement_at(response, "tip", vec3(0.0, 0.0, 1.0))
}
"#;

/// `displacement_at(...)`'s result must reach a `List<Length>` consumer as a
/// `List<Length>`.
#[test]
fn displacement_at_result_feeds_dimensioned_list_length_consumer() {
    let module = compile_source_with_stdlib(LIST_LENGTH_CONSUMER_PROBE);

    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "binding `displacement_at(...)` through a `let tip : List<Length>` \
         annotation must produce no Error diagnostics (#6094); got: {errs:?}"
    );

    // A `List<...>` let annotation is not yet checked against its initializer
    // (#6947), so this pins the inferred cell type, not the annotation.
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "DisplacementAtLengthConsumerProbe")
        .unwrap_or_else(|| {
            panic!(
                "DisplacementAtLengthConsumerProbe template not found; available: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "tip")
        .unwrap_or_else(|| {
            panic!(
                "cell 'tip' not found on DisplacementAtLengthConsumerProbe; available: {:?}",
                template
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        cell.cell_type,
        list_of_length(),
        "the `tip` cell should type as List<Scalar<LENGTH>> (= List<Length>); got: {:?}",
        cell.cell_type
    );
}
