//! BFS-fallback joint-recheck soundness tests (task 4434 γ).
//!
//! Targets `emit_fallback_warning_and_delegate_to_bfs` in
//! `crates/reify-compiler/src/auto_type_param.rs`.
//!
//! PRD: `docs/prds/v0_3/auto-type-param-resolution-completion.md` §6.2
//!
//! # What is tested
//!
//! γ restructures `emit_fallback_warning_and_delegate_to_bfs` to:
//! 1. Run BFS first → outcome.
//! 2. If BFS returned a COMPLETE assignment (`outcome.substitution.len() == params.len()`),
//!    build the full joint ValueMap and call `check_constraints_leaf` ONCE.
//! 3. If the joint check finds any `Violated` constraint → emit
//!    `AutoTypeParamBoundedInfeasible` Error + return empty substitution.
//! 4. If not Violated (or BFS incomplete) → emit the existing Warning and
//!    return the BFS outcome unchanged.
//!
//! # Test families
//!
//! * **Depth-bound infeasible** — `param_count ∈ {7, 8}` (> default max_depth=6):
//!   inject a checker that returns `Indeterminate` for the `param_count`
//!   per-param BFS calls and `Violated` for the single joint-recheck call.
//!   Assert: `outcome.substitution` is empty + exactly one
//!   `AutoTypeParamBoundedInfeasible` Error.
//!
//! * **Cross-product-cap infeasible** — 2 params × 2 candidates, cap=3 (4 > 3):
//!   same checker pattern (4 Indeterminate BFS calls + 1 Violated joint call).
//!   Assert same invariant.
//!
//! * **Feasible controls** (PRD §11.2 stub-path no-op guarantee) — same
//!   configurations but checker always `Indeterminate`.  Assert: the
//!   `DepthBoundExceeded` / `CrossProductSizeExceeded` Warning is emitted AND the
//!   full substitution is preserved.  GREEN both before and after γ — pins that γ
//!   is a no-op on the "no Violated" path (equivalent to the production
//!   `CompileTimeIndeterminateChecker`).

use std::collections::HashMap;

use reify_compiler::auto_type_param::{
    AutoTypeParam, MultiParamResolutionOutcome, SelectionResult,
    resolve_auto_type_params_with_backtracking,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_core::{Diagnostic, DiagnosticCode, Severity, SourceSpan, Type};
use reify_ir::{CompiledExpr, CompiledFunction, Satisfaction, Value};
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder, parse_and_compile};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build `(template_registry, trait_registry)` from a compiled module.
fn build_registries(
    module: &CompiledModule,
) -> (
    HashMap<String, &TopologyTemplate>,
    HashMap<String, &CompiledTrait>,
) {
    let template_registry = module.templates.iter().map(|t| (t.name.clone(), t)).collect();
    let trait_registry = module.trait_defs.iter().map(|t| (t.name.clone(), t)).collect();
    (template_registry, trait_registry)
}

/// Build a Reify source with `n` distinct traits `Ti` and one implementing
/// structure `Si : Ti` each, carrying a `param x : Real = i.0`.
fn build_n_trait_one_candidate_source(n: usize) -> String {
    let mut src = String::new();
    for i in 1..=n {
        src.push_str(&format!("trait T{i} {{}}\n"));
    }
    for i in 1..=n {
        src.push_str(&format!(
            "structure def S{i} : T{i} {{ param x : Real = {i}.0 }}\n"
        ));
    }
    src
}

/// Build a parameterized template that carries one boolean-literal constraint.
/// The constraint expression is `true` (a no-op semantically); what matters is
/// that `build_constraints_template(template).len() == 1` so the mock checker
/// receives a non-empty slice and can bind its queued `Satisfaction` verdict to
/// the constraint, producing a non-empty `Vec<ConstraintResult>`.  Without at
/// least one constraint, every `checker.check()` call returns an empty
/// `Vec` regardless of queue content — `violated.is_empty()` is vacuously true
/// and the joint recheck can never report a violation.
fn build_parameterized_template_with_one_constraint(name: &str) -> TopologyTemplate {
    let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    TopologyTemplateBuilder::new(name)
        .constraint(name, 0, Some("joint_check"), expr)
        .build()
}

/// Assert exactly one `AutoTypeParamBoundedInfeasible` Error is present.
fn assert_exactly_one_bounded_infeasible_error(diagnostics: &[Diagnostic]) {
    let matching: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::AutoTypeParamBoundedInfeasible)
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one AutoTypeParamBoundedInfeasible Error; \
         got diagnostics:\n{:#?}",
        diagnostics
    );
}

// ─── Depth-bound infeasible: generated family (7..=8 params, 1 candidate each) ──

/// For each `param_count` in `{7, 8}` (> max_depth=6 → depth-bound BFS fallback),
/// inject a checker that returns `Indeterminate` for the `param_count` per-param
/// BFS `check()` calls and `Violated` for the single joint-recheck `check()` call.
///
/// Assert for each count:
///   (a) `outcome.substitution` is **empty** (no substitution on the
///       jointly-infeasible path — PRD §6.2 step 4).
///   (b) Exactly one `AutoTypeParamBoundedInfeasible` Error diagnostic.
///
/// **RED**: before γ's joint-recheck is wired, `emit_fallback_warning_and_delegate_to_bfs`
/// pushes the `DepthBoundExceeded` Warning and returns the full BFS substitution —
/// both `(a)` and `(b)` fail.
///
/// **GREEN** after step-4 (γ impl): BFS runs first → complete assignment → joint
/// recheck gets `Violated` → Error + empty substitution.
#[test]
fn depth_bound_infeasible_joint_check_emits_bounded_infeasible_error_generated_family() {
    let functions: &[CompiledFunction] = &[];

    for param_count in [7usize, 8] {
        let source = build_n_trait_one_candidate_source(param_count);
        let module = parse_and_compile(&source);
        let (template_registry, trait_registry) = build_registries(&module);

        let parameterized_template =
            build_parameterized_template_with_one_constraint("Stack");

        // Queue layout:
        //   - `param_count` × Indeterminate: the per-param BFS calls in
        //     `filter_feasible_candidates` (1 candidate per param → 1 call per param).
        //   - 1 × Violated: the single joint-recheck call in γ's
        //     `emit_fallback_warning_and_delegate_to_bfs`.
        let queue: Vec<Satisfaction> = std::iter::repeat(Satisfaction::Indeterminate)
            .take(param_count)
            .chain(std::iter::once(Satisfaction::Violated))
            .collect();
        let checker = MockConstraintChecker::new().with_call_queue(queue);

        let params: Vec<AutoTypeParam> = (1..=param_count)
            .map(|i| AutoTypeParam {
                name: format!("T{i}"),
                bounds: vec![format!("T{i}")],
                free: false,
                use_site_span: SourceSpan::new(i as u32 * 10, i as u32 * 10 + 5),
            })
            .collect();

        let mut diagnostics = Vec::new();
        let outcome = resolve_auto_type_params_with_backtracking(
            &params,
            &template_registry,
            &trait_registry,
            &parameterized_template,
            &checker,
            functions,
            6,          // max_depth=6; param_count ∈ {7,8} > 6 → depth-bound fires
            usize::MAX, // cross-product cap not relevant
            &mut diagnostics,
        );

        assert!(
            outcome.substitution.is_empty(),
            "param_count={param_count}: expected empty substitution on jointly-infeasible \
             depth-bound BFS-fallback path; got substitution:\n{:?}",
            outcome.substitution,
        );
        assert_exactly_one_bounded_infeasible_error(&diagnostics);
    }
}

// ─── Cross-product cap infeasible (2 params, 2 candidates each, cap=3) ───────

/// Two `free=true` params, each with 2 candidates; `max_cross_product_size=3`
/// (2×2=4 > 3 → cross-product-cap BFS fallback fires).
///
/// BFS in free mode picks the lex-first feasible candidate for each param
/// (4 `check()` calls: 2 candidates × 2 params).  γ then runs one joint
/// recheck (5th `check()` call) → `Violated` → Error + empty substitution.
///
/// Assert:
///   (a) `outcome.substitution` is empty.
///   (b) Exactly one `AutoTypeParamBoundedInfeasible` Error diagnostic.
///
/// **RED**: before γ, the cap fallback emits `CrossProductSizeExceeded` Warning
/// and returns the full BFS substitution.
#[test]
fn cross_product_cap_infeasible_joint_check_emits_bounded_infeasible_error() {
    let source = r#"
trait T1 {}
trait T2 {}

structure def S1A : T1 { param x : Real = 1.0 }
structure def S1B : T1 { param x : Real = 2.0 }
structure def S2A : T2 { param x : Real = 3.0 }
structure def S2B : T2 { param x : Real = 4.0 }
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    let parameterized_template =
        build_parameterized_template_with_one_constraint("Coupled");
    let functions: &[CompiledFunction] = &[];

    // BFS queue: 2 params × 2 candidates = 4 per-param Indeterminate checks.
    // 5th call = joint recheck → Violated.
    let checker = MockConstraintChecker::new().with_call_queue(vec![
        Satisfaction::Indeterminate, // BFS: P1's S1A
        Satisfaction::Indeterminate, // BFS: P1's S1B
        Satisfaction::Indeterminate, // BFS: P2's S2A
        Satisfaction::Indeterminate, // BFS: P2's S2B
        Satisfaction::Violated,      // joint recheck → jointly infeasible
    ]);

    let params = vec![
        AutoTypeParam {
            name: "P1".to_string(),
            bounds: vec!["T1".to_string()],
            free: true, // free mode: lex-first S1A selected (not Ambiguous with 2 feasibles)
            use_site_span: SourceSpan::new(10, 15),
        },
        AutoTypeParam {
            name: "P2".to_string(),
            bounds: vec!["T2".to_string()],
            free: true,
            use_site_span: SourceSpan::new(20, 25),
        },
    ];

    let mut diagnostics = Vec::new();
    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &parameterized_template,
        &checker,
        functions,
        6, // max_depth=6; 2 params ≤ 6, depth-bound not relevant
        3, // max_cross_product_size=3; 2×2=4 > 3 → cap fires
        &mut diagnostics,
    );

    assert!(
        outcome.substitution.is_empty(),
        "expected empty substitution on jointly-infeasible cross-product-cap \
         BFS-fallback path; got substitution:\n{:?}",
        outcome.substitution
    );
    assert_exactly_one_bounded_infeasible_error(&diagnostics);
}

// ─── Feasible controls (PRD §11.2: γ is a no-op on the all-Indeterminate path) ──

/// Depth-bound (7 params, 1 candidate each), checker always `Indeterminate`.
///
/// γ's joint recheck finds no `Violated` → falls through to the existing
/// `DepthBoundExceeded` Warning + full BFS substitution.
///
/// Assert:
///   (a) No Error diagnostics.
///   (b) Exactly one `AutoTypeParamDepthBoundExceeded` Warning.
///   (c) `substitution.len() == 7` (full substitution preserved).
///
/// **GREEN** both before and after γ — pins that the stub path is unchanged.
#[test]
fn depth_bound_feasible_joint_check_falls_through_to_warning_with_full_substitution() {
    let param_count = 7usize;
    let source = build_n_trait_one_candidate_source(param_count);
    let module = parse_and_compile(&source);
    let (template_registry, trait_registry) = build_registries(&module);

    let parameterized_template =
        build_parameterized_template_with_one_constraint("Stack");
    let functions: &[CompiledFunction] = &[];

    // All-Indeterminate default: no Violated on any check → γ falls through to Warning.
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Indeterminate);

    let params: Vec<AutoTypeParam> = (1..=param_count)
        .map(|i| AutoTypeParam {
            name: format!("T{i}"),
            bounds: vec![format!("T{i}")],
            free: false,
            use_site_span: SourceSpan::new(i as u32 * 10, i as u32 * 10 + 5),
        })
        .collect();

    let mut diagnostics = Vec::new();
    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &parameterized_template,
        &checker,
        functions,
        6,
        usize::MAX,
        &mut diagnostics,
    );

    // (c) full substitution
    assert_eq!(
        outcome.substitution.len(),
        param_count,
        "depth-bound feasible control: expected full substitution ({param_count} entries); \
         got: {:?}",
        outcome.substitution
    );

    // (a) no Errors
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "expected no Errors on all-Indeterminate path; got:\n{:#?}", errors);

    // (b) exactly one DepthBoundExceeded Warning
    let depth_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamDepthBoundExceeded))
        .collect();
    assert_eq!(
        depth_warnings.len(), 1,
        "expected exactly one AutoTypeParamDepthBoundExceeded Warning; got:\n{:#?}",
        diagnostics
    );
}

/// Cross-product-cap (2 params × 2 candidates, cap=3), checker always `Indeterminate`.
///
/// γ's joint recheck finds no `Violated` → falls through to the existing
/// `CrossProductSizeExceeded` Warning + full BFS substitution.
///
/// Assert:
///   (a) No Error diagnostics.
///   (b) Exactly one `AutoTypeParamCrossProductSizeExceeded` Warning.
///   (c) `substitution.len() == 2` (full substitution preserved).
///
/// **GREEN** both before and after γ.
#[test]
fn cross_product_cap_feasible_joint_check_falls_through_to_warning_with_full_substitution() {
    let source = r#"
trait T1 {}
trait T2 {}

structure def S1A : T1 { param x : Real = 1.0 }
structure def S1B : T1 { param x : Real = 2.0 }
structure def S2A : T2 { param x : Real = 3.0 }
structure def S2B : T2 { param x : Real = 4.0 }
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);

    let parameterized_template =
        build_parameterized_template_with_one_constraint("Coupled");
    let functions: &[CompiledFunction] = &[];

    // All-Indeterminate: no Violated → cap Warning path.
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Indeterminate);

    let params = vec![
        AutoTypeParam {
            name: "P1".to_string(),
            bounds: vec!["T1".to_string()],
            free: true,
            use_site_span: SourceSpan::new(10, 15),
        },
        AutoTypeParam {
            name: "P2".to_string(),
            bounds: vec!["T2".to_string()],
            free: true,
            use_site_span: SourceSpan::new(20, 25),
        },
    ];

    let mut diagnostics = Vec::new();
    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &parameterized_template,
        &checker,
        functions,
        6, // max_depth=6
        3, // max_cross_product_size=3; 4 > 3 → cap fires
        &mut diagnostics,
    );

    // (c) full substitution
    assert_eq!(
        outcome.substitution.len(),
        2,
        "cap feasible control: expected full substitution (2 entries); got: {:?}",
        outcome.substitution
    );

    // (a) no Errors
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "expected no Errors on all-Indeterminate cap path; got:\n{:#?}", errors);

    // (b) exactly one CrossProductSizeExceeded Warning
    let cap_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamCrossProductSizeExceeded))
        .collect();
    assert_eq!(
        cap_warnings.len(),
        1,
        "expected exactly one AutoTypeParamCrossProductSizeExceeded Warning; got:\n{:#?}",
        diagnostics
    );
}
