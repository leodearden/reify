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
// Task #6094's `displacement_at` return-type pins live HERE, as a nested module
// of this already-baseline-registered binary, rather than as a new top-level
// `tests/modal_analysis_fns_stdlib_compile.rs`. That standalone form was flagged
// `reason=unregistered-standalone` by scripts/check-harness-baseline-registration.sh.
// Of the three sanctioned remedies (a baseline grandfather row is NOT one of them
// — SUPERSEDED, Leo 2026-07-22 esc-5056-11: the manifest is a shrinking ratchet,
// not an allow-list to grow), this is the one that adds ZERO new link units, which
// is the actual goal of the C1/C2 contract (PRD docs/prds/merge-gate-compile-cost.md
// §5); a fresh single-module `harness_<subsystem>.rs` root would satisfy the letter
// while re-adding the very binary the contract removes. This file is the
// semantically right host: it is the same CLASS of check (a compile-typing pin over
// a `std.modal.*` stdlib fn, driven through `compile_source_with_stdlib`) on a
// sibling module of the same subsystem — `std.modal.mechanism.fns` here,
// `std.modal.analysis.fns` below.
//
// The nested `mod` keeps the absorbed file's stem, so its tests select as
// `modal_analysis_fns_stdlib_compile::<test>`; no `#[test]` fn is added or removed
// relative to the standalone form. Its imports stay inside the module rather than
// joining this file's top-level `use reify_core::*;`.
mod modal_analysis_fns_stdlib_compile {
    //! Compile-side type pins for `crates/reify-compiler/stdlib/modal_analysis_fns.ri`
    //! — the `std.modal.analysis.fns` module (split out from `std.modal.analysis`
    //! because `phase_functions` runs before `phase_entities`; see the WHY header at
    //! modal_analysis_fns.ri:6-20).
    //!
    //! Scope (#6094): `displacement_at`'s DECLARED return type. The trampoline
    //! reconstructs u(t_j) = Σ_i (Φ_i[node]·direction)·ξ_i[j] — with mass-normalized
    //! mode shapes Φ (kg^-1/2) and conjugate modal coordinates ξ (kg^1/2·m), the
    //! PRODUCT is plain metres: the √mass factors cancel. So the return type is
    //! `List<Length>`, not `List<Real>`.
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
    //!
    //! Note the deliberate NON-scope: `DisplacementTimeHistory.mode_coords` (ξ
    //! alone) and `Mode.shape` (Φ alone) each carry a genuinely unrepresentable
    //! SI-root exponent and stay `Real`/`Dimensionless` — see their notes in
    //! modal_analysis.ri.

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
    /// RED before the retype: both declare `List<Real>` (= `List<Real>`, a
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

    /// A `structure` running the committed modal_analysis → transient_response →
    /// displacement_at pipeline, binding the result through a `List<Length>`
    /// ANNOTATION.
    ///
    /// Adapted from `compile_displacement_at_probe`
    /// (reify-eval-fea-tests/tests/r3b_modal_selector_displacement.rs) so the
    /// `displacement_at` call site is reached with a well-typed
    /// `DisplacementTimeHistory`; the only change is the annotated binding.
    const LIST_LENGTH_CONSUMER_PROBE: &str = r#"
    structure DisplacementAtLengthConsumerProbe {
        param length : Length = 200mm
        param width  : Length = 10mm
        param height : Length = 2mm

        let material = Steel_AISI_1045()
        let mi = FEAMaterialInput(material: material)
        let root = FixedSupport(target: "x_min")
        let opts = ModalOptions(
            n_modes: 3,
            boundary_conditions: [root],
            damping: RayleighDamping(alpha: 0.0, beta: 0.0003),
            sigma: 0.0,
            tol: 0.000000001,
            max_iters: 200,
            reference_direction: vec3(0.0, 0.0, 1.0),
            element_order: ElementOrder.P1
        )
        let result = modal_analysis(mi.material, length, width, height, opts)

        let beam = box(length, width, height)
        let tip_dir = vec3(1.0, 0.0, 0.0)
        let tip_tol = 1deg
        let tip_face = faces_by_normal(beam, tip_dir, tip_tol)
        let tip_push = StepForce(
            at: tip_face,
            direction: vec3(0.0, 0.0, 1.0),
            magnitude: 10N,
            start_time: 0s
        )
        let forcing = ForcingTimeHistory(part: Part(), sources: [tip_push])

        let t_start = 0s
        let t_end   = 0.25s
        let dt      = 0.0002s
        let response = transient_response(result, forcing, t_start, t_end, dt)

        let tip : List<Length> = displacement_at(response, "tip", tip_push.direction)
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

        // The annotation must actually be recorded on the cell — otherwise the
        // zero-Error assertion above could pass by the annotation being dropped
        // rather than checked.
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
