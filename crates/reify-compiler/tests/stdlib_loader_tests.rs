//! Tests for stdlib_loader — embedded .ri stdlib loading, compilation, and caching.

use reify_ast::Pragma;
use reify_compiler::stdlib_loader;
use reify_core::{ContentHash, ModulePath, SourceSpan, Type};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, CompiledFnBody, CompiledFunction};
use reify_test_support::{
    CompiledModuleBuilder, EXPECTED_GEOMETRY_SUPERTRAITS, EXPECTED_GEOMETRY_TRAITS,
    EXPECTED_MATERIAL_TRAITS, collect_errors, collect_value_ref_members, steel_elastic_source,
    steel_strong_source,
};

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

// ─── task-4016 ζ: determinacy_purposes load-order invariant ─────────
//
// std.determinacy.purposes MUST be the last entry in the stdlib module list.
// The sequential prelude build runs merge_prelude_purposes for every compile
// including each intra-stdlib module compile, but no-ops because
// std.determinacy.purposes is the only module with pub purposes and it is
// registered last — no later stdlib module sees it as a prelude during
// load_stdlib(), keeping stdlib-internal count/hash goldens byte-stable.
//
// These two tests make the prose invariant machine-checkable so that a future
// contributor appending a module after std.determinacy.purposes is caught
// immediately rather than by a silent golden drift.

/// std.determinacy.purposes must be the last module compiled by load_stdlib().
///
/// If this fails it means a new stdlib module was appended AFTER
/// std.determinacy.purposes in stdlib_loader.rs — move it before the
/// determinacy module, or the sequential merge will leak standard purposes
/// into that later module during intra-stdlib compilation.
#[test]
fn std_determinacy_purposes_is_last_stdlib_module() {
    let modules = stdlib_loader::load_stdlib();
    assert!(
        !modules.is_empty(),
        "stdlib should have at least one module"
    );
    let last = modules.last().unwrap();
    let path_str = format!("{}", last.path);
    assert_eq!(
        path_str,
        "std/determinacy/purposes",
        "std.determinacy.purposes must be the LAST stdlib module compiled \
         (invariant: no later stdlib module may inherit its pub purposes \
         via the sequential merge). \
         Got last module: '{}'. \
         Full order: {:?}",
        path_str,
        modules
            .iter()
            .map(|m| format!("{}", m.path))
            .collect::<Vec<_>>()
    );
}

/// No stdlib module other than std.determinacy.purposes may have a compiled
/// purpose named "simulation_ready" or "design_review" in its compiled_purposes.
///
/// If this fails it means a module compiled AFTER std.determinacy.purposes
/// (or one that somehow directly declares these names) is inheriting/re-declaring
/// the standard purposes, which would break the 'stdlib-internal counts stay
/// stable' assumption and could cause golden test churn across the stdlib.
#[test]
fn no_stdlib_module_inherits_standard_purposes() {
    let modules = stdlib_loader::load_stdlib();
    let standard_purpose_names = ["simulation_ready", "design_review"];

    for module in modules {
        let path_str = format!("{}", module.path);
        if path_str == "std/determinacy/purposes" {
            // The declaring module itself is expected to have them.
            continue;
        }
        for purpose in &module.compiled_purposes {
            assert!(
                !standard_purpose_names.contains(&purpose.name.as_str()),
                "stdlib module '{}' unexpectedly has a purpose named '{}' in \
                 its compiled_purposes. Standard purposes (simulation_ready, \
                 design_review) must only appear in std.determinacy.purposes \
                 and in user modules compiled against the full stdlib. \
                 Check that std.determinacy.purposes is still the LAST entry \
                 in stdlib_loader.rs's sources vec.",
                path_str,
                purpose.name
            );
        }
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

/// `std.geometry.traits` contains exactly the union of the inferred-marker set
/// (`EXPECTED_GEOMETRY_TRAITS`) and the §3.10 supertrait set
/// (`EXPECTED_GEOMETRY_SUPERTRAITS`) — same names, same total count. Single
/// source of truth for the geometry trait set; per-module
/// `geometry_traits_tests.rs` delegates to this rather than re-asserting names
/// locally. Scoped to the geometry module specifically so the count assertion
/// is meaningful (a flat cross-module count would not be).
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

    let expected_total = EXPECTED_GEOMETRY_TRAITS.len() + EXPECTED_GEOMETRY_SUPERTRAITS.len();
    assert_eq!(
        trait_names.len(),
        expected_total,
        "std.geometry.traits should contain exactly {} traits ({} markers + {} supertraits), \
         got {}: {:?}",
        expected_total,
        EXPECTED_GEOMETRY_TRAITS.len(),
        EXPECTED_GEOMETRY_SUPERTRAITS.len(),
        trait_names.len(),
        trait_names
    );

    for name in EXPECTED_GEOMETRY_TRAITS
        .iter()
        .chain(EXPECTED_GEOMETRY_SUPERTRAITS)
    {
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
    assert_eq!(cm.dimension, reify_core::DimensionVector::LENGTH);
    assert!((cm.factor - 0.01).abs() < 1e-12);

    let deg = units_module.units.iter().find(|u| u.name == "deg").unwrap();
    assert_eq!(deg.dimension, reify_core::DimensionVector::ANGLE);
    assert!(
        (deg.factor - std::f64::consts::PI / 180.0).abs() < 1e-15,
        "deg factor should be PI/180, got {}",
        deg.factor
    );

    let kg = units_module.units.iter().find(|u| u.name == "kg").unwrap();
    assert_eq!(kg.dimension, reify_core::DimensionVector::MASS);
    assert!((kg.factor - 1.0).abs() < 1e-12);

    let s = units_module.units.iter().find(|u| u.name == "s").unwrap();
    assert_eq!(s.dimension, reify_core::DimensionVector::TIME);
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
    let mut violations: Vec<(String, &reify_ast::Pragma)> = Vec::new();
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
    // NOTE: `std/materials/mechanical` was REMOVED from this list by task β
    // (#4761). It now declares `structure def Material : Visual` with a
    // `param appearance : Appearance = Appearance()`, both resolved from
    // `std.materials.appearance` (an inter-stdlib dependency). Per the
    // invariant above, a module with an inter-stdlib dependency must NOT
    // carry `#no_prelude` and must NOT be a bootstrap target.
    // NOTE: `std/result` was ADDED to this list by task B-α (#4035). It
    // declares only the generic enum `Result<T, E> { Ok{value:T}, Err{error:E} }`,
    // which references nothing but its own type params — zero inter-stdlib
    // dependencies, same as `std/option_recovery`.
    let targets = [
        "std/units",
        "std/analysis",
        "std/tolerancing",
        "std/fields",
        "std/option_recovery",
        "std/result",
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
/// `ultimate_tensile_strength >= yield_strength` constraint injected. Verifies
/// both presence and content of the injected constraint.
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
        "expected constraint from Strong trait (ultimate_tensile_strength >= yield_strength) injected into Steel, but constraints is empty"
    );

    // Structurally verify the constraint encodes ultimate_tensile_strength >= yield_strength.
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
        refs.iter()
            .any(|m| m.as_str() == "ultimate_tensile_strength"),
        "expected 'ultimate_tensile_strength' ValueRef in >= constraint, got refs: {:?}",
        refs
    );
    assert!(
        refs.iter().any(|m| m.as_str() == "yield_strength"),
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
            hardness.variants.iter().any(|v| v.name == *variant),
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
    let params = vec![("x".to_string(), Type::dimensionless_scalar())];
    let double_fn = CompiledFunction {
        name: "double".to_string(),
        doc: None,
        is_pub: true,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::dimensionless_scalar(),
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr {
                kind: CompiledExprKind::Literal(reify_ir::Value::Real(0.0)),
                result_type: Type::dimensionless_scalar(),
                content_hash: ContentHash::of_str("double_stub"),
            },
        },
        content_hash: ContentHash::of_str("fn_double"),
        annotations: vec![],
        optimized_target: None,
        type_params: vec![],
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

// ─── task 5496 (stdlib-namespace β): NS-P2 intra-stdlib collision gate ───────
//
// PRD docs/prds/v0_6/stdlib-namespace.md §7 boundary #3. The observable the PRD
// asks for is "the stdlib BUILD fails", not "a helper returns findings", so
// these tests drive a synthetic source set through
// `stdlib_loader::build_stdlib_modules` — the exact function
// `load_stdlib()` delegates to. The stdlib is `include_str!`-embedded, so no
// user-level `.ri` fixture can inject a duplicate stdlib module; a synthetic
// source set through the production entry point is the only way to observe the
// gate fire.

/// Drive `build_stdlib_modules` on `sources` and return the panic message.
///
/// Panics (failing the test) if the build did NOT panic, or if the payload is
/// neither `String` nor `&'static str`. Downcast idiom mirrors
/// `multiple_non_bootstrap_modules_with_no_prelude_pragma_all_named_in_panic`
/// above.
fn stdlib_build_panic_message(sources: &[(&str, &str)]) -> String {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        stdlib_loader::build_stdlib_modules(sources);
    }));
    let err = result.expect_err("build_stdlib_modules should have panicked, but returned Ok");
    if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else {
        panic!("panic payload was neither String nor &'static str");
    }
}

/// Assert that building `sources` panics with the NS-P2 collision message, and
/// that the message names the kind, the colliding name, and BOTH modules.
///
/// Message-CONTENT matching is the point. A bare "it panicked" assertion would
/// pass vacuously on any stdlib build failure — a cycle, an Error diagnostic, an
/// unwrap deep inside the compiler — and would therefore keep passing if the
/// collision scan were deleted outright.
#[track_caller]
fn assert_collision_panic(
    sources: &[(&str, &str)],
    kind: &str,
    name: &str,
    module_a: &str,
    module_b: &str,
) {
    let msg = stdlib_build_panic_message(sources);
    for needle in [NS_P2_PANIC_ANCHOR, kind, name, module_a, module_b] {
        assert!(
            msg.contains(needle),
            "NS-P2 collision panic should name {needle:?}; got:\n{msg}"
        );
    }
}

/// Stable anchor substring identifying the NS-P2 code path. Kept as a constant
/// so every collision test agrees on it and a reworded message is a one-line fix.
const NS_P2_PANIC_ANCHOR: &str = "intra-stdlib pub-name collision";

/// Boundary #3: two stdlib modules declaring the same `structure def` name must
/// fail the stdlib BUILD, with a message naming both modules.
///
/// The synthetic set reproduces exactly the collision the real corpus carried
/// until this task's rename: `structure def Mode` in two modules, with
/// DIFFERENT member sets, which is what made the pre-state dangerous (the two
/// resolution phases silently disagreed about the winner).
///
/// Both modules carry `#no_prelude` so they compile against builtins only. That
/// matters for the same reason it matters in the bootstrap-invariant tests
/// above: without it the sources would draw Error diagnostics and the build's
/// pre-existing Error-diagnostic assert would fire FIRST, so the test would
/// observe the wrong panic and pass for the wrong reason.
#[test]
fn stdlib_build_with_injected_duplicate_structure_name_panics_naming_both_modules() {
    const DUP_A: &str = "#no_prelude\nstructure def Mode {\n    param eigenvalue : Real\n}\n";
    const DUP_B: &str = "#no_prelude\nstructure def Mode {\n    param frequency : Real\n}\n";
    let sources: &[(&str, &str)] = &[("std.test.dup_a", DUP_A), ("std.test.dup_b", DUP_B)];

    assert_collision_panic(
        sources,
        "structure",
        "Mode",
        "std.test.dup_a",
        "std.test.dup_b",
    );
}

/// The positive counterpart: the REAL stdlib must still build. This is what
/// proves the gate is wired into the production path rather than only reachable
/// from a synthetic test set — if the scan were too coarse (kind-agnostic, or
/// keying functions by name alone), this test goes red on the live 47-file
/// corpus while the injected-duplicate test above stays green.
#[test]
fn real_stdlib_build_is_collision_free() {
    let modules = stdlib_loader::load_stdlib();
    assert!(
        !modules.is_empty(),
        "load_stdlib() must return modules; the NS-P2 gate must not fire on the real corpus"
    );

    let paths: Vec<String> = modules.iter().map(|m| m.path.to_string()).collect();
    for expected in ["std/solver/buckling", "std/modal/analysis"] {
        assert!(
            paths.iter().any(|p| p == expected),
            "real stdlib should still contain {expected}; got: {paths:?}"
        );
    }
}

// ─── task 5496: NS-P3 kind uniformity + the live-corpus carve-outs ───────────
//
// Every case below is a two-module `#no_prelude` synthetic set driven through
// `build_stdlib_modules`, the same production entry point boundary #3 uses.
//
// The NEGATIVE cases (g)/(h)/(i) are the load-bearing half. Each mirrors a
// declaration that is LIVE in the real stdlib as of 2026-07-28, so each is a
// guard against a future implementer coarsening the scan and detonating the
// real stdlib build:
//
//   (g) cross-module function OVERLOADS — `unwrap_or` (option_recovery.ri:37 vs
//       result.ri:52), `or_else` (:42 vs :57), `fallback` (:56 vs :75). Three
//       real pairs. Keying functions by name alone panics the real build.
//   (h) cross-KIND name reuse — `structure def Planar` (kinematic.ri:163) vs
//       `trait Planar {}` (geometry_traits.ri:56).
//   (i) ASSOCIATED types are not module-level type aliases — `type MotionValue`
//       is declared in `trait HasMotion` (kinematic.ri:110) and bound in
//       `Prismatic` (:133), `Revolute` (:151) and `Coupling` (:198). A syntactic
//       `^\s*(pub )?type <name>` scan sees four `MotionValue`s and panics on day
//       one; reading `CompiledModule.type_aliases` (module-level only) does not.
//
// Deliberately NOT covered: INTRA-module duplicates. `displacement_at` x2
// (modal_analysis_fns.ri:153,193), `solve_elastic_static` x3
// (solver_elastic.ri:727,768,809) and `solve_load_cases` x2
// (fea_multi_case.ri:653,700) are all same-module overloads, out of a
// cross-module scan's reach by construction, and remain the business of the
// per-module duplicate path at ctx.rs:157.

/// Assert that building `sources` does NOT panic — the negative counterpart of
/// [`assert_collision_panic`].
///
/// `catch_unwind` rather than a bare call so a failure reports the panic message
/// (which distinguishes "the scan is too coarse" from "the synthetic source
/// failed to compile") instead of just aborting the test binary's thread.
#[track_caller]
fn assert_no_collision_panic(sources: &[(&str, &str)], why: &str) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        stdlib_loader::build_stdlib_modules(sources);
    }));
    if let Err(err) = result {
        let msg = err
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&'static str>().map(|s| (*s).to_string()))
            .unwrap_or_else(|| "<non-string panic payload>".to_string());
        panic!("stdlib build should have accepted {why}; got:\n{msg}");
    }
}

/// Build a two-module source set from two `#no_prelude` bodies.
fn two_modules<'a>(a: &'a str, b: &'a str) -> [(&'a str, &'a str); 2] {
    [("std.test.dup_a", a), ("std.test.dup_b", b)]
}

// ─── (a)-(f): every remaining kind must collide ──────────────────────────────

#[test]
fn duplicate_enum_name_across_modules_fails_the_stdlib_build() {
    let sources = two_modules(
        "#no_prelude\npub enum Grade { A, B }\n",
        "#no_prelude\npub enum Grade { C, D }\n",
    );
    assert_collision_panic(
        &sources,
        "enum",
        "Grade",
        "std.test.dup_a",
        "std.test.dup_b",
    );
}

#[test]
fn duplicate_trait_name_across_modules_fails_the_stdlib_build() {
    let sources = two_modules(
        "#no_prelude\ntrait Foo { }\n",
        "#no_prelude\ntrait Foo { }\n",
    );
    assert_collision_panic(&sources, "trait", "Foo", "std.test.dup_a", "std.test.dup_b");
}

#[test]
fn duplicate_unit_symbol_across_modules_fails_the_stdlib_build() {
    let sources = two_modules(
        "#no_prelude\npub unit zz : Length = 1.0\n",
        "#no_prelude\npub unit zz : Length = 2.0\n",
    );
    assert_collision_panic(&sources, "unit", "zz", "std.test.dup_a", "std.test.dup_b");
}

#[test]
fn duplicate_top_level_type_alias_across_modules_fails_the_stdlib_build() {
    let sources = two_modules(
        "#no_prelude\npub type Alias = Real\n",
        "#no_prelude\npub type Alias = Int\n",
    );
    assert_collision_panic(
        &sources,
        "type alias",
        "Alias",
        "std.test.dup_a",
        "std.test.dup_b",
    );
}

#[test]
fn duplicate_constraint_def_name_across_modules_fails_the_stdlib_build() {
    let body = "#no_prelude\npub constraint def Bounded {\n    param v : Real\n    constraint v >= 0\n}\n";
    let sources = two_modules(body, body);
    assert_collision_panic(
        &sources,
        "constraint def",
        "Bounded",
        "std.test.dup_a",
        "std.test.dup_b",
    );
}

#[test]
fn duplicate_purpose_name_across_modules_fails_the_stdlib_build() {
    let body = "#no_prelude\npub purpose ready(subject : Structure) {\n    constraint forall p in subject.geometric_params: determined(p)\n}\n";
    let sources = two_modules(body, body);
    assert_collision_panic(
        &sources,
        "purpose",
        "ready",
        "std.test.dup_a",
        "std.test.dup_b",
    );
}

/// (f) Same function name AND identical signature in two modules is a genuine
/// collision — one silently shadows the other today (lib.rs first-wins, with a
/// comment deferring to "stdlib-level review"; this gate replaces that).
///
/// This case is also what forbids keying functions by `content_hash`: the two
/// bodies below differ, so a body-derived key would hash them apart and miss
/// the collision entirely — exactly the hole this task closes.
#[test]
fn duplicate_function_with_identical_signature_across_modules_fails_the_stdlib_build() {
    let sources = two_modules(
        "#no_prelude\npub fn twin(x : Real) -> Real {\n    x\n}\n",
        "#no_prelude\npub fn twin(x : Real) -> Real {\n    x + 1.0\n}\n",
    );
    assert_collision_panic(&sources, "function", "twin", "std.test.dup_a", "std.test.dup_b");
}

// ─── (g)-(i): the three carve-outs the LIVE stdlib depends on ────────────────

/// (g) Cross-module function OVERLOADS must be accepted. Mirrors `unwrap_or` /
/// `or_else` / `fallback`, each declared in both `option_recovery.ri` and
/// `result.ri` with different first-parameter types.
///
/// NS-P3 keys functions by name + SIGNATURE (param types in order, plus return
/// type). A name-only key panics the real stdlib build on three separate pairs.
#[test]
fn cross_module_function_overloads_are_accepted() {
    let sources = two_modules(
        "#no_prelude\npub fn recover(x : Real, dflt : Real) -> Real {\n    x\n}\n",
        "#no_prelude\npub fn recover(x : Int, dflt : Real) -> Real {\n    dflt\n}\n",
    );
    assert_no_collision_panic(
        &sources,
        "cross-module function overloads differing in parameter type \
         (live: unwrap_or/or_else/fallback in option_recovery.ri vs result.ri)",
    );
}

/// (h) Cross-KIND name reuse must be accepted — per-kind namespaces. Mirrors
/// `structure def Planar` (kinematic.ri:163) and `trait Planar {}`
/// (geometry_traits.ri:56). A kind-agnostic scan would force an out-of-scope
/// rename of `Planar`.
#[test]
fn cross_kind_name_reuse_is_accepted() {
    let sources = two_modules(
        "#no_prelude\nstructure def Planar {\n    param v : Real\n}\n",
        "#no_prelude\ntrait Planar { }\n",
    );
    assert_no_collision_panic(
        &sources,
        "the same name used for a structure in one module and a trait in another \
         (live: kinematic.ri Planar vs geometry_traits.ri Planar)",
    );
}

/// (i) ASSOCIATED types are members of their template, not module-level
/// aliases, so the same associated-type name in two modules must be accepted.
/// Mirrors `type MotionValue` in `trait HasMotion` (kinematic.ri:110) and its
/// bindings in `Prismatic` / `Revolute` / `Coupling`.
///
/// This is the case that makes "scan the materialized surface, not the syntax"
/// enforceable rather than advisory: a syntactic `^\s*(pub )?type <name>` scan
/// reports four duplicate `MotionValue`s and panics the real stdlib build
/// immediately, while `CompiledModule.type_aliases` holds only module-level
/// aliases and correctly reports none.
#[test]
fn associated_types_are_not_module_level_type_aliases() {
    let sources = two_modules(
        "#no_prelude\ntrait HasMotion {\n    type MotionValue\n}\n",
        "#no_prelude\ntrait AlsoHasMotion {\n    type MotionValue\n}\n",
    );
    assert_no_collision_panic(
        &sources,
        "the same ASSOCIATED type name declared inside two different traits \
         (live: MotionValue in kinematic.ri HasMotion/Prismatic/Revolute/Coupling)",
    );
}
