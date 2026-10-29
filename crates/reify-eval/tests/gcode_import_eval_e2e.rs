//! End-to-end eval tests for `gcode_import` reaching the real eval path
//! (task 4073).
//!
//! Guards that `gcode_import(source, dialect)` in a `.ri` structure actually
//! returns a real `Value::List` of motion segments — NOT the `{ [] }` empty-list
//! stub the body previously contained.
//!
//! **RED today:** the `{ [] }` body makes `imported` evaluate to an empty list,
//! `segment_count` to 0, and the `segment_count > 0` constraint to Violated.
//! After the dispatch fix (step-2) these assertions go GREEN.
//!
//! Reads the shipped example file via `CARGO_MANIFEST_DIR` so this test is a
//! true guard of `examples/trajectory/gcode_import_smoke.ri`, not an inlined
//! copy. Follows the idiom from `kinematic_examples_e2e.rs`.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use reify_compiler::{CompiledModule, ValueCellKind};
use reify_core::{DiagnosticCode, DimensionVector, Type, ValueCellId};
use reify_ir::{Satisfaction, Value, ValueMap};
use reify_test_support::{
    check_source_with_stdlib, collect_errors, compile_source_with_stdlib, make_simple_engine,
    parse_and_compile_with_stdlib,
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

/// `GcodeImportSmoke.segment_count` must be `Value::Int(n)` with `n >= 1`.
///
/// Before the fix: `segment_count` is `Value::Int(0)` (count of the empty list).
/// After the fix: `segment_count` is `Value::Int(1)` (one segment for the single
/// contiguous `G1` move).
#[test]
fn gcode_import_smoke_segment_count_is_at_least_one() {
    let compiled = smoke_compiled();
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let id = ValueCellId::new("GcodeImportSmoke", "segment_count");
    let count = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GcodeImportSmoke.segment_count cell missing"));

    match count {
        Value::Int(n) => {
            assert!(
                *n >= 1,
                "GcodeImportSmoke.segment_count should be >= 1, got {n} — \
                 gcode_import is still returning the stub {{ [] }} empty list"
            );
        }
        other => panic!("expected Value::Int for GcodeImportSmoke.segment_count, got {other:?}"),
    }
}

/// The imported waypoints are READABLE in the DSL with their declared types:
/// `imported[0].waypoints[0].x` is a `Length`, so the shipped example's
/// `first_x` cell evaluates to exactly 10 mm in SI metres.
#[test]
fn gcode_import_smoke_first_waypoint_x_is_typed_length() {
    let compiled = smoke_compiled();
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let id = ValueCellId::new("GcodeImportSmoke", "first_x");
    let first_x = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GcodeImportSmoke.first_x cell missing from eval result"));

    assert_eq!(
        *first_x,
        Value::length(0.01),
        "GcodeImportSmoke.first_x should be the first waypoint's x, 10 mm as an \
         SI Length (0.01 m), got {first_x:?}"
    );
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
///    `result_type == List<GcodeMotionSegment>` — confirming that the `.ri`
///    declaration shadows the builtin name and the declared signature is intact.
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
/// `List<GcodeMotionSegment>`. This mis-inferred type is intentionally discarded
/// by `compile_function` (no body-vs-declared-return-type check). The outer
/// `UserFunctionCall` result_type (`List<GcodeMotionSegment>`) is the
/// authoritative one.
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
    assert_eq!(
        init_expr.result_type,
        Type::List(Box::new(Type::StructureRef(
            "GcodeMotionSegment".to_string()
        ))),
        "GcodeImportSmoke.imported init expr should have result_type \
         List<GcodeMotionSegment> (from the declared gcode_import signature)"
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
// TYPE-LEVEL gate — an imported segment is not a Profile
// ═══════════════════════════════════════════════════════════════════════════════

/// A `GcodeMotionSegment` carries move-target waypoints with no time
/// parameterisation, so it must not reach any `Profile` parameter. Each Profile
/// consumer rejects it AT COMPILE TIME with `TypeNotConformingToTrait` naming
/// both types, instead of compiling clean and evaluating to undef at runtime.
#[test]
fn imported_segment_is_rejected_at_every_profile_param() {
    let calls = [
        "profile_duration(seg)",
        "evaluate_profile(seg, 0.0s)",
        "evaluate_profile_dot(seg, 0.0s)",
        "evaluate_profile_ddot(seg, 0.0s)",
    ];
    for call in calls {
        let source = format!(
            r#"structure def ImportedIsNotAProfile {{
    let imported = gcode_import("G1 X10 Y10 F1800", MarlinDialect())
    let seg = imported[0]
    let r = {call}
}}
"#
        );
        let module = compile_source_with_stdlib(&source);
        let errors = collect_errors(&module.diagnostics);
        assert!(
            errors.iter().any(|d| {
                d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
                    && d.message.contains("GcodeMotionSegment")
                    && d.message.contains("Profile")
            }),
            "`{call}` with an imported G-code segment should be rejected at compile \
             time with TypeNotConformingToTrait naming GcodeMotionSegment and \
             Profile; got errors: {errors:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECONDARY: constraint-satisfaction assertion — segment_count > 0 is Satisfied
// ═══════════════════════════════════════════════════════════════════════════════

/// **SECONDARY RED→GREEN signal (reify check path).**
///
/// The `constraint segment_count > 0` in `GcodeImportSmoke` must surface as
/// `Satisfaction::Satisfied` after the dispatch fix.
///
/// Before the fix: `segment_count` is 0 → constraint is `Satisfied::Violated`.
/// After the fix: `segment_count` is 1 → constraint is `Satisfaction::Satisfied`.
///
/// This directly validates the task title: "its segment_count constraint is a
/// real eval-time assertion."
#[test]
fn gcode_import_smoke_segment_count_constraint_is_satisfied() {
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
         check that the example still has `constraint segment_count > 0`"
    );

    // The GcodeImportSmoke structure has exactly one inline constraint:
    // `constraint segment_count > 0`. After the fix it must be Satisfied.
    let violated: Vec<_> = gcode_import_smoke_constraints
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "expected no Violated constraints in GcodeImportSmoke after gcode_import \
         dispatch fix; got: {violated:?}. gcode_import may still be returning \
         the stub {{ [] }} body (segment_count == 0)."
    );

    let satisfied: Vec<_> = gcode_import_smoke_constraints
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Satisfied)
        .collect();
    assert!(
        !satisfied.is_empty(),
        "expected GcodeImportSmoke.segment_count > 0 to be Satisfied; \
         got: {:?}",
        gcode_import_smoke_constraints
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// UNIT-REGIME contract — the DSL-visible waypoint payload is SI and dimensioned
// ═══════════════════════════════════════════════════════════════════════════════

/// **Unit-regime pin (task #6478).**
///
/// The `Value` payload `gcode_import` hands to the DSL must be SI and
/// dimensioned, not raw native G-code millimetres. `gcode_import_smoke.ri`'s
/// source is `"G1 X10 Y10"` — a single Marlin linear move to (10 mm, 10 mm) —
/// so the one lowered segment carries exactly one waypoint at
/// (0.01 m, 0.01 m, 0 m) with zero extrusion.
///
/// Asserted END-TO-END through the real compiler + evaluator (not just the
/// marshaller) against the SHIPPED example, unmodified — the same
/// "true guard of the example, not an inlined copy" idiom as the tests above.
///
/// Every field is asserted with its DIMENSION, not just its magnitude: an
/// undimensioned `Value::Real(0.01)` would pass a value-only check while
/// leaving the regime unstated. The `si_value` comparison is EXACT
/// (`assert_eq!`, no tolerance) because `10.0 * 1.0e-3 == 0.01` is bit-exact.
///
/// The runtime carrier is the declared type: a `GcodeMotionSegment` instance
/// whose waypoints are `GcodeWaypoint` instances. What keeps segments out of
/// the Profile trampolines is that declared type — a compile-time
/// `TypeNotConformingToTrait`, pinned by
/// `imported_segment_is_rejected_at_every_profile_param` — not the carrier
/// shape. The example's `G1` carries no `F` word, so `feedrate` is `none`
/// (never a `0.0` velocity sentinel).
#[test]
fn gcode_import_smoke_waypoints_are_si_lengths() {
    let compiled = smoke_compiled();
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled);

    let id = ValueCellId::new("GcodeImportSmoke", "imported");
    let imported = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GcodeImportSmoke.imported cell missing from eval result"));

    let Value::List(segments) = imported else {
        panic!("expected Value::List for GcodeImportSmoke.imported, got {imported:?}")
    };
    assert_eq!(
        segments.len(),
        1,
        "the single `G1 X10 Y10` move lowers to exactly one segment, got {segments:?}"
    );

    let Value::StructureInstance(segment) = &segments[0] else {
        panic!(
            "expected a GcodeMotionSegment instance, got {:?}",
            segments[0]
        )
    };
    assert_eq!(segment.type_name, "GcodeMotionSegment");

    let Some(Value::List(waypoints)) = segment.fields.get("waypoints") else {
        panic!("segment should carry a Value::List under \"waypoints\", got {segment:?}")
    };
    assert_eq!(
        waypoints.len(),
        1,
        "one `G1` move yields exactly one waypoint, got {waypoints:?}"
    );

    let Value::StructureInstance(wp) = &waypoints[0] else {
        panic!("expected a GcodeWaypoint instance, got {:?}", waypoints[0])
    };
    assert_eq!(wp.type_name, "GcodeWaypoint");

    // x / y / z / e are SI metres carrying the LENGTH dimension.
    // 10 mm → 0.01 m; the untouched Z and E axes stay at the origin.
    for (axis, expected) in [("x", 0.01_f64), ("y", 0.01), ("z", 0.0), ("e", 0.0)] {
        let got = wp
            .fields
            .get(axis)
            .unwrap_or_else(|| panic!("waypoint should carry an \"{axis}\" field: {wp:?}"));
        match got {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert_eq!(
                    *si_value, expected,
                    "waypoint.{axis} should be {expected} SI metres (the g-code's \
                     millimetres converted at the marshalling boundary), got {si_value}"
                );
                assert_eq!(
                    *dimension,
                    DimensionVector::LENGTH,
                    "waypoint.{axis} should carry the LENGTH dimension, got {dimension:?}"
                );
            }
            other => panic!(
                "waypoint.{axis} should be a dimensioned Value::Scalar, got {other:?} — \
                 an undimensioned Real leaves the unit regime unstated"
            ),
        }
    }

    assert_eq!(
        wp.fields.get("feedrate"),
        Some(&Value::Option(None)),
        "no F word is in effect for `G1 X10 Y10`, so feedrate must be none \
         (never a 0.0 sentinel); got {wp:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// DSL READ-BACK — every declared waypoint field, with an F word in effect
// ═══════════════════════════════════════════════════════════════════════════════

const READ_BACK_STRUCTURE: &str = "GcodeWaypointReadBack";

/// One Marlin move with a DISTINCT value on every axis and an `F` word in
/// effect, so a swapped, misnamed or dropped field cannot read back correctly.
const READ_BACK_SOURCE: &str = r#"structure def GcodeWaypointReadBack {
    let imported = gcode_import("G1 X10 Y20 Z3 E4 F1800", MarlinDialect())
    let wp = imported[0].waypoints[0]
    let x = wp.x
    let y = wp.y
    let z = wp.z
    let e = wp.e
    let feedrate = wp.feedrate
    let feed : Velocity = unwrap_or(wp.feedrate, 0mm/s)
}
"#;

/// Compile [`READ_BACK_SOURCE`] (zero Error diagnostics required) and evaluate
/// it, caching the result as [`smoke_compiled`] does.
fn read_back_values() -> &'static ValueMap {
    static V: OnceLock<ValueMap> = OnceLock::new();
    V.get_or_init(|| {
        let compiled = parse_and_compile_with_stdlib(READ_BACK_SOURCE);
        make_simple_engine().eval(&compiled).values
    })
}

fn read_back_cell<'a>(values: &'a ValueMap, member: &str) -> &'a Value {
    values
        .get(&ValueCellId::new(READ_BACK_STRUCTURE, member))
        .unwrap_or_else(|| panic!("{READ_BACK_STRUCTURE}.{member} cell missing from eval result"))
}

/// The declared `GcodeWaypoint` field types accept the minted values: every
/// field reads back through the DSL, and `feedrate`, the field carried as an
/// `Option`, is `some(0.03 m/s)` and unwraps to that `Velocity`.
#[test]
fn every_waypoint_field_reads_back_through_the_dsl() {
    let values = read_back_values();

    for (member, metres) in [("x", 0.01), ("y", 0.02), ("z", 0.003), ("e", 0.004)] {
        assert_eq!(
            *read_back_cell(values, member),
            Value::length(metres),
            "wp.{member} should read back as the Length {metres} m"
        );
    }

    let feed = Value::Scalar {
        si_value: 0.03,
        dimension: DimensionVector::VELOCITY,
    };
    assert_eq!(
        *read_back_cell(values, "feedrate"),
        Value::Option(Some(Box::new(feed.clone()))),
        "F1800 mm/min should read back as some(0.03 m/s)"
    );
    assert_eq!(
        *read_back_cell(values, "feed"),
        feed,
        "unwrap_or should yield the present 0.03 m/s Velocity, not the 0 mm/s default"
    );
}

/// Params declared by the stdlib `structure def` named `type_name`.
fn declared_params<'a>(stdlib: &'a [CompiledModule], type_name: &str) -> BTreeSet<&'a str> {
    let template = stdlib
        .iter()
        .flat_map(|m| m.templates.iter())
        .find(|t| t.name == type_name)
        .unwrap_or_else(|| panic!("stdlib declares no structure named `{type_name}`"));
    template
        .value_cells
        .iter()
        .filter(|cell| cell.kind == ValueCellKind::Param)
        .map(|cell| cell.id.member.as_str())
        .collect()
}

/// Referential integrity between the marshaller and the stdlib declarations:
/// each instance `gcode_import` mints names a stdlib `structure def` and carries
/// exactly that declaration's params as fields. A drifted name would still
/// type-check against the declaration and then evaluate to undef.
#[test]
fn minted_instances_carry_exactly_their_declared_params() {
    let values = read_back_values();
    let stdlib = reify_compiler::stdlib_loader::load_stdlib();

    let Value::List(segments) = read_back_cell(values, "imported") else {
        panic!("expected Value::List for {READ_BACK_STRUCTURE}.imported")
    };
    let first_waypoint = read_back_cell(values, "wp");

    for instance in [&segments[0], first_waypoint] {
        let Value::StructureInstance(data) = instance else {
            panic!("expected a minted StructureInstance, got {instance:?}")
        };
        let minted: BTreeSet<&str> = data.fields.keys().map(String::as_str).collect();
        assert_eq!(
            minted,
            declared_params(stdlib, &data.type_name),
            "the fields of a minted `{}` should be exactly the params its stdlib \
             structure def declares",
            data.type_name
        );
    }
}
