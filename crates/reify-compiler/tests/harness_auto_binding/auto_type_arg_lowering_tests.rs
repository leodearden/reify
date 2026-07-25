//! Integration tests for the compile-pipeline `auto:` / `auto(free):`
//! type-argument call-site (task 3558, B1).
//!
//! These tests drive real `.ri` source containing `Bearing<auto: Seal>()`
//! through `parse_and_compile_with_stdlib` / `compile_source_with_stdlib` and
//! assert on the resulting `CompiledModule.auto_type_substitution` and
//! diagnostics. Before the call-site wiring lands, `auto:` type-args fall into
//! the "unexpected dimensional expression in type argument" else-arm and the
//! substitution stays empty.

use reify_core::*;
use reify_test_support::{compile_source_with_stdlib, parse_and_compile_with_stdlib};

/// True when `code` belongs to the `auto:` type-param resolution diagnostic
/// family. Used by the negative-case tests to assert the new phase stays silent
/// on modules that declare no `auto:` type-args.
fn is_auto_type_param_diagnostic(code: Option<DiagnosticCode>) -> bool {
    matches!(
        code,
        Some(
            DiagnosticCode::AutoTypeParamPoolOverflow
                | DiagnosticCode::AutoTypeParamAmbiguous
                | DiagnosticCode::AutoTypeParamNoCandidate
                | DiagnosticCode::AutoTypeParamNonUnique
                | DiagnosticCode::AutoTypeParamDepthBoundExceeded
                | DiagnosticCode::AutoTypeParamCrossProductSizeExceeded
        )
    )
}

/// Single Seal-conformant candidate (`ORingSeal`) → the `auto: Seal` type-arg
/// resolves deterministically and populates the module's
/// `auto_type_substitution` with `("T", "ORingSeal")`, with no error
/// diagnostics.
#[test]
fn bearing_auto_seal_single_candidate_populates_substitution() {
    let source = r#"
        trait Seal {}
        structure def ORingSeal : Seal { param d : Real = 10.0 }
        structure def Bearing<T: Seal> { param bore : Real = 25.0 }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let compiled = parse_and_compile_with_stdlib(source);

    assert_eq!(
        compiled.auto_type_substitution.as_slice(),
        &[("T".to_string(), "ORingSeal".to_string())],
        "expected the auto: Seal slot to resolve to the single candidate ORingSeal"
    );

    let error_count = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    assert_eq!(
        error_count, 0,
        "expected no error diagnostics, got: {:?}",
        compiled.diagnostics
    );
}

/// No Seal-conformant structure exists, so the strict `auto: Seal` slot has an
/// empty candidate pool → the resolver emits a single
/// `AutoTypeParamNoCandidate` error and leaves the substitution empty. Pins the
/// diagnostic-plumbing path: the resolver must be dispatched (and its
/// diagnostics routed into `ctx.diagnostics`) even when `pending_auto_resolutions`
/// resolves to nothing.
#[test]
fn auto_type_arg_no_candidate_emits_diagnostic() {
    let source = r#"
        trait Seal {}
        structure def Bearing<T: Seal> { param x : Real = 1.0 }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    // Error-tolerant helper: we EXPECT an error diagnostic here.
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<&Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error diagnostic, got: {:?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::AutoTypeParamNoCandidate),
        "the lone error must be the no-candidate diagnostic"
    );

    assert!(
        compiled.auto_type_substitution.as_slice().is_empty(),
        "a failed resolution must leave the substitution empty, got: {:?}",
        compiled.auto_type_substitution.as_slice()
    );
}

/// Two Seal-conformant candidates under STRICT `auto:` → ambiguous. The
/// resolver emits a single `AutoTypeParamAmbiguous` error and leaves the
/// substitution empty (strict mode never auto-picks among ≥2 feasible).
#[test]
fn strict_auto_type_arg_ambiguous_emits_error_diagnostic() {
    let source = r#"
        trait Seal {}
        structure def ORingSeal : Seal { param d : Real = 10.0 }
        structure def GasketSeal : Seal { param w : Real = 2.0 }
        structure def Bearing<T: Seal> { param bore : Real = 25.0 }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<&Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error diagnostic, got: {:?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::AutoTypeParamAmbiguous),
        "the lone error must be the ambiguous diagnostic"
    );

    assert!(
        compiled.auto_type_substitution.as_slice().is_empty(),
        "strict ambiguous resolution must leave the substitution empty, got: {:?}",
        compiled.auto_type_substitution.as_slice()
    );
}

/// Two Seal-conformant candidates under FREE `auto(free):` → the resolver picks
/// the lexicographically-first feasible candidate (`GasketSeal` < `ORingSeal`)
/// and emits a single `AutoTypeParamNonUnique` *warning* (not an error), so the
/// substitution is populated with the lex-first pick.
#[test]
fn free_auto_type_arg_ambiguous_selects_lex_first_with_warning() {
    let source = r#"
        trait Seal {}
        structure def ORingSeal : Seal { param d : Real = 10.0 }
        structure def GasketSeal : Seal { param w : Real = 2.0 }
        structure def Bearing<T: Seal> { param bore : Real = 25.0 }
        structure def Assembly { sub b = Bearing<auto(free): Seal>() }
    "#;

    // No Error-severity diagnostics expected (free mode warns, never errors).
    let compiled = parse_and_compile_with_stdlib(source);

    let nonunique: Vec<&Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamNonUnique))
        .collect();
    assert_eq!(
        nonunique.len(),
        1,
        "expected exactly one non-unique warning, got: {:?}",
        compiled.diagnostics
    );
    assert_eq!(
        nonunique[0].severity,
        Severity::Warning,
        "auto(free) non-unique resolution must warn, not error"
    );

    assert_eq!(
        compiled.auto_type_substitution.as_slice(),
        &[("T".to_string(), "GasketSeal".to_string())],
        "free mode must pick the lexicographically-first feasible candidate"
    );
}

/// Two `auto:` slots on one sub-component, each bound to a different trait with
/// a single conformant candidate → both resolve, and the substitution preserves
/// the target template's declared type-param order (`T` then `U`).
#[test]
fn multi_param_auto_type_args_resolve_in_declared_order() {
    let source = r#"
        trait Seal {}
        trait Cooled {}
        structure def ORingSeal : Seal { param d : Real = 10.0 }
        structure def AirCooled : Cooled { param f : Real = 5.0 }
        structure def Coupling<T: Seal, U: Cooled> { param x : Real = 1.0 }
        structure def Assembly { sub c = Coupling<auto: Seal, auto: Cooled>() }
    "#;

    let compiled = parse_and_compile_with_stdlib(source);

    assert_eq!(
        compiled.auto_type_substitution.as_slice(),
        &[
            ("T".to_string(), "ORingSeal".to_string()),
            ("U".to_string(), "AirCooled".to_string()),
        ],
        "multi-param substitution must follow the target's declared type-param order"
    );

    let error_count = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    assert_eq!(
        error_count, 0,
        "expected no error diagnostics, got: {:?}",
        compiled.diagnostics
    );
}

/// An empty module declares no `auto:` slots → the new phase early-returns,
/// leaving `auto_type_substitution` at its empty default with no AutoTypeParam
/// diagnostics. The empty-substitution invariant is load-bearing for cache
/// stability (an empty Vec hashes deterministically).
#[test]
fn empty_module_has_default_substitution() {
    let compiled = parse_and_compile_with_stdlib("");

    assert!(
        compiled.auto_type_substitution.as_slice().is_empty(),
        "empty module must leave the substitution empty, got: {:?}",
        compiled.auto_type_substitution.as_slice()
    );
    assert!(
        !compiled
            .diagnostics
            .iter()
            .any(|d| is_auto_type_param_diagnostic(d.code)),
        "empty module must emit no AutoTypeParam-family diagnostics, got: {:?}",
        compiled.diagnostics
    );
}

/// A concrete (non-`auto:`) generic instantiation `Bearing<ORingSeal>()` raises
/// no `AutoResolutionRequest` → the phase short-circuits and the substitution
/// stays empty with no AutoTypeParam diagnostics. Pins the negative case so the
/// new phase cannot corrupt substitution or fire spuriously for ordinary
/// generics.
#[test]
fn module_without_auto_type_arg_has_default_substitution() {
    let source = r#"
        trait Seal {}
        structure def ORingSeal : Seal {}
        structure def Bearing<T: Seal> {}
        structure def Assembly { sub b = Bearing<ORingSeal>() }
    "#;

    let compiled = parse_and_compile_with_stdlib(source);

    assert!(
        compiled.auto_type_substitution.as_slice().is_empty(),
        "non-auto generic must leave the substitution empty, got: {:?}",
        compiled.auto_type_substitution.as_slice()
    );
    assert!(
        !compiled
            .diagnostics
            .iter()
            .any(|d| is_auto_type_param_diagnostic(d.code)),
        "non-auto generic must emit no AutoTypeParam-family diagnostics, got: {:?}",
        compiled.diagnostics
    );
}

/// Two sub-components in the same module, each instantiating a *different*
/// template whose corresponding `auto:` type-param happens to share the same
/// identifier (`T`) → the per-`SubComponentDecl` rewrites both succeed (so
/// downstream consumers see the correct resolved `Type::StructureRef` on every
/// sub), but the *module-level aggregate* `auto_type_substitution` retains only
/// the first resolution (first-wins) because `AutoTypeSubstitution::new` panics
/// on duplicate param names.
///
/// Pins the known lossy aggregation called out in
/// `auto_type_param_phase.rs::phase_auto_type_param_resolution`: the aggregate
/// field is a debug/audit view rather than an authoritative per-use-site map.
/// A future qualification scheme (e.g. `Owner.sub.T` keys or a richer
/// `Vec<(owner, sub_name, param, template)>`) would lift this restriction —
/// when that happens, this test will fail loudly, forcing an intentional
/// review of the shape change.
#[test]
fn multi_subs_with_colliding_param_names_first_wins() {
    let source = r#"
        trait Seal {}
        trait Cooled {}
        structure def ORingSeal : Seal { param d : Real = 10.0 }
        structure def AirCooled : Cooled { param f : Real = 5.0 }
        structure def Bearing<T: Seal> { param bore : Real = 25.0 }
        structure def Heatsink<T: Cooled> { param fins : Real = 8.0 }
        structure def Assembly {
            sub b = Bearing<auto: Seal>()
            sub h = Heatsink<auto: Cooled>()
        }
    "#;

    let compiled = parse_and_compile_with_stdlib(source);

    // No errors — both resolutions are unambiguous.
    let error_count = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    assert_eq!(
        error_count, 0,
        "expected no error diagnostics, got: {:?}",
        compiled.diagnostics
    );

    // First-wins on the colliding `T` key: only the Bearing resolution lands.
    // This is the lossy aggregation invariant; the per-sub rewrites below
    // confirm that the loss is confined to this module-level view.
    assert_eq!(
        compiled.auto_type_substitution.as_slice(),
        &[("T".to_string(), "ORingSeal".to_string())],
        "lossy aggregation: only the first `T` resolution survives the dedup, \
         got: {:?}",
        compiled.auto_type_substitution.as_slice(),
    );

    // Per-sub correctness: every `SubComponentDecl.type_args[0]` is correctly
    // rewritten to the resolved candidate, regardless of the aggregate's
    // first-wins behaviour. The slot rewrites carry their own
    // (owner, sub_index, position) tuples and are not subject to name dedup.
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template must compile");
    let bearing_sub = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "b")
        .expect("sub b must be present");
    let heatsink_sub = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "h")
        .expect("sub h must be present");
    assert_eq!(
        bearing_sub.type_args.first(),
        Some(&Type::StructureRef("ORingSeal".to_string())),
        "sub b's type-arg slot must rewrite to the resolved ORingSeal, got: {:?}",
        bearing_sub.type_args,
    );
    assert_eq!(
        heatsink_sub.type_args.first(),
        Some(&Type::StructureRef("AirCooled".to_string())),
        "sub h's type-arg slot must rewrite to the resolved AirCooled \
         (per-sub rewrite must not be dropped by the aggregate's first-wins \
         dedup), got: {:?}",
        heatsink_sub.type_args,
    );
}

/// A user-declared type-param whose name starts with the reserved `__auto_`
/// prefix is rejected with an Error diagnostic. The prefix is the namespace
/// the compiler uses to mint synthetic placeholders for `auto:` type-arg
/// slots (`Type::TypeParam("__auto_<bound>")`); a user-named type-param
/// sharing the prefix could mask a bound check at the wrong site because
/// `check_type_param_bounds` transparently skips every `Type::TypeParam(_)`.
/// Reserving the prefix at the declaration site keeps the two namespaces
/// disjoint without requiring a new `Type` variant in `reify-core`.
#[test]
fn user_type_param_with_reserved_auto_prefix_is_rejected() {
    let source = r#"
        trait Seal {}
        structure def Foo<__auto_Seal: Seal> { param x : Real = 1.0 }
    "#;

    let compiled = compile_source_with_stdlib(source);

    // Find any error whose message mentions the reserved prefix.
    let prefix_errors: Vec<&Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| d.message.contains("reserved") && d.message.contains("__auto_"))
        .collect();
    assert_eq!(
        prefix_errors.len(),
        1,
        "expected exactly one reserved-prefix error diagnostic, got: {:?}",
        compiled.diagnostics
    );
    assert!(
        prefix_errors[0].message.contains("__auto_Seal"),
        "the error must name the offending type-param, got: {:?}",
        prefix_errors[0].message
    );
}
