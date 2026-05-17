//! Tests for stdlib_loader — embedded .ri stdlib loading, compilation, and caching.

use reify_compiler::stdlib_loader;
use reify_syntax::Pragma;
use reify_test_support::{
    CompiledModuleBuilder, EXPECTED_GEOMETRY_TRAITS, EXPECTED_MATERIAL_TRAITS, collect_errors,
    steel_elastic_source, steel_strong_source,
};
use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFnBody, CompiledFunction, ContentHash,
    ModulePath, SourceSpan, Type,
};

/// Recursively collect ValueRef member names from a compiled expression tree.
fn collect_value_ref_members(expr: &CompiledExpr) -> Vec<&str> {
    match &expr.kind {
        CompiledExprKind::ValueRef(cell_id) => vec![cell_id.member.as_str()],
        CompiledExprKind::BinOp { left, right, .. } => {
            let mut refs = collect_value_ref_members(left);
            refs.extend(collect_value_ref_members(right));
            refs
        }
        CompiledExprKind::UnOp { operand, .. } => collect_value_ref_members(operand),
        _ => vec![],
    }
}

// ─── step-1: basic loading ──────────────────────────────────────────────

/// load_stdlib() returns a non-empty slice of compiled modules.
#[test]
fn load_stdlib_returns_non_empty_slice() {
    let modules = stdlib_loader::load_stdlib();
    assert!(
        !modules.is_empty(),
        "load_stdlib() should return at least one compiled module"
    );
}

/// All stdlib modules compile without error-severity diagnostics.
#[test]
fn all_stdlib_modules_have_no_errors() {
    let modules = stdlib_loader::load_stdlib();
    for module in modules {
        let errors = collect_errors(&module.diagnostics);
        assert!(
            errors.is_empty(),
            "stdlib module '{}' has error diagnostics: {:?}",
            module.path,
            errors
        );
    }
}

/// materials_mechanical.ri traits are present in the stdlib (MaterialSpec, Elastic,
/// Strong, Hard, FatigueRated, FractureTough, Ductile, ImpactResistant, Damping).
#[test]
fn materials_mechanical_traits_present() {
    let modules = stdlib_loader::load_stdlib();

    // Collect all trait names across all stdlib modules
    let all_traits: Vec<&str> = modules
        .iter()
        .flat_map(|m| m.trait_defs.iter().map(|t| t.name.as_str()))
        .collect();

    for name in EXPECTED_MATERIAL_TRAITS {
        assert!(
            all_traits.contains(name),
            "expected trait '{}' in stdlib, found: {:?}",
            name,
            all_traits
        );
    }
}

/// `std.geometry.traits` contains exactly the EXPECTED_GEOMETRY_TRAITS list
/// — same names, same count. Single source of truth for the geometry trait
/// set; per-module `geometry_traits_tests.rs` delegates to this rather than
/// re-asserting names locally. Scoped to the geometry module specifically so
/// the count assertion is meaningful (a flat cross-module count would not be).
#[test]
fn geometry_traits_present() {
    let modules = stdlib_loader::load_stdlib();

    let geometry_module = modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/geometry/traits")
        .expect("std.geometry.traits module should be present in the stdlib");

    let trait_names: Vec<&str> = geometry_module
        .trait_defs
        .iter()
        .map(|t| t.name.as_str())
        .collect();

    assert_eq!(
        trait_names.len(),
        EXPECTED_GEOMETRY_TRAITS.len(),
        "std.geometry.traits should contain exactly {} traits, got {}: {:?}",
        EXPECTED_GEOMETRY_TRAITS.len(),
        trait_names.len(),
        trait_names
    );

    for name in EXPECTED_GEOMETRY_TRAITS {
        assert!(
            trait_names.contains(name),
            "expected trait '{}' in std.geometry.traits, found: {:?}",
            name,
            trait_names
        );
    }
}

/// Second call to load_stdlib() returns the same pointer (OnceLock cached).
#[test]
fn load_stdlib_is_cached() {
    let first = stdlib_loader::load_stdlib();
    let second = stdlib_loader::load_stdlib();
    assert!(
        std::ptr::eq(first, second),
        "load_stdlib() should return the same slice reference on repeated calls"
    );
}

// ─── step-1b: std.units is the first stdlib module (bootstrap order) ─

/// load_stdlib() returns std.units as the first module in the slice.
/// This ensures units are available to all subsequent stdlib modules.
#[test]
fn std_units_is_first_module() {
    let modules = stdlib_loader::load_stdlib();
    assert!(
        modules.len() >= 2,
        "expected at least 2 stdlib modules (units + materials), got {}",
        modules.len()
    );
    let first = &modules[0];
    let path_str = format!("{}", first.path);
    assert!(
        path_str.contains("units"),
        "first stdlib module should be std.units, got path: {}",
        path_str
    );
}

// ─── step-3b: std.units module content validation ───────────────────

/// std.units module has zero error diagnostics and contains the hand-written
/// SI base + non-SI units (cm, m, in, deg, rad, kg, g, s, ...). Note: SI
/// prefixed units like `mm` and `km` now live in the generated `std.si_units`
/// module — see `si_units_tests.rs` for their coverage.
#[test]
fn std_units_module_has_expected_units() {
    let modules = stdlib_loader::load_stdlib();
    // ModulePath Display uses '/' as the separator.
    let units_module = modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/units")
        .expect("std.units module not found in stdlib");

    // No error diagnostics
    let errors = collect_errors(&units_module.diagnostics);
    assert!(
        errors.is_empty(),
        "std.units should have zero error diagnostics, got: {:?}",
        errors
    );

    // At least the 8 hand-written base units surviving after the SI prefix
    // split-out.
    assert!(
        units_module.units.len() >= 8,
        "expected at least 8 units, got {}",
        units_module.units.len()
    );

    let unit_names: Vec<&str> = units_module.units.iter().map(|u| u.name.as_str()).collect();

    // These are the base / imperial / temperature units that remain hand-written.
    let required = ["cm", "m", "in", "deg", "rad", "kg", "g", "s"];
    for name in &required {
        assert!(
            unit_names.contains(name),
            "expected unit '{}' in std.units, found: {:?}",
            name,
            unit_names
        );
    }

    // Verify dimensions for a few key units.
    let cm = units_module.units.iter().find(|u| u.name == "cm").unwrap();
    assert_eq!(cm.dimension, reify_types::DimensionVector::LENGTH);
    assert!((cm.factor - 0.01).abs() < 1e-12);

    let deg = units_module.units.iter().find(|u| u.name == "deg").unwrap();
    assert_eq!(deg.dimension, reify_types::DimensionVector::ANGLE);
    assert!(
        (deg.factor - std::f64::consts::PI / 180.0).abs() < 1e-15,
        "deg factor should be PI/180, got {}",
        deg.factor
    );

    let kg = units_module.units.iter().find(|u| u.name == "kg").unwrap();
    assert_eq!(kg.dimension, reify_types::DimensionVector::MASS);
    assert!((kg.factor - 1.0).abs() < 1e-12);

    let s = units_module.units.iter().find(|u| u.name == "s").unwrap();
    assert_eq!(s.dimension, reify_types::DimensionVector::TIME);
    assert!((s.factor - 1.0).abs() < 1e-12);
}

// ─── step-2492: bidirectional #no_prelude invariant ─────────────────

/// Build a [`CompiledModule`] with dotted path `dotted` that carries a single
/// `#no_prelude` pragma.  Used by the synthetic-fixture `#no_prelude` tests
/// so the builder-and-push boilerplate lives in one place.
fn module_with_no_prelude(dotted: &str) -> reify_compiler::CompiledModule {
    let no_prelude = Pragma {
        name: "no_prelude".to_string(),
        args: vec![],
        span: SourceSpan::new(0, 0),
    };
    let mut module = CompiledModuleBuilder::new(ModulePath::from_dotted(dotted).unwrap()).build();
    module.pragmas.push(no_prelude);
    module
}

/// Synthetic fixture: a non-bootstrap module (`std/materials/thermal`) that
/// incorrectly carries `#no_prelude` must cause the bidirectional invariant
/// helper to panic, naming the offending path in the panic message.
///
/// This exercises the *inverse* direction of the invariant: any module whose
/// path is NOT in the bootstrap `targets` list must NOT carry `#no_prelude`.
/// The `#[should_panic(expected = "std/materials/thermal")]` attribute pins
/// that the panic message names the offending path (substring match — wording
/// tolerant), satisfying TDD's "test fails first" requirement by failing to
/// compile until `assert_no_prelude_pragma_invariant_bidirectional` exists.
#[test]
#[should_panic(expected = "std/materials/thermal")]
fn non_bootstrap_module_with_no_prelude_pragma_panics() {
    // Build a synthetic module set: std/units (bootstrap, pragma OK) plus
    // std/materials/thermal (non-bootstrap, pragma is the planted violation).
    let modules = vec![
        module_with_no_prelude("std.units"),
        module_with_no_prelude("std.materials.thermal"),
    ];

    // Only "std/units" is the bootstrap target in this synthetic set; the
    // other three production targets are omitted because they are not present
    // in the synthetic module slice.  The forward direction checks std/units
    // (passes — pragma is present), then the inverse direction fires on
    // std/materials/thermal (fails — pragma present but path not in targets).
    let targets = ["std/units"];

    // Must panic naming "std/materials/thermal" because thermal is not a
    // bootstrap target yet carries #no_prelude.
    assert_no_prelude_pragma_invariant_bidirectional(&modules, &targets);
}

/// Synthetic fixture: a bootstrap module (`std/units`) that is missing
/// `#no_prelude` must cause the bidirectional invariant helper to panic,
/// naming the offending path in the panic message.
///
/// This exercises the *forward* direction of the invariant: every module
/// in the bootstrap `targets` list must carry `#no_prelude`.  The
/// `#[should_panic(expected = "std/units")]` attribute pins that the panic
/// message names the offending path (substring match — wording tolerant),
/// locking the forward-direction `assert!` branch so a future refactor
/// that accidentally drops or inverts it is caught even when the real
/// stdlib is invariant-compliant.
#[test]
#[should_panic(expected = "std/units")]
fn bootstrap_module_missing_no_prelude_pragma_panics() {
    // Build std/units with NO pragmas — the forward-direction violation.
    let units_module =
        CompiledModuleBuilder::new(ModulePath::from_dotted("std.units").unwrap()).build();

    let modules = vec![units_module];

    // std/units is declared a bootstrap target but carries no #no_prelude;
    // the forward direction must fire and name "std/units".
    let targets = ["std/units"];

    assert_no_prelude_pragma_invariant_bidirectional(&modules, &targets);
}

/// Multi-violation fixture: when TWO non-bootstrap modules both carry
/// `#no_prelude`, the bidirectional invariant helper must name BOTH offending
/// paths in its panic message.
///
/// This is a red test for the "collect-all" refactor of the inverse-direction
/// loop. Before the refactor, the helper panics on the first offender only
/// (`std/materials/thermal`) and never reports `std/geometry/traits`. After
/// the refactor, a single aggregated panic message lists every violator so
/// developers don't have to iterate fix-and-rerun.
#[test]
fn multiple_non_bootstrap_modules_with_no_prelude_pragma_all_named_in_panic() {
    let modules = vec![
        module_with_no_prelude("std.units"), // bootstrap target, pragma OK
        module_with_no_prelude("std.materials.thermal"), // non-bootstrap violation #1
        module_with_no_prelude("std.geometry.traits"), // non-bootstrap violation #2
    ];
    let targets = ["std/units"];

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_no_prelude_pragma_invariant_bidirectional(&modules, &targets);
    }));

    let err = result.expect_err("expected panic naming all offending paths");
    let msg: &str = if let Some(s) = err.downcast_ref::<String>() {
        s.as_str()
    } else if let Some(s) = err.downcast_ref::<&'static str>() {
        s
    } else {
        panic!("panic payload was neither String nor &'static str");
    };

    // Anchor: confirm the panic originates from the aggregated-violations code
    // path, not a stray panic from elsewhere (e.g. an unwrap() inside the helper).
    assert!(
        msg.contains("non-bootstrap stdlib modules"),
        "panic message should be from the aggregated-violations code path, got:\n{msg}"
    );
    assert!(
        msg.contains("std/materials/thermal"),
        "panic message should name 'std/materials/thermal', got:\n{msg}"
    );
    assert!(
        msg.contains("std/geometry/traits"),
        "panic message should name 'std/geometry/traits', got:\n{msg}"
    );
}

// ─── step-2322 / step-2492: bidirectional #no_prelude invariant ─────

/// Assert the `#no_prelude` pragma invariant in both directions:
///
/// **Forward** — every module in `targets` must carry `#no_prelude`.
/// **Inverse** — every module whose path is NOT in `targets` must NOT carry
/// `#no_prelude`.
///
/// Call this helper from both the real-stdlib test and the synthetic-fixture
/// test so that the failure path of the inverse direction can be exercised
/// with a planted violation (a `#[should_panic]` test plants `#no_prelude`
/// on `std/materials/thermal` and expects this helper to name it in the
/// panic message).
///
/// `targets` is the bootstrap-module list: paths of stdlib modules that have
/// ZERO inter-stdlib dependencies and therefore legitimately carry `#no_prelude`.
fn assert_no_prelude_pragma_invariant_bidirectional(
    modules: &[reify_compiler::CompiledModule],
    targets: &[&str],
) {
    // Forward direction: every bootstrap target must carry #no_prelude.
    for target_path in targets {
        let module = modules
            .iter()
            .find(|m| m.path.to_string() == *target_path)
            .unwrap_or_else(|| {
                panic!("stdlib module '{}' not found in load_stdlib()", target_path)
            });

        assert!(
            module.pragmas.iter().any(|p| p.name == "no_prelude"),
            "stdlib module '{}' should carry `#no_prelude` pragma, but none found. \
             pragmas: {:?}",
            target_path,
            module.pragmas
        );
    }

    // Inverse direction: no non-bootstrap module may carry #no_prelude.
    //
    // A spurious #no_prelude on a module like std/materials/thermal silently
    // disables prelude access during compilation, breaking inter-stdlib
    // refinements (e.g. materials_thermal.ri refines MaterialSpec from
    // materials_mechanical.ri). The check is bidirectional so that both
    // adding #no_prelude to the wrong file and removing it from a bootstrap
    // file are caught.
    let mut violations: Vec<(String, &reify_syntax::Pragma)> = Vec::new();
    for module in modules {
        let path_str = module.path.to_string();
        if targets.contains(&path_str.as_str()) {
            continue;
        }
        if let Some(bad_pragma) = module.pragmas.iter().find(|p| p.name == "no_prelude") {
            violations.push((path_str, bad_pragma));
        }
    }
    assert!(
        violations.is_empty(),
        "non-bootstrap stdlib modules carry unauthorized `#no_prelude` pragma:\n{}\n\
         \n\
         Impact: `#no_prelude` silently disables prelude access during compilation, \
         breaking inter-stdlib refinements (e.g. if a module refines a trait from \
         another stdlib file, that trait will be unresolved at compile time).\n\
         \n\
         Fix: remove `#no_prelude` from the .ri source for each listed path. \
         If a module truly has ZERO inter-stdlib dependencies and should \
         be a bootstrap module, add its path to the `targets` list in \
         `prelude_modules_carry_no_prelude_pragma` AND keep the pragma.",
        violations
            .iter()
            .map(|(path, pragma)| format!("  - '{}' (pragma: {:?})", path, pragma))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// The four stdlib modules that have no inter-stdlib dependencies must carry
/// `#no_prelude` as a self-documenting bootstrap directive, and no other
/// stdlib module may carry it (bidirectional invariant).
///
/// Target modules (only built-in dims + hardcoded-fallback units, no prelude dep):
///   - std/units, std/materials/mechanical, std/analysis, std/tolerancing
///
/// Asserts via `module.pragmas` so the full parse→load pipeline is exercised
/// (a typo like `#no-prelude` would make the parser skip the pragma).
///
/// The invariant is enforced in both directions by
/// `assert_no_prelude_pragma_invariant_bidirectional`: any module in `targets`
/// must carry `#no_prelude`, and any module NOT in `targets` must not.
#[test]
fn prelude_modules_carry_no_prelude_pragma() {
    let modules = stdlib_loader::load_stdlib();

    // Invariant: a stdlib module belongs in this list if and only if it has
    // ZERO inter-stdlib dependencies — i.e. it references only built-in
    // dimension types (Length, Angle, …), built-in primitives (Real, Int,
    // String), and units from the hardcoded `unit_to_scalar` fallback table
    // in `crates/reify-compiler/src/units.rs` (mm, cm, m, in, deg, rad, kg,
    // g, s).  Modules that refine or reference a trait/type first defined in
    // another stdlib file (e.g. materials_thermal.ri refines `MaterialSpec`
    // from materials_mechanical.ri) must NOT be added here.
    //
    // If you add a new stdlib .ri file that meets the invariant above, add it
    // here AND add `#no_prelude` to its source.  If you add an inter-stdlib
    // dependency to one of these four files, remove it from this list AND
    // remove `#no_prelude` from its source (see Task 2322 design decision).
    let targets = [
        "std/units",
        "std/materials/mechanical",
        "std/analysis",
        "std/tolerancing",
    ];

    assert_no_prelude_pragma_invariant_bidirectional(modules, &targets);
}

// ─── step-3: compile_with_prelude makes prelude traits visible ──────

/// compile_with_prelude() makes prelude traits visible to user code.
/// A structure conforming to the prelude's Elastic trait compiles without
/// errors and has 'Elastic' in trait_bounds.
#[test]
fn compile_with_prelude_makes_traits_visible() {
    let source = steel_elastic_source();
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "compile_with_prelude should produce no errors for Elastic-conforming Steel, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");
    assert!(
        template.trait_bounds.contains(&"Elastic".to_string()),
        "Steel should have 'Elastic' trait bound, got: {:?}",
        template.trait_bounds
    );
}

// ─── step-5: compile_with_prelude injects trait constraint defaults ──

/// compile_with_prelude injects trait constraint defaults from the prelude.
/// A structure conforming to the prelude's Strong trait gets the
/// `uts >= yield_strength` constraint injected. Verifies both presence
/// and content of the injected constraint.
#[test]
fn compile_with_prelude_injects_trait_constraints() {
    let source = steel_strong_source();
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "compile_with_prelude should produce no errors for Strong-conforming Steel, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");
    assert!(
        !template.constraints.is_empty(),
        "expected constraint from Strong trait (uts >= yield_strength) injected into Steel, but constraints is empty"
    );

    // Structurally verify the constraint encodes uts >= yield_strength.
    // Pattern-match on CompiledExprKind variants rather than relying on Debug formatting.
    let ge_constraint = template
        .constraints
        .iter()
        .find(|c| matches!(&c.expr.kind, CompiledExprKind::BinOp { op: BinOp::Ge, .. }));
    assert!(
        ge_constraint.is_some(),
        "expected a >= constraint from Strong trait, got constraint kinds: {:?}",
        template
            .constraints
            .iter()
            .map(|c| format!("{:?}", c.expr.kind))
            .collect::<Vec<_>>()
    );
    let ge_expr = &ge_constraint.unwrap().expr;
    let refs = collect_value_ref_members(ge_expr);
    assert!(
        refs.contains(&"uts"),
        "expected 'uts' ValueRef in >= constraint, got refs: {:?}",
        refs
    );
    assert!(
        refs.contains(&"yield_strength"),
        "expected 'yield_strength' ValueRef in >= constraint, got refs: {:?}",
        refs
    );
}

// ─── negative tests: compiling without prelude must produce errors ────

/// Compiling Steel:Elastic source WITHOUT the prelude should produce ≥1
/// error diagnostic, proving the prelude is required for trait resolution.
#[test]
fn compile_without_prelude_errors_for_elastic() {
    let source = steel_elastic_source();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one compile error when compiling Steel:Elastic without prelude, \
         but no errors were produced"
    );
}

/// Compiling Steel:Strong source WITHOUT the prelude should produce ≥1
/// error diagnostic, proving the prelude is required for trait resolution.
#[test]
fn compile_without_prelude_errors_for_strong() {
    let source = steel_strong_source();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one compile error when compiling Steel:Strong without prelude, \
         but no errors were produced"
    );
}

// ─── prelude exclusion: prelude defs must not leak into output ────────

/// Prelude definitions (traits, enums, units) should NOT appear in the
/// output CompiledModule when compiling user code via compile_with_prelude.
/// Only user-defined content (Steel template) should be present.
#[test]
fn prelude_definitions_excluded_from_output_module() {
    let source = steel_elastic_source();
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // The output module should NOT contain any prelude trait_defs.
    // The user source only defines a structure, not any traits.
    assert!(
        compiled.trait_defs.is_empty(),
        "output module should not contain prelude trait_defs, but found: {:?}",
        compiled
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );

    // The output module should NOT contain prelude enum_defs (e.g., HardnessScale).
    assert!(
        compiled.enum_defs.is_empty(),
        "output module should not contain prelude enum_defs, but found: {:?}",
        compiled
            .enum_defs
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );

    // The output module should NOT contain prelude units (cm, m, kg, etc.).
    assert!(
        compiled.units.is_empty(),
        "output module should not contain prelude units, but found: {:?}",
        compiled.units.iter().map(|u| &u.name).collect::<Vec<_>>()
    );

    // User content (Steel template) SHOULD be present.
    assert!(
        !compiled.templates.is_empty(),
        "output module should contain the user's Steel template"
    );
}

// ─── enum coverage: HardnessScale ────────────────────────────────────

/// HardnessScale enum from materials_mechanical.ri should be present in
/// the stdlib with exactly 7 variants.
#[test]
fn hardness_scale_enum_present_in_stdlib() {
    let modules = stdlib_loader::load_stdlib();

    // Collect all enum_defs across all stdlib modules.
    let all_enums: Vec<_> = modules.iter().flat_map(|m| m.enum_defs.iter()).collect();

    let hardness = all_enums
        .iter()
        .find(|e| e.name == "HardnessScale")
        .expect("HardnessScale enum should exist in stdlib");

    let expected_variants = [
        "Rockwell_A",
        "Rockwell_B",
        "Rockwell_C",
        "Brinell",
        "Vickers",
        "Shore_A",
        "Shore_D",
    ];

    assert_eq!(
        hardness.variants.len(),
        expected_variants.len(),
        "HardnessScale should have {} variants, got {}: {:?}",
        expected_variants.len(),
        hardness.variants.len(),
        hardness.variants
    );

    for variant in &expected_variants {
        assert!(
            hardness.variants.contains(&variant.to_string()),
            "HardnessScale should contain variant '{}', found: {:?}",
            variant,
            hardness.variants
        );
    }
}

// ─── function-merging path ───────────────────────────────────────────

/// Prelude functions are resolved during compilation: user code that calls
/// a function defined in a prelude module compiles without errors.
/// This test exercises the function-merging path using a synthetic prelude
/// module (no stdlib modules currently define functions).
#[test]
fn prelude_function_merging_path() {
    // Build a synthetic prelude module containing a single function: double(x: Real) -> Real
    let params = vec![("x".to_string(), Type::Real)];
    let double_fn = CompiledFunction {
        name: "double".to_string(),
        is_pub: true,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Real,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr {
                kind: CompiledExprKind::Literal(reify_types::Value::Real(0.0)),
                result_type: Type::Real,
                content_hash: ContentHash::of_str("double_stub"),
            },
        },
        content_hash: ContentHash::of_str("fn_double"),
        annotations: vec![],
        optimized_target: None,
    };

    let synthetic_prelude = CompiledModuleBuilder::new(ModulePath::single("synthetic"))
        .function(double_fn)
        .build();

    // User code that calls the prelude function.
    // Note: 21.5 (not 21.0) to ensure the literal is inferred as Real, not Int.
    // The Reify compiler infers whole-number literals as Int; fractional as Real.
    let source = r#"
structure def S {
    param x : Real = double(21.5)
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, &[synthetic_prelude]);
    let errors = collect_errors(&compiled.diagnostics);

    // Prelude functions are resolved during compilation — no errors expected.
    assert!(
        errors.is_empty(),
        "compile_with_prelude should resolve prelude function 'double', got errors: {:?}",
        errors
    );

    // The output module should contain the user's template.
    let template = compiled
        .templates
        .first()
        .expect("output module should contain the user's S template");

    // Verify param 'x' has a default expression that is a call to 'double'.
    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("template S should have param 'x'");
    let default_expr = x_cell
        .default_expr
        .as_ref()
        .expect("param 'x' should have a default expression");
    // Prelude (user-defined) functions compile to UserFunctionCall, not FunctionCall.
    // FunctionCall is reserved for built-in stdlib functions resolved at compile time.
    match &default_expr.kind {
        CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(
                function_name, "double",
                "expected resolved call to 'double'"
            );
        }
        other => {
            panic!(
                "param 'x' default should be a UserFunctionCall to 'double', got: {:?}",
                other
            );
        }
    }

    // Prelude functions should NOT be duplicated in the output module.
    assert!(
        compiled.functions.is_empty(),
        "output module should not contain prelude function 'double', but found: {:?}",
        compiled
            .functions
            .iter()
            .map(|f| &f.name)
            .collect::<Vec<_>>()
    );
}
