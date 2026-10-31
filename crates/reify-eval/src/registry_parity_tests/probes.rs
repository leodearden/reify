//! The representative argument shapes each `BindingKind::EvalBuiltin` row is
//! swept at by the parity harness, compile-forced per row.
//!
//! Argument DATA only: this module answers "what does the harness call each
//! row with", and nothing about how a call is executed or judged. It imports
//! nothing from its parent, so the edge is strictly parent → child.

use reify_builtins::EvalBuiltinId;
use reify_core::{DimensionVector, Type};
use reify_ir::Value;

/// The uniaxial stress magnitude every analysis probe is built from, in SI
/// pascals. The value is irrelevant to a KIND comparison; only the shape and
/// the dimension are load-bearing.
const SIGMA_PA: f64 = 100e6;

/// A yield strength for `safety_factor`'s second argument, chosen non-zero so
/// the ratio is finite and `sanitize_value` does not intervene.
const YIELD_PA: f64 = 250e6;

/// A 3×3 `Value::Tensor` of `Value::Tensor` of `Value::Scalar`, every element
/// carrying `dimension`.
///
/// Shape reuse of `crates/reify-stdlib/tests/registry_dispatch_seed_parity.rs`'s
/// `dimensioned_matrix` — the nesting is what `analysis::matrix_components_f64`
/// reads, so getting it wrong makes every analysis kernel answer `Value::Undef`.
///
/// **A third copy, and a scope deferral rather than a design conclusion.** The
/// right home is `reify_test_support::values`, which already holds exactly this
/// class of `Value` fixture (`mm`, `newton`, `matrix3x3`, …) and is ALREADY a
/// dev-dependency of BOTH reify-eval and reify-stdlib — so sharing it would
/// cost no production dependency edge. That crate is outside this task's lock
/// set; filed as follow-up work together with the harness's row filter
/// (`eval_builtin_rows`).
fn dimensioned_matrix_3x3(rows_f64: &[[f64; 3]; 3], dimension: DimensionVector) -> Value {
    Value::Tensor(
        rows_f64
            .iter()
            .map(|r| {
                Value::Tensor(
                    r.iter()
                        .map(|&si_value| Value::Scalar {
                            si_value,
                            dimension,
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

/// The static type of [`dimensioned_matrix_3x3`]'s PRESSURE result.
///
/// `quantity` is a real `Scalar<PRESSURE>` rather than a bare placeholder
/// because the analysis resolvers read arg0's quantity out of exactly this
/// field (`resolvers::tensor_quantity`, `crates/reify-builtins/src/resolvers.rs`)
/// and DEFAULT TO `DIMENSIONLESS` when they cannot find one. A placeholder here
/// would silently route every analysis row through `scalar_or_real`'s
/// dimensionless branch and compare the executed `Scalar<PRESSURE>` against a
/// declared dimensionless type — still a `Matches` at kind level, so the
/// mis-shaped probe would never be noticed while making the row's real
/// signature unobserved.
fn pressure_tensor_type() -> Type {
    Type::Tensor {
        rank: 2,
        n: 3,
        quantity: Box::new(Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        }),
    }
}

/// One probe of one row: the `(value, static type)` pair for each ARGUMENT of a
/// single call.
///
/// Paired rather than two parallel lists so "same length" and "describes the
/// same argument" are unrepresentable-when-wrong instead of asserted at runtime
/// (house heuristic 10 — enforce an invariant in the type wherever the type can
/// carry it). The two halves are unzipped at the two call sites, because
/// `ResultSpec::ArgAware` resolves from `&[Type]` while `eval_builtin` takes
/// `&[Value]`; the pairing is what guarantees those two views describe the same
/// arguments.
pub(super) type Probe = Vec<(Value, Type)>;

/// The one-argument concrete-tensor probe the analysis reductions share.
fn concrete_tensor_probe() -> Probe {
    vec![(uniaxial_stress(), pressure_tensor_type())]
}

/// Every probe one row is swept at.
///
/// # Why a LIST of probes, not one
///
/// A row can have more than one argument SHAPE whose result type its resolver
/// answers differently, and one probe per row can only ever check one of them.
/// Four of α's seven rows are `ResultSpec::ArgAware` over `reduce_tensor_arg`
/// (`crates/reify-builtins/src/resolvers.rs`), which has two branches — a
/// `Type::Field` arm added by #6577 and the concrete fall-through — and only
/// the concrete branch is probed here. Returning a list means a τ row registers
/// its Field shape ALONGSIDE its concrete one rather than replacing it, instead
/// of the one-shape-per-row ceiling being frozen in by the signature. See the
/// harness module header's "What this harness cannot see" for why that Field
/// probe cannot be written today.
///
/// # Why an exhaustive `match` with no `_` arm
///
/// This is the mechanism that makes PRD §9's "grows per τ" true on the
/// ARGUMENT side, the half `eval_builtin_rows` cannot cover. Membership is
/// genuinely automatic; argument DATA is not, so completeness is enforced by
/// the compiler instead: a τ migration that registers a row mints a variant in
/// `EvalBuiltinId` and stops reify-eval's test build with
///
/// ```text
/// error[E0004]: non-exhaustive patterns: `EvalBuiltinId::<NewRow>` not covered
/// ```
///
/// until representative args exist. That is the same forcing function
/// `reify_stdlib::registry_dispatch::dispatch`
/// (`crates/reify-stdlib/src/registry_dispatch.rs`) uses for I-REG-2, and
/// it is strictly better than a test that must remember to complain. **Adding a
/// `_` arm here deletes the property.**
///
/// A name-keyed `HashMap` was rejected for the opposite property: an unmatched
/// new row would fall through to a default, be swept with degenerate args,
/// yield `Value::Undef`, and read as a vacuous green — the precise hollowing-out
/// this module exists to prevent.
///
/// # Why the arguments are not derived from `arg_slots`
///
/// Slot-driven synthesis is **impossible today, not merely unchosen**. Every
/// seed row declares `arg_slots: [Any]` / `[Any, Any]`, and `ArgSlot` has
/// exactly one variant — `Any`, "no constraint on this slot"
/// (`crates/reify-builtins/src/row.rs`) — which carries no shape to
/// synthesize from. Nor could the registry supply a `Value`: by PRD decision 3
/// reify-builtins depends on `reify-core` only and holds no `Value` at all,
/// structurally locked by its own `tests/dag_invariant.rs`. The richer slot
/// vocabulary (dimension checks, `SameDimensionAs(slot)`) arrives in τ-numeric;
/// when it does, part of this function can become a derivation.
///
/// Every probe returned here is self-validated by the harness's
/// `representative_probes_are_well_formed_for_every_row`, so a mis-built probe
/// fails AT THE PROBE rather than being misattributed to the row under test.
pub(super) fn representative_probes(id: EvalBuiltinId) -> Vec<Probe> {
    // NO `_` ARM. See the doc-comment: its absence is the I-REG-2-shaped
    // forcing function that makes a τ row without representative args a BUILD
    // failure rather than a silent vacuous pass.
    match id {
        // `parse::parse_length` matches on `Value::String` and returns
        // `Value::Undef` for any other argument kind.
        EvalBuiltinId::ParseLength | EvalBuiltinId::ParseLengthR => {
            vec![vec![(Value::String("12mm".to_string()), Type::String)]]
        }

        // `analysis::{von_mises, max_shear}` read a 3×3 window through
        // `matrix_components_f64` and reduce it to a scalar carrying the
        // element dimension.
        EvalBuiltinId::VonMises | EvalBuiltinId::MaxShear => vec![concrete_tensor_probe()],

        // `analysis::principal_stresses` needs the same 3×3 window; it returns
        // the three eigenvalues as a `Value::List`.
        EvalBuiltinId::PrincipalStresses => vec![concrete_tensor_probe()],

        // `analysis::safety_factor` is `binary(tensor, yield)`: arg0 is the
        // same 3×3 window, arg1 must answer `as_f64` or the kernel returns
        // `Value::Undef`. The ratio cancels, so the result is a bare
        // `Value::Real` whatever dimension arg1 carries.
        EvalBuiltinId::SafetyFactor => vec![vec![
            (uniaxial_stress(), pressure_tensor_type()),
            (
                Value::Scalar {
                    si_value: YIELD_PA,
                    dimension: DimensionVector::PRESSURE,
                },
                Type::Scalar {
                    dimension: DimensionVector::PRESSURE,
                },
            ),
        ]],

        // `analysis::stress_invariants` returns `Value::Undef` for ANYTHING
        // but a 3×3 tensor (`crates/reify-stdlib/src/analysis.rs`) — the
        // strictest shape requirement among the seeds, and the one that makes
        // a degenerate probe here read as a vacuous pass.
        EvalBuiltinId::StressInvariants => vec![concrete_tensor_probe()],
    }
}

/// The uniaxial 100 MPa stress tensor every analysis probe is fed.
fn uniaxial_stress() -> Value {
    dimensioned_matrix_3x3(
        &[[SIGMA_PA, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
        DimensionVector::PRESSURE,
    )
}
