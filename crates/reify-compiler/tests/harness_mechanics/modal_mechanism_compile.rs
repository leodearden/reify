//! Step-1 (RED) / Step-2 (GREEN) compile-typing test for
//! `mechanism_modal_analysis(mechanism, options) -> ModalResult` (task 4271).
//!
//! Observable signal: a `.ri` call to `mechanism_modal_analysis(mech, ModalOptions())`
//! must compile without errors and type as `StructureRef("ModalResult")`.
//!
//! Mirrors the Probe type-check pattern in
//! `crates/reify-compiler/tests/harness_mechanics/dynamics_stdlib_compile.rs`
//! (`point_mass_and_mass_properties_ctors_type_as_mass_properties_struct_ref`,
//! ~line 440) — embeds a `structure def Probe` whose `let` cells are
//! inspected for their resolved `cell_type`.
//!
//! RED until step-2 adds `modal_mechanism_fns.ri` + stdlib_loader registration.
//!
//! Also hosts the sibling-subsystem `displacement_at` return-type pins over
//! `std.modal.analysis.fns` (`mod modal_analysis_fns_stdlib_compile`, below).

use reify_core::*;
use reify_test_support::compile_source_with_stdlib;

/// `mechanism_modal_analysis(mech, ModalOptions())` must compile without errors
/// and the result cell must type as `Type::StructureRef("ModalResult")`.
///
/// Uses `mechanism()` (a JOINT_TYPED_FN_NAMES builtin → `StructureRef("Mechanism")`)
/// and passes it to `mechanism_modal_analysis` with a default `ModalOptions()`.
/// Mirrors the `inverse_dynamics(mechanism, trajectory)` Mechanism-param precedent in
/// dynamics.ri:257 — the function's `mechanism : Mechanism` parameter accepts the
/// `StructureRef("Mechanism")` value produced by `mechanism()`.
///
/// RED: fails with a compile error until step-2 registers
/// `std.modal.mechanism.fns` (containing the `@optimized("modal::mechanism_modal")`
/// function declaration) in stdlib_loader.rs.
#[test]
fn mechanism_modal_analysis_call_types_as_modal_result() {
    let source = r#"
structure def Probe {
    let mech   = mechanism()
    let result = mechanism_modal_analysis(mech, ModalOptions())
}
"#;
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "mechanism_modal_analysis Probe should compile without errors; got: {:?}",
        errors
    );

    let probe = compiled
        .templates
        .iter()
        .find(|t| t.name == "Probe")
        .expect("Probe template should be present in compiled module");

    let result_cell = probe
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "result")
        .expect("Probe.result cell should exist");

    assert_eq!(
        result_cell.cell_type,
        Type::StructureRef("ModalResult".to_string()),
        "mechanism_modal_analysis(mech, ModalOptions()) should type as \
         StructureRef(\"ModalResult\"), not {:?}",
        result_cell.cell_type
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Nested rather than a top-level `tests/modal_analysis_fns_stdlib_compile.rs`: a
// new integration-test file is a new link unit, which the merge-gate compile-cost
// contract exists to remove (docs/prds/merge-gate-compile-cost.md §5) and which
// scripts/check-harness-baseline-registration.sh flags `unregistered-standalone`.
// Absorbing it into an existing binary costs zero link units; the `mod` keeps the
// would-be file's stem, so its tests select as
// `modal_analysis_fns_stdlib_compile::<test>`.
mod modal_analysis_fns_stdlib_compile {
    //! Compile-side type pins for `crates/reify-compiler/stdlib/modal_analysis_fns.ri`
    //! — the `std.modal.analysis.fns` module (split out from `std.modal.analysis`
    //! because `phase_functions` runs before `phase_entities`; see the WHY header at
    //! modal_analysis_fns.ri:6-20).
    //!
    //! Scope (#6094): `displacement_at`'s DECLARED return type, which is
    //! `List<Length>` and not `List<Real>`. WHY the reconstruction Φ·ξ is plain
    //! metres — and why Φ and ξ *alone* stay undimensioned — is derived once, at
    //! the declaration the type belongs to:
    //! `crates/reify-compiler/stdlib/modal_analysis_fns.ri` :: `displacement_at`.
    //!
    //! Two independent signals:
    //!   (a) both `displacement_at` overloads declare `List<Length>`;
    //!   (b) at a real call site the bound cell TYPES as `List<Length>` (with zero
    //!       Error diagnostics) — the user-observable half, since a consumer of
    //!       `tip[j]` gets a Length rather than a bare Real.
    //!
    //! MEASURED (#6094, before the retype): the zero-Error half of (b) passes even
    //! RED — a `let tip : List<Length> = <List<Real> expr>` annotation raises NO
    //! diagnostic today; the inferred type simply wins and the cell types as
    //! `List<Scalar<dimensionless>>`. So the `cell_type` assertion is what actually
    //! carries (b)'s RED signal, and the zero-Error assertion is a companion guard
    //! that the retype does not introduce a diagnostic. Do not read (b) as evidence
    //! that annotated-let mismatches are diagnosed — they are not.

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

    /// Collect ALL compiled functions named `name` in `std/modal/analysis/fns`.
    ///
    /// Deliberately a filter, NOT a `.find()`: `displacement_at` is declared TWICE
    /// (a `location: String` overload and a `location: FaceSelector` overload)
    /// sharing the ONE registered `modal::displacement_at` trampoline. A single
    /// lookup would silently leave the second declaration unpinned — and
    /// `stdlib_loader_tests.rs` records that INTRA-module duplicates like this are
    /// not covered by the loader's duplicate detection, so nothing else catches it.
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

    /// Both `displacement_at` overloads must declare `-> List<Length>`.
    ///
    /// `List<Length>` = `Type::List(Box::new(Type::Scalar { dimension: LENGTH }))`
    /// — one Length scalar per timestep, the same shape as the near-identical
    /// per-time-sample displacement series `deviation_from_nominal(...) ->
    /// List<Length>` (trajectory.ri).
    ///
    /// RED before the retype: both declare `List<Real>` (= `List<Scalar<dimensionless>>`, a
    /// dimensionless scalar list).
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

        // Non-vacuity guard 2: the two overloads are genuinely DIFFERENT
        // declarations — their `location` param types must differ (String vs
        // FaceSelector). Without this, the count above could not distinguish "both
        // covered" from "the same declaration twice".
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

    /// A `structure` reaching a `displacement_at` call site with a well-typed
    /// `DisplacementTimeHistory`, binding the result through a `List<Length>`
    /// annotation.
    ///
    /// DELIBERATELY MINIMAL — it constructs the `DisplacementTimeHistory`
    /// directly rather than driving the modal_analysis -> transient_response ->
    /// displacement_at pipeline. Only the DECLARED return type of
    /// `displacement_at` decides the bound cell's type, so the whole solver
    /// preamble (`Steel_AISI_1045` / `FEAMaterialInput` / `ModalOptions` / `box` /
    /// `faces_by_normal` / `StepForce` / `ForcingTimeHistory` / `modal_analysis` /
    /// `transient_response`) contributed nothing to the signal while coupling
    /// this pin to nine further stdlib signatures — as a third near-verbatim copy
    /// of a fixture already carried by `examples/modal/transient_step_response.ri`
    /// and by `compile_displacement_at_probe`
    /// (reify-eval-fea-tests/tests/r3b_modal_selector_displacement.rs), it would
    /// have rotted into a compile error on any of their signature changes while
    /// adding no coverage those two do not already give. They keep the end-to-end
    /// pipeline coverage; this pin needs only a well-typed `history` argument, so
    /// it builds one.
    ///
    /// `modes` / `boundary_conditions` are empty and the matrix norms are
    /// placeholders: nothing here is evaluated, only type-checked.
    const LIST_LENGTH_CONSUMER_PROBE: &str = r#"
    structure DisplacementAtLengthConsumerProbe {
        let modal_result = ModalResult(
            part: Part(),
            modes: [],
            boundary_conditions: [],
            damping: RayleighDamping(alpha: 0.0, beta: 0.0),
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
    /// `List<Length>` — the task's stated user-observable signal.
    ///
    /// RED before the retype comes from the `cell_type` assertion: the cell types
    /// as `List<Scalar<dimensionless>>` (= `List<Real>`). The zero-Error assertion
    /// passes in BOTH states (measured — the annotation mismatch is undiagnosed);
    /// it is kept as a guard that the retype introduces no new diagnostic, not as
    /// the RED signal.
    #[test]
    fn displacement_at_result_feeds_dimensioned_list_length_consumer() {
        let module = compile_source_with_stdlib(LIST_LENGTH_CONSUMER_PROBE);

        let errs = errors_only(&module);
        assert!(
            errs.is_empty(),
            "binding `displacement_at(...)` through a `let tip : List<Length>` \
             annotation must produce no Error diagnostics (#6094); got: {errs:?}"
        );

        // What follows pins the call site's INFERRED cell type — the type the
        // `displacement_at` declaration propagates into `tip`. It does NOT pin
        // the `: List<Length>` annotation: per the module docs above, an
        // annotated-let mismatch raises no diagnostic today and the inferred type
        // simply wins, so this assertion would read identically with the
        // annotation deleted. The annotation is kept only because it spells out
        // the consumer shape the task is about ("a user who writes
        // `let tip : List<Length> = displacement_at(...)` gets what they asked
        // for"); do not read it as a checked constraint.
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
}
