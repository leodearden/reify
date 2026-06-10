//! Anti-cascade producer audit tests for `Type::Error` sentinel policy (task-1921).
//!
//! ## Purpose and scope
//!
//! These tests verify that **producer sites** in `expr.rs` — branches that emit a
//! `Severity::Error` diagnostic for an unrecoverable type-inference failure — pair
//! the diagnostic with a `Type::Error` result (the poison-value sentinel), so that
//! consumer-side guards (`type_compat::implicitly_converts_to`, `type_compat::type_compatible`,
//! `type_compat::infer_binop_type`) can short-circuit and suppress cascading diagnostics.
//!
//! ## Distinction from `type_error_propagation_tests.rs`
//!
//! `type_error_propagation_tests.rs` (task-448) covers the **consumer side**: it verifies
//! that the `infer_binop_type` / aggregation / index-access / quantifier guards in
//! `type_compat.rs` and `expr.rs` propagate `Type::Error` when they *receive* a poisoned
//! operand.  The producer stub used in those tests is the `"member access not yet supported"`
//! branch, which already returns `Type::Error`.
//!
//! This file (task-1921, review follow-up S5 from task-1912) covers the **producer side**:
//! ≥ 30 branches that today return `Type::Real` when they emit an error diagnostic, thereby
//! bypassing the consumer guards and causing cascade diagnostics.  Each test here compiles
//! a minimal fixture, pairs a specific error producer with an enclosing `+ 5.0` (BinOp
//! consumer), and asserts:
//!   (a) `result_type == Type::Error` on the enclosing let-expression (the BinOp consumer
//!       short-circuits via `infer_binop_type`), and
//!   (b) No `"mismatch"` / `"incompatible"` cascade diagnostic is present.
//!
//! **Exception — `COLLECTION_AGGREGATION_MEMBERS` carve-out (task-3657 section):** the
//! `count`/`sum`/`keys`/`values` aggregation members on collection subs intentionally pin a
//! *concrete* fallback type (`Type::Int` for `count`; `Type::Real` for `sum`/`keys`/`values`)
//! rather than `Type::Error`.  The two tests in the task-3657 section therefore assert a
//! concrete type instead of `Type::Error` — per task-3639 design decision #2
//! ("user-knows-the-type cascade-suppression": the return type is known, so downstream
//! checks against it are legitimate, not spurious cascade).
//!
//! ## Policy reference
//!
//! See the module-header doc block in `crates/reify-compiler/src/expr.rs` for the full
//! poison-policy documentation, including intentional Category-B non-Error fallbacks.

use reify_test_support::{assert_no_type_cascade, compile_source, get_let_expr, get_let_expr_in};
use reify_core::Type;

// ── steps 1/2: baseline contract (pre-existing unknown-member producer) ──────

/// Baseline anti-cascade contract for the `unknown member` producer.
///
/// This is an independent twin of `type_error_propagation_tests::
/// stub_error_plus_arithmetic_emits_exactly_one_diagnostic`. A failure here
/// (rather than in step-3/4) isolates the root cause to the test helpers or
/// the existing `Type::Error` producer at `expr.rs:~737`.
///
/// Asserts:
///   (a) `get_let_expr(&module, "broken").result_type == Type::Error`
///       (the BinOp's `infer_binop_type` short-circuits on the Type::Error
///       left operand and returns Type::Error for the whole BinOp expression).
///   (b) At least one error whose message contains `"unknown member"`.
///   (c) No error message contains `"mismatch"` or `"incompatible"`.
#[test]
fn make_poison_literal_baseline_contract() {
    let source = r#"
structure S {
    let broken = self.unknown_field + 5.0
}
"#;
    let module = compile_source(source);

    let expr = get_let_expr(&module, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["unknown member"]);
}

// ── step-3: bare-identifier and struct-member resolution producers ────────────

/// `unresolved name` producer anti-cascade contract (Category A, line ~205).
///
/// The `+ 5.0` consumer BinOp must short-circuit to `Type::Error` once the
/// operand is `Type::Error`. On current code, `nonexistent_name` returns
/// `Type::Real`, so `Real + Real = Real` → `result_type = Real ≠ Error` → RED.
#[test]
fn unresolved_identifier_no_cascade() {
    let source = r#"
structure S {
    let broken = nonexistent_name + 5.0
}
"#;
    let module = compile_source(source);
    let expr = get_let_expr(&module, "broken");

    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["unresolved name"]);
}

/// `port has no member` producer anti-cascade contract (Category A, line ~860).
///
/// On current code, `p.nonexistent` returns `Type::Real`, so `Real + Real = Real`
/// → `result_type = Real ≠ Error` → RED.
#[test]
fn port_unknown_member_no_cascade() {
    let source = r#"
trait MechPort {
    param diameter : Length
}

structure S {
    port p : MechPort {
        param diameter : Length = 5mm
    }
    let broken = p.nonexistent + 5.0
}
"#;
    let module = compile_source(source);

    // The let-binding is on the S template; search all templates
    // since the compiler may produce both a MechPort template and S.
    let expr = get_let_expr_in(&module, "S", "broken");

    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["no member"]);
}

/// `unknown member on sub` producer anti-cascade contract (Category A, line ~837).
///
/// `self.s.nonexistent` accesses a non-collection sub member that does not exist.
/// On current code, the error branch returns `Type::Real`, so `Real + Real = Real`
/// → `result_type = Real ≠ Error` → RED.
#[test]
fn sub_unknown_member_no_cascade() {
    let source = r#"
structure Inner {
    param x : Real
}

structure Outer {
    sub s = Inner()
    let broken = self.s.nonexistent + 5.0
}
"#;
    let module_wrap = compile_source(source);
    let expr = get_let_expr_in(&module_wrap, "Outer", "broken");

    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module_wrap.diagnostics, &["unknown member"]);
}

// ── step-5: function / operator-dispatch failures ─────────────────────────────

/// `ambiguous function call` producer anti-cascade contract (Category A, line ~556).
///
/// Two overloads with identical param types but different return types cause ambiguity.
/// On current code, the `Ambiguous` arm returns `Type::Real`, so `Real + Real = Real`
/// → `result_type = Real ≠ Error` → RED.
#[test]
fn ambiguous_overload_no_cascade() {
    let source = r#"
fn f(x: Int) -> Int { x }
fn f(x: Int) -> Real { x + 0.0 }
structure S { let broken = f(3) + 5.0 }
"#;
    let module = compile_source(source);
    let expr = get_let_expr_in(&module, "S", "broken");

    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    // Two expected root-cause errors: "duplicate function signature" (from identical
    // param types) and "ambiguous function call" (from the ambiguous resolution at the
    // call site).  Both are legitimate producer errors, not cascades.
    assert_no_type_cascade(&module.diagnostics, &["ambiguous", "duplicate"]);
}

/// `no matching overload` producer anti-cascade contract (Category A, line ~577).
///
/// Calling a Real-param function with an Int arg causes "no matching overload"
/// (Int→Real widening is not used during resolution).
/// On current code, the `NoMatch` arm returns `Type::Real`, so `Real + Real = Real`
/// → `result_type = Real ≠ Error` → RED.
#[test]
fn no_match_overload_no_cascade() {
    let source = r#"
fn f(x: Real) -> Real { x }
structure S { let broken = f(3) + 5.0 }
"#;
    let module = compile_source(source);
    let expr = get_let_expr_in(&module, "S", "broken");

    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["no matching overload"]);
}

/// `some() wrong arity` producer anti-cascade contract (Category A, line ~473).
///
/// Calling `some()` with 0 arguments triggers the arity guard.
/// On current code, the wrong-arity path returns `Type::Real`, so `Real + Real = Real`
/// → `result_type = Real ≠ Error` → RED.
#[test]
fn some_wrong_arity_no_cascade() {
    let source = r#"
structure S { let broken = some() + 5.0 }
"#;
    let module = compile_source(source);

    let expr = get_let_expr(&module, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["some() expects 1 argument"]);
}

// ── step-7: enum / selector / qualified-access failures ───────────────────────

/// `Name::Variant` syntax routes through `QualifiedAccess` ("trait not found"),
/// not `EnumAccess` — anti-cascade contract for the `QualifiedAccess` "trait not
/// found" producer (Category A, line ~1671; covered also by
/// `qualified_unknown_trait_no_cascade`).
///
/// ## Parser routing
///
/// `UnknownEnum::Variant` uses `::` syntax, which tree-sitter always parses as
/// `qualified_access` (trait member access).  The `EnumAccess` AST node is only
/// produced for `Name.Variant` **dot-notation** when `Name` appears in the
/// parser's known-enum set (populated from `enum` declarations in the same source
/// text).  Since the parser and semantic-analysis stage share the same source, if
/// the parser treats `Foo` as an enum, the compiler will also find it in
/// `enum_defs` — making the `EnumAccess` "unknown enum type" branch (line ~1310)
/// architecturally unreachable from user source.  That branch exists as a safety
/// net and is exercised by the `debug_assert!` in `make_poison_literal`.
///
/// On pre-step-8 code, `QualifiedAccess` returned `Type::Real`, so
/// `Real + Real = Real` → `result_type = Real ≠ Error` → RED.
#[test]
fn enum_colon_colon_syntax_routes_to_qualified_access_no_cascade() {
    let source = r#"
structure S { let broken = UnknownEnum::Variant + 5.0 }
"#;
    let module = compile_source(source);

    let expr = get_let_expr(&module, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    // `UnknownEnum::Variant` routes through QualifiedAccess → "trait not found".
    assert_no_type_cascade(&module.diagnostics, &["not found"]);
}

/// `unknown selector kind` producer anti-cascade contract (Category A, line ~1521).
///
/// On current code, the unknown-selector arm returns `Type::Real`.
/// The test asserts `result_type == Type::Error` on the selector let-binding.
/// (No `+ 5.0` wrapper: selector expressions are geometric references and adding
/// 5.0 after `@` syntax is ambiguous to the parser; the result_type check alone
/// makes this RED on current code.)
#[test]
fn unknown_selector_kind_no_cascade() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let broken = p @ bogus("arg")
}
"#;
    let module = compile_source(source);
    let expr = get_let_expr_in(&module, "S", "broken");

    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["unknown selector"]);
}

/// `trait not found` producer anti-cascade contract (Category A QualifiedAccess, line ~1671).
///
/// On current code, the unknown-trait arm returns `Type::Real`, so `Real + Real = Real`
/// → `result_type = Real ≠ Error` → RED.
#[test]
fn qualified_unknown_trait_no_cascade() {
    let source = r#"
structure S { let broken = UnknownTrait::member + 5.0 }
"#;
    let module = compile_source(source);

    let expr = get_let_expr(&module, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["not found"]);
}

/// `unknown sub-component` producer anti-cascade contract (InstanceQualifiedAccess, line ~1773).
///
/// On current code, the unknown-sub arm returns `Type::Real`, so `Real + Real = Real`
/// → `result_type = Real ≠ Error` → RED.
#[test]
fn instance_qualified_unknown_sub_no_cascade() {
    let source = r#"
structure S { let broken = nonexistent_sub.(SomeTrait::x) + 5.0 }
"#;
    let module = compile_source(source);

    let expr = get_let_expr(&module, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["unknown sub-component"]);
}

// ── step-9: lambda-param types + collection-index-member ──────────────────────

/// `unresolved type in lambda param` anti-cascade contract (Category A, lines ~1376/1384).
///
/// On current code (pre-step-10), the two `Type::Real` fallbacks in the lambda-param
/// type-resolution path mean:
/// - `param_types[0] == Type::Real` (not `Type::Error`)
/// - `result_type == Type::Function { params: [Real], return_type: Real }`
///   → The lambda's param type is Real, not Error → RED.
///
/// After step-10 flips the fallbacks to `Type::Error`:
/// - `param_types[0] == Type::Error`
/// - `result_type == Type::Function { params: [Error], return_type: Error }`
///   → GREEN.
#[test]
fn lambda_param_unresolved_type_no_cascade() {
    let source = r#"
structure S { let f = |x: UnknownType| x + 1.0 }
"#;
    let module = compile_source(source);

    assert_no_type_cascade(&module.diagnostics, &["unresolved type in lambda param"]);

    // The lambda's result_type is Type::Function { params: [...], return_type: ... }.
    // After step-10, params[0] must be Type::Error (not Type::Real).
    let expr = get_let_expr(&module, "f");
    match &expr.result_type {
        Type::Function { params, .. } => {
            assert_eq!(
                params.first().cloned(),
                Some(Type::Error),
                "lambda param type should be Type::Error after step-10 flip; got {:?}",
                params,
            );
        }
        other => panic!(
            "expected Type::Function result_type for lambda, got {:?}",
            other
        ),
    }
}

/// `unknown member on collection sub` (indexed access) anti-cascade contract
/// (Category A, line ~888).
///
/// On current code (pre-step-10), the path continues with `member_type = Type::Real`
/// and builds a ValueRef, so `broken = Real + Real = Real` → `result_type = Real ≠ Error` → RED.
///
/// After step-10 returns `make_poison_literal` early, `broken = Type::Error` → GREEN.
#[test]
fn coll_index_unknown_member_no_cascade() {
    let source = r#"
structure Inner { param x : Length = 0mm }
structure S {
    sub bolts : List<Inner>
    let broken = bolts[0].nonexistent + 5.0
}
"#;
    let module = compile_source(source);

    // Multi-structure module: find the S template explicitly (get_let_expr only
    // looks at templates.first(), which would be Inner here).
    let expr = get_let_expr_in(&module, "S", "broken");

    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["collection sub"]);
}

/// `self.sub_coll[i].unknown_member` anti-cascade contract.
///
/// When a collection sub is accessed through `self.sub[index].member` syntax
/// (where the index object is `self.sub` rather than the bare sub name), the
/// expression falls through to the general member-access fallback at line ~1138
/// and hits `make_poison_literal` via the "member access not yet supported" path.
/// This test locks that behavior: `result_type == Type::Error` and no cascade.
///
/// Note: `coll_index_unknown_member_no_cascade` covers the `sub[i].member` bare-name
/// path (line ~996-1006).  This test covers the `self.sub[i].member` path.
#[test]
fn collection_sub_self_indexed_unknown_member_no_cascade() {
    let source = r#"
structure Inner { param x : Length = 0mm }
structure S {
    sub insts : List<Inner>
    let broken = self.insts[0].nonexistent + 5.0
}
"#;
    let module = compile_source(source);

    let expr = get_let_expr_in(&module, "S", "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["not yet supported"]);
}

// ── task 3639: self.<collection-sub>.<unknown-member> producer ─────────────

/// `self.bolts.nonexistent` bare-collection-sub-through-self anti-cascade contract.
///
/// `self.bolts.nonexistent` hits the `_` arm of the `fallback_type` match inside
/// the collection-sub `MemberAccess` branch, reached via the
/// `MemberAccess { object: MemberAccess { Ident("self"), sub_name }, member }` path.
///
/// Before step-2 fixes the `_` arm of the `fallback_type` match (in `compile_expr`'s
/// `self.<collection-sub>.<member>` branch), `unwrap_or(Type::Real)` causes the literal to
/// carry `Type::Real` → BinOp sees `Real + Real = Real` → `result_type = Real ≠ Error`
/// → RED.
///
/// After step-2 changes to `unwrap_or(Type::Error)`, the literal carries `Type::Error`
/// → `infer_binop_type` short-circuits on the poisoned LHS → `result_type = Type::Error`
/// → GREEN.
#[test]
fn self_collection_sub_unknown_member_no_cascade() {
    let source = r#"
structure Inner { param x : Length = 0mm }
structure Outer {
    sub bolts : List<Inner>
    let broken = self.bolts.nonexistent + 5.0
}
"#;
    let module = compile_source(source);

    let expr = get_let_expr_in(&module, "Outer", "broken");

    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected let-expr result_type == Type::Error (anti-cascade), got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["unknown member"]);
}

// ── task 3657: self.<collection-sub>.<aggregation-member> carve-out pins ──────

/// Pins the `"count" => Type::Int` arm of the `fallback_type` match in
/// `compile_expr`'s `self.<collection-sub>.<member>` branch
/// (`COLLECTION_AGGREGATION_MEMBERS` carve-out, task 3639).
///
/// **Key contrast with the sibling `self_collection_sub_unknown_member_no_cascade`:**
/// Every other test in this file asserts `result_type == Type::Error` (anti-cascade
/// poison for unrecoverable unknowns).  This test deliberately asserts a *concrete*
/// type (`Type::Int`) because the carve-out does **not** poison `count`/`sum`/`keys`/
/// `values`: the user typed a known aggregation method whose return type they know,
/// so downstream type checks against that concrete type are legitimate, not spurious
/// cascade (design decision #2, task 3639 review; see the comment block above the
/// `fallback_type` match in `compile_expr`).
///
/// Fixture shape: `let broken = self.bolts.count + 5`.
/// - `self.bolts.count` → `CompiledExpr::literal(Value::Undef, Type::Int)` (carve-out arm),
///   plus the "cannot access aggregation … through self" `Diagnostic::error`.
/// - `5` → `Type::Int` (integer literal; `classify_number_literal` returns `NumberClass::Int`).
/// - `infer_binop_type(Add, Int, Int)` = `left.clone()` = `Type::Int` (neither operand is
///   `Type::Error`, so no short-circuit).
/// - `(Int, Int)` hits `_ => {}` in the Add/Sub dimension check in `compile_expr` — no
///   extra "incompatible" diagnostic.
/// - Net: `result_type == Type::Int`; exactly one error ("cannot access aggregation").
///
/// If `"count" => Type::Int` regressed to `Type::Error`: `infer_binop_type` short-circuits
/// on the poisoned LHS → `result_type = Type::Error ≠ Type::Int` → assertion fails (RED).
///
/// `assert_no_type_cascade(&diags, &["cannot access aggregation"])` dual-asserts:
/// (a) ≥ 1 error contains that fragment (the diagnostic IS emitted), and
/// (b) every error matches it (no cascade).
#[test]
fn self_collection_sub_count_aggregation_pins_int_fallback() {
    let source = r#"
structure Inner { param x : Length = 0mm }
structure Outer {
    sub bolts : List<Inner>
    let broken = self.bolts.count + 5
}
"#;
    let module = compile_source(source);

    let expr = get_let_expr_in(&module, "Outer", "broken");

    assert_eq!(
        expr.result_type,
        Type::Int,
        "count carve-out must pin BinOp result_type == Type::Int (user-knows-the-type cascade-suppression); got {:?}",
        expr.result_type,
    );

    assert_no_type_cascade(&module.diagnostics, &["cannot access aggregation"]);
}

/// Pins the `"sum" | "keys" | "values" => Type::Real` arm of the `fallback_type` match
/// in `compile_expr`'s `self.<collection-sub>.<member>` branch
/// (`COLLECTION_AGGREGATION_MEMBERS` carve-out, task 3639).
///
/// Iterates over all three members (`sum`, `keys`, `values`) to actively guard against a
/// future split of the merged `expr.rs` arm (e.g. if `keys`/`values` were moved to
/// `Type::List(...)` once collection-iteration lands). Each member is tested independently
/// so the test goes RED immediately if any arm diverges from `Type::Real` (design decision
/// #4, task 3657).
///
/// Fixture shape per member: `let broken = self.bolts.<member> + 5`.
/// - `self.bolts.<member>` → `CompiledExpr::literal(Value::Undef, Type::Real)` (carve-out arm),
///   plus the "cannot access aggregation … through self" `Diagnostic::error`.
/// - `5` → `Type::Int` (integer literal).
/// - `infer_binop_type(Add, Real, Int)` = `left.clone()` = `Type::Real` (neither operand
///   is `Type::Error`; `infer_binop_type` returns `left.clone()` for matching-kind numeric
///   operands).
/// - `(Real, Int)` hits `_ => {}` in the Add/Sub dimension check — the
///   dimensioned+dimensionless error arm only matches `Type::Scalar`, not `Type::Real`, so
///   no extra diagnostic is emitted.
/// - Net: `result_type == Type::Real`; exactly one error ("cannot access aggregation").
///
/// If `"sum" | "keys" | "values" => Type::Real` regressed to `Type::Error`:
/// `infer_binop_type` short-circuits → `result_type = Type::Error ≠ Type::Real` → RED.
///
/// Same CONCRETE-type rationale as `self_collection_sub_count_aggregation_pins_int_fallback`;
/// see that test's docstring for the contrast with sibling Type::Error-asserting tests.
#[test]
fn self_collection_sub_sum_keys_values_aggregation_pins_real_fallback() {
    for member in ["sum", "keys", "values"] {
        let source = format!(
            r#"
structure Inner {{ param x : Length = 0mm }}
structure Outer {{
    sub bolts : List<Inner>
    let broken = self.bolts.{member} + 5
}}
"#
        );
        let module = compile_source(&source);

        let expr = get_let_expr_in(&module, "Outer", "broken");

        assert_eq!(
            expr.result_type,
            Type::Real,
            "sum/keys/values carve-out must pin BinOp result_type == Type::Real for member `{member}`; got {:?}",
            expr.result_type,
        );

        assert_no_type_cascade(&module.diagnostics, &["cannot access aggregation"]);
    }
}
