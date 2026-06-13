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
use std::sync::{Arc, Mutex};

use reify_compiler::auto_type_param::{
    AutoTypeParam, resolve_auto_type_params_with_backtracking,
};
use reify_compiler::{CompiledModule, CompiledTrait, TopologyTemplate};
use reify_core::{Diagnostic, DiagnosticCode, Severity, SourceSpan, Type, ValueCellId};
use reify_ir::{
    CompiledExpr, CompiledFunction, ConstraintChecker, ConstraintDiagnostics, ConstraintInput,
    ConstraintResult, Satisfaction, Value, ValueMap,
};
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
        let queue: Vec<Satisfaction> = std::iter::repeat_n(Satisfaction::Indeterminate, param_count)
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
        // PRD §6.2 step 4: Error is emitted INSTEAD of the Warning.
        // A regression that emitted both would still pass the above assertion.
        let depth_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamDepthBoundExceeded))
            .collect();
        assert!(
            depth_warnings.is_empty(),
            "param_count={param_count}: expected zero AutoTypeParamDepthBoundExceeded \
             Warnings on the infeasible path (Error replaces Warning); got:\n{:#?}",
            depth_warnings,
        );
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
    // PRD §6.2 step 4: Error is emitted INSTEAD of the Warning.
    let cap_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamCrossProductSizeExceeded))
        .collect();
    assert!(
        cap_warnings.is_empty(),
        "expected zero AutoTypeParamCrossProductSizeExceeded Warnings on the \
         infeasible path (Error replaces Warning); got:\n{:#?}",
        cap_warnings,
    );
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

// ─── Step-5: Hoist-reversion wiring tests (RED before step-6) ────────────────
//
// These tests pin the PRD §11 "hoist reversion" — the three
// NOTE(substitution-pass-trigger) sites in auto_type_param.rs must be reverted
// so that per-candidate and per-leaf `checker.check()` calls receive a
// ValueMap seeded from the candidate template's literal defaults (via
// `seed_candidate_value_map`) instead of an empty map.
//
// **RED before step-6**: `filter_feasible_candidates` passes `empty_values` and
// `dfs_search` passes `leaf_values = ValueMap::new()` to every `check()` call.
// **GREEN after step-6**: `seed_candidate_value_map(candidate_template, param_member)`
// is wired inside the per-candidate loop and the DFS leaf, and the spy captures
// non-empty ValueMaps.

/// Spy constraint checker that records a clone of `input.values` on every
/// `check()` call. Returns `Satisfaction::Indeterminate` for every constraint
/// so all candidates pass Phase B and the DFS resolves normally.
struct ValueMapSpyChecker {
    captured: Arc<Mutex<Vec<ValueMap>>>,
}

impl ValueMapSpyChecker {
    /// Create a new spy; also returns the shared capture handle so the test can
    /// read captured snapshots after the resolver returns.
    fn new() -> (Self, Arc<Mutex<Vec<ValueMap>>>) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        (Self { captured: Arc::clone(&captured) }, captured)
    }
}

impl ConstraintChecker for ValueMapSpyChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        let snapshot = input.values.clone();
        self.captured
            .lock()
            .expect("ValueMapSpyChecker: mutex poisoned")
            .push(snapshot);
        // Always Indeterminate so the spy never rejects a candidate.
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: Satisfaction::Indeterminate,
                diagnostics: ConstraintDiagnostics::default(),
            })
            .collect()
    }
}

/// Single-param Phase B path: after the hoist reversion in step-6,
/// `filter_feasible_candidates` calls `checker.check()` with a ValueMap seeded
/// from the candidate template's literal defaults (`seed_candidate_value_map`),
/// NOT with an empty ValueMap as today.
///
/// Setup:
/// - Trait `T1` + structure `S1 : T1 { param x : Real = 1.0 }`.
/// - Parameterized template with one `Type::TypeParam("T1")` value cell named
///   `"p1"` (→ `param_type_member("T1") == "p1"` via the param→member helper).
/// - One trivial boolean constraint so `check()` is invoked with a non-empty
///   constraints slice.
/// - Single `AutoTypeParam` for `T1`; 1 param ≤ 6 max_depth → single-param DFS
///   path → calls `filter_feasible_candidates` once for `S1`.
///
/// **RED before step-6**: `filter_feasible_candidates` passes `empty_values`
/// — the captured ValueMap is empty, so `vm.get(&ValueCellId::new("p1", "x"))`
/// returns `None` and the assertion fails.
///
/// **GREEN after step-6**: the seeded map contains `ValueCellId::new("p1","x")`
/// → `Value::Real(1.0)` (S1's literal default).
#[test]
fn single_param_phase_b_spy_captures_seeded_value_map_after_hoist_reversion() {
    let source = r#"
trait T1 {}
structure def S1 : T1 { param x : Real = 1.0 }
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);
    let functions: &[CompiledFunction] = &[];

    // Parameterized template: one value cell "p1 : TypeParam(T1)" + one
    // trivial constraint so check() is always invoked.
    let parameterized_template = TopologyTemplateBuilder::new("Stack")
        .param(
            "Stack",
            "p1",
            Type::TypeParam("T1".to_string()),
            None,
        )
        .constraint(
            "Stack",
            0,
            Some("trivial"),
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
        )
        .build();

    let (spy, captured) = ValueMapSpyChecker::new();

    let params = vec![AutoTypeParam {
        name: "T1".to_string(),
        bounds: vec!["T1".to_string()],
        free: false,
        use_site_span: SourceSpan::new(10, 15),
    }];

    let mut diagnostics = Vec::new();
    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &parameterized_template,
        &spy,
        functions,
        6,          // max_depth; 1 param ≤ 6 → single-param DFS path
        usize::MAX, // cross-product cap: not relevant here
        &mut diagnostics,
    );

    // Sanity: S1 was selected (no errors on the resolution path).
    assert!(
        diagnostics.iter().all(|d| d.severity != Severity::Error),
        "unexpected error diagnostics: {:#?}",
        diagnostics
    );
    assert_eq!(
        outcome.substitution.len(),
        1,
        "expected single-param substitution (S1); got: {:?}",
        outcome.substitution
    );

    let captured = captured.lock().expect("spy mutex poisoned");
    assert!(
        !captured.is_empty(),
        "expected at least one check() call to have been recorded by the spy"
    );

    // After step-6: the per-candidate ValueMap passed to check() must contain
    // `ValueCellId::new("p1", "x")` — seeded from S1's `param x : Real = 1.0`
    // via `seed_candidate_value_map(S1_template, "p1")`.
    //
    // Before step-6: every captured ValueMap is empty (uses `empty_values`),
    // so the assertion below fails → RED.
    let key = ValueCellId::new("p1", "x");
    assert!(
        captured.iter().any(|vm| vm.get(&key).is_some()),
        "expected at least one check() call to receive a ValueMap containing \
         the seeded key {key:?} (from S1's literal default x=1.0); \
         actual captured maps: {:?}",
        captured.iter().map(|m| m.iter().collect::<Vec<_>>()).collect::<Vec<_>>()
    );
}

/// Multi-param DFS-leaf path: after the hoist reversion in step-6, the
/// DFS leaf's `check_constraints_leaf` call receives a ValueMap seeded with
/// ALL selected candidates' literal defaults, NOT the shared empty `leaf_values`.
///
/// Setup:
/// - Traits `T1`, `T2` + structures `S1 : T1 { param x : Real = 1.0 }`,
///   `S2 : T2 { param y : Real = 2.0 }`.
/// - Parameterized template with TWO `TypeParam` cells: `"p1 : T1"` and
///   `"p2 : T2"`, plus one trivial constraint.
/// - Two `AutoTypeParam`s for `T1` and `T2`; 2 params ≤ 6 max_depth and
///   1×1=1 ≤ cap → multi-param DFS path.
/// - The DFS finds one feasible leaf `(S1, S2)` and calls `check_constraints_leaf`
///   once for it.
///
/// **RED before step-6**: DFS leaf uses the pre-hoisted `leaf_values =
/// ValueMap::new()` (empty); the spy captures no seeded entries.
///
/// **GREEN after step-6**: the leaf ValueMap is seeded with both candidates'
/// literal defaults → `"p1.x"` and `"p2.y"` are present.
#[test]
fn multi_param_dfs_leaf_spy_captures_seeded_value_map_after_hoist_reversion() {
    let source = r#"
trait T1 {}
trait T2 {}
structure def S1 : T1 { param x : Real = 1.0 }
structure def S2 : T2 { param y : Real = 2.0 }
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);
    let functions: &[CompiledFunction] = &[];

    // Parameterized template with two TypeParam cells and one trivial constraint.
    let parameterized_template = TopologyTemplateBuilder::new("Assembly")
        .param(
            "Assembly",
            "p1",
            Type::TypeParam("T1".to_string()),
            None,
        )
        .param(
            "Assembly",
            "p2",
            Type::TypeParam("T2".to_string()),
            None,
        )
        .constraint(
            "Assembly",
            0,
            Some("trivial"),
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
        )
        .build();

    let (spy, captured) = ValueMapSpyChecker::new();

    let params = vec![
        AutoTypeParam {
            name: "T1".to_string(),
            bounds: vec!["T1".to_string()],
            free: false,
            use_site_span: SourceSpan::new(10, 15),
        },
        AutoTypeParam {
            name: "T2".to_string(),
            bounds: vec!["T2".to_string()],
            free: false,
            use_site_span: SourceSpan::new(20, 25),
        },
    ];

    let mut diagnostics = Vec::new();
    let outcome = resolve_auto_type_params_with_backtracking(
        &params,
        &template_registry,
        &trait_registry,
        &parameterized_template,
        &spy,
        functions,
        6,          // max_depth; 2 params ≤ 6 → multi-param DFS path
        usize::MAX, // cross-product cap
        &mut diagnostics,
    );

    // Sanity: both params selected, no errors.
    assert!(
        diagnostics.iter().all(|d| d.severity != Severity::Error),
        "unexpected error diagnostics: {:#?}",
        diagnostics
    );
    assert_eq!(
        outcome.substitution.len(),
        2,
        "expected two-param substitution (S1, S2); got: {:?}",
        outcome.substitution
    );

    let captured = captured.lock().expect("spy mutex poisoned");
    assert!(
        !captured.is_empty(),
        "expected at least one check() call to have been recorded by the spy"
    );

    // After step-6: at least one check() call (the DFS leaf) must have
    // received a ValueMap seeded with BOTH candidates' literal defaults.
    // Key "p1.x" comes from `seed_candidate_value_map(S1, "p1")`,
    // key "p2.y" from `seed_candidate_value_map(S2, "p2")`.
    //
    // Before step-6: `leaf_values` is always empty → both assertions below
    // fail → RED.
    let key_p1_x = ValueCellId::new("p1", "x");
    let key_p2_y = ValueCellId::new("p2", "y");
    assert!(
        captured
            .iter()
            .any(|vm| vm.get(&key_p1_x).is_some() && vm.get(&key_p2_y).is_some()),
        "expected at least one check() call to receive a ValueMap seeded with \
         both {key_p1_x:?} (from S1) and {key_p2_y:?} (from S2); \
         actual captured maps: {:?}",
        captured.iter().map(|m| m.iter().collect::<Vec<_>>()).collect::<Vec<_>>()
    );
}

/// Stub-path no-op guard (PRD §11.2): all-Indeterminate checker → outcomes
/// are unchanged from before the hoist reversion.  Verified GREEN both before
/// and after step-6 to pin "stub-path callers unchanged".
///
/// Uses `MockConstraintChecker::with_default(Indeterminate)` (functionally
/// identical to `CompileTimeIndeterminateChecker`).
///
/// Single-param: expects `Selected("S1")`, no diagnostics.
/// Multi-param: expects `Selected("S1"), Selected("S2")`, no diagnostics.
#[test]
fn stub_path_no_op_single_and_multi_param_outcomes_unchanged_by_hoist_reversion() {
    let source = r#"
trait T1 {}
trait T2 {}
structure def S1 : T1 { param x : Real = 1.0 }
structure def S2 : T2 { param y : Real = 2.0 }
"#;
    let module = parse_and_compile(source);
    let (template_registry, trait_registry) = build_registries(&module);
    let functions: &[CompiledFunction] = &[];
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Indeterminate);

    let parameterized_template = TopologyTemplateBuilder::new("Assembly")
        .param("Assembly", "p1", Type::TypeParam("T1".to_string()), None)
        .param("Assembly", "p2", Type::TypeParam("T2".to_string()), None)
        .constraint(
            "Assembly",
            0,
            Some("trivial"),
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
        )
        .build();

    // ── Single-param ──────────────────────────────────────────────────────────
    let single_params = vec![AutoTypeParam {
        name: "T1".to_string(),
        bounds: vec!["T1".to_string()],
        free: false,
        use_site_span: SourceSpan::new(10, 15),
    }];
    let mut single_diags = Vec::new();
    let single_outcome = resolve_auto_type_params_with_backtracking(
        &single_params,
        &template_registry,
        &trait_registry,
        &parameterized_template,
        &checker,
        functions,
        6,
        usize::MAX,
        &mut single_diags,
    );
    assert!(
        single_diags.iter().all(|d| d.severity != Severity::Error),
        "stub single-param: unexpected error diagnostics: {:#?}",
        single_diags
    );
    assert_eq!(
        single_outcome.substitution,
        vec![("T1".to_string(), "S1".to_string())],
        "stub single-param: expected S1 selected"
    );

    // ── Multi-param ───────────────────────────────────────────────────────────
    let multi_params = vec![
        AutoTypeParam {
            name: "T1".to_string(),
            bounds: vec!["T1".to_string()],
            free: false,
            use_site_span: SourceSpan::new(10, 15),
        },
        AutoTypeParam {
            name: "T2".to_string(),
            bounds: vec!["T2".to_string()],
            free: false,
            use_site_span: SourceSpan::new(20, 25),
        },
    ];
    let mut multi_diags = Vec::new();
    let multi_outcome = resolve_auto_type_params_with_backtracking(
        &multi_params,
        &template_registry,
        &trait_registry,
        &parameterized_template,
        &checker,
        functions,
        6,
        usize::MAX,
        &mut multi_diags,
    );
    assert!(
        multi_diags.iter().all(|d| d.severity != Severity::Error),
        "stub multi-param: unexpected error diagnostics: {:#?}",
        multi_diags
    );
    assert_eq!(
        multi_outcome.substitution,
        vec![
            ("T1".to_string(), "S1".to_string()),
            ("T2".to_string(), "S2".to_string()),
        ],
        "stub multi-param: expected S1, S2 selected"
    );
}
