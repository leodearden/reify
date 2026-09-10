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

/// **The ladder arm's cheap-miss guard is behaviour-neutral.**
///
/// `expr.rs`'s registry arm projects `compiled_args` into a `Vec<Type>` (an
/// allocation plus a deep `Type::clone` per argument) before it can ask
/// `registry_result_type` anything, so it guards that projection on
/// `registry_owns(name)` — a name-only test — and pays the projection only on
/// a hit. That is safe for exactly one reason: `registry_owns` is
/// `registry_result_type`'s OWN precondition, so a `false` there can never
/// suppress an answer the registry would otherwise have given.
///
/// This pins that implication in both directions it admits:
///
/// - Row-derived, never a restated list: every registered name is `owns`,
///   swept over `reify_builtins::rows()` so a name added by a later τ is
///   covered without touching this test.
/// - `!owns(n)` ⇒ `registry_result_type(n, args) == None`, checked over the
///   unregistered names AND at several arities, since the guard drops the
///   argument types entirely.
///
/// The converse is deliberately NOT asserted: an `ArgAware` row may own a name
/// and still decline the arguments it is handed, which is a legal `None` on
/// the hit path.
#[test]
fn registry_owns_is_exactly_registry_result_type_s_precondition() {
    use reify_compiler::__registry_owns_for_test as registry_owns;

    let t = pressure_tensor();
    let arg_shapes: [&[Type]; 3] = [&[], std::slice::from_ref(&t), &[t.clone(), t.clone()]];

    for r in reify_builtins::rows() {
        assert!(
            registry_owns(r.name),
            "registry row {:?} must be claimed by registry_owns — otherwise \
             expr.rs's guard skips the projection and the ladder falls through \
             to the first-arg fallback, silently mis-typing every call to it",
            r.name
        );
    }

    for name in ["sqrt", "volume", "envelope_von_mises", "transform3", ""] {
        assert!(
            !registry_owns(name),
            "{name:?} holds no registry row, so registry_owns must decline it"
        );
        for args in arg_shapes {
            assert_eq!(
                registry_result_type(name, args),
                None,
                "registry_owns({name:?}) is false, so registry_result_type must \
                 be None at every arity — the guard drops the argument types, so \
                 any answer here would be one expr.rs never asks for"
            );
        }
    }
}

// ── (c) #6577's Field-argument contract, through the registry path ───────────
//
// Carried onto the registry surface when α merged main forward and resolved
// `analysis_signatures.rs` as DELETE: main had taught `analysis_fn_result_type`
// a Field prelude (task #6577) inside the very file α deletes, so the registry
// must reproduce it or the merge silently regresses that task. The end-to-end
// witness is `harness_geometry_solver::solver_elastic_static_stdlib_compile`'s
// `von_mises_over_solver_stress_field_types_as_pressure_field`; these pins fix
// the same contract at the seam, where a failure names the resolver directly.

/// The domain of `solve_elastic_static(..).stress` — preserved verbatim into
/// every Field answer.
fn field_domain() -> Type {
    Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    })
}

/// `Field<Point3<Length>, Tensor<2,3,Scalar<PRESSURE>>>`.
fn pressure_tensor_field() -> Type {
    Type::Field {
        domain: Box::new(field_domain()),
        codomain: Box::new(pressure_tensor()),
    }
}

/// `Field<Point3<Length>, codomain>`.
fn field_of(codomain: Type) -> Type {
    Type::Field {
        domain: Box::new(field_domain()),
        codomain: Box::new(codomain),
    }
}

/// `von_mises` / `max_shear` over a Field argument answer with a **`Field`**,
/// not a reduced scalar.
///
/// Eval wraps the field lazily and returns a `Value::Field`
/// (`crates/reify-expr/src/analysis.rs`, `wrap_tensor_field`), and
/// `value_type_kind_matches` (`crates/reify-eval/src/lib.rs:330`) maps a
/// `Value::Field` onto `Type::Field` alone — so a `Scalar` here is a kind lie,
/// not merely a dimension slip.
#[test]
fn registry_result_type_carries_the_field_contract_for_von_mises_and_max_shear() {
    let f = pressure_tensor_field();

    for name in ["von_mises", "max_shear"] {
        assert_eq!(
            registry_result_type(name, std::slice::from_ref(&f)),
            Some(field_of(scalar_pressure())),
            "{name}(Field<D, Tensor<2,3,Pressure>>) must type as \
             Field<D, Scalar<Pressure>> — this is task #6577's contract, which \
             lived in the file α deletes"
        );
    }
}

/// The concrete-Tensor path is provably unperturbed by the Field prelude.
///
/// Restated here at the registry seam as an explicit non-regression lock on the
/// prelude's insertion point: the prelude fires for `Type::Field` arguments and
/// for nothing else.
#[test]
fn the_field_contract_leaves_the_concrete_tensor_path_untouched() {
    let t = pressure_tensor();

    assert_eq!(
        registry_result_type("von_mises", std::slice::from_ref(&t)),
        Some(scalar_pressure()),
        "von_mises over a CONCRETE Tensor must still reduce to Scalar<Pressure>"
    );
    assert_eq!(
        registry_result_type("max_shear", std::slice::from_ref(&t)),
        Some(scalar_pressure()),
        "max_shear over a CONCRETE Tensor must still reduce to Scalar<Pressure>"
    );
}

/// `principal_stresses` at argc 1 over a Field → `Field<D, List(Q)>`.
///
/// The `List` sits INSIDE the `Field`: eval samples the field, and each sample
/// is the three eigenvalues. Mirrors `compute_principal_stresses`
/// (`crates/reify-expr/src/analysis.rs:239-256`).
#[test]
fn registry_result_type_carries_the_field_contract_for_principal_stresses() {
    assert_eq!(
        registry_result_type("principal_stresses", &[pressure_tensor_field()]),
        Some(field_of(Type::List(Box::new(scalar_pressure())))),
        "principal_stresses(Field<D, Tensor<2,3,Pressure>>) must type as \
         Field<D, List(Scalar<Pressure>)> — the List sits inside the Field"
    );
}

/// `safety_factor` at argc 2 over a Field → `Field<D, Real>`.
///
/// Dimensionless in BOTH forms: yield/von_mises cancels pointwise over a field
/// exactly as it does for a scalar. The result is nonetheless a `Field`, because
/// eval still hands back a `Value::Field` — which is why the row cannot stay
/// `ResultSpec::Const`.
#[test]
fn registry_result_type_carries_the_field_contract_for_safety_factor() {
    assert_eq!(
        registry_result_type(
            "safety_factor",
            &[pressure_tensor_field(), scalar_pressure()]
        ),
        Some(field_of(Type::dimensionless_scalar())),
        "safety_factor(Field<D, Tensor<2,3,Pressure>>, Pressure) must type as \
         Field<D, Real> — dimensionless codomain, but still a Field"
    );
}

/// `stress_invariants` deliberately keeps its `StructureRef` under a Field
/// argument: eval has NO Field arm for that name, so a Field-typed answer here
/// would be a claim eval cannot honour.
#[test]
fn stress_invariants_is_still_a_structure_ref_under_a_field_argument() {
    assert_eq!(
        registry_result_type("stress_invariants", &[pressure_tensor_field()]),
        Some(Type::StructureRef("StressInvariants".to_string())),
        "stress_invariants has no Field arm in eval's dispatch ladder, so the \
         registry must keep answering StructureRef"
    );
}

/// The Field arm's arity gate, per NAME, through the registry path.
///
/// The compiler seam is deliberately arity-INSENSITIVE — `registry_result_type`
/// resolves via `name_group`, not the argc-keyed `lookup`, because the legacy
/// ladder arms gated on the name alone (see the module docs on
/// `builtin_registry.rs`). So a mis-arity call DOES reach the resolver; it is
/// the resolver's own gate, mirroring eval's dispatch condition, that makes it
/// fall through to the concrete-tensor answer rather than claiming a `Field`
/// eval would never produce.
#[test]
fn the_field_arm_is_gated_on_each_name_s_own_arity() {
    let f = pressure_tensor_field();

    for name in ["von_mises", "max_shear"] {
        assert_eq!(
            registry_result_type(name, &[f.clone(), scalar_pressure()]),
            Some(Type::dimensionless_scalar()),
            "{name} at argc 2 must fall through — eval's Field dispatch gate is \
             evaluated_args.len() == 1"
        );
    }
    assert_eq!(
        registry_result_type("principal_stresses", &[f.clone(), scalar_pressure()]),
        Some(Type::List(Box::new(Type::dimensionless_scalar()))),
        "principal_stresses at argc 2 must fall through to the concrete List"
    );
    assert_eq!(
        registry_result_type("safety_factor", std::slice::from_ref(&f)),
        Some(Type::dimensionless_scalar()),
        "safety_factor at argc 1 must fall through — its Field dispatch gate is \
         evaluated_args.len() == 2"
    );
}
