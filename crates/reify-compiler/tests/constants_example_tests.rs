//! Regression tests for `examples/stdlib/constants.ri` (tasks 4026, 4176).
//!
//! Both tests share a single parse → compile-clean gate via `load_constants_example`.
//!
//! Test 1 — compile-clean + leaf-signal pins:
//!   1. The file parses with zero errors.
//!   2. It compiles under the stdlib prelude with zero Error-severity diagnostics.
//!   3. The compiled module exposes a `PhysicalConstants` structure template.
//!   4. Positive source-text pins: originally-showcased constants appear in source.
//!   5. Negative source-text pins: SI digit sequences must NOT appear in source.
//!
//! Test 2 — eval cross-check assertions (task 4176):
//!   6. Eval produces zero Error diagnostics.
//!   7. `circ` ≈ 2π (proves pi resolves correctly).
//!   8. `euler` ≈ e (proves e resolves correctly).
//!   9. |r_check| < 1e-6  — R ≈ N_A·k_B (gas-constant identity).
//!  10. em_check is dimensionless AND ≈ 1 — ε₀μ₀c² ≈ 1 (EM identity).
//!
//! Pattern lifted from `multi_load_bracket_example_tests.rs` (task 3587);
//! eval pipeline from `crates/reify-eval/tests/m8_3_stdlib_integration.rs`.
//! PRD references: `docs/prds/v0_6/stdlib-reconstruction.md` task ζ,
//!                 `docs/prds/v0_6/units-physical-constants.md` §7 task δ.

use reify_compiler::CompiledModule;
use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::make_simple_engine;

// ─── Shared setup helper ──────────────────────────────────────────────────────

/// Read, parse, and compile `examples/stdlib/constants.ri`, asserting zero
/// `Severity::Error` diagnostics at each stage.  Returns `(src, compiled)`
/// so callers can inspect the source text and compiled module independently.
///
/// Mirrors the `compiled_ri` helper in `crates/reify-eval/tests/m8_3_stdlib_integration.rs`.
fn load_constants_example() -> (String, CompiledModule) {
    const EXAMPLE_PATH: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/stdlib/constants.ri");

    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/stdlib/constants.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists",
    );

    let parsed = reify_syntax::parse(&src, ModulePath::single("constants"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in examples/stdlib/constants.ri: {:?}",
        parsed.errors
    );

    let module = reify_compiler::compile_with_stdlib(&parsed);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling examples/stdlib/constants.ri under stdlib \
         (a wrong-dimension constant causes a cross-check `-`/`*` to emit \
         'dimension mismatch' here):\n{:#?}",
        errors
    );

    (src, module)
}

// ─── examples/stdlib/constants.ri compiles clean and pins leaf signals ─────

/// `examples/stdlib/constants.ri` must parse, compile under the stdlib
/// prelude with zero Error diagnostics, expose a `PhysicalConstants`
/// structure template, reference the originally-showcased constants by name,
/// and contain no inline magic numbers (`299792458` / `1380649` / new 4176
/// digit sequences).
///
/// Path resolution uses `CARGO_MANIFEST_DIR` so it works in any worktree.
#[test]
fn constants_example_compiles_under_stdlib_with_zero_errors_and_pins_constant_references() {
    let (src, module) = load_constants_example();

    // ── Template presence ──────────────────────────────────────────────────────

    assert!(
        module.templates.iter().any(|t| t.name == "PhysicalConstants"),
        "expected a 'PhysicalConstants' structure template in compiled constants.ri; \
         found templates: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    // ── Positive source-text leaf-signal pins ──────────────────────────────────
    //
    // The originally-showcased constants must appear by name in the example so
    // a reader can discover them. The compile-clean assertion (in
    // `load_constants_example`) already proves the names resolved; source-text
    // pins additionally confirm they are not hidden inside dead or removed code.
    //
    // The 8 newer constants added in task 4176 (tau, AVOGADRO_CONSTANT,
    // PLANCK_CONSTANT, STEFAN_BOLTZMANN_CONSTANT, VACUUM_PERMITTIVITY,
    // VACUUM_PERMEABILITY, MOLAR_GAS_CONSTANT, ELEMENTARY_CHARGE) are NOT
    // source-text pinned here — the header comment in constants.ri lists each
    // by name, so `src.contains()` would be satisfied even if the corresponding
    // bindings were deleted. Compile-clean is sufficient to prove resolution for
    // those; the eval cross-checks (test 2) additionally prove value correctness.

    assert!(
        src.contains("SPEED_OF_LIGHT"),
        "constants.ri must reference SPEED_OF_LIGHT"
    );
    assert!(
        src.contains("BOLTZMANN_CONSTANT"),
        "constants.ri must reference BOLTZMANN_CONSTANT"
    );

    // ── Negative source-text pins (no inline magic numbers) ───────────────────
    //
    // Per design decision 4: comments must describe the constant's *role*,
    // not echo its SI numeric value. A substring check on the raw digit
    // sequences catches any reconstruction of the SI value regardless of
    // identifier choice. `1380649` matches both the decimal literal
    // `0.00000000000000000000001380649` and any inline `1.380649e-23` variant.
    //
    // Note: abbreviated literals (e.g. "6.022e23" for Avogadro, "6.63e-34" for
    // Planck) are NOT caught by these guards — the pins match full-precision
    // substrings only. This is an accepted partial guard; the meaningful
    // protection comes from compile-clean (name resolution) plus the eval
    // cross-checks (value correctness).
    //
    // Pattern from multi_load_bracket_example_tests.rs:185-194.

    assert!(
        !src.contains("299792458"),
        "constants.ri must NOT contain the magic number '299792458' inline — \
         use SPEED_OF_LIGHT() instead"
    );
    assert!(
        !src.contains("1380649"),
        "constants.ri must NOT contain the magic number '1380649' inline — \
         use BOLTZMANN_CONSTANT() instead"
    );

    // New constants (task 4176):
    assert!(
        !src.contains("60221"),
        "constants.ri must NOT contain '60221' inline — use AVOGADRO_CONSTANT() instead"
    );
    assert!(
        !src.contains("662607015"),
        "constants.ri must NOT contain '662607015' inline — use PLANCK_CONSTANT() instead"
    );
    assert!(
        !src.contains("5670374419"),
        "constants.ri must NOT contain '5670374419' inline — \
         use STEFAN_BOLTZMANN_CONSTANT() instead"
    );
    assert!(
        !src.contains("88541878128"),
        "constants.ri must NOT contain '88541878128' inline — use VACUUM_PERMITTIVITY() instead"
    );
    assert!(
        !src.contains("125663706212"),
        "constants.ri must NOT contain '125663706212' inline — use VACUUM_PERMEABILITY() instead"
    );
    assert!(
        !src.contains("8314462618"),
        "constants.ri must NOT contain '8314462618' inline — use MOLAR_GAS_CONSTANT() instead"
    );
    assert!(
        !src.contains("1602176634"),
        "constants.ri must NOT contain '1602176634' inline — use ELEMENTARY_CHARGE() instead"
    );
}

// ─── eval cross-check: physics-identity assertions (task 4176) ────────────────

/// Evaluates `examples/stdlib/constants.ri` end-to-end and asserts that the
/// four cross-check fields in `PhysicalConstants` satisfy physics-identity
/// tolerance bounds (task δ, PRD §3.8).
///
/// Cross-checks:
///   circ  = 2.0 * pi          → ≈ 2π  (proves pi resolves and is correct)
///   euler = e                  → ≈ e   (proves e resolves and is correct)
///   r_check = MOLAR_GAS_CONSTANT() - AVOGADRO_CONSTANT() * BOLTZMANN_CONSTANT()
///             → |r_check| < 1e-6  (R = N_A·k_B; residual ≈ −1.53e-10)
///   em_check = VACUUM_PERMITTIVITY() * VACUUM_PERMEABILITY()
///              * SPEED_OF_LIGHT() * SPEED_OF_LIGHT()
///             → dimensionless; |em_check − 1| < 1e-6  (ε₀μ₀c² = 1; residual ≈ −4.34e-14)
///
/// Compile-clean (asserted in `load_constants_example`) catches wrong-dimension
/// constants in γ (stdlib units.ri): a dimension mismatch in a cross-check
/// operator emits a `dimension mismatch` Error diagnostic there. The eval
/// assertions catch wrong-value errors (e.g. a misplaced exponent) that the
/// inert fn return annotation cannot surface.
#[test]
fn constants_example_cross_checks_eval_within_tolerance() {
    let (_src, compiled) = load_constants_example();

    // ── Eval (reify-eval) ─────────────────────────────────────────────────────

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "expected zero Error diagnostics evaluating examples/stdlib/constants.ri:\n{:#?}",
        eval_errors
    );

    // ── Helpers ───────────────────────────────────────────────────────────────
    //
    // `Value::Real(v)` is the canonical representation for dimensionless scalars
    // (see `Value::from_real_scalar`): dimensionless arithmetic (pi, e, 2.0 * pi,
    // ε₀μ₀c²) produces `Value::Real`, while dimensioned arithmetic (R − N_A·k_B)
    // produces `Value::Scalar { si_value, dimension }`.

    // Extract the numeric payload from Real or Scalar; panic on other variants.
    let get_numeric = |field: &str| -> f64 {
        let id = ValueCellId::new("PhysicalConstants", field);
        let val = result
            .values
            .get(&id)
            .unwrap_or_else(|| {
                panic!(
                    "PhysicalConstants.{} not found in eval result; \
                     available keys: {:?}",
                    field,
                    result.values.iter().map(|(k, _v)| k).collect::<Vec<_>>()
                )
            });
        match val {
            Value::Real(v) => *v,
            Value::Scalar { si_value, .. } => *si_value,
            other => panic!(
                "PhysicalConstants.{} expected Value::Real or Value::Scalar, got {:?}",
                field, other
            ),
        }
    };

    // Assert a field's value is dimensionless: it must be `Value::Real` (the
    // canonical dimensionless representation) or `Value::Scalar` with a
    // dimensionless dimension vector.
    let assert_dimensionless = |field: &str| {
        let id = ValueCellId::new("PhysicalConstants", field);
        let val = result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("PhysicalConstants.{} not found in eval result", field));
        match val {
            Value::Real(_) => {} // Real is always dimensionless — OK
            Value::Scalar { dimension, .. } => {
                assert!(
                    dimension.is_dimensionless(),
                    "PhysicalConstants.{} must be dimensionless (ε₀μ₀c² = 1); \
                     got dimension {:?}",
                    field, dimension
                );
            }
            other => panic!(
                "PhysicalConstants.{} expected Real or Scalar, got {:?}",
                field, other
            ),
        }
    };

    // ── circ = 2.0 * pi  — proves pi resolves and is correct ─────────────────

    let circ_val = get_numeric("circ");
    assert!(
        (circ_val - std::f64::consts::TAU).abs() < 1e-9,
        "PhysicalConstants.circ: expected ≈ 2π ({:.17}), got {:.17}",
        std::f64::consts::TAU,
        circ_val
    );

    // ── euler = e  — proves e resolves and is correct ─────────────────────────

    let euler_val = get_numeric("euler");
    assert!(
        (euler_val - std::f64::consts::E).abs() < 1e-9,
        "PhysicalConstants.euler: expected ≈ e ({:.17}), got {:.17}",
        std::f64::consts::E,
        euler_val
    );

    // ── r_check = R − N_A·k_B ≈ 0  (gas-constant identity) ──────────────────
    // Residual ≈ −1.53e-10 (exact 2019-SI: R = N_A·k_B to machine precision).

    let r_val = get_numeric("r_check");
    assert!(
        r_val.abs() < 1e-6,
        "PhysicalConstants.r_check: |R − N_A·k_B| expected < 1e-6, got {}",
        r_val
    );

    // ── em_check = ε₀·μ₀·c² ≈ 1  (dimensionless) ────────────────────────────
    // Residual ≈ −4.34e-14 (exact identity ε₀μ₀ = 1/c²). Dimensionless result
    // is stored as Value::Real by the evaluator.

    assert_dimensionless("em_check");
    let em_val = get_numeric("em_check");
    assert!(
        (em_val - 1.0_f64).abs() < 1e-6,
        "PhysicalConstants.em_check: |ε₀μ₀c² − 1| expected < 1e-6, got {}",
        em_val
    );
}
