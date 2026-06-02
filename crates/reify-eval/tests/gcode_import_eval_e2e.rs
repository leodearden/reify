//! End-to-end eval tests for `gcode_import` reaching the real eval path
//! (task 4073).
//!
//! Guards that `gcode_import(source, dialect)` in a `.ri` structure actually
//! returns a real `Value::List` of motion profiles — NOT the `{ [] }` empty-list
//! stub the body previously contained.
//!
//! **RED today:** the `{ [] }` body makes `imported` evaluate to an empty list,
//! `profile_count` to 0, and the `profile_count > 0` constraint to Violated.
//! After the dispatch fix (step-2) these assertions go GREEN.
//!
//! Reads the shipped example file via `CARGO_MANIFEST_DIR` so this test is a
//! true guard of `examples/trajectory/gcode_import_smoke.ri`, not an inlined
//! copy. Follows the idiom from `kinematic_examples_e2e.rs`.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_core::{Type, ValueCellId};
use reify_ir::{Satisfaction, Value};
use reify_test_support::{
    check_source_with_stdlib, collect_errors, make_simple_engine, parse_and_compile_with_stdlib,
};

// ── Path constant ──────────────────────────────────────────────────────────────

const SMOKE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/trajectory/gcode_import_smoke.ri"
);

// ── Cached source + compile helpers ───────────────────────────────────────────

/// Read `examples/trajectory/gcode_import_smoke.ri`, caching the result.
fn smoke_source() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(SMOKE_PATH)
            .unwrap_or_else(|e| panic!("{SMOKE_PATH} should exist: {e}"))
    })
    .as_str()
}

/// Parse and compile `gcode_import_smoke.ri` with stdlib, caching the result.
///
/// Shared across the compile-clean gate and both primary eval tests to avoid
/// redundant stdlib compiles — stdlib compilation is non-trivial and the
/// source/compile result is identical for all three.
fn smoke_compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(smoke_source()))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Compile-clean gate (sanity check — should always be green)
// ═══════════════════════════════════════════════════════════════════════════════

/// The example file exists, is non-empty, and compiles with stdlib without any
/// Error-severity diagnostics. This is a pre-condition for the eval assertions.
#[test]
fn gcode_import_smoke_compiles_clean() {
    let source = smoke_source();
    assert!(
        !source.is_empty(),
        "gcode_import_smoke.ri should be non-empty"
    );
    let compiled = smoke_compiled();
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "gcode_import_smoke.ri should compile with zero Error diagnostics, got: {errors:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRIMARY: eval-path assertion — imported is a non-empty Value::List
// ═══════════════════════════════════════════════════════════════════════════════

/// **PRIMARY RED→GREEN signal.**
///
/// `GcodeImportSmoke.imported` must be a `Value::List` with at least one
/// element (the single `G1 X10 Y10` move lowers to exactly one profile).
///
/// Before the fix: `imported` is an empty `Value::List` (the `{ [] }` stub body
/// runs instead of `eval_gcode_import`).
/// After the fix: `imported` is a 1-element `Value::List` of profile records.
#[test]
fn gcode_import_smoke_imported_is_nonempty_list() {
    let compiled = smoke_compiled();
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let id = ValueCellId::new("GcodeImportSmoke", "imported");
    let imported = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GcodeImportSmoke.imported cell missing from eval result"));

    match imported {
        Value::List(items) => {
            assert!(
                !items.is_empty(),
                "GcodeImportSmoke.imported should be a non-empty list \
                 (the G1 move lowers to >= 1 profile); got an empty list — \
                 gcode_import is still returning the stub {{ [] }} body"
            );
        }
        other => panic!("expected Value::List for GcodeImportSmoke.imported, got {other:?}"),
    }
}

/// `GcodeImportSmoke.profile_count` must be `Value::Int(n)` with `n >= 1`.
///
/// Before the fix: `profile_count` is `Value::Int(0)` (count of the empty list).
/// After the fix: `profile_count` is `Value::Int(1)` (one profile for the single
/// contiguous `G1` move).
#[test]
fn gcode_import_smoke_profile_count_is_at_least_one() {
    let compiled = smoke_compiled();
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let id = ValueCellId::new("GcodeImportSmoke", "profile_count");
    let count = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GcodeImportSmoke.profile_count cell missing"));

    match count {
        Value::Int(n) => {
            assert!(
                *n >= 1,
                "GcodeImportSmoke.profile_count should be >= 1, got {n} — \
                 gcode_import is still returning the stub {{ [] }} empty list"
            );
        }
        other => panic!("expected Value::Int for GcodeImportSmoke.profile_count, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// IR dispatch-contract regression guard (Suggestion 1 from code-review)
// ═══════════════════════════════════════════════════════════════════════════════

/// **Dispatch-contract regression guard.**
///
/// Pins two properties of the compiled IR that the `gcode_import_lower` delegate
/// scheme depends on simultaneously:
///
/// 1. The `imported` let cell in `GcodeImportSmoke` compiles to
///    `CompiledExprKind::UserFunctionCall { function_name: "gcode_import" }` with
///    `result_type == Type::List(_)` — confirming that the `.ri` declaration
///    shadows the builtin name and the declared `List<Profile>` signature is intact.
///
/// 2. The `gcode_import` stdlib function body's result expression compiles to
///    `CompiledExprKind::FunctionCall` — confirming that `gcode_import_lower`
///    inside the body resolves via the `NoUserFunctions` → `FunctionCall` →
///    `eval_builtin` path (not recursively back to `gcode_import`).
///
/// If either property breaks (e.g., the `.ri` declaration is removed, the
/// resolver is changed to prefer builtins, or a body-vs-return-type check is
/// introduced), this test will fail with a clear diagnostic before any eval
/// regression surfaces.
///
/// NOTE — inner-call type discard (load-bearing implicit): `gcode_import_lower`
/// inside the body is inferred as `String` (first-arg fallback) rather than
/// `List<Profile>`. This mis-inferred type is intentionally discarded by
/// `compile_function` (no body-vs-declared-return-type check). The outer
/// `UserFunctionCall` result_type (`List<Profile>`) is the authoritative one.
#[test]
fn gcode_import_dispatch_ir_contract() {
    use reify_ir::CompiledExprKind;

    let compiled = smoke_compiled();

    // ── Part 1: call site in GcodeImportSmoke.imported ────────────────────────
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "GcodeImportSmoke")
        .expect("GcodeImportSmoke template should exist in compiled module");

    let imported_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "imported")
        .expect("GcodeImportSmoke.imported value cell should exist");

    let init_expr = imported_cell
        .default_expr
        .as_ref()
        .expect("GcodeImportSmoke.imported should have a default_expr (let binding)");

    match &init_expr.kind {
        CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(
                function_name, "gcode_import",
                "GcodeImportSmoke.imported should call 'gcode_import' as a \
                 UserFunctionCall — if this fails the .ri declaration may have \
                 been removed or the resolver changed to prefer builtins"
            );
        }
        other => panic!(
            "GcodeImportSmoke.imported init expr should be \
             UserFunctionCall(\"gcode_import\"), got: {other:?}"
        ),
    }
    assert!(
        matches!(&init_expr.result_type, Type::List(_)),
        "GcodeImportSmoke.imported init expr should have result_type List<_> \
         (from the declared gcode_import signature), got: {:?}",
        init_expr.result_type
    );

    // ── Part 2: body of the stdlib gcode_import function ──────────────────────
    // compile_with_stdlib returns a module containing ONLY user definitions;
    // stdlib functions are compiled context, not copied into the output module.
    // Use load_stdlib() to access the stdlib compiled modules directly and
    // find gcode_import in the trajectory module.
    let stdlib_modules = reify_compiler::stdlib_loader::load_stdlib();
    let gcode_import_fn = stdlib_modules
        .iter()
        .flat_map(|m| m.functions.iter())
        .find(|f| f.name == "gcode_import")
        .expect(
            "stdlib gcode_import function should appear in one of the stdlib \
             CompiledModules (trajectory stdlib module)",
        );

    match &gcode_import_fn.body.result_expr.kind {
        CompiledExprKind::FunctionCall { function, .. } => {
            assert_eq!(
                function.name, "gcode_import_lower",
                "gcode_import body should call 'gcode_import_lower' as a \
                 FunctionCall (stdlib builtin path), got function name: {:?}",
                function.name
            );
        }
        other => panic!(
            "gcode_import body result_expr should be \
             FunctionCall(\"gcode_import_lower\"), got: {other:?} — \
             the body may have changed or gcode_import_lower may now have \
             a .ri declaration (making it resolve as UserFunctionCall)"
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECONDARY: constraint-satisfaction assertion — profile_count > 0 is Satisfied
// ═══════════════════════════════════════════════════════════════════════════════

/// **SECONDARY RED→GREEN signal (reify check path).**
///
/// The `constraint profile_count > 0` in `GcodeImportSmoke` must surface as
/// `Satisfaction::Satisfied` after the dispatch fix.
///
/// Before the fix: `profile_count` is 0 → constraint is `Satisfied::Violated`.
/// After the fix: `profile_count` is 1 → constraint is `Satisfaction::Satisfied`.
///
/// This directly validates the task title: "its profile_count constraint is a
/// real eval-time assertion."
#[test]
fn gcode_import_smoke_profile_count_constraint_is_satisfied() {
    let source = smoke_source();
    let check_result = check_source_with_stdlib(source);

    // Scope to the GcodeImportSmoke structure — its id.entity field is the
    // structure name. This avoids coupling to global constraint state (stdlib or
    // future example constraints that are unrelated to this fix).
    let gcode_import_smoke_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|entry| entry.id.entity == "GcodeImportSmoke")
        .collect();

    assert!(
        !gcode_import_smoke_constraints.is_empty(),
        "expected at least one constraint entry for GcodeImportSmoke, got none — \
         check that the example still has `constraint profile_count > 0`"
    );

    // The GcodeImportSmoke structure has exactly one inline constraint:
    // `constraint profile_count > 0`. After the fix it must be Satisfied.
    let violated: Vec<_> = gcode_import_smoke_constraints
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "expected no Violated constraints in GcodeImportSmoke after gcode_import \
         dispatch fix; got: {violated:?}. gcode_import may still be returning \
         the stub {{ [] }} body (profile_count == 0)."
    );

    let satisfied: Vec<_> = gcode_import_smoke_constraints
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Satisfied)
        .collect();
    assert!(
        !satisfied.is_empty(),
        "expected GcodeImportSmoke.profile_count > 0 to be Satisfied; \
         got: {:?}",
        gcode_import_smoke_constraints
    );
}
