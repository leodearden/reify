//! The compiler seam of the builtin-signature registry (task #6001 α,
//! `docs/prds/v0_6/builtin-signature-registry.md`).
//!
//! Two layers, deliberately separated:
//!
//! **(a) End-to-end cell types.** One `.ri` fixture binds all 7 α seed calls
//! and each cell's compile-time type is pinned. These pass BEFORE the swap
//! (via the legacy `is_parse_typed_fn` / `is_analysis_typed_fn` ladder arms)
//! and must still pass AFTER it — that is the point. They are the regression
//! pin proving the registry swap is type-preserving, NOT the RED signal.
//!
//! **(b) The registry path itself.** Asserted against
//! `reify_compiler::__registry_result_type_for_test`, the `test-support`-gated
//! shim over `builtin_registry::registry_result_type` — the crate's single
//! registry entry point. This layer is the RED signal: `builtin_registry.rs`
//! does not exist yet, so the binary does not compile.
//!
//! Built on the `common::compile_with_stdlib_helper` template of
//! `analysis_stress_fn_compile.rs` (same `cell_type` helper, same
//! zero-Error-diagnostics precondition). Compiles WITH stdlib because
//! `parse_length_r`'s result type is the PRELUDE `Result<T,E>` (task #4035).
//!
//! LAYOUT: this is a module of the `harness_builtin_registry.rs` compile unit
//! (C1 contract, `tests/infra/test_harness_kloc_cap.sh`), not a standalone
//! `tests/*.rs` binary. `common` is therefore declared ONCE at that root and
//! imported here as `use crate::common::…` — a local `mod common;` would load
//! the same source twice in this unit (`clippy::duplicate_mod`).

use crate::common::compile_with_stdlib_helper;
use reify_compiler::__registry_result_type_for_test as registry_result_type;
use reify_core::{DimensionVector, Severity, Type};

/// `.ri` fixture binding every one of α's 7 seed builtins, so a single
/// compile pins the whole seed set's call-site typing.
///
/// The stress tensor is the same 3×3 uniaxial Pressure `matrix([[..Pa..]])`
/// used by `analysis_stress_fn_compile.rs`.
const SEED_TYPE_FIXTURE: &str = r#"
structure def RegistrySeedPins {
    let stress = matrix([[1.0e6Pa, 0.0Pa, 0.0Pa],
                         [0.0Pa, 0.0Pa, 0.0Pa],
                         [0.0Pa, 0.0Pa, 0.0Pa]])

    let pl   = parse_length("3mm")
    let plr  = parse_length_r("3mm")
    let vm   = von_mises(stress)
    let ms   = max_shear(stress)
    let ps   = principal_stresses(stress)
    let sf   = safety_factor(stress, 250.0e6Pa)
    let inv  = stress_invariants(stress)
}
"#;

/// Compile the fixture, asserting zero Error-severity diagnostics.
fn compile_fixture() -> reify_compiler::CompiledModule {
    let module = compile_with_stdlib_helper(SEED_TYPE_FIXTURE);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "registry_seed_result_types fixture must produce no Error diagnostics; got: {errs:?}"
    );
    module
}

/// The `cell_type` of `member` on the `RegistrySeedPins` template.
fn cell_type(module: &reify_compiler::CompiledModule, member: &str) -> Type {
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "RegistrySeedPins")
        .unwrap_or_else(|| panic!("RegistrySeedPins template not found"));
    template
        .value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| {
            panic!(
                "cell '{}' not found on RegistrySeedPins; available: {:?}",
                member,
                template
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        })
        .cell_type
        .clone()
}

fn scalar_pressure() -> Type {
    Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    }
}

/// `Tensor{rank:2, n:3, quantity: Scalar<PRESSURE>}` — the compile-time type
/// of the fixture's `matrix([[..Pa..]])` stress tensor.
fn pressure_tensor() -> Type {
    Type::Tensor {
        rank: 2,
        n: 3,
        quantity: Box::new(scalar_pressure()),
    }
}

// ── (a) end-to-end: the task's named user-observable signal ──────────────────

/// The explicitly required α signal: `compile_source`'s result type for
/// `parse_length(...)` is `Option<Length>` — reached via the registry path
/// after the swap, via `parse_fn_result_type` before it, and identical either
/// way.
#[test]
fn parse_length_cell_type_is_option_length() {
    let module = compile_fixture();
    assert_eq!(
        cell_type(&module, "pl"),
        Type::Option(Box::new(Type::length())),
        "parse_length(...) must compile as Option<Length>"
    );
}

/// `parse_length_r(...)` types as the PRELUDE `Result<T,E>` (task #4035),
/// registered as `Type::Enum("Result")`.
#[test]
fn parse_length_r_cell_type_is_enum_result() {
    let module = compile_fixture();
    assert_eq!(
        cell_type(&module, "plr"),
        Type::Enum("Result".to_string()),
        "parse_length_r(...) must compile as Type::Enum(\"Result\")"
    );
}

/// `von_mises(stress)` / `max_shear(stress)` reduce a `Tensor<PRESSURE>` to
/// `Scalar<PRESSURE>` — NOT the first-arg `Tensor<PRESSURE>` fallback drift.
#[test]
fn von_mises_and_max_shear_cell_types_are_scalar_pressure() {
    let module = compile_fixture();
    for member in ["vm", "ms"] {
        assert_eq!(
            cell_type(&module, member),
            scalar_pressure(),
            "cell '{member}' must compile as Scalar<PRESSURE>"
        );
    }
}

/// `principal_stresses(stress)` types as `List(Scalar<PRESSURE>)`.
#[test]
fn principal_stresses_cell_type_is_list_scalar_pressure() {
    let module = compile_fixture();
    assert_eq!(
        cell_type(&module, "ps"),
        Type::List(Box::new(scalar_pressure())),
        "principal_stresses(...) must compile as List(Scalar<PRESSURE>)"
    );
}

/// `safety_factor(stress, 250.0e6Pa)` is dimensionless — pressure cancels.
#[test]
fn safety_factor_cell_type_is_dimensionless() {
    let module = compile_fixture();
    assert_eq!(
        cell_type(&module, "sf"),
        Type::dimensionless_scalar(),
        "safety_factor(...) must compile as Type::dimensionless_scalar()"
    );
}

/// `stress_invariants(stress)` types as the `StressInvariants` structure
/// declared in `crates/reify-compiler/stdlib/fea.ri`.
#[test]
fn stress_invariants_cell_type_is_structure_ref() {
    let module = compile_fixture();
    assert_eq!(
        cell_type(&module, "inv"),
        Type::StructureRef("StressInvariants".to_string()),
        "stress_invariants(...) must compile as StructureRef(\"StressInvariants\")"
    );
}

// ── (b) the registry path — the RED signal ───────────────────────────────────

/// Every α seed name resolves through `registry_result_type` at its real
/// arity, to exactly the type the legacy arms produced.
#[test]
fn registry_result_type_answers_every_seed_at_its_real_arity() {
    let t = pressure_tensor();

    assert_eq!(
        registry_result_type("parse_length", &[Type::String]),
        Some(Type::Option(Box::new(Type::length())))
    );
    assert_eq!(
        registry_result_type("parse_length_r", &[Type::String]),
        Some(Type::Enum("Result".to_string()))
    );
    assert_eq!(
        registry_result_type("von_mises", std::slice::from_ref(&t)),
        Some(scalar_pressure())
    );
    assert_eq!(
        registry_result_type("max_shear", std::slice::from_ref(&t)),
        Some(scalar_pressure())
    );
    assert_eq!(
        registry_result_type("principal_stresses", std::slice::from_ref(&t)),
        Some(Type::List(Box::new(scalar_pressure())))
    );
    assert_eq!(
        registry_result_type("safety_factor", &[t.clone(), scalar_pressure()]),
        Some(Type::dimensionless_scalar())
    );
    assert_eq!(
        registry_result_type("stress_invariants", std::slice::from_ref(&t)),
        Some(Type::StructureRef("StressInvariants".to_string()))
    );
}

/// A name α has not seeded must yield `None`, so the `expr.rs` ladder falls
/// through to the surviving legacy arms and the terminal first-arg fallback.
/// This is I-REG-3's pre-ω carve-out: the registry is authoritative for the
/// names it holds and silent about every other.
#[test]
fn registry_result_type_declines_unseeded_names() {
    let t = pressure_tensor();
    for name in ["sqrt", "volume", ""] {
        assert_eq!(
            registry_result_type(name, std::slice::from_ref(&t)),
            None,
            "{name:?} is not an α seed row — the registry must decline it so the \
             legacy ladder arms still see it"
        );
    }
}

/// **Arity-mismatch preservation — the byte-identical guard.**
///
/// The compiler ladder is arity-INSENSITIVE today: `is_parse_typed_fn` /
/// `is_analysis_typed_fn` gate on the NAME only, so an arity-mismatched call
/// still gets the family's result type. α must not change that, or a
/// mis-arity call would silently start typing through the terminal first-arg
/// fallback instead. Real arity diagnostics arrive with the first genuine
/// overload in τ-numeric.
#[test]
fn registry_result_type_is_arity_insensitive_like_the_legacy_arms() {
    let t = pressure_tensor();

    // Legacy: `analysis_fn_result_type("safety_factor", &[])` →
    // `Type::dimensionless_scalar()` (pinned by the deleted
    // `safety_factor_is_always_real` unit test).
    assert_eq!(
        registry_result_type("safety_factor", &[]),
        Some(Type::dimensionless_scalar()),
        "safety_factor at the WRONG arity must still answer — the ladder never \
         gated on argc"
    );

    // A 1-arg row called with 2 args still yields the 1-arg answer, computed
    // from arg0 exactly as the legacy resolver did.
    assert_eq!(
        registry_result_type("von_mises", &[t.clone(), t.clone()]),
        Some(scalar_pressure()),
        "von_mises at the WRONG arity must still reduce arg0"
    );
    assert_eq!(
        registry_result_type("stress_invariants", &[]),
        Some(Type::StructureRef("StressInvariants".to_string())),
        "stress_invariants with no args must still answer"
    );
    assert_eq!(
        registry_result_type("parse_length", &[]),
        Some(Type::Option(Box::new(Type::length()))),
        "parse_length with no args must still answer — its result is \
         arg-independent"
    );
}
