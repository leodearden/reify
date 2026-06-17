//! Acceptance tests for fn-param structure-ctor defaults (task-4544).
//!
//! ## Test (a): same-module regression pin
//!
//! `same_module_struct_ctor_default_compiles_and_consumes` — a fn whose sole
//! param has a same-module StructureInstanceCtor default compiles clean, the
//! default is recorded as a StructureInstanceCtor, and a call-site that omits
//! the arg is padded to a 1-arg UserFunctionCall.  GREEN via task-3895 skeleton
//! pre-pass + task-4544 step-2's prefix wildcard fix.
//!
//! ## Test (b): elastic def-site + consumption (RED until step-4)
//!
//! `solve_elastic_static_options_defaults_and_omittable` — verifies that
//! `solve_elastic_static`'s `options` param carries a `StructureInstanceCtor`
//! default AND that a 6-arg call (omitting `options`) compiles with zero Error
//! diagnostics.  RED until step-4 adds `= ElasticOptions()` to solver_elastic.ri.

use reify_compiler::*;
use reify_core::Severity;
use reify_ir::CompiledExprKind;
use reify_test_support::compile_source_with_stdlib;

// ─── test (a): same-module struct-ctor default regression pin ────────────────

/// Regression pin (task-4544, task-3895 skeleton pre-pass).
///
/// A same-module fn `run(o : Opts = Opts()) -> Int` must:
/// 1. Compile with zero Error-severity diagnostics.
/// 2. Record its `options` param_default as `StructureInstanceCtor("Opts")`.
/// 3. Allow a call-site `run()` (0 args) that pads to a 1-arg UserFunctionCall
///    carrying the compiled Opts() default.
///
/// GREEN on main after steps 1-2 — serves as the regression anchor for the
/// same-module skeleton pre-pass AND the `try_default_padding` prefix wildcard fix.
#[test]
fn same_module_struct_ctor_default_compiles_and_consumes() {
    let src = r#"
module test.opts

structure def Opts {
    param iters : Int = 10
}

pub fn run(o : Opts = Opts()) -> Int { o.iters }

structure def App {
    let v = run()
}
"#;
    let module = compile_source_with_stdlib(src);

    // (1) Zero Error diagnostics.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "test.opts module should compile with zero errors; got: {:#?}",
        errors
    );

    // (2) run's param_default is StructureInstanceCtor("Opts").
    let run_fn = module
        .functions
        .iter()
        .find(|f| f.name == "run")
        .unwrap_or_else(|| {
            panic!(
                "expected fn `run` in compiled module; found functions: {:?}",
                module.functions.iter().map(|f| f.name.as_str()).collect::<Vec<_>>()
            )
        });
    assert_eq!(
        run_fn.params.len(),
        1,
        "run must have exactly 1 param (o); got {:?}",
        run_fn.params.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        run_fn.param_defaults.len(),
        1,
        "param_defaults must be length-aligned to params (invariant task-3702)"
    );
    let default_expr = run_fn.param_defaults[0]
        .as_ref()
        .expect("run's param `o` must carry a compiled default (= Opts())");
    match &default_expr.kind {
        CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
            assert_eq!(
                type_name, "Opts",
                "run's param default must be StructureInstanceCtor(\"Opts\"); got type_name: {}",
                type_name
            );
        }
        other => panic!(
            "run's param default must be StructureInstanceCtor; got: {:?}",
            other
        ),
    }

    // (3) App's `v` cell is a UserFunctionCall padded to 1 arg.
    let app_template = module
        .templates
        .iter()
        .find(|t| t.name == "App")
        .unwrap_or_else(|| {
            panic!(
                "expected template `App` in compiled module; found templates: {:?}",
                module.templates.iter().map(|t| t.name.as_str()).collect::<Vec<_>>()
            )
        });
    let v_cell = app_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("App must have a `v` value cell (let v = run())");
    let v_expr = v_cell
        .default_expr
        .as_ref()
        .expect("App's `v` cell must have a compiled expression");
    match &v_expr.kind {
        CompiledExprKind::UserFunctionCall { function_name, args } => {
            assert_eq!(
                function_name, "run",
                "App's `v` must call `run`; got function_name: {}",
                function_name
            );
            assert_eq!(
                args.len(),
                1,
                "App's `v` call must carry 1 arg (the padded Opts() default); \
                 got {} args",
                args.len()
            );
        }
        other => panic!(
            "App's `v` cell must be a UserFunctionCall; got: {:?}",
            other
        ),
    }
}

// ─── test (b): elastic def-site + consumption ────────────────────────────────

/// `solve_elastic_static`'s `options` param must carry a StructureInstanceCtor
/// default (`= ElasticOptions()`), AND a 6-arg call omitting `options` must
/// compile with zero Error-severity diagnostics.
///
/// GREEN after step-4 adds `= ElasticOptions()` to solver_elastic.ri.
#[test]
fn solve_elastic_static_options_defaults_and_omittable() {
    // ── (1) def-site: param_default is StructureInstanceCtor("ElasticOptions") ──
    let elastic_module = stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/solver/elastic")
        .unwrap_or_else(|| {
            panic!(
                "stdlib must contain std/solver/elastic; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        });
    let solve_fn = elastic_module
        .functions
        .iter()
        .find(|f| f.name == "solve_elastic_static")
        .unwrap_or_else(|| {
            panic!(
                "fn solve_elastic_static not found in std/solver/elastic; \
                 available functions: {:?}",
                elastic_module
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    // The options param is the 7th (index 6).
    assert_eq!(
        solve_fn.params.len(),
        7,
        "solve_elastic_static must have 7 params; got: {:?}",
        solve_fn.params.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );
    let options_default = solve_fn.param_defaults[6]
        .as_ref()
        .expect(
            "solve_elastic_static's `options` param must carry a compiled default \
             (= ElasticOptions()); currently None — add the default in step-4",
        );
    match &options_default.kind {
        CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
            assert_eq!(
                type_name, "ElasticOptions",
                "options param default must be StructureInstanceCtor(\"ElasticOptions\"); \
                 got type_name: {}",
                type_name
            );
        }
        other => panic!(
            "options param default must be StructureInstanceCtor; got: {:?}",
            other
        ),
    }

    // ── (2) call-site: 6-arg call (omitting options) compiles clean ──────────
    let src = r#"
structure ElasticDefaultsTest {
    let result = solve_elastic_static(
        Steel_AISI_1045(),
        1000mm,
        100mm,
        100mm,
        [PointLoad(point: "tip", force: 1000.0)],
        [FixedSupport(target: "root")]
    )
}
"#;
    let module = compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "6-arg call to solve_elastic_static (omitting options) must compile \
         with zero Error diagnostics once = ElasticOptions() default is added; \
         got: {:#?}",
        errors
    );
}

// ─── test (c): buckling def-site + consumption ───────────────────────────────

/// `solve_buckling`'s `options` param must carry a StructureInstanceCtor default
/// (`= BucklingOptions()`), AND a 6-arg call omitting `options` must compile
/// with zero Error-severity diagnostics.
///
/// GREEN; `= BucklingOptions()` was added to solver_buckling_fns.ri in step-6
/// (same commit as this test).
#[test]
fn solve_buckling_options_defaults_and_omittable() {
    // ── (1) def-site: param_default is StructureInstanceCtor("BucklingOptions") ──
    let buckling_module = stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/solver/buckling/fns")
        .unwrap_or_else(|| {
            panic!(
                "stdlib must contain std/solver/buckling/fns; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        });
    let solve_fn = buckling_module
        .functions
        .iter()
        .find(|f| f.name == "solve_buckling")
        .unwrap_or_else(|| {
            panic!(
                "fn solve_buckling not found in std/solver/buckling/fns; \
                 available functions: {:?}",
                buckling_module
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        solve_fn.params.len(),
        7,
        "solve_buckling must have 7 params; got: {:?}",
        solve_fn.params.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );
    let options_default = solve_fn.param_defaults[6]
        .as_ref()
        .expect(
            "solve_buckling's `options` param must carry a compiled default \
             (= BucklingOptions()); None means the default is missing from \
             solver_buckling_fns.ri",
        );
    match &options_default.kind {
        CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
            assert_eq!(
                type_name, "BucklingOptions",
                "options param default must be StructureInstanceCtor(\"BucklingOptions\"); \
                 got type_name: {}",
                type_name
            );
        }
        other => panic!(
            "options param default must be StructureInstanceCtor; got: {:?}",
            other
        ),
    }

    // ── (2) call-site: 6-arg call (omitting options) compiles clean ──────────
    let src = r#"
structure BucklingDefaultsTest {
    let result = solve_buckling(
        Steel_AISI_1045(),
        1000mm,
        100mm,
        100mm,
        [PointLoad(point: "tip", force: 1000.0)],
        [FixedSupport(target: "root")]
    )
}
"#;
    let module = compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "6-arg call to solve_buckling (omitting options) must compile \
         with zero Error diagnostics once = BucklingOptions() default is added; \
         got: {:#?}",
        errors
    );
}

// ─── test (d): modal def-site + consumption ──────────────────────────────────

/// `modal_analysis`'s `options` param must carry a StructureInstanceCtor default
/// (`= ModalOptions()`), AND a 4-arg call omitting `options` must compile
/// with zero Error-severity diagnostics.
///
/// GREEN; `= ModalOptions()` was added to modal_analysis_fns.ri in step-8
/// (same commit as this test).
#[test]
fn modal_analysis_options_defaults_and_omittable() {
    // ── (1) def-site: param_default is StructureInstanceCtor("ModalOptions") ──
    let modal_module = stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/modal/analysis/fns")
        .unwrap_or_else(|| {
            panic!(
                "stdlib must contain std/modal/analysis/fns; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        });
    let modal_fn = modal_module
        .functions
        .iter()
        .find(|f| f.name == "modal_analysis")
        .unwrap_or_else(|| {
            panic!(
                "fn modal_analysis not found in std/modal/analysis/fns; \
                 available functions: {:?}",
                modal_module
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        modal_fn.params.len(),
        5,
        "modal_analysis must have 5 params; got: {:?}",
        modal_fn.params.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );
    let options_default = modal_fn.param_defaults[4]
        .as_ref()
        .expect(
            "modal_analysis's `options` param must carry a compiled default \
             (= ModalOptions()); None means the default is missing from \
             modal_analysis_fns.ri",
        );
    match &options_default.kind {
        CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
            assert_eq!(
                type_name, "ModalOptions",
                "options param default must be StructureInstanceCtor(\"ModalOptions\"); \
                 got type_name: {}",
                type_name
            );
        }
        other => panic!(
            "options param default must be StructureInstanceCtor; got: {:?}",
            other
        ),
    }

    // ── (2) call-site: 4-arg call (omitting options) compiles clean ──────────
    let src = r#"
structure ModalDefaultsTest {
    let result = modal_analysis(
        Steel_AISI_1045(),
        1000mm,
        100mm,
        100mm
    )
}
"#;
    let module = compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "4-arg call to modal_analysis (omitting options) must compile \
         with zero Error diagnostics once = ModalOptions() default is added; \
         got: {:#?}",
        errors
    );
}

// ─── test (e): non-conforming arg to trait-typed param still errors ───────────

/// The wildcard relaxation in `try_default_padding` defers trait-conformance
/// validation to `phase_fn_arg_conformance`, NOT skips it.  A padded call whose
/// explicit leading arg does NOT conform to the trait-typed param must still
/// produce at least one Error-severity diagnostic.
///
/// Uses `solve_buckling` (first param: `ElasticMaterial`) with a custom struct
/// `NotAMaterial` that carries no trait conformance.  `options` is omitted —
/// the wildcard prefix check passes (trait param → wildcard, any StructureRef
/// accepted at prefix stage), the options default is padded in, but
/// `phase_fn_arg_conformance` then rejects NotAMaterial at the ElasticMaterial
/// slot.
#[test]
fn padded_call_nonconforming_arg_still_errors() {
    let src = r#"
structure def NotAMaterial {
    param x : Int = 0
}

structure NonConformingPaddedCall {
    let result = solve_buckling(
        NotAMaterial(),
        1000mm,
        100mm,
        100mm,
        [PointLoad(point: "tip", force: 1000.0)],
        [FixedSupport(target: "root")]
    )
}
"#;
    let module = compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "passing NotAMaterial (non-conforming) to solve_buckling's ElasticMaterial \
         param must produce at least one Error diagnostic (conformance enforced \
         downstream by phase_fn_arg_conformance even after wildcard prefix padding); \
         got zero errors"
    );
}
