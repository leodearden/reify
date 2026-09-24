//! Compile-clean regression for `examples/geometric_relations/feature_datum_axis.ri`
//! (geometric-relations ε, task 4385, step-17 RED / step-18 GREEN).
//!
//! This is the OCCT-free half of the B8 worked example: it pins that the
//! feature→datum projection example **parses and compiles clean under the
//! stdlib prelude** and that `cyl.axis` types + lowers to an `a` value cell on
//! the `Cyl` structure. It mirrors `anisotropic_bar_example_tests.rs` /
//! `examples_smoke.rs` (the example file is also auto-discovered by
//! `examples_smoke`'s recursive walk, so it cannot rot).
//!
//! The **runtime** B8 signal — that `cyl.axis` over the realized
//! revolved-rectangle cylinder resolves to exactly one `Value::Axis` equal to
//! the revolution axis — is asserted end-to-end against a real OCCT kernel in
//! `crates/reify-eval/tests/feature_datum_tests.rs`
//! (`feature_datum_axis_example_resolves_to_single_revolution_axis`). It lives
//! in the eval crate because realizing the revolve needs an OCCT-backed engine,
//! and `reify-compiler` is intentionally NOT an OCCT-touching crate
//! (`scripts/occt-touching-crates.txt`); see esc-4385-134.
//!
//! RED until step-18 creates the `.ri` fixture (this test panics on the missing
//! file read).

use reify_compiler::{CompiledGeometryOp, CompiledModule, SweepKind, TopologyTemplate};
use reify_core::{DimensionVector, Severity, Type};
use reify_ir::{CompiledExpr, CompiledExprKind};

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/geometric_relations/feature_datum_axis.ri"
);

/// Read, parse (asserting zero parse errors), and compile under the stdlib
/// prelude (asserting zero Error-severity diagnostics) the B8 example.
fn compile_example() -> CompiledModule {
    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/geometric_relations/feature_datum_axis.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists (step-18)",
    );

    let parsed = reify_compiler::parse_with_stdlib(
        &src,
        reify_core::ModulePath::single("feature_datum_axis"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors in feature_datum_axis.ri: {:#?}",
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
        "expected zero Error diagnostics compiling feature_datum_axis.ri under stdlib, got:\n{:#?}",
        errors
    );

    module
}

/// The compiled `Cyl` structure template — the example's single structure, and
/// the entry point every assertion below reaches through. Shared the same way
/// `compile_example()` is, so the lookup and its diagnostic message live in one
/// place rather than once per test.
fn cyl_template(module: &CompiledModule) -> &TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == "Cyl")
        .unwrap_or_else(|| {
            panic!(
                "Cyl template not found; got: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        })
}

/// Look up a compiled op's argument by name, panicking with the full argument
/// list when it is absent.
///
/// Shared by every argument assertion below (the angle and each axis-direction
/// component) so the "not found" diagnostic — the message that actually has to
/// carry a reader through a signature change — is written once.
///
/// Deduplication LOCAL TO THIS BINARY, and it cannot be anything else: a Rust
/// `tests/` integration binary exports no items, so neither another crate nor
/// another test file in this one can link `named_arg`. A γ/δ/ε corpus pin that
/// wants the same lookup must either re-inline it or first promote it into
/// `reify-test-support` (already a dev-dependency of both `reify-compiler` and
/// `reify-eval`). Stated explicitly because the earlier wording here invited a
/// reuse the module boundary forbids.
fn named_arg<'a>(args: &'a [(String, CompiledExpr)], name: &str) -> &'a CompiledExpr {
    &args
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| {
            panic!(
                "op carries no \"{name}\" argument; args present: {:?}",
                args.iter().map(|(n, _)| n).collect::<Vec<_>>()
            )
        })
        .1
}

/// The dimension a compiled argument expression carries, for the
/// dimension assertions below.
///
/// Every scalar in the compiled IR is a `Type::Scalar { dimension }` — there is
/// no separate `Type::Real` variant, a bare number is
/// `Type::dimensionless_scalar()` (which merely *Displays* as `Real`). Any other
/// shape means the argument is not a scalar at all, which is itself a finding,
/// so this returns `None` rather than silently defaulting to dimensionless.
fn arg_dimension(expr: &CompiledExpr) -> Option<DimensionVector> {
    match &expr.result_type {
        Type::Scalar { dimension } => Some(*dimension),
        _ => None,
    }
}

/// Render an argument expression for an assertion message: its resolved type
/// plus, when it constant-folded, the literal it folded to.
fn describe_arg(expr: &CompiledExpr) -> String {
    match &expr.kind {
        CompiledExprKind::Literal(v) => format!(
            "result_type={:?}, folded literal={:?} (literal dimension {:?})",
            expr.result_type,
            v,
            v.dimension()
        ),
        other => format!(
            "result_type={:?}, kind={:?} (not constant-folded)",
            expr.result_type, other
        ),
    }
}

/// The B8 example's `revolve` angle argument must be ANGLE-dimensioned, and its
/// axis DIRECTION components must stay dimensionless — task 5777 (angle-units
/// α), PRD `docs/prds/v0_6/angle-units-surface-convergence.md` decision D2
/// ("migrate the corpus, then gate it").
///
/// **Why this exists.** Task γ (5779) gates `revolve`'s `angle` argument at the
/// eval chokepoint. Until the literal here carries a unit, this very example —
/// auto-discovered by `examples_smoke.rs` and read by path from two OCCT-backed
/// B8 tests — would self-reject the moment γ lands. Migrating first and gating
/// second is what keeps the corpus green at every commit.
///
/// **Why `rad`, not `deg`.** `revolve` reads its angle through
/// `eval_named_arg_f64` (`crates/reify-eval/src/geometry_ops.rs`) → `Value::as_f64`,
/// with no degree conversion on the path — so the bare literal was ALREADY being
/// consumed as radians, and `rad` is the suffix that names what it already meant.
/// `rad`'s conversion factor is exactly 1.0 (`crates/reify-core/src/units.rs`), so
/// the SI magnitude equals the source literal bit for bit; the magnitude assertion
/// below is therefore `==` and not an epsilon compare, which would mask precisely
/// the regression it exists to catch. That bit-exactness is the whole reason.
///
/// **`deg` would NOT have been unsafe** — do not read this comment as "avoid `deg`
/// in `.ri` source". Unit suffixes are resolved to SI in the COMPILER, not at eval
/// (`unit_to_scalar`, `crates/reify-compiler/src/units.rs` →
/// `reify_core::units::unit_symbol_to_si`, where `deg` carries a factor of π/180),
/// so `360deg` would reach eval as SI radians too — and `360.0 * (PI / 180.0)` is
/// bit-exactly `TAU`, i.e. the identical `si_value` and identical geometry. Writing
/// the suffix *is* the conversion. The rest of the `.ri` corpus already writes
/// `deg`, and that stays correct; see
/// `docs/notes/angle-literal-migration-ledger.md` §3.3 and §5.
///
/// **Why the direction half is here too.** `ax`/`ay`/`az` are dimensionless
/// unit-vector components and are explicitly NOT gated (PRD C1 invariant 4). That
/// half is green today and must stay green: the likeliest future error on this
/// file is an over-eager migration that dimensions the `0.0, 1.0, 0.0` as well,
/// and pinning the negative here makes that surface as a named assertion failure
/// instead of a mysterious downstream geometry break.
#[test]
fn feature_datum_axis_example_revolve_angle_is_angle_dimensioned() {
    let module = compile_example();
    let cyl = cyl_template(&module);

    // Anti-vacuity: locate the revolve explicitly and require EXACTLY one, so a
    // future refactor that renames the op or drops the realization fails loudly
    // here rather than leaving the assertions below unreached.
    let revolves: Vec<&Vec<(String, CompiledExpr)>> = cyl
        .realizations
        .iter()
        .flat_map(|r| r.operations.iter())
        .filter_map(|op| match op {
            CompiledGeometryOp::Sweep {
                kind: SweepKind::Revolve,
                args,
                ..
            } => Some(args),
            _ => None,
        })
        .collect();
    assert_eq!(
        revolves.len(),
        1,
        "expected exactly one Revolve sweep in Cyl's realizations (the \
         `revolve(rectangle(20mm, 10mm), …)` at feature_datum_axis.ri); found {}. \
         Ops present: {:?}",
        revolves.len(),
        cyl.realizations
            .iter()
            .flat_map(|r| r.operations.iter())
            .map(std::mem::discriminant)
            .collect::<Vec<_>>()
    );
    let args = revolves[0];

    let angle = named_arg(args, "angle");

    // (a) DIMENSION — the angle is ANGLE-dimensioned, not bare.
    assert_eq!(
        arg_dimension(angle),
        Some(DimensionVector::ANGLE),
        "revolve's \"angle\" argument in examples/geometric_relations/feature_datum_axis.ri \
         must be ANGLE-dimensioned (write the literal as `6.283185307179586rad`), so that the \
         γ (5779) eval-chokepoint gate does not reject this example. Observed: {}",
        describe_arg(angle)
    );

    // (b) MAGNITUDE — `rad` has unit factor exactly 1.0, so the SI value is the
    // source literal unchanged. Bit-exact `==`, never an epsilon compare.
    match &angle.kind {
        CompiledExprKind::Literal(v) => {
            assert_eq!(
                v.as_f64(),
                Some(std::f64::consts::TAU),
                "revolve's \"angle\" must carry SI magnitude exactly 2π rad \
                 (6.283185307179586) — `rad` converts by a factor of exactly 1.0, so \
                 dimensioning the literal must not perturb its value by a single bit. \
                 Observed: {}",
                describe_arg(angle)
            );
        }
        _ => panic!(
            "revolve's \"angle\" did not constant-fold to a literal, so its SI magnitude \
             cannot be pinned here; assert it in the eval-side witness \
             (crates/reify-eval/tests/feature_datum_tests.rs) instead. Observed: {}",
            describe_arg(angle)
        ),
    }

    // The axis DIRECTION components stay DIMENSIONLESS — PRD C1 invariant 4.
    for name in ["ax", "ay", "az"] {
        let dir = named_arg(args, name);
        assert_eq!(
            arg_dimension(dir),
            Some(DimensionVector::DIMENSIONLESS),
            "revolve's \"{name}\" is a unit-vector component and must stay DIMENSIONLESS — \
             PRD C1 invariant 4 explicitly does not gate axis-direction components, so \
             they must NOT be migrated along with the angle. Observed: {}",
            describe_arg(dir)
        );
    }
}

/// The B8 example parses + compiles clean under the stdlib prelude and exposes
/// a `Cyl` structure template. Baseline compile-clean gate (the `cyl.axis`
/// feature→datum projection must type without any Error diagnostic).
#[test]
fn feature_datum_axis_example_compiles_under_stdlib_with_zero_errors() {
    let module = compile_example();

    assert!(
        module.templates.iter().any(|t| t.name == "Cyl"),
        "expected a 'Cyl' structure template in compiled feature_datum_axis.ri; \
         found templates: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

/// The compiled `Cyl` template's `a` value cell — the `let a : Axis = cyl.axis`
/// binding — must carry an initializer that TYPED to `Type::Axis`. Asserting the
/// initializer's resolved type (rather than mere cell presence, which a *failed*
/// projection also satisfies — the binding's cell is still created, just with an
/// `Error` initializer type) directly proves the feature→datum `.axis` projection
/// typed (`Type::Geometry` receiver → `Axis`) and lowered rather than being
/// dropped. Mirrors `projection_type` in `datum_projection_tests.rs` (read the
/// initializer's computed `result_type`, not the `: Axis` annotation — which would
/// read `Axis` even if the RHS poisoned).
#[test]
fn feature_datum_axis_example_lowers_cyl_axis_to_value_cell() {
    let module = compile_example();
    let cyl = cyl_template(&module);

    let a_cell = cyl
        .value_cells
        .iter()
        .find(|c| c.id.member == "a")
        .unwrap_or_else(|| {
            panic!(
                "expected Cyl to carry an 'a' value cell (the `let a : Axis = cyl.axis` \
                 feature→datum projection); found cells: {:?}",
                cyl.value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        });

    let init = a_cell.default_expr.as_ref().expect(
        "`a` value cell must carry its `cyl.axis` initializer expr (the feature→datum \
         projection); a missing initializer means the projection was dropped",
    );
    assert_eq!(
        init.result_type,
        Type::Axis,
        "`let a : Axis = cyl.axis` initializer must type to Axis (proving the \
         feature→datum `.axis` projection resolved, not poisoned to Error); got {:?}",
        init.result_type
    );
}
