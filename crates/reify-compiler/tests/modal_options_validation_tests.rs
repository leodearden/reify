#![allow(clippy::doc_overindented_list_items)]
//! Tests for `crates/reify-compiler/stdlib/modal_analysis.ri` —
//! `std.modal.analysis` module: `DampingDescriptor`, `NoDamping`,
//! `RayleighDamping`, `Mode`, `ModalResult`, and `ModalOptions` structure
//! definitions for the v0.3 modal-analysis kernel surface (task α), plus
//! the task η ForcingFunction family: `ForcingFunction` marker trait,
//! `StepForce`, `ImpulseForce`, `HarmonicForce`, `SampledForce`, and
//! `ForcingTimeHistory` structure definitions for the transient-response
//! forcing-time-history input surface (PRD §5.1 / §10 task η).
//!
//! Observable signal for PRD §10 tasks α and η
//! (docs/prds/v0_3/modal-analysis.md). Per the PRD, this file parses
//! the structure_defs and confirms type resolution matches the expected
//! shape.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path
//! (mirroring `buckling_stdlib_compile.rs`), that the five α structures
//! (`NoDamping`, `RayleighDamping`, `Mode`, `ModalResult`, `ModalOptions`)
//! and one α trait (`DampingDescriptor`) are correctly represented in the
//! compiled module, that the positivity constraints on
//! `ModalOptions.{n_modes, tol, max_iters}` are declared at the
//! structure-def level, and that the η ForcingFunction family (one marker
//! trait + five structure_defs with constraints and defaults) matches the
//! PRD §5.1 spec.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production. This mirrors the helper trio in `buckling_stdlib_compile.rs`.
//!
//! OUT-OF-SCOPE LODGERS — read this before changing the structure-ctor
//! argument binder in `crates/reify-compiler/src/expr.rs`. Two tests near the
//! bottom of this file pin GENERIC ctor BINDING semantics (task 4522's
//! by-name resolution, unlabelled args filling only the REMAINING slots in
//! declaration order, an unknown label DIAGNOSED since task 5303 (ε) as
//! `DiagnosticCode::CtorUnknownField` at the `CTOR_FIELD_CONFORMANCE_SEVERITY`
//! knob — Warning pre-δ, Error at δ — while STILL taking the lenient
//! `__arg{i}` push it always did, and a duplicate label diagnosed as a
//! codeless Error) rather than anything modal:
//!   - `structure_ctor_args_bind_by_name_not_positionally`
//!   - `misspelled_ctor_label_is_diagnosed_but_still_leniently_appended`
//!
//! `RayleighDamping` is only their vehicle — they landed here because task
//! 6093 held this file's lock and not the binder's. Their contract home is
//! `crates/reify-compiler/tests/struct_ctor_field_conformance_tests.rs`; the
//! relocation is filed as follow-up (escalation id
//! `agent-followup-6093-mishomed-tests`). Until it happens, a binder change
//! must run THIS binary too.

use reify_compiler::*;
use reify_core::*;
use reify_ir::*;
use reify_test_support::{
    collect_value_ref_members, compile_source_with_stdlib, errors_only, warnings_only,
};

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/modal/analysis` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/modal/analysis")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/modal/analysis module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a structure template by name within the `std/modal/analysis` module.
///
/// `Mode`, `ModalResult`, `ModalOptions`, `NoDamping`, and `RayleighDamping`
/// are top-level structures, so we go through `module.templates` and filter on
/// `EntityKind::Structure` to keep the assertion stable against future
/// non-structure additions to the module.
#[allow(dead_code)]
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/modal/analysis, got templates: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

/// Collect the param-kind value cells (ignoring `let` and auto cells) from a
/// template, returning them in the file order they were declared.
#[allow(dead_code)]
fn param_cells(template: &TopologyTemplate) -> Vec<&ValueCellDecl> {
    template
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect()
}

/// Look up the named param cell on `template` and return its `default_expr`.
/// Panics with a clear message if the cell or its default is missing.
#[allow(dead_code)]
fn require_default<'a>(template: &'a TopologyTemplate, member: &str) -> &'a CompiledExpr {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("{}.{} missing", template.name, member));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("{}.{} missing default_expr", template.name, member))
}

/// True when `code` is one of the diagnostic codes emitted by the struct-ctor
/// field-conformance pass (tasks 5302 / 5303 / 4584 / 4598 / 4622 / 4444).
///
/// Severity-agnostic ON PURPOSE: `CTOR_FIELD_CONFORMANCE_SEVERITY`
/// (the const of that name in `reify-compiler/src/conformance/mod.rs` — cited
/// by SYMBOL, never by line, so the cite cannot rot) is `Warning` pre-δ, and
/// the planned Warning→Error flip must not move any pin that filters here.
///
/// THIRD verbatim copy of this set — `harness_compilation_surface/
/// examples_smoke.rs` and `struct_ctor_field_conformance_tests.rs` hold the
/// other two, because integration tests are separate binaries and cannot share
/// a private helper without a support-crate hop. At three copies the
/// duplication no longer pays for itself: a new member of `DiagnosticCode`'s
/// conformance family has to be added in three places, and a miss is SILENT
/// (the stale copy simply stops matching rather than failing to build). The
/// hoist into `reify-test-support` is filed as #6323 — it is out of this task's
/// module locks, so it is cited here rather than done here.
fn is_ctor_conformance_code(code: Option<DiagnosticCode>) -> bool {
    matches!(
        code,
        Some(
            DiagnosticCode::ArgTypeMismatch
                | DiagnosticCode::SelectorKindMismatch
                | DiagnosticCode::TypeNotConformingToTrait
                | DiagnosticCode::TypeNotConformingToStructureRef
                | DiagnosticCode::TypeNotConformingToVector
                | DiagnosticCode::CtorUnknownField
                | DiagnosticCode::CtorArity
        )
    )
}

/// The prefix `emit_arg_type_mismatch` puts before the offending param label in
/// every ctor-conformance message it words
/// (`reify-compiler/src/conformance/mod.rs`; full shape `argument 'X' has type
/// 'A' but param 'X' requires type 'B'`).
///
/// Mirrors `CTOR_DIAGNOSTIC_ARG_PREFIX` in
/// `harness_compilation_surface/examples_smoke.rs`, and rides the same #6323
/// hoist as [`is_ctor_conformance_code`].
const CTOR_DIAGNOSTIC_ARG_PREFIX: &str = "argument '";

/// The two `RayleighDamping` params task #6093 retyped, in declaration order.
const RAYLEIGH_PARAMS: [&str; 2] = ["alpha", "beta"];

/// True when `message` is a ctor-conformance diagnostic naming exactly `param`.
///
/// Matched on the QUOTED label (`argument 'beta'`), never a bare
/// `contains(param)`: a module compiled through `compile_source_with_stdlib`
/// carries the diagnostics of the probe source AND of the whole stdlib prelude,
/// so an unquoted match would also catch any prelude message that merely
/// contains the word. Same reasoning as the `names_typo` closure in
/// [`misspelled_ctor_label_is_diagnosed_but_still_leniently_appended`].
fn names_ctor_arg(message: &str, param: &str) -> bool {
    message.contains(&format!("{CTOR_DIAGNOSTIC_ARG_PREFIX}{param}'"))
}

/// True when `d` is a ctor-conformance diagnostic naming one of
/// [`RAYLEIGH_PARAMS`] — i.e. one attributable to a `RayleighDamping` ctor
/// rather than to some unrelated prelude construct.
///
/// This is the narrowing every count-based pin below filters through. Without
/// it a pin counts EVERY conformance diagnostic in the module, prelude
/// included, so a future stdlib ctor emitting one would flip the pin red with a
/// message pointing at RayleighDamping. `CTOR_FIELD_CONFORMANCE_SEVERITY` is
/// `Warning` pre-δ, so such a diagnostic need not even break the build to do it.
fn judges_rayleigh_ctor_arg(d: &Diagnostic) -> bool {
    is_ctor_conformance_code(d.code)
        && RAYLEIGH_PARAMS
            .iter()
            .any(|p| names_ctor_arg(&d.message, p))
}

// ─── step-1: module loads with zero error diagnostics ────────────────────────

/// The std/modal/analysis module must load through the production stdlib path
/// with zero error-severity diagnostics. The loader-level `assert!` already
/// fails fast on Error diagnostics during init, but this test independently
/// asserts the post-init invariant so a regression is caught at the test
/// boundary rather than at first stdlib touch.
#[test]
fn std_modal_analysis_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in modal_analysis.ri: {:?}",
        errors
    );
}

// ─── step-3: DampingDescriptor marker trait declared ─────────────────────────

/// `DampingDescriptor` is the marker trait the two damping-descriptor
/// structures (`NoDamping`, `RayleighDamping`) refine. Empty trait surface,
/// no methods — matches the marker-trait precedent in
/// `fea_multi_case.ri:288 trait Support { }` and
/// `trajectory.ri::trait BoundaryCondition { }`.
///
/// The trait must exist as an entry in `CompiledModule.trait_defs` (not
/// `templates`, which stores `Structure` / `Occurrence` entities only) in
/// the compiled `std/modal/analysis` module so the `: DampingDescriptor`
/// refinement clause on `NoDamping` / `RayleighDamping` resolves at
/// structure-def compile time, and so `Type::TraitObject("DampingDescriptor")`
/// resolves on `ModalResult.damping` and `ModalOptions.damping` once those
/// land.
#[test]
fn damping_descriptor_trait_declared() {
    let module = load_stdlib_module();

    let matches: Vec<_> = module
        .trait_defs
        .iter()
        .filter(|t| t.name == "DampingDescriptor")
        .collect();

    assert_eq!(
        matches.len(),
        1,
        "expected exactly one `trait DampingDescriptor` in \
         std/modal/analysis::trait_defs; got {} matches. Module trait_defs: {:?}",
        matches.len(),
        module
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );
}

// ─── step-5: NoDamping marker structure ──────────────────────────────────────

/// `NoDamping` is a zero-field marker structure refining `DampingDescriptor`.
/// Semantically equivalent to `RayleighDamping(alpha: 0.0Hz, beta: 0.0s)` but a
/// distinct nominal type so the future `modal_analysis` trampoline can
/// discriminate the no-damping fast path via SIR-α nominal type-tag.
///
/// Assertions mirror the "no constraints or defaults" discipline from
/// `buckling_stdlib_compile.rs::mode_struct_has_no_constraints_or_defaults`
/// (445-472): zero params, zero constraints, and refines `DampingDescriptor`
/// via `template.trait_bounds`.
#[test]
fn no_damping_marker_structure() {
    let template = find_structure("NoDamping");

    // (a) zero param cells — pure marker structure
    let params = param_cells(template);
    assert_eq!(
        params.len(),
        0,
        "NoDamping should be a zero-field marker structure, but got params: {:?}",
        params.iter().map(|vc| &vc.id.member).collect::<Vec<_>>()
    );

    // (b) no constraints — nothing to constrain
    assert!(
        template.constraints.is_empty(),
        "NoDamping should declare no constraints (zero-field marker); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // (c) refines DampingDescriptor via the structure-def `: DampingDescriptor`
    // refinement clause. The plan analysis points at the `materials_fea.ri::
    // Steel_AISI_1045 : ElasticMaterial` precedent; `TopologyTemplate.
    // trait_bounds` (types.rs:518) is the canonical store for the names of
    // traits a structure declares conformance to.
    assert!(
        template
            .trait_bounds
            .iter()
            .any(|t| t == "DampingDescriptor"),
        "NoDamping should refine DampingDescriptor; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-7: RayleighDamping param shape ─────────────────────────────────────

/// `RayleighDamping` declares two PRD §4.2 params with the canonical types:
///
///   - `alpha : Frequency`  (mass-proportional damping coefficient, = s⁻¹)
///   - `beta  : Time`       (stiffness-proportional damping coefficient, = s)
///
/// Per-mode damping ratio: ζ_i = (α + β·ω_i²) / (2·ω_i). Preserves mode-shape
/// orthogonality so transient response stays in real arithmetic.
///
/// Assertions:
///   (a) exactly 2 params, (b) the declared (name, dimension) PAIRS are
///       (alpha, Frequency) and (beta, Time). Declaration ORDER is not part
///       of the claim — it appears only because the assertion loop below
///       walks `params` positionally. That ctor args bind by NAME, and that
///       the mislabel path still binds leniently to `__arg{i}` (so a renamed
///       param falls back to its default, under a Warning pre-δ), are pinned
///       executably by
///       [`structure_ctor_args_bind_by_name_not_positionally`] and
///       [`misspelled_ctor_label_is_diagnosed_but_still_leniently_appended`],
///   (c) neither carries a `default_expr` (input-only fields without a
///       canonical default — PRD §4.2 lists no defaults),
///   (d) no constraints — alpha and beta are conventionally non-negative
///       but physically meaningful at zero (stiffness-only or mass-only
///       damping). Mirrors `solver_buckling.ri:97-107` "explicitly NOT
///       constrained" discipline applied to `sigma`,
///   (e) refines `DampingDescriptor`.
#[test]
fn rayleigh_damping_param_shape() {
    let template = find_structure("RayleighDamping");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        2,
        "RayleighDamping should have exactly 2 param cells (alpha, beta), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    // `RayleighDamping.alpha` / `.beta` tightened from the `Real` PLACEHOLDER
    // to their registered named dimensions — task #6093, following the
    // `Mode.frequency` precedent from task 4548 below. The units previously
    // lived ONLY in the stdlib prose comment; they now live in the type.
    let expected: &[(&str, Type)] = &[
        (
            "alpha",
            Type::Scalar {
                dimension: DimensionVector::FREQUENCY,
            },
        ),
        (
            "beta",
            Type::Scalar {
                dimension: DimensionVector::TIME,
            },
        ),
    ];
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.id.member.as_str(),
            *expected_name,
            "RayleighDamping param at index {} should be `{}`, got `{}`",
            i,
            expected_name,
            cell.id.member
        );
        assert_eq!(
            cell.cell_type, *expected_ty,
            "RayleighDamping.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults on either param
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "RayleighDamping.{} should have no default_expr (no canonical \
             default for damping coefficients per PRD §4.2), but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) no constraints — mirrors solver_buckling.ri:97-107 "explicitly NOT
    // constrained" discipline applied to sigma (zero is physically valid).
    assert!(
        template.constraints.is_empty(),
        "RayleighDamping should declare no constraints (alpha/beta are \
         conventionally non-negative but physically meaningful at zero — \
         stiffness-only or mass-only damping); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // (e) refines DampingDescriptor
    assert!(
        template
            .trait_bounds
            .iter()
            .any(|t| t == "DampingDescriptor"),
        "RayleighDamping should refine DampingDescriptor; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── task-6093 amendment: ctor args bind BY NAME (an `expr.rs` binder
//     contract, not a modal one — see OUT-OF-SCOPE LODGERS above) ────────────

/// Structure-ctor arguments bind BY NAME, not positionally.
///
/// The guard for a claim three example files in this task's scope state in
/// prose (`printer_gantry_modes.ri`, `transient_step_response.ri`,
/// `printer_print_envelope.ri`) — all three previously asserted the OPPOSITE
/// ("binding is POSITIONAL; `name:` labels are cosmetic"), which task-4522's
/// by-name binder in `expr.rs` had already made false. A prose-only correction
/// would rot the same way, so it is pinned here.
///
/// Compiles a `RayleighDamping` ctor in each argument FORM the prose claims
/// about and asserts the lowered `ordered_args` always re-key to
/// `[(alpha, 0.0 s⁻¹), (beta, 0.0003 s)]`. A positional binder would instead
/// produce alpha = 0.0003 s and beta = 0.0 s⁻¹ for the reverse-labelled form,
/// i.e. the same two names carrying each other's value.
///
/// Five arms, because the prose makes a TWO-clause claim and an all-labelled
/// probe only exercises the first:
///   (a) all-labelled, in REVERSE declaration order — the by-name clause;
///   (b) MIXED, `beta` labelled, positional first (`0.0Hz, beta: 0.0003s`);
///   (c) MIXED, `beta` labelled, positional last (`beta: 0.0003s, 0.0Hz`);
///   (d) MIXED, `alpha` labelled, positional last (`alpha: 0.0Hz, 0.0003s`);
///   (e) MIXED, `alpha` labelled, positional first (`0.0003s, alpha: 0.0Hz`).
///
/// (d) and (e) are the two that carry the second clause — "only UNLABELLED args
/// fill the REMAINING slots in declaration order" — because they are the only
/// forms whose result CHANGES if the binder's positional pass stops skipping
/// already-named-bound slots (`reify-compiler/src/expr.rs`, pass 2's
/// `while … param_arg[next_slot].is_some() { next_slot += 1 }`). With the skip
/// removed, both bind the unlabelled `0.0003s` at slot 0, clobbering `alpha`
/// and leaving `beta` unbound — so `ordered_args` becomes `[("alpha", 0.0003 s)]`
/// and the key-list assertion goes red.
///
/// (b) and (c) do NOT discriminate that: `beta` occupies slot 1, so the
/// positional pass lands on slot 0 either way. They are kept as the
/// mixed-form PARSE + by-name coverage the prose also claims, not as skip
/// pins — recorded here so a later reader does not mistake them for the guard.
#[test]
fn structure_ctor_args_bind_by_name_not_positionally() {
    // (a) all-labelled, reverse declaration order.
    assert_rayleigh_ctor_binds_canonically("CtorBindByNameProbe", "beta: 0.0003s, alpha: 0.0Hz");
    // (b)/(c) mixed with `beta` labelled — the unlabelled arg fills slot 0.
    assert_rayleigh_ctor_binds_canonically("CtorMixedBetaLabelLastProbe", "0.0Hz, beta: 0.0003s");
    assert_rayleigh_ctor_binds_canonically("CtorMixedBetaLabelFirstProbe", "beta: 0.0003s, 0.0Hz");
    // (d)/(e) mixed with `alpha` labelled — the unlabelled arg must SKIP the
    //     named-bound slot 0 and land on slot 1 (`beta`). The skip pins.
    assert_rayleigh_ctor_binds_canonically(
        "CtorMixedAlphaLabelFirstProbe",
        "alpha: 0.0Hz, 0.0003s",
    );
    assert_rayleigh_ctor_binds_canonically("CtorMixedAlphaLabelLastProbe", "0.0003s, alpha: 0.0Hz");
}

/// Compile `structure {probe} { let damping = RayleighDamping({ctor_args}) }`
/// and assert the ctor lowers to exactly `[(alpha, 0.0 s⁻¹), (beta, 0.0003 s)]`
/// — the canonical binding, whatever the argument FORM.
///
/// Shared by every arm of [`structure_ctor_args_bind_by_name_not_positionally`]
/// so a new form is one call, not a copied block.
fn assert_rayleigh_ctor_binds_canonically(probe: &str, ctor_args: &str) {
    let source = format!(
        r#"
structure {probe} {{
    let damping = RayleighDamping({ctor_args})
}}
"#
    );
    let module = compile_source_with_stdlib(&source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "`RayleighDamping({ctor_args})` must compile clean, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == probe)
        .unwrap_or_else(|| panic!("{probe} template should be compiled"));
    let damping_expr = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "damping")
        .and_then(|vc| vc.default_expr.as_ref())
        .expect("the `damping` let cell should carry its ctor expression");

    let CompiledExprKind::StructureInstanceCtor { ordered_args, .. } = &damping_expr.kind else {
        panic!(
            "`damping` should lower to a StructureInstanceCtor, got {:?}",
            damping_expr.kind
        );
    };

    let bound: Vec<(&str, Option<&Value>)> = ordered_args
        .iter()
        .map(|(name, e)| {
            (
                name.as_str(),
                match &e.kind {
                    CompiledExprKind::Literal(v) => Some(v),
                    _ => None,
                },
            )
        })
        .collect();

    assert_eq!(
        bound.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        vec!["alpha", "beta"],
        "`RayleighDamping({ctor_args})`: ordered_args must be re-keyed into \
         template declaration order, with each label routed to its OWN param \
         and each UNLABELLED arg filling the next REMAINING slot (never a \
         synthetic `__arg{{i}}`); got: {:?}",
        bound
    );

    let expected: &[(&str, f64, DimensionVector)] = &[
        ("alpha", 0.0, DimensionVector::FREQUENCY),
        ("beta", 0.0003, DimensionVector::TIME),
    ];
    for (i, (name, si, dim)) in expected.iter().enumerate() {
        match bound[i].1 {
            Some(Value::Scalar {
                si_value,
                dimension,
            }) => {
                assert_eq!(
                    (*si_value, *dimension),
                    (*si, *dim),
                    "`RayleighDamping({ctor_args})`: `{}` must carry the value \
                     written against ITS OWN label (a positional binder that \
                     ignored labels, or one whose positional pass did not skip \
                     named-bound slots, would swap the two)",
                    name
                );
            }
            other => panic!(
                "`RayleighDamping({ctor_args})`: `{}` should bind a dimensioned \
                 Scalar literal, got {:?}",
                name, other
            ),
        }
    }
}

// ─── task-6093 amendment: the MISLABEL path is diagnosed but still lenient
//     (an `expr.rs` binder contract, not a modal one — see OUT-OF-SCOPE
//     LODGERS above) ───────────────────────────────────────────────────────

/// An UNKNOWN ctor label is DIAGNOSED — task 5303 (ε) emits
/// `E_CTOR_UNKNOWN_FIELD` / [`DiagnosticCode::CtorUnknownField`] at the
/// `CTOR_FIELD_CONFORMANCE_SEVERITY` knob (Warning pre-δ, Error at δ) — and is
/// STILL appended as a positional `__arg{i}`, so the param it was meant for
/// falls back to its default (or stays unbound).
///
/// The diagnostic and the lenient push carry DIFFERENT predicates on purpose
/// (`crates/reify-compiler/src/expr.rs`, the by-name binder): ε is
/// diagnostics-only and left the IR byte-for-byte what it was before, which is
/// why (b) and (c) below are unchanged from this pin's pre-ε shape while (a)
/// is inverted.
///
/// The hazard three example files in this task's scope describe in prose;
/// pinned here so their wording cannot rot. If the diagnosis moves again —
/// δ's Warning→Error flip is the scheduled one — update this pin AND the
/// binding notes in `examples/modal/printer_gantry_modes.ri`,
/// `examples/modal/transient_step_response.ri` and
/// `examples/trajectory/printer_print_envelope.ri`.
#[test]
fn misspelled_ctor_label_is_diagnosed_but_still_leniently_appended() {
    // `bta` is a typo for `beta`. Nothing REJECTS it: ε diagnoses it at
    // Warning and binds it leniently anyway, so the compile still succeeds.
    let module = compile_source_with_stdlib(
        r#"
structure CtorMisspelledLabelProbe {
    let damping = RayleighDamping(alpha: 0.0Hz, bta: 0.0003s)
}
"#,
    );

    // (a) the typo IS judged. Deliberately NOT counted over
    // `module.diagnostics` whole: that ranges over the probe source AND the
    // whole stdlib prelude, so any unrelated future lint would turn this red
    // with a message that actively misdirects. Narrowed to the diagnostics
    // naming the typo'd label `bta` — a SOURCE IDENTIFIER that occurs nowhere
    // in the prelude, so the count below is exact — with the code, severity
    // and wording then asserted on the single hit.
    //
    // Narrowing on the IDENTIFIER rather than on the ctor-conformance code set
    // is also what keeps this pin able to see a CODELESS emission. Codeless is
    // no longer the shape of THIS diagnostic — ε (task 5303) gave it
    // `DiagnosticCode::CtorUnknownField`, which is what (a) now asserts — but
    // it is still a live shape on this surface: the sibling duplicate-named-arg
    // diagnostic in the same binder is built with a bare `Diagnostic::error`
    // and carries no code at all, so a code-set filter alone would miss a
    // re-emission in that style.
    //
    // The label match is QUOTED (`'bta'` / `` `bta` ``), never a bare
    // `contains("bta")`: the bare form matches the substring inside ordinary
    // English words — "o(bta)in" is the obvious one — which would reintroduce
    // exactly the unrelated-axis false red this narrowing exists to remove.
    // Both quotings are accepted because the live emitters disagree:
    // `E_CTOR_UNKNOWN_FIELD` says `argument 'bta'` and the duplicate-named-arg
    // error says `duplicate named argument 'x'`, while backtick-quoting is
    // common elsewhere in the diagnostic corpus. Passing this filter at all is
    // therefore the "message names the offending label" assertion; the
    // constructor half is asserted separately below.
    let names_typo = |m: &str| m.contains("'bta'") || m.contains("`bta`");
    let judging: Vec<&Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| names_typo(&d.message))
        .collect();
    assert_eq!(
        judging.len(),
        1,
        "an unknown ctor label must emit exactly one diagnostic naming it \
         (ε, task 5303 — before that it was silently accepted). Got {}: {:#?}",
        judging.len(),
        judging
    );
    assert_eq!(
        judging[0].code,
        Some(DiagnosticCode::CtorUnknownField),
        "the unknown-label diagnostic must carry the CtorUnknownField code — \
         `reify check` never prints the code, so this is the only place the \
         machine-readable half is pinned from this file. Got: {:?}",
        judging[0]
    );
    assert!(
        is_ctor_conformance_code(judging[0].code),
        "…and the LOCAL copy of the ctor-conformance code set must recognise \
         it. A stale copy stops matching rather than failing to build, which \
         is exactly the SILENT miss that copy's docstring warns about. Got: \
         {:?}",
        judging[0].code
    );
    assert_eq!(
        judging[0].severity,
        Severity::Warning,
        "ε emits at the `CTOR_FIELD_CONFORMANCE_SEVERITY` knob, whose value is \
         `Warning` pre-δ. The knob is `pub(crate)` inside a private \
         `conformance` module, so an integration-test binary cannot name it \
         and has to restate the value — δ's one-const flip must therefore move \
         THIS line together with the contract-home pin in \
         `struct_ctor_field_conformance_tests.rs`. Got: {:?}",
        judging[0]
    );
    assert!(
        judging[0].message.starts_with("E_CTOR_UNKNOWN_FIELD: "),
        "the mnemonic must be a message PREFIX — `reify check` renders \
         `{{severity}}: {{message}}` and never prints the DiagnosticCode, so \
         without it the signal is invisible at the CLI. Got: {:?}",
        judging[0].message
    );
    assert!(
        judging[0].message.contains("RayleighDamping"),
        "the message must name the CONSTRUCTOR as well as the offending \
         label, or a reader cannot tell which call is wrong. Got: {:?}",
        judging[0].message
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "CtorMisspelledLabelProbe")
        .expect("CtorMisspelledLabelProbe template should be compiled");
    let damping_expr = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "damping")
        .and_then(|vc| vc.default_expr.as_ref())
        .expect("the `damping` let cell should carry its ctor expression");
    let CompiledExprKind::StructureInstanceCtor { ordered_args, .. } = &damping_expr.kind else {
        panic!(
            "`damping` should lower to a StructureInstanceCtor, got {:?}",
            damping_expr.kind
        );
    };
    let bound: Vec<&str> = ordered_args.iter().map(|(n, _)| n.as_str()).collect();

    // (b) the typo survives lowering as a synthetic positional key.
    assert!(
        bound.iter().any(|n| n.starts_with("__arg")),
        "the unknown label must survive as a synthetic `__arg{{i}}` key; \
         got ordered_args keys: {:?}",
        bound
    );

    // (c) …and `beta` — the param the author meant — never got a value.
    assert!(
        !bound.contains(&"beta"),
        "`beta` must be left unbound when its label is misspelled (it falls \
         back to its default, which RayleighDamping does not have); got \
         ordered_args keys: {:?}",
        bound
    );
}

// ─── task-6093 amendment: the REAL corpus carries the dimension ──────────────

/// The migrated ctor args in `examples/modal/transient_step_response.ri` lower
/// to DIMENSIONED `Value::Scalar` literals — a debug-runnable pin over the real
/// corpus file, not a synthetic snippet.
///
/// Compiles the file itself (no eigensolve, so no release gate) and destructures
/// the lowered `RayleighDamping` ctor args. RED before task #6093's migration,
/// while the file still said `alpha: 0.0, beta: 0.0003`. Companion to the
/// release-gated value-layer pin (e) in
/// `reify-eval-fea-tests/tests/modal_transient_e2e.rs`. The other migrated
/// corpus sites are guarded by
/// `examples_smoke::no_example_emits_ctor_field_conformance_diagnostics`,
/// which gates at ANY severity.
#[test]
fn corpus_rayleigh_ctor_args_lower_to_dimensioned_literals() {
    let module = compile_source_with_stdlib(include_str!(
        "../../../examples/modal/transient_step_response.ri"
    ));
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "examples/modal/transient_step_response.ri must compile clean; got {}: {:#?}",
        errors.len(),
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "CantileverStepResponse")
        .expect("CantileverStepResponse template should be compiled");
    let opts_expr = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "opts")
        .and_then(|vc| vc.default_expr.as_ref())
        .expect("the `opts` let cell should carry its ModalOptions ctor");
    let CompiledExprKind::StructureInstanceCtor { ordered_args, .. } = &opts_expr.kind else {
        panic!("`opts` should lower to a StructureInstanceCtor, got {:?}", opts_expr.kind);
    };
    let damping_expr = ordered_args
        .iter()
        .find(|(name, _)| name == "damping")
        .map(|(_, e)| e)
        .expect("ModalOptions ctor should carry a `damping` arg");
    let CompiledExprKind::StructureInstanceCtor {
        type_name,
        ordered_args: damping_args,
        ..
    } = &damping_expr.kind
    else {
        panic!(
            "`damping` should lower to a nested StructureInstanceCtor, got {:?}",
            damping_expr.kind
        );
    };
    assert_eq!(type_name, "RayleighDamping");

    // Exact SI equality: both `Hz` and `s` have unit factor 1.0, so the
    // migration must reproduce the previous bare-`Real` magnitudes bit-for-bit.
    let expected: &[(&str, f64, DimensionVector)] = &[
        ("alpha", 0.0, DimensionVector::FREQUENCY),
        ("beta", 0.0003, DimensionVector::TIME),
    ];
    for (name, si, dim) in expected {
        let arg = damping_args
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, e)| e)
            .unwrap_or_else(|| {
                panic!(
                    "RayleighDamping ctor should bind `{}`; got keys: {:?}",
                    name,
                    damping_args.iter().map(|(n, _)| n).collect::<Vec<_>>()
                )
            });
        match &arg.kind {
            CompiledExprKind::Literal(Value::Scalar {
                si_value,
                dimension,
            }) => assert_eq!(
                (*si_value, *dimension),
                (*si, *dim),
                "RayleighDamping.{} in transient_step_response.ri must lower to \
                 a dimensioned literal with the pre-migration SI magnitude",
                name
            ),
            CompiledExprKind::Literal(Value::Real(r)) => panic!(
                "RayleighDamping.{} is still a BARE Real({}) — the corpus file \
                 has been reverted to a dimensionless literal (task #6093)",
                name, r
            ),
            other => panic!(
                "RayleighDamping.{} should lower to a Scalar literal, got {:?}",
                name, other
            ),
        }
    }
}

// ─── task-6093: declared dimensions propagate to field reads ─────────────────

/// The retype's user-observable consequence: a field READ now carries the
/// declared dimension. Asserts (i) that `damping.beta + 1.0s` and
/// `damping.alpha + 1.0Hz` type-check (RED while the params were `Real`), and
/// (ii) that adding a bare `1.0` to `damping.beta` is now REJECTED (RED the
/// other way — this is what proves the retype tightened rather than widened).
/// Asserted on diagnostic substance, "dimension mismatch in addition", not
/// exact prose.
///
/// The CTOR-ARG half is pinned separately by
/// [`bare_real_rayleigh_ctor_arg_emits_arg_type_mismatch`]; the eval-side
/// reader is a third seam again, owned by
/// docs/prds/v0_6/dimension-checked-readers.md and deliberately left tolerant.
#[test]
fn rayleigh_damping_fields_propagate_declared_dimensions() {
    // (i) Dimensioned reads must type-check: alpha is a Frequency, beta a Time.
    let dimensioned = r#"
structure DampingFieldReadProbe {
    let damping = RayleighDamping(alpha: 0.0Hz, beta: 0.0003s)
    let beta_plus_time  = damping.beta + 1.0s
    let alpha_plus_freq = damping.alpha + 1.0Hz
}
"#;
    let module = compile_source_with_stdlib(dimensioned);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "adding `1.0s` to RayleighDamping.beta and `1.0Hz` to .alpha must \
         type-check once the params carry their declared dimensions. \
         RED while the params are `Real` (`dimension mismatch in addition: \
         Real vs Scalar[s]`). Got {}: {:#?}",
        errors.len(),
        errors
    );

    // (ii) Inversely, adding a BARE dimensionless literal to a now-dimensioned
    // field must be rejected. This arm is what proves the retype actually
    // tightened something rather than merely widening what is accepted.
    let bare = r#"
structure DampingFieldBareProbe {
    let damping = RayleighDamping(alpha: 0.0Hz, beta: 0.0003s)
    let beta_plus_bare = damping.beta + 1.0
}
"#;
    let bare_module = compile_source_with_stdlib(bare);
    let bare_errors = errors_only(&bare_module);
    // Filtered on `DiagnosticCode` IDENTITY, not on the message prose:
    // `DimensionMismatch` is minted with Add/Sub-specific semantics
    // (`reify-core/src/diagnostics.rs:497`) and is attached by the single
    // producer `type_compat::format_dimension_mismatch_diagnostic`
    // (`type_compat.rs:1369`), so a wording touch-up to that message must not
    // read here as a semantic regression. The messages stay in the failure
    // output for diagnosability.
    assert!(
        bare_errors
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::DimensionMismatch)),
        "adding a bare dimensionless `1.0` to RayleighDamping.beta (declared \
         `Time`) must raise a `DimensionMismatch` error (message shape today: \
         \"dimension mismatch in addition: Real vs Scalar[s]\"). RED while the \
         param is `Real`, where this snippet compiles clean. Got {}: {:#?}",
        bare_errors.len(),
        bare_errors
    );
}

// ─── task-6093 amendment: the ctor-arg half of the retype ────────────────────

/// A bare-`Real` ctor arg at the now-dimensioned `alpha` / `beta` slots is
/// REJECTED with `ArgTypeMismatch`, and the migrated corpus form is silent.
///
/// This is task #6093's originally-stated acceptance signal, and it IS
/// deliverable — the retype is what makes these slots dimensioned, and the
/// struct-ctor field-conformance pass judges dimensioned slots under strict
/// `DimensionVector` equality since docs/prds/v0_6/dimensioned-construction-
/// strictness.md §7.1 (task #5627) landed. Measured on this branch: the same
/// snippet against the pre-retype `Real` params was silent.
///
/// Judged through [`judges_rayleigh_ctor_arg`]: severity-agnostic (the
/// `Warning`→`Error` flip of `CTOR_FIELD_CONFORMANCE_SEVERITY` must not move
/// this pin) and narrowed to diagnostics naming `alpha`/`beta`, so an unrelated
/// future prelude conformance diagnostic cannot flip this red with a message
/// that misdirects at RayleighDamping.
#[test]
fn bare_real_rayleigh_ctor_arg_emits_arg_type_mismatch() {
    let bare = compile_source_with_stdlib(
        r#"
structure BareCtorArgProbe {
    let damping = RayleighDamping(alpha: 0.0, beta: 0.0003)
}
"#,
    );
    let mismatches: Vec<&str> = bare
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch) && judges_rayleigh_ctor_arg(d))
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        mismatches.len(),
        2,
        "a bare dimensionless literal at each of the two dimensioned slots must \
         raise exactly one ArgTypeMismatch naming that slot; got {:?} out of \
         module diagnostics {:#?}",
        mismatches,
        bare.diagnostics
    );
    for param in RAYLEIGH_PARAMS {
        assert!(
            mismatches.iter().any(|m| names_ctor_arg(m, param)),
            "one ArgTypeMismatch must name `{param}`; got: {:?}",
            mismatches
        );
    }

    // The migrated corpus form is the silent one — otherwise
    // `examples_smoke::no_example_emits_ctor_field_conformance_diagnostics`
    // (which gates at ANY severity) would be red on the whole modal corpus.
    let migrated = compile_source_with_stdlib(
        r#"
structure MigratedCtorArgProbe {
    let damping = RayleighDamping(alpha: 0.0Hz, beta: 0.0003s)
}
"#,
    );
    let migrated_judged: Vec<&Diagnostic> = migrated
        .diagnostics
        .iter()
        .filter(|d| judges_rayleigh_ctor_arg(d))
        .collect();
    assert!(
        migrated_judged.is_empty(),
        "the migrated unit-literal form must be accepted — no ctor-conformance \
         diagnostic at ANY severity may name `alpha` or `beta`; got: {:#?}",
        migrated_judged
    );
}

/// TRANSPOSED units at CORRECT labels — `RayleighDamping(alpha: 0.0003s,
/// beta: 0.0Hz)` — are rejected with one `ArgTypeMismatch` per slot.
///
/// This is the retype's highest-value failure mode, and the one seam that ONLY
/// the compile-time ctor gate can catch. At the value layer a transposition is
/// invisible: `read_scalar_si` (`reify-eval/src/modal_ops.rs`) folds
/// `Value::Scalar` to its bare `si_value` and drops the dimension entirely, so
/// a swap evaluates to a silently wrong damping curve with no error anywhere
/// downstream. Gating that reader is a separate seam, owned by
/// docs/prds/v0_6/dimension-checked-readers.md and deliberately left tolerant
/// here.
///
/// Distinct from [`bare_real_rayleigh_ctor_arg_emits_arg_type_mismatch`]: that
/// probe pins Real-vs-`Scalar`, i.e. that the slot is dimensioned AT ALL. This
/// one pins that the conformance walker compares under strict
/// `DimensionVector` EQUALITY, so two well-formed dimensioned args at the two
/// correct labels still fail when their dimensions are transposed.
///
/// Judged through [`judges_rayleigh_ctor_arg`], for the reasons recorded there.
#[test]
fn swapped_rayleigh_ctor_unit_dimensions_emit_arg_type_mismatch() {
    let swapped = compile_source_with_stdlib(
        r#"
structure SwappedCtorArgProbe {
    let damping = RayleighDamping(alpha: 0.0003s, beta: 0.0Hz)
}
"#,
    );
    let mismatches: Vec<&str> = swapped
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch) && judges_rayleigh_ctor_arg(d))
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        mismatches.len(),
        2,
        "a `Time` literal at the `Frequency` slot and a `Frequency` literal at \
         the `Time` slot must EACH raise ArgTypeMismatch — strict \
         DimensionVector equality, not merely dimensioned-vs-Real; got {:?} out \
         of module diagnostics {:#?}",
        mismatches,
        swapped.diagnostics
    );
    for param in RAYLEIGH_PARAMS {
        assert!(
            mismatches.iter().any(|m| names_ctor_arg(m, param)),
            "one ArgTypeMismatch must name `{param}` — a gate that fired only \
             once would leave half the transposition unreported; got: {:?}",
            mismatches
        );
    }
}

// ─── step-9: Mode param shape (no constraints, no defaults) ──────────────────

/// `Mode` (in std/modal/analysis — NOT std/solver/buckling's coexisting
/// Mode; see plan.json design-decision-6) must declare exactly the four
/// PRD §4.1 params with the canonical types:
///
///   - `frequency          : Frequency`            (natural frequency, = s⁻¹;
///                                                  tightened from the Real placeholder in task 4548)
///   - `shape              : List<Vector3<Dimensionless>>`  (mass-normalized eigenvector;
///                                                  dimensionless under Φᵀ·M·Φ = I — NOT a placeholder)
///   - `participation_mass : Real`                 (effective modal mass along reference direction)
///   - `damping_ratio      : Real`                 (ζ_i derived from Rayleigh α/β, or 0 for undamped)
///
/// Mode lives in std/modal/analysis; the `find_structure` helper at the top
/// of this file already filters to that module via `load_stdlib_module()`,
/// so this lookup does NOT see buckling's Mode template even though both
/// modules share the simple name (per the per-module template storage
/// invariant pinned in plan design-decision-6).
#[test]
fn mode_struct_has_correct_param_shape() {
    let template = find_structure("Mode");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        4,
        "Mode should have exactly 4 param cells \
         (frequency, shape, participation_mass, damping_ratio), got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        // `Mode.frequency` tightened from the `Real` PLACEHOLDER to the
        // registered `Frequency` named dimension (= s⁻¹) — task 4548.
        (
            "frequency",
            Type::Scalar {
                dimension: DimensionVector::FREQUENCY,
            },
        ),
        (
            "shape",
            Type::List(Box::new(Type::vec3(Type::dimensionless_scalar()))),
        ),
        // participation_mass / damping_ratio remain dimensionless-scalar
        // placeholders (out of scope for task 4548's Mode.frequency tightening;
        // adopt main's Type::Real → dimensionless_scalar() sweep).
        ("participation_mass", Type::dimensionless_scalar()),
        ("damping_ratio", Type::dimensionless_scalar()),
    ];

    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.id.member.as_str(),
            *expected_name,
            "Mode param at index {} should be `{}`, got `{}`",
            i,
            expected_name,
            cell.id.member
        );
        assert_eq!(
            cell.cell_type, *expected_ty,
            "Mode.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }
}

/// `Mode` is a solver-populated output container — every field is determined
/// by the modal solve, so caller-supplied defaults are meaningless and no
/// per-field scalar invariant is expressible per-field (frequency depends on
/// the geometry, shape is collection-shaped, participation_mass and
/// damping_ratio are derived). Mirrors `Mode` discipline in
/// `buckling_stdlib_compile.rs::mode_struct_has_no_constraints_or_defaults`
/// (445-472).
#[test]
fn mode_struct_has_no_constraints_or_defaults() {
    let template = find_structure("Mode");

    // No defaults: every Mode instance must be solver-populated.
    for cell in param_cells(template) {
        assert!(
            cell.default_expr.is_none(),
            "Mode.{} should have no default_expr (solver-only-produced), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // No constraints: frequency / participation_mass / damping_ratio are all
    // physically non-negative but every modal-solver implementation enforces
    // that as a producer invariant; declaring them at the structure-def
    // level would be redundant duplication and could fire spuriously on
    // floating-point round-off. shape is collection-shaped, not scalar.
    assert!(
        template.constraints.is_empty(),
        "Mode should declare no constraints (solver-only-produced output \
         container, producer-enforced invariants only); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-11: ModalResult param shape (no constraints, no defaults) ──────────

/// `ModalResult` is the solver-output container (PRD §4.1). It must declare
/// exactly the six PRD §4.1 params with the canonical types, in declaration
/// order:
///
///   - `part                  : Part`                    (StructureRef — task 4578)
///   - `modes                 : List<Mode>`              (computed eigenpairs;
///                                                        `Mode` is module-local
///                                                        → `Type::StructureRef`)
///   - `boundary_conditions    : List<Support>`           (`Support` is the
///                                                        marker trait from
///                                                        `std.fea.multi_case`,
///                                                        in the growing prelude
///                                                        → `Type::TraitObject`,
///                                                        same as trajectory's
///                                                        `List<BoundaryCondition>`)
///   - `damping               : DampingDescriptor`       (trait-typed
///                                                        → `Type::TraitObject`)
///   - `mass_matrix_norm       : Real`                    (‖M‖ diagnostic)
///   - `stiffness_matrix_norm  : Real`                    (‖K‖ diagnostic)
///
/// Type representations confirmed against the trajectory precedent
/// (`trajectory_stdlib_compile.rs:628-639`): a module-local structure name
/// resolves to `Type::StructureRef`, a prelude marker trait resolves to
/// `Type::TraitObject`, and `List<Trait>` wraps the trait object in
/// `Type::List`.
///
/// `ModalResult` is solver-populated only: every field is determined by the
/// modal solve, so no caller-supplied defaults are meaningful and no scalar
/// constraint is declared at the structure-def level (collection invariants
/// such as "modes non-empty / sorted by frequency" are enforced at the future
/// modal_analysis trampoline, mirroring `BucklingResult` discipline at
/// `solver_buckling.ri:196-205` and the no-constraints-no-defaults shape from
/// `buckling_stdlib_compile.rs::mode_struct_has_no_constraints_or_defaults`).
#[test]
fn modal_result_struct_has_correct_param_shape() {
    let template = find_structure("ModalResult");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        6,
        "ModalResult should have exactly 6 param cells (part, modes, \
         boundary_conditions, damping, mass_matrix_norm, \
         stiffness_matrix_norm), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("part", Type::StructureRef("Part".to_string())),
        (
            "modes",
            Type::List(Box::new(Type::StructureRef("Mode".to_string()))),
        ),
        (
            "boundary_conditions",
            Type::List(Box::new(Type::TraitObject("Support".to_string()))),
        ),
        (
            "damping",
            Type::TraitObject("DampingDescriptor".to_string()),
        ),
        ("mass_matrix_norm", Type::dimensionless_scalar()),
        ("stiffness_matrix_norm", Type::dimensionless_scalar()),
    ];

    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ModalResult params must be declared in canonical order \
         (part, modes, boundary_conditions, damping, mass_matrix_norm, \
         stiffness_matrix_norm); got: {:?}",
        names
    );

    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ModalResult.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — solver-populated output container
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "ModalResult.{} should have no default_expr (solver-only-produced \
             output container), but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (c) no constraints — collection invariants (modes non-empty / sorted)
    // are enforced at the future modal_analysis trampoline, not declared at
    // the structure-def level (mirrors BucklingResult discipline at
    // solver_buckling.ri:196-205).
    assert!(
        template.constraints.is_empty(),
        "ModalResult should declare no constraints (solver-only-produced \
         output container; collection invariants are trampoline-enforced); \
         got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-13: ModalOptions param shape ───────────────────────────────────────

/// `ModalOptions` is the modal-analysis solver-input knob bundle (PRD §4.3).
/// It must declare exactly the eight params (the seven PRD §4.3 params plus the
/// task-4066 `element_order` selector) with the canonical types, in declaration
/// order:
///
///   - `n_modes             : Int`                       (# modes to extract)
///   - `boundary_conditions  : List<Support>`             (`Support` marker
///                                                        trait from
///                                                        `std.fea.multi_case`
///                                                        → `List<TraitObject>`)
///   - `damping             : DampingDescriptor`         (trait-typed
///                                                        → `Type::TraitObject`)
///   - `sigma               : Real`                       (spectral shift origin)
///   - `tol                 : Real`                       (convergence tolerance)
///   - `max_iters           : Int`                        (Lanczos iteration cap)
///   - `reference_direction  : Vector3<Dimensionless>`     (unit excitation
///                                                        direction — a unit
///                                                        vector is
///                                                        dimensionless, so
///                                                        `Dimensionless` is
///                                                        mathematically
///                                                        accurate, NOT a
///                                                        placeholder)
///   - `element_order        : ElementOrder`               (P1/P2 finite-element
///                                                        order for the (K, M)
///                                                        assembly; task 4066 —
///                                                        `Type::Enum("ElementOrder")`,
///                                                        same as
///                                                        `ElasticOptions.element_order`)
///
/// `reference_direction` uses `Vector3<Dimensionless>` — identical to the
/// `Mode.shape : List<Vector3<Dimensionless>>` encoding. `Vector3<Real>` is
/// NOT valid .ri syntax: the `Vector3<Q>` resolver requires `Q` to resolve
/// to a `DimensionVector`, and `Real` is a primitive scalar, not a dimension
/// name. `Vector3<Dimensionless>` resolves to
/// `Type::vec3(Type::dimensionless_scalar())` (same representation pinned by
/// `mode_struct_has_correct_param_shape`).
///
/// This test pins ONLY the param count, names, declaration order, and types.
/// Defaults are pinned separately by step-15
/// (`modal_options_param_defaults_match_spec`) and constraints by step-17
/// (`modal_options_constrains_positivity_invariants`), so this test
/// deliberately asserts neither.
#[test]
fn modal_options_struct_has_correct_param_shape() {
    let template = find_structure("ModalOptions");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        8,
        "ModalOptions should have exactly 8 param cells (n_modes, \
         boundary_conditions, damping, sigma, tol, max_iters, \
         reference_direction, element_order), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("n_modes", Type::Int),
        (
            "boundary_conditions",
            Type::List(Box::new(Type::TraitObject("Support".to_string()))),
        ),
        (
            "damping",
            Type::TraitObject("DampingDescriptor".to_string()),
        ),
        ("sigma", Type::dimensionless_scalar()),
        ("tol", Type::dimensionless_scalar()),
        ("max_iters", Type::Int),
        (
            "reference_direction",
            Type::vec3(Type::dimensionless_scalar()),
        ),
        // task 4066 — P1/P2 finite-element-order selector for the (K, M)
        // assembly; `Type::Enum("ElementOrder")`, exactly like
        // `ElasticOptions.element_order` (solver_elastic_tests.rs:204).
        ("element_order", Type::Enum("ElementOrder".to_string())),
    ];

    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ModalOptions params must be declared in canonical order \
         (n_modes, boundary_conditions, damping, sigma, tol, max_iters, \
         reference_direction, element_order); got: {:?}",
        names
    );

    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ModalOptions.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }
}

// ─── step-15: ModalOptions literal-valued defaults ───────────────────────────

/// `ModalOptions` carries literal-valued defaults on its four scalar knobs
/// (PRD §4.3), limited to literal-valued per plan design-decision-5
/// (trait-typed / Vector3-literal default code-paths are not exercised by
/// any existing stdlib structure_def, so those three params stay defaultless
/// and required-at-construction in this wave):
///
///   - `n_modes   = 10`           (mirrors `BucklingOptions.n_modes`; the
///                                "first few modes" inspection workflow)
///   - `sigma     = 0.0`          (smallest-|λ| / lowest-frequency cluster)
///   - `tol       = 0.000000001`  (= 1e-9; decimal literal because Reify's
///                                number grammar has no scientific notation —
///                                strict-equality discipline per
///                                solver_buckling.ri:62-64)
///   - `max_iters = 200`          (PRD §4.3 — NOT 1000; modal converges
///                                faster than buckling)
///
/// `boundary_conditions`, `damping`, and `reference_direction` are required
/// at construction (no canonical default — see plan design-decision-5).
///
/// Mirrors `buckling_stdlib_compile.rs::buckling_options_param_defaults_match_spec`
/// (208-288), including the strict-equality float discipline (IEEE-754
/// round-to-nearest of these exact decimal literals is deterministic).
#[test]
fn modal_options_param_defaults_match_spec() {
    let template = find_structure("ModalOptions");

    // n_modes = 10
    let n_modes_default = require_default(template, "n_modes");
    match &n_modes_default.kind {
        CompiledExprKind::Literal(Value::Int(v)) => {
            assert_eq!(*v, 10, "n_modes default should be 10, got: {}", v)
        }
        other => panic!(
            "n_modes default should be Literal(Value::Int(10)), got: {:?}",
            other
        ),
    }

    // sigma = 0.0 (strict equality; IEEE-754 round-to-nearest deterministic)
    let sigma_default = require_default(template, "sigma");
    match &sigma_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) => {
            assert_eq!(*v, 0.0, "sigma default should be exactly 0.0, got: {}", v)
        }
        other => panic!(
            "sigma default should be Literal(Value::Real(0.0)), got: {:?}",
            other
        ),
    }

    // tol = 0.000000001 (= 1e-9 in decimal; strict-equality discipline per
    // solver_buckling.ri:62-64 decimal-encoding note)
    let tol_default = require_default(template, "tol");
    match &tol_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) => assert_eq!(
            *v, 0.000000001,
            "tol default should be exactly 0.000000001 (= 1e-9), got: {}",
            v
        ),
        other => panic!(
            "tol default should be Literal(Value::Real(0.000000001)), got: {:?}",
            other
        ),
    }

    // max_iters = 200 (NOT 1000 — PRD §4.3 specifies 200 for modal)
    let max_iters_default = require_default(template, "max_iters");
    match &max_iters_default.kind {
        CompiledExprKind::Literal(Value::Int(v)) => {
            assert_eq!(*v, 200, "max_iters default should be 200, got: {}", v)
        }
        other => panic!(
            "max_iters default should be Literal(Value::Int(200)), got: {:?}",
            other
        ),
    }

    // boundary_conditions / damping / reference_direction are required at
    // construction — no canonical default (plan design-decision-5).
    for member in ["boundary_conditions", "damping", "reference_direction"] {
        let cell = template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == member)
            .unwrap_or_else(|| panic!("ModalOptions.{} missing", member));
        assert!(
            cell.default_expr.is_none(),
            "ModalOptions.{} should have NO default_expr (required at \
             construction per plan design-decision-5), but got: {:?}",
            member,
            cell.default_expr
        );
    }
}

// ─── step-17: ModalOptions positivity-invariant constraints ──────────────────

/// `ModalOptions` must declare exactly the three PRD §4.3 positivity
/// constraints at the structure-def level:
///
///   constraint n_modes   > 0
///   constraint tol       > 0
///   constraint max_iters > 0
///
/// Making the contract explicit in production code rather than relying solely
/// on test coverage is the task-2544 convention (recorded in memory id
/// 0773d3a8).
///
/// Scope note: this test asserts only the *presence and shape* of the
/// constraint AST nodes on the compiled `ModalOptions` template. It does NOT
/// instantiate `ModalOptions(n_modes: 0)` and assert a diagnostic — that
/// user-observable signal is satisfied compositionally (plan
/// design-decision 7), not re-verified here. These structure-def
/// declarations feed the SIR-α generic constraint-firing pipeline, which is
/// pinned end-to-end by
/// `crates/reify-eval/tests/harness_fea_solver_e2e/stress_error_messages.rs::constraint_violation_diagnostic`
/// (constraint → `Satisfaction::Violated` diagnostic) and the
/// `Value::StructureInstance` round-trip in
/// `crates/reify-eval/tests/structure_instance_e2e.rs`. A modal-specific
/// eval test would duplicate that generic coverage without adding signal.
///
/// Explicitly NOT constrained (regression-gated by the tight count==3):
///   - `sigma`               : any spectral shift is physically valid (the
///                              negative side of the spectrum is meaningful);
///                              `sigma >= 0` would wrongly forbid it. Mirrors
///                              the `BucklingOptions.sigma` discipline.
///   - `reference_direction` : the `norm() > 0` invariant is a method-call on
///                              Vector3, NOT a scalar predicate, so it is not
///                              expressible in Reify's `constraint` grammar.
///                              Deferred to the runtime trampoline (future
///                              task ζ) per plan design-decision-4, mirroring
///                              `BucklingOptions.mode` allowlist-deferral.
///   - `damping`             : trait-typed; not scalar-predicable.
///   - `boundary_conditions` : collection of trait objects; not scalar-
///                              predicable.
///
/// Assertion shape mirrors
/// `buckling_stdlib_compile.rs::buckling_options_constrains_positivity_invariants`
/// (320-376), including the tight count==3 regression gate and the
/// Int(0)/Real(0.0) RHS-literal future-proofing.
#[test]
fn modal_options_constrains_positivity_invariants() {
    let template = find_structure("ModalOptions");

    // Tight count: exactly 3 constraints. A weaker `>= 3` would let a bogus
    // 4th constraint (e.g., an accidental `constraint sigma >= 0` that would
    // silently exclude negative-side-of-spectrum shifts) pass. The .ri file's
    // "explicitly NOT constrained" note is enforced here as a regression gate.
    assert_eq!(
        template.constraints.len(),
        3,
        "ModalOptions should declare exactly 3 constraints \
         (n_modes > 0, tol > 0, max_iters > 0); sigma / damping / \
         boundary_conditions / reference_direction are explicitly NOT \
         constrained per the .ri file. Got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    for required in &["n_modes", "tol", "max_iters"] {
        let matched = template.constraints.iter().any(|c| {
            // Constraint expression must be a `>` BinOp with a ValueRef to the
            // required member on the LHS and the literal `0` on the RHS.
            // Accept either `Int(0)` or `Real(0.0)` for the RHS literal
            // (mirrors buckling_stdlib_compile.rs:356-360 future-proofing).
            match &c.expr.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    if *op != BinOp::Gt
                        || !collect_value_ref_members(left)
                            .iter()
                            .any(|m| m.as_str() == *required)
                    {
                        return false;
                    }
                    match &right.kind {
                        CompiledExprKind::Literal(Value::Int(0)) => true,
                        CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => true,
                        _ => false,
                    }
                }
                _ => false,
            }
        });
        assert!(
            matched,
            "ModalOptions should declare `constraint {} > 0`; got constraints: {:?}",
            required,
            template
                .constraints
                .iter()
                .map(|c| &c.expr.kind)
                .collect::<Vec<_>>()
        );
    }
}

// ─── η additions: ForcingFunction family ─────────────────────────────────────

/// Recursively walk an expression tree collecting `(method_name, member_name)`
/// pairs from `MethodCall { object: ValueRef(member), method: name, .. }` nodes.
/// The traversal also recurses into `BinOp`, `UnOp`, and nested `MethodCall`
/// receivers so a deeply-nested chain like `sources.count > 0` surfaces
/// `("count", "sources")`.
///
/// Ported verbatim from `crates/reify-compiler/tests/trajectory_stdlib_compile.rs:125-144`
/// (same helper used by `piecewise_polynomial_profile_constrains_waypoints_nonempty`
/// for the `waypoints.count > 0` assertion shape needed here).
fn collect_method_call_chain(expr: &CompiledExpr) -> Vec<(&str, &str)> {
    let mut pairs = Vec::new();
    match &expr.kind {
        CompiledExprKind::MethodCall { object, method, .. } => {
            if let CompiledExprKind::ValueRef(cell_id) = &object.kind {
                pairs.push((method.as_str(), cell_id.member.as_str()));
            }
            pairs.extend(collect_method_call_chain(object));
        }
        CompiledExprKind::BinOp { left, right, .. } => {
            pairs.extend(collect_method_call_chain(left));
            pairs.extend(collect_method_call_chain(right));
        }
        CompiledExprKind::UnOp { operand, .. } => {
            pairs.extend(collect_method_call_chain(operand));
        }
        _ => {}
    }
    pairs
}

// ─── step-3 (η): StepForce param shape ───────────────────────────────────────

/// `StepForce` (PRD §5.1) applies a unit-step force at a location. Must
/// declare exactly 4 params in declaration order:
///
///   - `at        : Selector`                 (topology selector for force location; task 4577)
///   - `direction : Vector3<Dimensionless>`   (unit excitation vector)
///   - `magnitude : Force`                    (positive scalar size)
///   - `start_time : Time`                    (step onset time)
///
/// Must refine `ForcingFunction` via `trait_bounds`. No defaults on any
/// param (all caller-supplied). Constraint lands in step-6.
///
/// `at` param type RED until step-6 changes `param at : String` to `param at : Selector`
/// in modal_analysis.ri (task 4577).
#[test]
fn step_force_struct_has_correct_param_shape() {
    let template = find_structure("StepForce");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        4,
        "StepForce should have exactly 4 param cells \
         (at, direction, magnitude, start_time), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("at", Type::AnySelector), // RED until step-6 changes at:String->at:Selector
        ("direction", Type::vec3(Type::dimensionless_scalar())),
        (
            "magnitude",
            Type::Scalar {
                dimension: DimensionVector::FORCE,
            },
        ),
        (
            "start_time",
            Type::Scalar {
                dimension: DimensionVector::TIME,
            },
        ),
    ];
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "StepForce params must be in canonical order (at, direction, magnitude, start_time)"
    );
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "StepForce.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — all caller-supplied
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "StepForce.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) refines ForcingFunction
    assert!(
        template.trait_bounds.iter().any(|t| t == "ForcingFunction"),
        "StepForce should refine ForcingFunction; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-7 (η): ImpulseForce param shape ────────────────────────────────────

/// `ImpulseForce` (PRD §5.1) applies a Dirac-delta impulse at a location.
/// Must declare exactly 4 params in declaration order:
///
///   - `at        : Selector`                 (topology selector for force location; task 4577)
///   - `direction : Vector3<Dimensionless>`   (unit excitation vector)
///   - `impulse   : Impulse`                  (N·s = momentum = kg·m·s⁻¹)
///   - `time      : Time`                     (delta-application time)
///
/// Must refine `ForcingFunction` via `trait_bounds`. No defaults.
/// `impulse` is now tightened from the `Real` PLACEHOLDER to the registered
/// `Impulse` named dimension (= N·s = momentum = MASS·LENGTH·TIME⁻¹; task 4548
/// added it to NAMED_DIMENSIONS). The positivity constraint is verified
/// separately.
///
/// `at` param type RED until step-6 changes `param at : String` to `param at : Selector`
/// in modal_analysis.ri (task 4577).
#[test]
fn impulse_force_struct_has_correct_param_shape() {
    let template = find_structure("ImpulseForce");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        4,
        "ImpulseForce should have exactly 4 param cells \
         (at, direction, impulse, time), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("at", Type::AnySelector), // RED until step-6 changes at:String->at:Selector
        ("direction", Type::vec3(Type::dimensionless_scalar())),
        (
            "impulse",
            // Tightened from the `Real` PLACEHOLDER to the registered Impulse
            // dimension (N·s = momentum = kg·m·s⁻¹) — task 4548.
            Type::Scalar {
                dimension: DimensionVector::IMPULSE,
            },
        ),
        (
            "time",
            Type::Scalar {
                dimension: DimensionVector::TIME,
            },
        ),
    ];
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ImpulseForce params must be in canonical order (at, direction, impulse, time)"
    );
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ImpulseForce.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — all caller-supplied
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "ImpulseForce.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) refines ForcingFunction
    assert!(
        template.trait_bounds.iter().any(|t| t == "ForcingFunction"),
        "ImpulseForce should refine ForcingFunction; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-11 (η): HarmonicForce param shape ──────────────────────────────────

/// `HarmonicForce` (PRD §5.1) applies F(t) = amplitude·sin(2π·frequency·t + phase).
/// Must declare exactly 5 params in declaration order:
///
///   - `at        : Selector`                 (topology selector for force location; task 4577)
///   - `direction : Vector3<Dimensionless>`   (unit excitation vector)
///   - `amplitude : Force`                    (positive peak force)
///   - `frequency : Frequency`                (positive cycles/second)
///   - `phase     : Angle`                    (phase offset; default 0deg)
///
/// Must refine `ForcingFunction`. The `phase` param carries a default of
/// `0deg` (zero Angle literal) per PRD §5.1 default spec; the other four
/// are caller-supplied (no defaults). Constraints land in step-14.
///
/// `at` param type RED until step-6 changes `param at : String` to `param at : Selector`
/// in modal_analysis.ri (task 4577).
#[test]
fn harmonic_force_struct_has_correct_param_shape() {
    let template = find_structure("HarmonicForce");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        5,
        "HarmonicForce should have exactly 5 param cells \
         (at, direction, amplitude, frequency, phase), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("at", Type::AnySelector), // RED until step-6 changes at:String->at:Selector
        ("direction", Type::vec3(Type::dimensionless_scalar())),
        (
            "amplitude",
            Type::Scalar {
                dimension: DimensionVector::FORCE,
            },
        ),
        (
            "frequency",
            Type::Scalar {
                dimension: DimensionVector::FREQUENCY,
            },
        ),
        (
            "phase",
            Type::Scalar {
                dimension: DimensionVector::ANGLE,
            },
        ),
    ];
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "HarmonicForce params must be in canonical order \
         (at, direction, amplitude, frequency, phase)"
    );
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "HarmonicForce.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults on at/direction/amplitude/frequency; phase HAS a default
    for name in &["at", "direction", "amplitude", "frequency"] {
        let cell = params.iter().find(|vc| vc.id.member == *name).unwrap();
        assert!(
            cell.default_expr.is_none(),
            "HarmonicForce.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            name,
            cell.default_expr
        );
    }
    // phase = 0deg — must have a default that is a zero Angle literal
    let phase_default = require_default(template, "phase");
    match &phase_default.kind {
        CompiledExprKind::Literal(Value::Scalar {
            si_value,
            dimension,
        }) if *si_value == 0.0 && *dimension == DimensionVector::ANGLE => {
            // correct: 0deg = 0 radians in SI
        }
        CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => {
            // acceptable fallback if literal-lowering emits Real for zero
        }
        other => panic!(
            "HarmonicForce.phase default should be Literal(0 Angle), got: {:?}",
            other
        ),
    }

    // (d) refines ForcingFunction
    assert!(
        template.trait_bounds.iter().any(|t| t == "ForcingFunction"),
        "HarmonicForce should refine ForcingFunction; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-15 (η): SampledForce param shape ───────────────────────────────────

/// `SampledForce` (PRD §5.1 / §5.3) applies a non-uniform-sample force table
/// (Duhamel/Newmark-β fallback). Must declare exactly 4 params in order:
///
///   - `at           : Selector`       (topology selector for force location; task 4577)
///   - `direction    : Vector3<Dimensionless>` (unit excitation vector)
///   - `time_samples : List<Time>`     (non-uniform time stamps)
///   - `force_samples: List<Force>`    (force magnitudes at each sample)
///
/// Must refine `ForcingFunction`. No defaults. Constraints land in step-18.
/// The cross-list invariant `time_samples.count == force_samples.count` is NOT
/// expressible in Reify constraint grammar (deferred to trampoline task θ).
///
/// `at` param type RED until step-6 changes `param at : String` to `param at : Selector`
/// in modal_analysis.ri (task 4577).
#[test]
fn sampled_force_struct_has_correct_param_shape() {
    let template = find_structure("SampledForce");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        4,
        "SampledForce should have exactly 4 param cells \
         (at, direction, time_samples, force_samples), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("at", Type::AnySelector), // RED until step-6 changes at:String->at:Selector
        ("direction", Type::vec3(Type::dimensionless_scalar())),
        (
            "time_samples",
            Type::List(Box::new(Type::Scalar {
                dimension: DimensionVector::TIME,
            })),
        ),
        (
            "force_samples",
            Type::List(Box::new(Type::Scalar {
                dimension: DimensionVector::FORCE,
            })),
        ),
    ];
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "SampledForce params must be in canonical order \
         (at, direction, time_samples, force_samples)"
    );
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "SampledForce.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — all caller-supplied
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "SampledForce.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) refines ForcingFunction
    assert!(
        template.trait_bounds.iter().any(|t| t == "ForcingFunction"),
        "SampledForce should refine ForcingFunction; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-17 (η): SampledForce non-empty sample constraints ──────────────────

/// `SampledForce` must declare EXACTLY 2 constraints:
///   - `time_samples.count > 0`
///   - `force_samples.count > 0`
///
/// Uses `collect_method_call_chain` to surface the `("count", "time_samples")`
/// and `("count", "force_samples")` method-call pairs on the LHS. Mirrors
/// `piecewise_polynomial_profile_constrains_waypoints_nonempty`
/// (trajectory_stdlib_compile.rs:702-761) for the `waypoints.count > 0` shape.
///
/// Cross-list invariant `time_samples.count == force_samples.count` is NOT
/// constrained here (Reify grammar: single-cell scalar predicates only).
#[test]
fn sampled_force_constrains_samples_nonempty() {
    let template = find_structure("SampledForce");

    assert_eq!(
        template.constraints.len(),
        2,
        "SampledForce should declare exactly 2 constraints \
         (time_samples.count > 0, force_samples.count > 0); \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    for required_member in &["time_samples", "force_samples"] {
        let matched = template.constraints.iter().any(|c| match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Gt {
                    return false;
                }
                let pairs = collect_method_call_chain(left);
                if !pairs.contains(&("count", *required_member)) {
                    return false;
                }
                match &right.kind {
                    CompiledExprKind::Literal(Value::Int(0)) => true,
                    CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => true,
                    _ => false,
                }
            }
            _ => false,
        });
        assert!(
            matched,
            "SampledForce should declare `constraint {}.count > 0`; \
             got constraints: {:?}",
            required_member,
            template
                .constraints
                .iter()
                .map(|c| &c.expr.kind)
                .collect::<Vec<_>>()
        );
    }
}

// ─── step-13 (η): HarmonicForce amplitude + frequency positivity constraints ──

/// `HarmonicForce` must declare EXACTLY 2 constraints:
///   - `amplitude > 0N`  (PRD §5.1 user-observable-signal anchor)
///   - `frequency > 0Hz` (zero/negative frequency is physically degenerate)
///
/// Tight count==2 regression-gates against accidental extras (e.g., a spurious
/// `phase >= 0` that would wrongly forbid negative phase offsets).
///
/// RHS literal is `Value::Scalar { si_value: 0.0, dimension: FORCE/FREQUENCY }`
/// (dimensioned) OR `Value::Real(0.0)` (future-proofing).
///
/// This test serves as the PRD §5.1 user-observable-signal anchor:
/// `HarmonicForce(amplitude: -1N, ...)` → constraint-violation diagnostic.
/// The actual ctor-firing is verified compositionally by
/// `stress_error_messages.rs::constraint_violation_diagnostic` (plan
/// design-decision-7) — this test pins only the structure-def-level AST.
#[test]
fn harmonic_force_constrains_amplitude_and_frequency_positive() {
    let template = find_structure("HarmonicForce");

    assert_eq!(
        template.constraints.len(),
        2,
        "HarmonicForce should declare exactly 2 constraints \
         (amplitude > 0N, frequency > 0Hz); got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    for (required_member, required_dim) in &[
        ("amplitude", DimensionVector::FORCE),
        ("frequency", DimensionVector::FREQUENCY),
    ] {
        let matched = template.constraints.iter().any(|c| match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Gt
                    || !collect_value_ref_members(left)
                        .iter()
                        .any(|m| m.as_str() == *required_member)
                {
                    return false;
                }
                match &right.kind {
                    CompiledExprKind::Literal(Value::Scalar {
                        si_value,
                        dimension,
                    }) if *si_value == 0.0 && dimension == required_dim => true,
                    CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => true,
                    _ => false,
                }
            }
            _ => false,
        });
        assert!(
            matched,
            "HarmonicForce should declare `constraint {} > 0`; \
             got constraints: {:?}",
            required_member,
            template
                .constraints
                .iter()
                .map(|c| &c.expr.kind)
                .collect::<Vec<_>>()
        );
    }
}

// ─── step-9 (η): ImpulseForce impulse positivity constraint ──────────────────

/// `ImpulseForce` must declare exactly 1 constraint: `impulse > 0 * 1N * 1s`.
///
/// `impulse : Impulse` (tightened from the `Real` PLACEHOLDER by task 4548) uses
/// the dimensioned-zero form `0 * 1N * 1s` (N·s = kg·m·s⁻¹ = Impulse), since
/// polymorphic-zero has not landed — same convention as `frequency > 0Hz` on the
/// `Frequency`-typed HarmonicForce.frequency and `magnitude > 0N` on StepForce.
/// Direction carries the sign; impulse is the positive scalar size.
/// Mirrors `step_force_constrains_magnitude_positive` discipline (tight count==1).
#[test]
fn impulse_force_constrains_impulse_positive() {
    let template = find_structure("ImpulseForce");

    assert_eq!(
        template.constraints.len(),
        1,
        "ImpulseForce should declare exactly 1 constraint (impulse > 0 * 1N * 1s); \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // `0 * 1N * 1s` does NOT constant-fold to a single Scalar literal. Unlike
    // the single-token unit literal `0N` (which lowers to one
    // `Literal(Scalar { si_value: 0.0, FORCE })`), the dimensioned-zero
    // *product* `0 * 1N * 1s` lowers to a left-nested `Mul`-chain
    // `(0 * 1N) * 1s` whose overall `result_type` is `Scalar<Impulse>`
    // (N·s = kg·m·s⁻¹). The matcher therefore accepts EITHER the unfolded
    // zero-valued Impulse-dimensioned product OR a future folded single
    // `Literal(Scalar { 0.0, IMPULSE })` (forward-compatible if a constant-fold
    // pass lands later).
    fn is_zero_valued(expr: &CompiledExpr) -> bool {
        match &expr.kind {
            CompiledExprKind::Literal(Value::Int(0)) => true,
            CompiledExprKind::Literal(Value::Real(v)) => *v == 0.0,
            CompiledExprKind::Literal(Value::Scalar { si_value, .. }) => *si_value == 0.0,
            // `0 * x` (or `x * 0`) is zero — recurse through the Mul-chain.
            CompiledExprKind::BinOp {
                op: BinOp::Mul,
                left,
                right,
            } => is_zero_valued(left) || is_zero_valued(right),
            _ => false,
        }
    }

    let impulse_dim_ty = Type::Scalar {
        dimension: DimensionVector::IMPULSE,
    };
    let matched = template.constraints.iter().any(|c| {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Gt
                    || !collect_value_ref_members(left)
                        .iter()
                        .any(|m| m.as_str() == "impulse")
                {
                    return false;
                }
                // RHS must be a zero-valued expression of dimension Impulse:
                // the `0 * 1N * 1s` dimensioned-zero positivity bound.
                right.result_type == impulse_dim_ty && is_zero_valued(right)
            }
            _ => false,
        }
    });
    assert!(
        matched,
        "ImpulseForce should declare `constraint impulse > 0 * 1N * 1s` \
         (dimensioned-zero of dimension Impulse); got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-5 (η): StepForce magnitude positivity constraint ───────────────────

/// `StepForce` must declare exactly 1 constraint: `magnitude > 0N`.
///
/// Convention: `direction : Vector3<Dimensionless>` carries the sign (unit
/// vector); `magnitude : Force` carries the positive scalar size. A negative
/// magnitude is meaningless when direction is the sign-carrying unit vector.
/// PRD §5.1 user-observable signal; task-2544 explicit-contract convention.
///
/// Mirrors `modal_options_constrains_positivity_invariants` (lines 773-826)
/// discipline: tight count==1 regression gate, and the dimensioned RHS literal
/// is accepted as `Value::Scalar { si_value: 0.0, dimension: FORCE }` OR
/// `Value::Real(0.0)` (same future-proofing as Int(0)/Real(0.0) at lines
/// 807-810, applied to the dimensioned-literal lowering path).
#[test]
fn step_force_constrains_magnitude_positive() {
    let template = find_structure("StepForce");

    // Tight count: exactly 1 constraint (regression gate — no accidental extras)
    assert_eq!(
        template.constraints.len(),
        1,
        "StepForce should declare exactly 1 constraint (magnitude > 0N); \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let matched = template.constraints.iter().any(|c| match &c.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            if *op != BinOp::Gt
                || !collect_value_ref_members(left)
                    .iter()
                    .any(|m| m.as_str() == "magnitude")
            {
                return false;
            }
            match &right.kind {
                CompiledExprKind::Literal(Value::Scalar { si_value, .. }) if *si_value == 0.0 => {
                    true
                }
                CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => true,
                _ => false,
            }
        }
        _ => false,
    });
    assert!(
        matched,
        "StepForce should declare `constraint magnitude > 0N`; \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-1 (η): ForcingFunction marker trait declared ───────────────────────

/// `ForcingFunction` is the marker trait for the four transient-forcing
/// primitives (PRD §5.1 / §10 task η). Empty trait surface, no methods —
/// same marker-trait pattern as `trait DampingDescriptor { }` (lines 154-176)
/// and `trait Support { }` (fea_multi_case.ri:288).
///
/// The trait must exist as an entry in `CompiledModule.trait_defs` (not
/// `templates`, which stores `Structure` / `Occurrence` entities only) in
/// the compiled `std/modal/analysis` module so the `: ForcingFunction`
/// refinement clause on each conformer resolves at structure-def compile
/// time, and so `Type::TraitObject("ForcingFunction")` resolves on
/// `ForcingTimeHistory.sources : List<ForcingFunction>`.
#[test]
fn forcing_function_trait_declared() {
    let module = load_stdlib_module();

    let matches: Vec<_> = module
        .trait_defs
        .iter()
        .filter(|t| t.name == "ForcingFunction")
        .collect();

    assert_eq!(
        matches.len(),
        1,
        "expected exactly one `trait ForcingFunction` in \
         std/modal/analysis::trait_defs; got {} matches. Module trait_defs: {:?}",
        matches.len(),
        module
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );
}

// ─── step-19 (η): ForcingTimeHistory param shape ──────────────────────────────

/// `ForcingTimeHistory` (PRD §5.1) is the aggregate container that bundles N
/// forcing sources at the per-Part layer. Must declare exactly 2 params in
/// declaration order:
///
///   - `part    : Part`                        (StructureRef — task 4578)
///   - `sources : List<ForcingFunction>`       (List of trait-object conformers;
///                                              resolves to
///                                              `Type::List(Box::new(Type::TraitObject("ForcingFunction")))`)
///
/// Must NOT refine `ForcingFunction` — `ForcingTimeHistory` is the AGGREGATE
/// container, not a forcing primitive. `trait_bounds` must be empty.
/// No defaults on either param (all caller-supplied).
/// Constraint lands in step-22.
#[test]
fn forcing_time_history_struct_has_correct_param_shape() {
    let template = find_structure("ForcingTimeHistory");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count — exactly 2 params
    assert_eq!(
        params.len(),
        2,
        "ForcingTimeHistory should have exactly 2 param cells (part, sources), \
         got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("part", Type::StructureRef("Part".to_string())),
        (
            "sources",
            Type::List(Box::new(Type::TraitObject("ForcingFunction".to_string()))),
        ),
    ];
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ForcingTimeHistory params must be in canonical order (part, sources)"
    );
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ForcingTimeHistory.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — all caller-supplied
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "ForcingTimeHistory.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) NOT a ForcingFunction conformer — trait_bounds must be empty
    // ForcingTimeHistory is the AGGREGATE container; only the four primitives
    // (StepForce, ImpulseForce, HarmonicForce, SampledForce) refine ForcingFunction.
    assert!(
        template.trait_bounds.is_empty(),
        "ForcingTimeHistory should NOT refine ForcingFunction (it is the \
         aggregate container, not a forcing primitive); got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-21 (η): ForcingTimeHistory sources non-empty constraint ─────────────

/// `ForcingTimeHistory` must declare EXACTLY 1 constraint: `sources.count > 0`.
///
/// Uses `collect_method_call_chain` to surface the `("count", "sources")` pair
/// on the LHS. Mirrors `sampled_force_constrains_samples_nonempty` discipline
/// (tight count==1, `BinOp::Gt`, RHS `Literal(Int(0))` or `Real(0.0)`).
///
/// This constraint encodes the PRD §1 `E_TransientForcingMissing` diagnostic
/// at the structure-def level — an empty `sources` list is caught at
/// construction (via SIR-α's `check_constraints_against_templates`) rather than
/// waiting for the transient_response trampoline (task θ) to flag it. Follows
/// the task-2544 explicit-contract convention mirrored by
/// `PiecewisePolynomialProfile.constraint waypoints.count > 0` (trajectory.ri:230).
#[test]
fn forcing_time_history_constrains_sources_nonempty() {
    let template = find_structure("ForcingTimeHistory");

    assert_eq!(
        template.constraints.len(),
        1,
        "ForcingTimeHistory should declare exactly 1 constraint \
         (sources.count > 0); got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let matched = template.constraints.iter().any(|c| match &c.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            if *op != BinOp::Gt {
                return false;
            }
            let pairs = collect_method_call_chain(left);
            if !pairs.contains(&("count", "sources")) {
                return false;
            }
            match &right.kind {
                CompiledExprKind::Literal(Value::Int(0)) => true,
                CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => true,
                _ => false,
            }
        }
        _ => false,
    });
    assert!(
        matched,
        "ForcingTimeHistory should declare `constraint sources.count > 0`; \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── task-4108: modal uses shared stdlib ElementOrder (no local copy) ─────────

/// Pins three invariants after the task-4108 prelude-enum value-lowering fix:
///
/// (a) `std/modal/analysis` does NOT own a local `enum ElementOrder` — the
///     type is now fully resolved from `std/solver/elastic`'s prelude copy.
/// (b) `ModalOptions.element_order`'s compiled default is
///     `Literal(Value::Enum { type_name: "ElementOrder", variant: "P1" })` —
///     identical to `ElasticOptions.element_order`'s default (solver_elastic_tests
///     `elastic_options_param_defaults_match_spec`), confirming the enum-access
///     lowered correctly through the prelude seeding path.
/// (c) The shared `ElementOrder` enum in `std/solver/elastic` carries variants
///     `["P1", "P2"]` in canonical order — the set modal's runtime trampoline
///     (`variant == "P2"`) reads. This is a cross-module drift anchor: it re-
///     anchors the `[P1, P2]` pin once modal's local copy has been dropped, so
///     the trampoline's dependency on solver_elastic's copy is explicit.
///
/// RED until step-4:
///   - assertion (a) fails: modal_analysis.ri still declares `enum ElementOrder`
///     (line ~290), so modal's `enum_defs` DOES contain an `ElementOrder` entry.
///   - assertions (b) and (c) pass (modal already compiled OK via its local copy,
///     and solver_elastic's ElementOrder is unaffected).
#[test]
fn modal_options_element_order_resolves_to_shared_stdlib_enum() {
    let modal_module = load_stdlib_module();

    // ── (a) modal has NO local ElementOrder enum_def ──────────────────────────
    assert!(
        modal_module
            .enum_defs
            .iter()
            .all(|e| e.name != "ElementOrder"),
        "std/modal/analysis should NOT declare a local `enum ElementOrder` after \
         task-4108 drops the duplicate; got enum_defs: {:?}",
        modal_module
            .enum_defs
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );

    // ── (b) ModalOptions.element_order default == Literal(Value::Enum{ElementOrder, P1}) ──
    let modal_options = find_structure("ModalOptions");
    let element_order_default = require_default(modal_options, "element_order");
    match &element_order_default.kind {
        CompiledExprKind::Literal(Value::Enum {
            type_name, variant, ..
        }) => {
            assert_eq!(
                type_name, "ElementOrder",
                "element_order default type_name should be \"ElementOrder\", got: {:?}",
                type_name
            );
            assert_eq!(
                variant, "P1",
                "element_order default variant should be \"P1\", got: {:?}",
                variant
            );
        }
        other => panic!(
            "ModalOptions.element_order default should be \
             Literal(Value::Enum {{ ElementOrder, P1 }}), got: {:?}",
            other
        ),
    }

    // ── (c) The shared solver_elastic ElementOrder carries [P1, P2] ───────────
    // Cross-module drift anchor: modal's runtime trampoline reads `variant == "P2"`
    // from the shared enum. Re-anchor the [P1, P2] canonical order here so the
    // solver_elastic dependency is explicit even after modal drops its local copy.
    //
    // The primary pin lives in solver_elastic_tests.rs:
    //   `element_order_enum_has_p1_and_p2_variants_in_canonical_order`
    // This assertion adds the cross-module link: it confirms the enum reachable
    // from modal's compiled perspective is that same solver_elastic definition.
    let elastic_module = stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/solver/elastic")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/solver/elastic; available: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        });
    let enum_def = elastic_module
        .enum_defs
        .iter()
        .find(|e| e.name == "ElementOrder")
        .unwrap_or_else(|| {
            panic!(
                "std/solver/elastic should contain `enum ElementOrder`; got: {:?}",
                elastic_module
                    .enum_defs
                    .iter()
                    .map(|e| &e.name)
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        enum_def.variants,
        vec!["P1".into(), "P2".into()],
        "std/solver/elastic ElementOrder variants should be [P1, P2] in canonical order; \
         modal trampoline reads `variant == \"P2\"` against this set. Got: {:?}",
        enum_def.variants
    );
}

// ─── task-4578: Part structure_def (step-1 RED) ───────────────────────────────

/// `Part` must be declared as a zero-field opaque marker structure in
/// `std/modal/analysis` (`docs/prds/v0_6/stdlib-surface-type-substrate.md`
/// §12 open-question-3: "minimal opaque structure_def first, then grow").
///
/// Mirrors `no_damping_marker_structure` (above) but asserts `trait_bounds`
/// is EMPTY — `Part` refines no trait (unlike `NoDamping : DampingDescriptor`).
///
/// RED before step-2 (Part is not yet declared in modal_analysis.ri).
/// GREEN once `structure def Part { }` lands before `ModalResult`.
#[test]
fn part_structure_def_declared() {
    let template = find_structure("Part");

    // (a) zero param cells — opaque marker, no fields
    let params = param_cells(template);
    assert_eq!(
        params.len(),
        0,
        "Part should be a zero-field opaque marker structure, but got params: {:?}",
        params.iter().map(|vc| &vc.id.member).collect::<Vec<_>>()
    );

    // (b) no constraints — nothing to constrain on a zero-field structure
    assert!(
        template.constraints.is_empty(),
        "Part should declare no constraints (zero-field opaque marker); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // (c) no trait refinements — Part refines no trait in this phase
    assert!(
        template.trait_bounds.is_empty(),
        "Part should have no trait_bounds (it refines no trait in the opaque-marker phase); \
         got: {:?}",
        template.trait_bounds
    );
}

/// POSITIVE boundary test: compiling `ForcingTimeHistory(part: Part(), sources: [StepForce(...)])`
/// via the stdlib must produce zero Error-severity diagnostics once `Part` is declared.
///
/// Reuses the `StepForce` construction from examples/modal/transient_step_response.ri:90-96.
/// The `sources.count > 0` constraint is satisfied by the single StepForce.
///
/// RED before step-2: `Part()` is an unknown symbol (no `structure def Part` yet).
/// GREEN once step-2 declares `structure def Part { }` in std/modal/analysis.
#[test]
fn part_value_accepted_where_part_param_declared() {
    let source = r#"
structure PartBoundarySmoke {
    let b   = box(10mm, 10mm, 10mm)
    let dir = vec3(0.0, 0.0, 1.0)
    let tol = 1deg
    let face_sel = faces_by_normal(b, dir, tol)
    let step = StepForce(
        at: face_sel,
        direction: vec3(0.0, 0.0, 1.0),
        magnitude: 10N,
        start_time: 0s
    )
    let forcing = ForcingTimeHistory(part: Part(), sources: [step])
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected zero Error-severity diagnostics for ForcingTimeHistory(part: Part(), ...); \
         RED until `structure def Part {{ }}` lands in std/modal/analysis (step-2). \
         Got {}: {:#?}",
        errors.len(),
        errors
    );
}

// ─── task-4578: Part param shape (step-3 RED) ────────────────────────────────

/// `DisplacementTimeHistory` (PRD §5.2) must declare exactly 4 params in order:
///   - `part         : Part`              (StructureRef — task 4578)
///   - `modal_result : ModalResult`       (StructureRef)
///   - `t_samples    : List<Time>`        (List<Scalar<Time>>)
///   - `mode_coords  : List<List<Real>>`  (List<List<dimensionless>>)
///
/// No existing compiler test pinned DisplacementTimeHistory's param shape;
/// this test adds the missing coverage (per plan design decision).
///
/// RED before step-4 (DisplacementTimeHistory.part is still `: String` in .ri).
/// GREEN once step-4 replaces `param part : String` with `param part : Part`.
#[test]
fn displacement_time_history_part_is_part_type() {
    let template = find_structure("DisplacementTimeHistory");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count — exactly 4 params
    assert_eq!(
        params.len(),
        4,
        "DisplacementTimeHistory should have exactly 4 params \
         (part, modal_result, t_samples, mode_coords), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("part", Type::StructureRef("Part".to_string())),
        (
            "modal_result",
            Type::StructureRef("ModalResult".to_string()),
        ),
        (
            "t_samples",
            Type::List(Box::new(Type::Scalar {
                dimension: DimensionVector::TIME,
            })),
        ),
        (
            "mode_coords",
            Type::List(Box::new(Type::List(Box::new(Type::dimensionless_scalar())))),
        ),
    ];

    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "DisplacementTimeHistory params must be in canonical order \
         (part, modal_result, t_samples, mode_coords); got: {:?}",
        names
    );

    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "DisplacementTimeHistory.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — solver-populated output container
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "DisplacementTimeHistory.{} should have no default_expr \
             (solver-only-produced output container), but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }
}

/// BOUNDARY test (rehomed from 4578 leniency pin-down — task 4584 flip):
/// a string arg to `part : Part` must produce exactly one Error-severity
/// diagnostic with code `TypeNotConformingToStructureRef`.
///
/// Previously pinned as `string_arg_to_part_param_silently_accepted` with an
/// `errors.is_empty()` assertion; flipped intentionally as required by that
/// test's own contract ("update intentionally when task 4584 lands nominal
/// StructureRef arg-rejection"). Task 4584 is the deliberate owner of this
/// behaviour change.
///
/// RED until step-4 (entities_phase): `check_expr_struct_ctor_args` still
/// `continue`-skips every param that is not `List<TraitObject>`, so the walker
/// arm added in step-2 is never reached for the bare StructureRef `part` param.
/// GREEN once step-4 broadens the gate to admit `Type::StructureRef(_)` params.
#[test]
fn string_arg_to_part_param_rejected() {
    let source = r#"
structure PartLeniencySmoke {
    let b   = box(10mm, 10mm, 10mm)
    let dir = vec3(0.0, 0.0, 1.0)
    let tol = 1deg
    let face_sel = faces_by_normal(b, dir, tol)
    let step = StepForce(
        at: face_sel,
        direction: vec3(0.0, 0.0, 1.0),
        magnitude: 10N,
        start_time: 0s
    )
    let forcing = ForcingTimeHistory(part: "beam", sources: [step])
}
"#;
    let module = compile_source_with_stdlib(source);
    // task 5302 α (Option-A uniform downgrade): StructureRef ctor conformance
    // (task 4584) is emitted at CTOR_FIELD_CONFORMANCE_SEVERITY (Warning); code
    // and count are unchanged, δ later flips the knob back to Error.
    let warnings = warnings_only(&module);
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly 1 Warning-severity diagnostic (TypeNotConformingToStructureRef) \
         for ForcingTimeHistory(part: \"beam\", ...) where part : Part; \
         got {}: {:#?}",
        warnings.len(),
        warnings,
    );
    let d = &warnings[0];
    assert_eq!(
        d.code,
        Some(DiagnosticCode::TypeNotConformingToStructureRef),
        "expected TypeNotConformingToStructureRef, got {:?}",
        d.code,
    );
}

// ─── task-4584 step-5/step-6: StructureRef param default rejection ────────────

/// RED until step-6 (impl): a structure declaring `param part : Part = "x"`
/// (StructureRef param with a non-conforming String default) must produce
/// exactly one Error-severity `TypeNotConformingToStructureRef` diagnostic.
///
/// Fails today: param defaults are not checked against their declared cell_type
/// for structure params (only function params are, via fn_param_default_compatible).
/// GREEN once step-6 adds check_param_default_conformance and wires it into
/// phase_fn_arg_conformance.
#[test]
fn structureref_param_with_string_default_rejected() {
    let source = r#"
structure PartDefaultSmoke {
    param part : Part = "x"
}
"#;
    let module = compile_source_with_stdlib(source);
    // task 5302 α (Option-A uniform downgrade): StructureRef param-default
    // conformance (task 4584) is emitted at CTOR_FIELD_CONFORMANCE_SEVERITY
    // (Warning); code and count are unchanged, δ later flips the knob to Error.
    let warnings = warnings_only(&module);
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly 1 Warning-severity diagnostic (TypeNotConformingToStructureRef) \
         for `param part : Part = \"x\"`; got {}: {:#?}",
        warnings.len(),
        warnings,
    );
    let d = &warnings[0];
    assert_eq!(d.severity, reify_core::Severity::Warning);
    assert_eq!(
        d.code,
        Some(DiagnosticCode::TypeNotConformingToStructureRef),
        "expected TypeNotConformingToStructureRef, got {:?}",
        d.code,
    );
}

// ─── task-4584 step-9: no-false-positive guard tests ─────────────────────────

/// NO-FALSE-POSITIVE GUARD (task 4584 step-9).
///
/// `param part : Part = Part()` — a StructureRef param with a valid StructureRef
/// default — must produce ZERO Error-severity diagnostics after task 4584 lands.
///
/// The `check_param_default_conformance` StructureRef branch calls
/// `walk_param_against_arg`, which promotes `Part()` (FunctionCall with scalar
/// placeholder type) to `StructureRef("Part")` and then validates via
/// `type_compatible(StructureRef("Part"), StructureRef("Part"))` → true → no emit.
///
/// Explicitly green-from-add: documents the "reject only genuine nominal mismatches;
/// NO false positives" acceptance criterion. Fails if the StructureRef check
/// incorrectly rejects a `Part()` default at a `Part`-typed param.
#[test]
fn structureref_param_with_valid_structureref_default_no_error() {
    let source = r#"
structure PartDefaultValid {
    param part : Part = Part()
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "NO-FALSE-POSITIVE: `param part : Part = Part()` should produce ZERO Error-severity \
         diagnostics (StructureRef identity is valid). Got {}: {:#?}",
        errors.len(),
        errors
    );
}

/// NO-FALSE-POSITIVE GUARD (task 4584 step-9).
///
/// A structure with non-StructureRef and non-Geometry param defaults must produce
/// ZERO Error-severity diagnostics. The `check_param_default_conformance` function
/// has a `_ => continue` guard for all cell_types other than StructureRef and
/// Geometry; this test documents that scalar/Int/Bool params are not affected.
///
/// Explicitly green-from-add: confirms the StructureRef and Geometry rejection checks
/// do NOT broaden to other param types.
#[test]
fn non_structureref_param_defaults_not_rejected() {
    let source = r#"
structure ScalarParamDefaults {
    param n : Real = 42.0
    param count : Int = 10
    param enabled : Bool = true
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "NO-FALSE-POSITIVE: scalar/Int/Bool param defaults should produce ZERO Error-severity \
         diagnostics (check_param_default_conformance `_ => continue` guard). \
         Got {}: {:#?}",
        errors.len(),
        errors
    );
}

// ─── task 4577: StepForce.at = Selector compile gates ────────────────────────

/// POSITIVE compile gate: a `StepForce` whose `at` is supplied as a kernel-free
/// FaceSelector (via `faces_by_normal`) compiles with zero Error-severity
/// diagnostics — the task's stated boundary "a StepForce.at selecting a face
/// type-checks" (PRD §6/§8.4).
///
/// Uses the bt7 kernel-free idiom: `faces_by_normal(b, dir, tol)` with
/// let-bound arguments so the selector stays kernel-free (never realized
/// against a mesh). The force-location value is type-only at compile time;
/// runtime Selector→mesh-node resolution is task 4122.
///
/// Selector struct-ctor arg enforcement is now active (task 4598 landed): see
/// `step_force_real_at_arg_rejected` (below) for the boundary case that asserts
/// non-Selector values are rejected at `at`. This gate is the no-false-positive
/// complement — it confirms a valid FaceSelector still compiles with zero errors
/// after enforcement. The authoritative proof that `at` resolves to `Selector` is
/// the param-shape assertion `step_force_struct_has_correct_param_shape`
/// (`("at", Type::AnySelector)`).
#[test]
fn step_force_at_selector_compiles_with_zero_errors() {
    let source = r#"
structure StepForceSelectorSmoke {
    let b   = box(10mm, 10mm, 10mm)
    let dir = vec3(0.0, 0.0, 1.0)
    let tol = 1deg
    let face_sel = faces_by_normal(b, dir, tol)
    let step = StepForce(
        at: face_sel,
        direction: vec3(0.0, 0.0, 1.0),
        magnitude: 10N,
        start_time: 0s
    )
}
"#;
    let module = compile_source_with_stdlib(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "StepForce(at: <FaceSelector>, ...) should produce zero Error diagnostics \
         (AnySelector accepts FaceSelector); \
         RED until step-6 changes param at : String -> param at : Selector. \
         Got {}: {:#?}",
        errs.len(),
        errs
    );
}

/// BOUNDARY test (task 4598 flip): a `StepForce` with `at: 0.0` (a Real
/// literal) at the `Selector`-typed `at` param must produce exactly one
/// `ArgTypeMismatch` Error-severity diagnostic.
///
/// Previously pinned as `step_force_real_at_arg_silently_accepted` with an
/// `errs.is_empty()` assertion documenting the soundness gap; flipped by task
/// 4598 (Selector struct-ctor arg enforcement) once both root causes are fixed:
/// (a) `check_expr_struct_ctor_args` gate broadened to admit `AnySelector`
/// params, (b) `walk_param_against_arg_type` leaf arm added for
/// `Selector/AnySelector` params that delegates to `type_compatible`.
///
/// Mirrors the `string_arg_to_part_param_rejected` precedent from task 4584.
#[test]
fn step_force_real_at_arg_rejected() {
    let source = r#"
structure StepForceRealAtSmoke {
    let step = StepForce(
        at: 0.0,
        direction: vec3(0.0, 0.0, 1.0),
        magnitude: 10N,
        start_time: 0s
    )
}
"#;
    let module = compile_source_with_stdlib(source);
    // 5302 α: Selector ctor conformance (task 4598) downgraded Error→Warning (knob).
    let warns = warnings_only(&module);
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 Warning-severity ArgTypeMismatch diagnostic for \
         StepForce(at: 0.0, ...) where at : Selector; got {}: {:#?}",
        warns.len(),
        warns,
    );
    let d = &warns[0];
    assert_eq!(
        d.code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "expected ArgTypeMismatch, got {:?}",
        d.code,
    );
}

/// BOUNDARY test (task 4598): a `StepForce` with `at: "tip"` (a String
/// literal) at the `Selector`-typed `at` param must produce exactly one
/// `ArgTypeMismatch` Error-severity diagnostic.
///
/// Sibling of `step_force_real_at_arg_rejected` above; exercises the String
/// literal path through the same `walk_param_against_arg_type` leaf arm for
/// `Selector/AnySelector` params. Both Real and String are rejected because
/// `type_compatible(AnySelector, Real)` and `type_compatible(AnySelector, String)`
/// are false (type_compat.rs AnySelector arms).
#[test]
fn step_force_string_at_arg_rejected() {
    let source = r#"
structure StepForceStringAtSmoke {
    let step = StepForce(
        at: "tip",
        direction: vec3(0.0, 0.0, 1.0),
        magnitude: 10N,
        start_time: 0s
    )
}
"#;
    let module = compile_source_with_stdlib(source);
    // 5302 α: Selector ctor conformance (task 4598) downgraded Error→Warning (knob).
    let warns = warnings_only(&module);
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 Warning-severity ArgTypeMismatch diagnostic for \
         StepForce(at: \"tip\", ...) where at : Selector; got {}: {:#?}",
        warns.len(),
        warns,
    );
    let d = &warns[0];
    assert_eq!(
        d.code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "expected ArgTypeMismatch, got {:?}",
        d.code,
    );
}

/// BOUNDARY test (task 4598): a `StepForce` with `at: 5` (an Int literal) at the
/// `Selector`-typed `at` param must produce exactly one `ArgTypeMismatch`
/// Error-severity diagnostic.
///
/// Int flows through a different literal branch than Real (no `promote_function_call`
/// promotion step applies), so `result_type` carries `Type::Int` directly into
/// `walk_param_against_arg_type`. This is a distinct path from the Real/String
/// siblings and confirms the arm comment's "Real/String/Int are genuine selector
/// mismatches" claim against `type_compatible(AnySelector, Int) → false`.
#[test]
fn step_force_int_at_arg_rejected() {
    let source = r#"
structure StepForceIntAtSmoke {
    let step = StepForce(
        at: 5,
        direction: vec3(0.0, 0.0, 1.0),
        magnitude: 10N,
        start_time: 0s
    )
}
"#;
    let module = compile_source_with_stdlib(source);
    // 5302 α: Selector ctor conformance (task 4598) downgraded Error→Warning (knob).
    let warns = warnings_only(&module);
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 Warning-severity ArgTypeMismatch diagnostic for \
         StepForce(at: 5, ...) where at : Selector; got {}: {:#?}",
        warns.len(),
        warns,
    );
    let d = &warns[0];
    assert_eq!(
        d.code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "expected ArgTypeMismatch, got {:?}",
        d.code,
    );
}

/// BOUNDARY test (task 4598): a `StepForce` where `at` receives a non-Selector value
/// through a `let` binding (ValueRef path) must produce exactly one `ArgTypeMismatch`
/// Error-severity diagnostic.
///
/// Exercises `walk_param_against_arg_type` directly — as opposed to the literal tests
/// above which reach it via `walk_param_against_arg`'s `_` fallback. Here `x` resolves
/// to `Type::Real` in `result_type`, so the type-level walker sees `(AnySelector, Real)`
/// and rejects via `type_compatible` without any literal-kind dispatch. This hardens
/// the arm against future refactors of the literal dispatch path.
#[test]
fn step_force_valueref_real_at_arg_rejected() {
    let source = r#"
structure StepForceValueRefAtSmoke {
    let x = 0.0
    let step = StepForce(
        at: x,
        direction: vec3(0.0, 0.0, 1.0),
        magnitude: 10N,
        start_time: 0s
    )
}
"#;
    let module = compile_source_with_stdlib(source);
    // 5302 α: Selector ctor conformance (task 4598) downgraded Error→Warning (knob).
    let warns = warnings_only(&module);
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 Warning-severity ArgTypeMismatch diagnostic for \
         StepForce(at: <Real ValueRef>, ...) where at : Selector; got {}: {:#?}",
        warns.len(),
        warns,
    );
    let d = &warns[0];
    assert_eq!(
        d.code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "expected ArgTypeMismatch, got {:?}",
        d.code,
    );
}
