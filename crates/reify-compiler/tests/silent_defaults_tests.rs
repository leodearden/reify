//! Tests for silent type defaults and missing diagnostics fixes (task 117).
//!
//! These tests verify that the compiler emits diagnostics instead of silently
//! swallowing errors or using misleading defaults.

use reify_test_support::{compile_source, errors_only, warnings_only};

// ── H2: collection member typo should produce a diagnostic ──────────────

#[test]
fn collection_member_typo_produces_diagnostic() {
    // "diametr" is a typo for "diameter" — the compiler should emit
    // a diagnostic about an unknown member rather than silently defaulting
    // to Type::Real.
    let source = r#"
        structure Bolt {
            param diameter : Scalar = 10mm
        }
        structure Assembly {
            sub bolts : List<Bolt>
            constraint bolts.count == 4
            let d = bolts[0].diametr
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_unknown_member = errors.iter().any(|d| d.message.contains("unknown member"));
    assert!(
        has_unknown_member,
        "expected diagnostic about 'unknown member', got: {:?}",
        errors
    );
}
// ── M7: compile_field returns direct value ──────────────────────────────

#[test]
fn compile_field_returns_direct_value() {
    // Regression guard: fields should compile successfully and be present
    // in compiled.fields, both before and after the Option removal refactor.
    let source = r#"
        field def temp : Point3 -> Scalar {
            source = analytical { |p| 1.0m }
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    assert_eq!(module.fields.len(), 1, "expected 1 compiled field");
    assert_eq!(module.fields[0].name, "temp");
}

// ── L1: duplicate function signature diagnostic has context ─────────────

#[test]
fn duplicate_function_signature_diagnostic_has_context() {
    // Two functions with the same name and param types should produce a
    // diagnostic that includes the function name and parameter types.
    let source = r#"
        fn add(a: Scalar, b: Scalar) -> Scalar { a + b }
        fn add(a: Scalar, b: Scalar) -> Scalar { a - b }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let dup_error = errors
        .iter()
        .find(|d| d.message.contains("duplicate function signature"));
    assert!(
        dup_error.is_some(),
        "expected 'duplicate function signature' diagnostic, got: {:?}",
        errors
    );
    let msg = &dup_error.unwrap().message;
    assert!(
        msg.contains("add"),
        "diagnostic should mention function name 'add', got: {}",
        msg
    );
    assert!(
        msg.contains("Scalar"),
        "diagnostic should mention parameter type 'Scalar', got: {}",
        msg
    );
}

// ── L6: unlabeled constraint in trait uses Option<String> ────────────────

#[test]
fn unlabeled_constraint_in_trait_uses_option_none() {
    // A trait with an unlabeled constraint should compile its default
    // with `name: None` (not an empty string sentinel).
    let source = r#"
trait Bounded {
    param x : Length
    constraint x > 0mm
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Bounded")
        .expect("should have trait Bounded");

    // Find the unlabeled constraint default
    let constraint_default = trait_def
        .defaults
        .iter()
        .find(|d| matches!(d.kind, reify_compiler::DefaultKind::Constraint(_)))
        .expect("trait should have a constraint default");

    assert!(
        constraint_default.name.is_none(),
        "unlabeled constraint should have name: None, got: {:?}",
        constraint_default.name
    );
}

// ── L6 regression: param and let defaults always have Some(name) ──────

#[test]
fn trait_default_param_and_let_always_have_name() {
    // A trait with both param and let defaults should have `name.is_some()`
    // for each Param and Let entry. This is a regression guard confirming
    // the invariant before hardening with .expect() in step-14.
    let source = r#"
trait Configurable {
    param width : Length = 100mm
    param height : Length = 50mm
    let area = width * height
}
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Configurable")
        .expect("should have trait Configurable");

    for default in &trait_def.defaults {
        match &default.kind {
            reify_compiler::DefaultKind::Param { .. } => {
                assert!(
                    default.name.is_some(),
                    "DefaultKind::Param should always have Some(name), got None"
                );
            }
            reify_compiler::DefaultKind::Let { .. } => {
                assert!(
                    default.name.is_some(),
                    "DefaultKind::Let should always have Some(name), got None"
                );
            }
            reify_compiler::DefaultKind::Constraint(_) => {
                // Constraints may or may not have names — not checked here
            }
            reify_compiler::DefaultKind::Fn(_) => {
                // task 3939 δ: assoc-fn defaults carry the fn name (Some).
                assert!(
                    default.name.is_some(),
                    "DefaultKind::Fn should always have Some(name), got None"
                );
            }
            reify_compiler::DefaultKind::AssocType(_) => {
                // task 3972 ιβ: assoc-type defaults carry the type name (Some).
                // Not currently produced by the Configurable trait in this test,
                // so this arm is a compile-only coverage guard.
            }
        }
    }
}

// ── H3: geometry call diagnostics ──────────────────────────────────────

#[test]
fn box_wrong_arg_count_produces_preexisting_diagnostic() {
    // box() expects 3 arguments — passing only 2 should produce a diagnostic
    let source = r#"
        structure S {
            let shape = box(10mm, 20mm)
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_arg_count_error = errors
        .iter()
        .any(|d| d.message.contains("expects 3 arguments"));
    assert!(
        has_arg_count_error,
        "expected diagnostic about argument count, got: {:?}",
        errors
    );
}

// ── task-823 step-5: conformance.rs no-type-annotation diagnostics ──────────

#[test]
fn trait_member_no_type_annotation_emits_diagnostic() {
    // A structure implementing a trait where one of the structure's params
    // has no type annotation should produce a diagnostic (conformance.rs:46
    // outer unwrap_or). Currently defaults silently to Type::Real.
    let source = r#"
        trait T {
            param x : Real
        }
        structure S : T {
            param x = 5.0
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_type_annotation_error = errors.iter().any(|d| {
        d.message.contains("no type annotation")
            || d.message.contains("missing type annotation")
            || d.message.contains("cannot infer type")
    });
    assert!(
        has_type_annotation_error,
        "expected diagnostic about missing type annotation for conformance, got: {:?}",
        errors
    );
}

#[test]
fn trait_let_no_type_annotation_compiles_clean() {
    // A structure implementing a trait may have let bindings without explicit
    // type annotations — the type is inferred from the expression. This must
    // NOT produce an error (conformance.rs:73 path). Previously this silently
    // defaulted to Type::Real; the correct behavior is to simply omit the
    // member from the structure_members map (its type is expression-inferred).
    let source = r#"
        trait T {
            param x : Real
        }
        structure S : T {
            param x : Real = 5.0
            let y = x * 2.0
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors.is_empty(),
        "let binding without type annotation is valid code and must not produce errors, got: {:?}",
        errors
    );
}

// ── task-823 step-3: entity.rs ICE paths — green path (no ICE on valid code) ──

#[test]
fn structure_param_resolves_without_ice() {
    // Verifies that entity.rs:525 (scope.resolve in pass 2 for structure param)
    // does NOT emit an ICE diagnostic for valid code. The two-pass compilation
    // registers all names in pass 1, so pass-2 resolve should always succeed
    // for well-formed structures.
    let source = r#"
        structure S {
            param x : Real = 1.0
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_ice = errors
        .iter()
        .any(|d| d.message.contains("internal compiler error"));
    assert!(
        !has_ice,
        "expected no ICE diagnostic on valid structure param, got: {:?}",
        errors
    );
    assert!(
        errors.is_empty(),
        "expected no errors at all, got: {:?}",
        errors
    );
}

#[test]
fn port_member_resolves_without_ice() {
    // Verifies that entity.rs:856 (scope.resolve in pass 2 for port member param)
    // does NOT emit an ICE diagnostic for valid code.
    let source = r#"
        trait MechPort {
            param diameter : Length
        }
        structure S {
            port mount : MechPort {
                param diameter : Length = 5mm
            }
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_ice = errors
        .iter()
        .any(|d| d.message.contains("internal compiler error"));
    assert!(
        !has_ice,
        "expected no ICE diagnostic on valid port member, got: {:?}",
        errors
    );
    assert!(
        errors.is_empty(),
        "expected no errors at all, got: {:?}",
        errors
    );
}

// ── task-823 step-7: guards.rs ICE path — green path (no ICE on valid code) ──

#[test]
fn guarded_param_resolves_without_ice() {
    // Verifies that guards.rs:272 (scope.resolve in compile_guarded_members for
    // guarded structure param) does NOT emit an ICE diagnostic for valid code.
    // Pass 1 registers all guarded member names via register_guarded_names, so
    // pass 2 resolve should always succeed for well-formed guarded structures.
    let source = r#"
        structure S {
            param mode : Bool = true
            where mode {
                param x : Real = 1.0
            }
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_ice = errors
        .iter()
        .any(|d| d.message.contains("internal compiler error"));
    assert!(
        !has_ice,
        "expected no ICE diagnostic on valid guarded param, got: {:?}",
        errors
    );
    assert!(
        errors.is_empty(),
        "expected no errors at all, got: {:?}",
        errors
    );
}

// ── task-823 step-1: port param unknown type name emits diagnostic ──────

#[test]
fn port_param_unknown_type_name_emits_error() {
    // A port param whose type name doesn't exist (Nonexistent) should produce
    // an error diagnostic. Previously, entity.rs:366 silently defaulted to
    // Type::Real via unwrap_or without any diagnostic.
    let source = r#"
        trait MechPort {
            param diameter : Length
        }
        structure S {
            port mount : MechPort {
                param diameter : Nonexistent
            }
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_type_error = errors
        .iter()
        .any(|d| d.message.contains("Nonexistent") || d.message.contains("unresolved type name"));
    assert!(
        has_type_error,
        "expected diagnostic about unknown type 'Nonexistent' in port param, got: {:?}",
        errors
    );
}

// ── task-823 step-9: empty collection literal type inference warnings ────

#[test]
fn empty_list_literal_emits_type_inference_warning() {
    // An empty list literal `[]` has no elements to infer the element type from.
    // expr.rs:895 silently defaulted to Type::Real. It should now emit a warning
    // diagnostic informing the user that element type is defaulting to Real.
    let source = r#"
        structure S {
            let x = []
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "empty list should not produce errors, got: {:?}",
        errors
    );

    let warnings = warnings_only(&module);
    let has_type_warning = warnings
        .iter()
        .any(|d| d.message.contains("empty list") || d.message.contains("cannot infer"));
    assert!(
        has_type_warning,
        "expected warning about empty list type inference, got: {:?}",
        warnings
    );
}

#[test]
fn empty_set_literal_emits_type_inference_warning() {
    // An empty set literal `set{}` has no elements to infer from.
    // expr.rs:917 silently defaulted to Type::Real. It should now emit a warning.
    let source = r#"
        structure S {
            let x = set{}
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "empty set should not produce errors, got: {:?}",
        errors
    );

    let warnings = warnings_only(&module);
    let has_type_warning = warnings
        .iter()
        .any(|d| d.message.contains("empty set") || d.message.contains("cannot infer"));
    assert!(
        has_type_warning,
        "expected warning about empty set type inference, got: {:?}",
        warnings
    );
}

#[test]
fn empty_map_literal_emits_type_inference_warning() {
    // An empty map literal `map{}` has no entries to infer key/value types from.
    // expr.rs:949 and 953 silently defaulted to Type::String/Type::Real. Should warn.
    let source = r#"
        structure S {
            let x = map{}
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "empty map should not produce errors, got: {:?}",
        errors
    );

    let warnings = warnings_only(&module);
    let has_type_warning = warnings
        .iter()
        .any(|d| d.message.contains("empty map") || d.message.contains("cannot infer"));
    assert!(
        has_type_warning,
        "expected warning about empty map type inference, got: {:?}",
        warnings
    );
}

// ── task-823 step-11: range/stdlib-fn-zero-arg/match-no-arms diagnostics ───

/// Range with valid bounds (green-path ICE documentation).
///
/// expr.rs:369 has `.unwrap_or(Type::Real)` for the case where both
/// `compiled_lower` and `compiled_upper` are `None`.  The parser
/// (`lower_range_expr`) requires **both** lower and upper nodes via `?`, so
/// `ExprKind::Range { lower: None, upper: None, .. }` is unreachable from user
/// code — it is a pure ICE path.  This test confirms a valid range compiles
/// without triggering the fallback or emitting any ICE diagnostic.
#[test]
fn range_valid_compiles_without_ice() {
    let source = r#"
        structure S {
            let x = 1.0..10.0
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    let has_ice = errors
        .iter()
        .any(|d| d.message.contains("internal compiler error"));
    assert!(
        !has_ice,
        "expected no ICE diagnostic on valid range, got: {:?}",
        errors
    );
    assert!(
        errors.is_empty(),
        "expected no errors on valid range, got: {:?}",
        errors
    );
}

/// Zero-arg stdlib function call emits a type-inference warning.
///
/// expr.rs:586 calls `unwrap_or_else` and emits
/// `Diagnostic::warning("cannot infer return type of zero-arg function…")`
/// when `compiled_args` is empty.  This test verifies that warning is
/// present so the silent-default is caught at compile time.
///
/// `__test_zero_arg_fn` is an intentionally-synthetic name chosen so that
/// future stdlib additions (e.g. promoting `pi` to a math constant) cannot
/// accidentally turn this into a user-fn-lookup or constant-folding test.
#[test]
fn stdlib_fn_no_args_emits_type_inference_warning() {
    let source = r#"
        structure S {
            let x = __test_zero_arg_fn()
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "zero-arg stdlib call should not produce errors, got: {:?}",
        errors
    );

    let warnings = warnings_only(&module);
    let has_type_warning = warnings.iter().any(|d| {
        d.message.contains("zero-arg function")
            && d.message.contains("defaulting to Real")
            && d.message.contains("__test_zero_arg_fn")
    });
    assert!(
        has_type_warning,
        "expected warning about type inference for zero-arg stdlib call, got: {:?}",
        warnings
    );
}

// ── task-1666 step-2: negative case — single-arg call must NOT emit zero-arg warning ──

/// Calling a stdlib-style function with one argument must NOT produce a
/// "zero-arg function" type-inference warning.
///
/// The NoUserFunctions overload branch infers the return type from the first
/// compiled arg when present, so the `unwrap_or_else` (which emits the warning)
/// is never reached. This test prevents regressions where the warning is emitted
/// unconditionally.
#[test]
fn stdlib_fn_single_arg_no_zero_arg_warning() {
    let source = r#"
        structure S {
            let x = sqrt(1.0)
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "single-arg stdlib call should not produce errors, got: {:?}",
        errors
    );

    let warnings = warnings_only(&module);
    assert!(
        warnings
            .iter()
            .all(|d| !d.message.contains("zero-arg function")),
        "single-arg call should not emit a zero-arg warning, got: {:?}",
        warnings
    );
}

// ── task-823 step-13: expr.rs:1511 sub-member type ICE — green path ─────────

/// Sub-component qualified member access compiles without ICE.
///
/// expr.rs resolves the member type from `scope.sub_member_types` (populated for
/// ALL subs by entity.rs pass 1).  In valid code the lookup succeeds and the
/// ICE diagnostic is never emitted.
///
/// The syntax `parts.(MechTrait::diameter)` is `InstanceQualifiedAccess` —
/// accessing collection sub-component `parts` (type `List<Inner>`) `diameter`
/// member through trait `MechTrait`.
#[test]
fn sub_member_type_resolves_without_ice() {
    let source = r#"
        trait MechTrait {
            param diameter : Length
        }
        structure Inner : MechTrait {
            param diameter : Length = 5mm
        }
        structure Outer {
            sub parts : List<Inner>
            let d = parts.(MechTrait::diameter)
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_ice = errors
        .iter()
        .any(|d| d.message.contains("internal compiler error"));
    assert!(
        !has_ice,
        "expected no ICE diagnostic on valid sub-member access, got: {:?}",
        errors
    );
    assert!(
        errors.is_empty(),
        "expected no errors on valid sub-member access, got: {:?}",
        errors
    );
}

// ── task-1442: non-collection sub-member access — green path ───────────────

/// Sub-member qualified access on a **non-collection** sub compiles without ICE.
///
/// Regression guard for expr.rs `InstanceQualifiedAccess` — the type lookup
/// uses `scope.sub_member_types`, which is populated for ALL sub-components
/// (collection and non-collection alike).  Accessing `part.(Trait::member)` on
/// a singular (non-collection) sub resolves correctly without hitting the ICE
/// diagnostic fallback.
///
/// This test
/// exercises the non-collection form (`sub part = Inner()`) to lock the fix
/// in place.  The collection form is covered by `sub_member_type_resolves_without_ice`.
#[test]
fn non_collection_sub_member_type_resolves_without_ice() {
    let source = r#"
        trait MechTrait {
            param diameter : Length
        }
        structure Inner : MechTrait {
            param diameter : Length = 5mm
        }
        structure Outer {
            sub part = Inner()
            let d = part.(MechTrait::diameter)
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_ice = errors
        .iter()
        .any(|d| d.message.contains("internal compiler error"));
    assert!(
        !has_ice,
        "expected no ICE diagnostic on non-collection sub-member access, got: {:?}",
        errors
    );
    assert!(
        errors.is_empty(),
        "expected no errors on non-collection sub-member access, got: {:?}",
        errors
    );
}

/// Match with no arms (ICE-path documentation / parse-guard).
///
/// expr.rs:1046 has `.unwrap_or(Type::Real)` on `compiled_arms.first()`.
/// If the grammar allows `match x {}` (no arms), that path is reachable and
/// should emit an ICE diagnostic.  If the grammar rejects it (parse error),
/// the code at expr.rs:1046 is an unreachable ICE path.
///
/// This test first checks parsability.  If the source parses without errors,
/// it asserts that some diagnostic is emitted so the user is informed.  If
/// the source produces parse errors, the ICE path is documented as unreachable.
#[test]
fn match_no_arms_emits_diagnostic() {
    let source = r#"
        structure S {
            let disc = 1
            let x = match disc { }
        }
    "#;
    let parsed = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("silent_defaults_test"),
    );
    if !parsed.errors.is_empty() {
        // Grammar does not allow empty match — expr.rs:1046 is an ICE path
        // that is unreachable from user code.  Document and pass.
        return;
    }
    // Grammar allows empty match: verify the compiler emits some diagnostic.
    let module = reify_compiler::compile(&parsed);
    assert!(
        !module.diagnostics.is_empty(),
        "expected a diagnostic for match with no arms, got none"
    );
}

// ── task-2066 step-1: index into non-collection emits diagnostic ─────────

/// Index access on a non-collection type should emit a diagnostic.
///
/// `x` has type `Int` (whole-number literal `5`), so `x[0]` hits the
/// currently-silent `_ => Type::Real` fallback at expr.rs:1323-1328.
/// Before the fix (task-2066), no diagnostic is emitted — the fallback
/// silently returns `Type::Real` for `y`.
///
/// Regression guard: guards expr.rs:1323-1328 against silent fallback (task-2066).
#[test]
fn index_into_non_collection_emits_diagnostic() {
    let source = r#"
        structure S {
            let x = 5
            let y = x[0]
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // Tighten to a single distinctive substring that appears only in the new diagnostic
    // (reviewer suggestion: drop the "non-collection" alternative which also appears in the
    // quantifier diagnostic, making the two tests non-distinct).
    let has_index_error = errors.iter().any(|d| d.message.contains("cannot index"));
    assert!(
        has_index_error,
        "expected diagnostic about indexing non-collection type, got: {:?}",
        errors
    );
}

// ── task-2066 step-3: quantifier over non-collection emits diagnostic ─────

/// Quantifier over a non-collection type should emit a diagnostic.
///
/// `x` has type `Int` (whole-number literal `5`), so `forall i in x : i > 0`
/// hits the currently-silent `_ => Type::Real` fallback at expr.rs:1635-1642
/// and silently infers `elem_type = Type::Real` with no diagnostic.
/// Before the fix (task-2066), no diagnostic is emitted.
///
/// Regression guard: guards expr.rs:1635-1642 against silent fallback (task-2066).
#[test]
fn quantifier_over_non_collection_emits_diagnostic() {
    let source = r#"
        structure S {
            let x = 5
            constraint forall i in x : i > 0
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // Tighten to a single distinctive substring (reviewer suggestion: "forall"/"exists"
    // alternatives are too loose — any unrelated diagnostic mentioning those keywords
    // would satisfy the assertion while the silent-fallback regression reappeared).
    let has_iter_error = errors.iter().any(|d| d.message.contains("cannot iterate"));
    assert!(
        has_iter_error,
        "expected diagnostic about iterating over non-collection type, got: {:?}",
        errors
    );
}

// ── task-2066 amend: exists over non-collection emits diagnostic ──────────

/// `exists` quantifier over a non-collection type should emit a diagnostic.
///
/// Shares the same compiler arm as `forall` (expr.rs:1652-1671); this twin test
/// ensures the `exists` code path is also covered as a regression guard.
///
/// Regression guard: guards expr.rs:1635-1642 against silent fallback (task-2066).
#[test]
fn exists_over_non_collection_emits_diagnostic() {
    let source = r#"
        structure S {
            let x = 5
            constraint exists i in x : i > 0
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_iter_error = errors.iter().any(|d| d.message.contains("cannot iterate"));
    assert!(
        has_iter_error,
        "expected diagnostic about iterating over non-collection type (exists), got: {:?}",
        errors
    );
}

// ── task-3252: integer-form overflow literal precision-loss warnings ──────────

#[test]
fn integer_form_overflow_literal_emits_precision_loss_warning() {
    // A bare integer literal too large to represent as i64 (e.g. 20-digit integer)
    // is parsed as f64 (→ Infinity) with is_real=false, classified as LossyReal.
    // The compiler must emit a non-fatal warning about precision loss.
    // Covers the expr.rs (compile_expr_guarded) call site.
    let source = r#"
        structure S {
            let x = 99999999999999999999
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "integer overflow literal should not produce errors, got: {:?}",
        errors
    );

    let warnings = warnings_only(&module);
    let precision_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.message.contains("integer literal") && d.message.contains("precision"))
        .collect();
    assert_eq!(
        precision_warnings.len(),
        1,
        "expected exactly 1 precision-loss warning, got: {:?}",
        precision_warnings
    );

    // Assert the label's span covers the offending literal token.
    let literal_offset = source.find("99999999999999999999").unwrap() as u32;
    let label = precision_warnings[0]
        .labels
        .first()
        .expect("precision-loss warning should carry a label");
    assert!(
        label.span.start <= literal_offset && label.span.end > literal_offset,
        "expected label span {:?} to cover the literal at byte offset {}",
        label.span,
        literal_offset
    );
}

#[test]
fn integer_form_overflow_in_annotation_arg_emits_precision_loss_warning() {
    // A too-large integer used as an annotation argument is also classified as
    // LossyReal (via the annotations.rs call site). The compiler must emit the
    // same non-fatal precision-loss warning.
    // Covers the annotations.rs (lower_annotations) call site.
    // @shell accepts a numeric thickness arg (Int or Real), so LossyReal→Real
    // passes the shape check; the precision-loss warning is the only signal.
    let source = r#"
        @shell(99999999999999999999)
        structure S {
            param x : Real
        }
    "#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "integer overflow annotation arg should not produce errors, got: {:?}",
        errors
    );

    let warnings = warnings_only(&module);
    let precision_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.message.contains("integer literal") && d.message.contains("precision"))
        .collect();
    assert_eq!(
        precision_warnings.len(),
        1,
        "expected exactly 1 precision-loss warning in annotation arg, got: {:?}",
        precision_warnings
    );

    // Assert the label's span covers the offending literal token.
    let literal_offset = source.find("99999999999999999999").unwrap() as u32;
    let label = precision_warnings[0]
        .labels
        .first()
        .expect("precision-loss warning should carry a label");
    assert!(
        label.span.start <= literal_offset && label.span.end > literal_offset,
        "expected label span {:?} to cover the literal at byte offset {}",
        label.span,
        literal_offset
    );
}
