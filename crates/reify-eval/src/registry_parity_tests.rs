//! Executed **static-vs-runtime parity harness** for the builtin-signature
//! registry — task 6013 ψ (PRD `docs/prds/v0_6/builtin-signature-registry.md`
//! §3 decision 12, §7.2 I-REG-4, §8 row 9; §9 "ψ — parity harness, lands with
//! α's seeds, grows per τ").
//!
//! # What this harness asserts
//!
//! For every `BindingKind::EvalBuiltin` row in `reify_builtins::rows()`:
//!
//! ```text
//! representative args  ->  reify_stdlib::eval_builtin(row.name, &args)  ->  Value
//!                                                                          |
//!                      crate::value_type_kind_matches(value, declared, None)
//!                                                                          |
//!            declared = row.result.resolve(&arg types)                  verdict
//! ```
//!
//! The registry's `result` column is a STATIC claim; `eval_builtin` is the
//! RUNTIME answer. Everything α's own tests pin is one side or the other — the
//! row table is checked for internal consistency, and the eval kernels are
//! checked value-for-value against their legacy string path. Neither closes the
//! residue PRD decision 12 names: **a buggy eval body returning the wrong
//! kind**, which is invisible to a table-only test and invisible to a
//! value-only test whose expectation was authored from the same buggy body.
//! This module is the join, and it is the only place the two sides meet under
//! execution.
//!
//! Membership is DERIVED, never declared: no builtin name, id, or count is
//! restated here. A τ migration that adds rows is swept with no edit to this
//! module (see [`eval_builtin_rows`]), and the argument side is forced by an
//! exhaustive `match` with no `_` arm (see [`representative_probes`]), so a row
//! that lands without representative args stops this crate's test build rather
//! than being swept with degenerate args.
//!
//! # PRD open question 5, resolved: in-crate, not `reify-compiler/tests/`
//!
//! Q5 asks whether the harness lives as a reify-eval in-crate module beside
//! the drift test, or in reify-compiler's `tests/` directory. **In-crate**, on
//! four grounds:
//!
//! 1. **Decisive: [`crate::value_type_kind_matches`] is private** — a plain
//!    `fn` in `crates/reify-eval/src/lib.rs`, not even `pub(crate)`. I-REG-4
//!    requires asserting against *that* function with **no second derivation**,
//!    and an integration test is a separate crate that cannot reach it. Making
//!    it `pub` would widen reify-eval's public API solely to host a test. The
//!    in-tree cost of the alternative is already visible: `crates/reify-compiler/
//!    tests/harness_type_checking/mul_div_static_runtime_parity.rs`
//!    re-authored its own `value_kind_matches_type` because it could not reach
//!    reify-eval's — precisely the second derivation I-REG-4 forbids.
//! 2. **Exact in-crate precedent**, for the identical reason and stated as
//!    such: `crate::registry_drift_tests`' "Why in-crate (not `tests/`)"
//!    section — "The eval-side oracles this module probes … are `pub(crate)` —
//!    unreachable from an external integration test under `reify-eval/tests/`".
//!    This module is hooked immediately after it so the two registry guards
//!    read as siblings.
//! 3. **No new production dependency edge.** reify-eval already normal-deps
//!    both reify-compiler and reify-stdlib; reify-builtins is a DEV-dep only
//!    (`crates/reify-eval/Cargo.toml`), so `cargo tree -e no-dev -p reify-eval`
//!    is unchanged and `engine_hash_closure.txt` needed no edit.
//! 4. **It creates no new gate artifact.** An in-crate `#[cfg(test)] mod` is
//!    neither a `crates/*/tests/*.rs` integration binary nor a
//!    `tests/infra/test_*.sh` script, so it fires neither the overlay rule's
//!    drift-guard registration trigger (`.claude/skills/prd/project.md:170`)
//!    nor the I-REG-1 string-dispatch scan, which masks `#[cfg(test)]` blocks
//!    before scanning. The per-anchor adjudication is recorded in
//!    `docs/prds/v0_6/builtin-signature-registry.capability-manifest.md`.
//!
//! # Why a trivial-accept pair is `Vacuous`, not a pass
//!
//! This is the load-bearing design constraint, and a two-way `true`/`false`
//! harness gets it wrong. [`crate::value_type_kind_matches`] has two arms that
//! answer `true` while carrying no information, one on each side of the pair:
//! its Auto/no-value sentinel arm accepts `Value::Undef` for every type, and
//! its anti-cascade guard accepts every VALUE for a declared `Type::Error`,
//! returning before it inspects the value at all.
//!
//! `Value::Undef` is the dangerous half today, because it is where every
//! failure mode in this workspace funnels:
//!
//! - `reify_stdlib::helpers::{unary, binary}` return it on the wrong argc;
//! - `analysis::stress_invariants` returns it for anything but a 3×3 tensor;
//! - `reify_stdlib::eval_builtin` returns it as its unresolved-name terminus.
//!
//! So a naive harness would be silently defeated by **exactly** the class of
//! bug it exists to catch: an eval body that falls off a match arm yields
//! `Undef`, the matcher says `true`, and the sweep reports green over a table
//! it never actually probed. `registry_drift_tests`' own header names "silently
//! degrades the call to `Value::Undef` at eval time" as the defect it was built
//! for; the same sentinel must not be this module's blind spot.
//!
//! The verdict is therefore three-way — `Matches` / `Vacuous` / `Diverges`
//! ([`ParityVerdict`]) — and only `Matches` is a pass. That this is the PRD's
//! intent rather than an embellishment is corroborated by the exemption the PRD
//! names as the ledger's seed: the `piecewise_polynomial` stub returns exactly
//! `Some(Value::Undef)` (`crates/reify-stdlib/src/trajectory/mod.rs`). Its
//! ledger entry would be unexplainable under a two-way verdict, because the
//! stub already "passes". The ledger's real semantics are *rows whose parity
//! assertion cannot be made non-vacuously*, with `Undef` as the mechanism.
//!
//! # What this harness cannot see
//!
//! Stated so the claims above are read at their real width, and so the next τ
//! author does not assume a gap here is covered.
//!
//! **The `Type::Field` branch of the `ArgAware` resolvers is not parity-checked
//! here, and structurally cannot be.** `reduce_tensor_arg`
//! (`crates/reify-builtins/src/resolvers.rs`) has two branches — a `Type::Field`
//! arm added by #6577 and a concrete fall-through — so `von_mises`, `max_shear`,
//! `principal_stresses` and `safety_factor` each declare a `Field<D, ..>` result
//! for a Field argument. Every probe in this module is concrete, and a Field
//! probe cannot be added: `reify_stdlib::eval_builtin` has no Field handling at
//! all. The lazy wrap that produces a `Value::Field` is `wrap_tensor_field` in
//! `crates/reify-expr/src/analysis.rs`, reached through reify-expr's evaluator
//! ladder and not through the `eval_builtin` path this harness executes — so a
//! Field probe here would evaluate to `Value::Undef` and read `Vacuous`,
//! certifying nothing.
//!
//! The residue PRD §3 decision 12 names therefore remains OPEN for the Field
//! half of those four rows, and closing it is reify-expr's to do, not ψ's.
//! [`representative_probes`] returns a LIST precisely so that a leaf which
//! routes Field evaluation through this path can register a Field shape
//! alongside the concrete one rather than replacing it.
//!
//! The resolvers' own `Type`-level Field contract IS covered — by
//! `reify_builtins::resolvers`' unit tests, ported from #6577 — but that is a
//! type-algebra test on one side of the pair, which is exactly the kind of
//! one-sided check this module exists to join.
//!
//! # Vocabulary
//!
//! Every concept this module reasons about is a Rust type, never a meaningful
//! string (house heuristic 12 — structured data, no ad-hoc parsers): the
//! disposition of a row is a [`ParityVerdict`], an accepted divergence is an
//! [`ExemptionEntry`] keyed on `EvalBuiltinId` (so a deleted row is a compile
//! error, not a dead ledger line), and an adjudication result is a [`Failure`].
//! Strings appear only inside assertion messages, which are FORMATTED from
//! those types at the point of failure — never parsed, compared, or used as a
//! key.
//!
//! # Mutation recipe — the proof these guards are not vacuously true
//!
//! Each leg below was OBSERVED by temporary mutate-run-revert, never guessed
//! (task 6013 ψ step 11, re-confirmed at the amendment pass). **No mutating
//! test is committed**, here or anywhere: a committed mutation asserts the bug,
//! not the contract. The permanent guards are the tests in this module; this
//! recipe is how you re-confirm each one still fires when a τ migration makes
//! you doubt it.
//!
//! Only the EDIT and the expected verdict CLASS are recorded. The observed
//! failure text is deliberately NOT transcribed here: it carries run-specific
//! detail — pass/fail tallies that shift the moment a row or a test is added,
//! and a 300-character `DimensionVector` Debug payload — which would go stale
//! with nothing in the tree able to detect it. The verdict class is the part
//! that carries the claim.
//!
//! Run command for every leg: `cargo test -p reify-eval --lib registry_parity`.
//!
//! **Leg 1 — the row's DECLARED type is wrong** (the PRD §8 row 9 recipe). In
//! `crates/reify-builtins/src/registry.rs`, change the `ParseLength` row's
//! `result` from `Const(Type::Option(Box::new(Type::length())))` to
//! `Const(Type::String)`. Expect
//! `every_eval_builtin_row_agrees_with_its_executed_kind` RED, reporting
//! `ParseLength` UNLEDGERED at verdict `Diverges`. Revert the row.
//!
//! **Leg 2 — the EVAL BODY is wrong and the table is untouched.** The leg that
//! proves this harness's distinctive claim. PRD §3 decision 12 names a buggy
//! eval body as the residue the row table alone cannot close, and no table-only
//! test can see it — here the registry is left exactly as shipped. In
//! `crates/reify-stdlib/src/parse.rs`, change `parse_length`'s `Some(s)` arm
//! from `Value::Option(parse_length_value(s).ok().map(Box::new))` to
//! `Value::String(s.to_string())`. Same test RED, same `Diverges` verdict, with
//! the declared type untouched. Revert the kernel.
//!
//! **Leg 3 — the vacuity arm is live, and the probe guard is its second
//! signal.** In THIS file, split [`representative_probes`]' shared
//! `ParseLength | ParseLengthR` arm and give `ParseLength` an argument-less
//! probe (`vec![vec![]]`). The kernel's arity guard yields `Value::Undef` and
//! TWO tests go RED: the sweep reports `ParseLength` UNLEDGERED at verdict
//! `Vacuous`, and `representative_probes_are_well_formed_for_every_row` fails
//! its check (a). That pair is the intended double signal — the second says the
//! fault is in THIS file rather than in the row, which is exactly the
//! misattribution it exists to prevent. Revert the arm.
//!
//! Legs 1 and 2 each transiently edit another crate, so `git status` must be
//! clean of `registry.rs` and `parse.rs` before anything is committed.

use std::collections::HashSet;

use reify_builtins::{BindingKind, BuiltinId, BuiltinRow, EvalBuiltinId, rows};
use reify_core::{DimensionVector, Type};
use reify_ir::Value;
use strum::{EnumCount, IntoEnumIterator};

// ── the verdict vocabulary ──────────────────────────────────────────────────

/// One row's parity disposition: whether its declared result type and its
/// executed result agree, disagree, or were never really compared.
///
/// Three-way rather than two-way is the whole design of this harness — see the
/// module header's "Why `Value::Undef` is `Vacuous`, not a pass".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParityVerdict {
    /// The row evaluated to a real value whose kind its declared type accepts.
    /// The only pass.
    Matches,
    /// One side of the pair was a trivial-accept sentinel — the row evaluated
    /// to `Value::Undef`, or its declared type resolved to `Type::Error`. The
    /// matcher accepts either unconditionally, so the comparison certifies
    /// NOTHING about the row — it is not a pass, and it needs an
    /// [`ExemptionEntry`] saying which leaf retires it.
    Vacuous,
    /// The row evaluated to a real value whose kind its declared type rejects.
    /// Either the row's `result` column or its eval body is wrong.
    Diverges,
}

impl ParityVerdict {
    /// Severity rank: `Matches` < `Vacuous` < `Diverges`.
    ///
    /// A row swept at several probes is only as certified as its WEAKEST probe
    /// — one shape that matches says nothing about a shape that was never
    /// really compared — so [`worst_probe`] reports a row's most severe probe,
    /// and a row with one `Vacuous` shape needs a ledger entry even when its
    /// other shapes match.
    ///
    /// Spelled as an explicit rank rather than a derived `Ord`, which would
    /// silently rebind this ordering to the variants' declaration order.
    fn severity(self) -> u8 {
        match self {
            ParityVerdict::Matches => 0,
            ParityVerdict::Vacuous => 1,
            ParityVerdict::Diverges => 2,
        }
    }
}

/// Classify one (executed value, declared type) pair.
///
/// # Both trivial-accept sentinels are tested BEFORE the matcher
///
/// `crate::value_type_kind_matches` has two paths that answer `true` while
/// carrying no information — one on each side of the pair, each able to hollow
/// this harness out from its own side:
///
/// - **`Value::Undef`**, the Auto/no-value sentinel, accepted for every type;
/// - **`Type::Error`**, the type-inference poison sentinel, accepted for every
///   VALUE — and short-circuited by the matcher's anti-cascade guard *before*
///   it inspects the value at all.
///
/// Either one alone makes the comparison certify nothing, so both classify as
/// [`ParityVerdict::Vacuous`], and both are tested before the matcher is
/// consulted. That order is load-bearing, not stylistic: reversing these
/// statements makes the harness green over rows it never probed.
///
/// A declared `Type::Error` is unreachable on α's rows — every seed resolver is
/// total and answers a concrete type — but it is exactly what a τ resolver
/// wired to return `None` / `E_BuiltinArgShape` diagnostics (PRD §3 decision 5)
/// begins producing, and such a row must surface as an unledgered `Vacuous`
/// rather than as a silent pass for whatever it happened to evaluate to.
///
/// Otherwise the answer is delegated to `crate::value_type_kind_matches` — the
/// workspace's single static-vs-runtime kind oracle, and the ONLY one this
/// harness may consult. That is literally what I-REG-4's "no second
/// derivation" requires, and it is the reason this module is in-crate at all
/// (the oracle is private). Do not re-author a local kind matcher here as
/// `crates/reify-compiler/tests/harness_type_checking/
/// mul_div_static_runtime_parity.rs` was forced to.
///
/// # Why `registry: None`, stated as a boundary
///
/// The oracle's third argument is an optional `StructureRegistry`, and this
/// harness passes `None`. That is sufficient and correct for every row today:
/// the `Type::StructureRef` arm compares `type_name` only, which is exactly
/// what `analysis::stress_invariants` needs — it builds a registry-free
/// `Value::StructureInstance` whose `type_id` is a sentinel and whose
/// `type_name` is `"StressInvariants"`.
///
/// It is NOT sufficient for `Type::TraitObject`, whose conformance check needs
/// a live registry from a constructed `Engine`. So a future τ row declaring a
/// trait-object result will read as [`ParityVerdict::Diverges`] here, and the
/// right response is a ledger entry naming that τ — NOT widening this call
/// into engine construction, which would drag the harness out of a unit test
/// and into the engine's whole startup surface.
fn classify(value: &Value, declared: &Type) -> ParityVerdict {
    // Order matters — see the doc above. BOTH sentinels before the matcher,
    // always; it answers `true` for either one.
    if matches!(value, Value::Undef) || declared.is_error() {
        return ParityVerdict::Vacuous;
    }
    if crate::value_type_kind_matches(value, declared, None) {
        ParityVerdict::Matches
    } else {
        ParityVerdict::Diverges
    }
}

// ── membership: derived from the row table, never declared ──────────────────

/// Every `BindingKind::EvalBuiltin` row, paired with the generated sub-enum id
/// it is dispatched on — the harness's single membership source.
///
/// Derived from `reify_builtins::rows()` rather than declared, which is what
/// makes PRD §9's "grows per τ" true on the membership side: a τ migration that
/// registers rows is swept here with **no edit to this module**, and a row that
/// is registered but somehow escapes this filter is caught by the cardinality
/// pin in [`sweep_covers_every_eval_builtin_row`] rather than silently reducing
/// coverage. There is deliberately no local name list and no local id list to
/// fall out of step.
///
/// The `EvalBuiltinId` half of the pair is what [`representative_probes`] keys
/// on, so the same derivation feeds both the membership and the argument side.
///
/// Shape reuse, not a shared import: `crates/reify-stdlib/tests/
/// registry_dispatch_seed_parity.rs`'s `eval_rows()` performs the identical
/// `BindingKind::EvalBuiltin` filter, and its own doc-comment gives the same
/// reason ("derived from the registry rather than restated … so a τ row added
/// later is covered here automatically instead of silently escaping the
/// sweep"). It cannot be imported as it stands — that is a `reify-stdlib`
/// integration-test target, this is a `reify-eval` lib unit-test module, and
/// test targets are separate compilation units with no path between them.
///
/// **The duplication is a scope deferral, not a design conclusion** (recorded
/// at task 6013's amendment pass, correcting an earlier claim here that a
/// shared home would drag a test helper into a production dependency — it would
/// not). The right home is `reify_builtins` itself: a
/// `rows_bound_by(BindingKind)` accessor beside `rows()` is ordinary production
/// code over the row table, involves no test helper at all, and would give both
/// callers ONE derivation. That accessor lives in
/// `crates/reify-builtins/src/lib.rs`, outside this task's lock set, so it is
/// filed as follow-up work rather than reached for here.
fn eval_builtin_rows() -> Vec<(EvalBuiltinId, &'static BuiltinRow<BuiltinId>)> {
    rows()
        .iter()
        .filter(|row| row.binding == BindingKind::EvalBuiltin)
        .map(|row| {
            let id = row.id.as_eval_builtin().unwrap_or_else(|| {
                panic!(
                    "registry inconsistency: row {:?} declares \
                     BindingKind::EvalBuiltin but its id {:?} does not narrow \
                     to an EvalBuiltinId",
                    row.name, row.id
                )
            });
            (id, row)
        })
        .collect()
}

// ── coverage: the sweep sees every EvalBuiltin row ───────────────────────────

/// The sweep's membership must be the WHOLE `EvalBuiltin` group, and must be
/// derived rather than declared.
///
/// The cardinality pin is the point of this test, not decoration. A filter or
/// mapping bug in [`eval_builtin_rows`] that silently SHRINKS coverage — drops
/// a row whose `binding` it fails to match, or loses one whose `id` declines to
/// narrow — would otherwise leave the harness reporting green over fewer rows
/// than exist. That is the failure mode which would quietly hollow this module
/// out as each τ migration adds rows, and it is indistinguishable from success
/// without an independent count. `EvalBuiltinId::COUNT` is that independent
/// count: strum derives it from the generated sub-enum, which the `registry!`
/// macro mints from the same row declarations, so the two can only agree when
/// the derivation is total.
///
/// No seed name, id, or count is restated here — the whole assertion is
/// derived from `reify_builtins`.
#[test]
fn sweep_covers_every_eval_builtin_row() {
    let swept = eval_builtin_rows();

    assert!(
        !swept.is_empty(),
        "the parity sweep is empty — no BindingKind::EvalBuiltin row was \
         derived from reify_builtins::rows(), so every assertion in this \
         module would pass vacuously"
    );

    let swept_ids: HashSet<EvalBuiltinId> = swept.iter().map(|(id, _)| *id).collect();
    let declared_ids: HashSet<EvalBuiltinId> = EvalBuiltinId::iter().collect();
    assert_eq!(
        swept_ids, declared_ids,
        "the swept id set must be exactly the EvalBuiltinId group"
    );

    assert_eq!(
        swept.len(),
        EvalBuiltinId::COUNT,
        "the sweep must visit each EvalBuiltin row exactly once: {} swept vs \
         {} variants in the generated sub-enum",
        swept.len(),
        EvalBuiltinId::COUNT
    );
}

// ── representative arguments: compile-forced, one arm per row ───────────────

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
/// set; filed as follow-up work together with the row filter above.
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
type Probe = Vec<(Value, Type)>;

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
/// module header's "What this harness cannot see" for why that Field probe
/// cannot be written today.
///
/// # Why an exhaustive `match` with no `_` arm
///
/// This is the mechanism that makes PRD §9's "grows per τ" true on the
/// ARGUMENT side, the half [`eval_builtin_rows`] cannot cover. Membership is
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
/// Every probe returned here is self-validated by
/// [`representative_probes_are_well_formed_for_every_row`], so a mis-built
/// probe fails AT THE PROBE rather than being misattributed to the row under
/// test.
fn representative_probes(id: EvalBuiltinId) -> Vec<Probe> {
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

// ── the three-way verdict, unit-pinned arm by arm ───────────────────────────

/// A non-`Undef` value whose kind the matcher accepts for its declared type is
/// the only pass.
#[test]
fn classify_matching_kind_is_matches() {
    assert_eq!(
        classify(
            &Value::Option(None),
            &Type::Option(Box::new(Type::length()))
        ),
        ParityVerdict::Matches
    );
}

/// A non-`Undef` value whose kind the matcher rejects diverges — the row's
/// static claim and its runtime answer disagree.
#[test]
fn classify_mismatching_kind_is_diverges() {
    assert_eq!(
        classify(
            &Value::String("12mm".to_string()),
            &Type::Option(Box::new(Type::length()))
        ),
        ParityVerdict::Diverges
    );
}

/// **The load-bearing arm.** `Value::Undef` must be `Vacuous`, never `Matches`.
///
/// `crate::value_type_kind_matches` accepts `Value::Undef` for ANY type
/// unconditionally, in its Auto/no-value sentinel arm. A two-way `true`/`false`
/// harness would therefore report a pass for every row whose eval body fell off
/// a match arm — exactly the residue PRD §3 decision 12 says this harness
/// exists to close. The module header lists the three in-tree funnels that
/// reach `Undef` with no registry involvement at all, which is why this is not
/// a hypothetical.
///
/// The type is swept deliberately rather than probed once: the point is that
/// `Undef` is `Vacuous` for EVERY declared type, so no row can be certified by
/// supplying one.
#[test]
fn classify_undef_is_vacuous_for_every_declared_type() {
    for declared in [
        Type::Option(Box::new(Type::length())),
        Type::String,
        Type::Enum("Result".to_string()),
        Type::dimensionless_scalar(),
        Type::List(Box::new(Type::dimensionless_scalar())),
        Type::StructureRef("StressInvariants".to_string()),
    ] {
        assert_eq!(
            classify(&Value::Undef, &declared),
            ParityVerdict::Vacuous,
            "Value::Undef must be Vacuous, not a pass, for declared {declared:?}"
        );
        assert!(
            crate::value_type_kind_matches(&Value::Undef, &declared, None),
            "premise of this test: the matcher itself DOES accept Undef for \
             {declared:?}, which is why a two-way verdict would be defeated"
        );
    }
}

/// **The second trivial-accept path**, pinned at the case the `Undef` guard
/// does NOT already cover: a REAL value against `Type::Error`.
///
/// `value_type_kind_matches` short-circuits `Type::Error` to `true` before it
/// inspects the value at all, so without its own guard [`classify`] would
/// report `Matches` for ANY observed value whenever a row's resolver answered
/// `Error` — the same hollowing-out the `Undef` guard prevents, arriving from
/// the other side of the pair.
///
/// Pairing `Type::Error` with `Value::Undef` alone cannot pin this: that case
/// is `Vacuous` on the `Undef` guard whether or not the `Error` guard exists,
/// so it would pass for the wrong reason.
#[test]
fn classify_against_the_error_type_is_vacuous_even_for_a_real_value() {
    let real = Value::String("12mm".to_string());

    assert!(
        crate::value_type_kind_matches(&real, &Type::Error, None),
        "premise of this test: the matcher accepts a REAL value for \
         Type::Error, which is why classify needs a guard of its own"
    );
    assert_eq!(
        classify(&real, &Type::Error),
        ParityVerdict::Vacuous,
        "a declared Type::Error certifies nothing about the row, whatever the \
         row evaluated to"
    );

    // Both sentinels at once. Still Vacuous — and neither guard may be the
    // reason the other one looks tested.
    assert_eq!(classify(&Value::Undef, &Type::Error), ParityVerdict::Vacuous);
}

// ── the probe itself must be well-formed before it can blame a row ──────────

/// Every probe must be well-formed for the row it probes, checked four ways —
/// and every row must have at least one.
///
/// This test exists so that a defective probe fails AT THE PROBE instead of
/// being misattributed to the row under test. Without it, the most likely
/// authoring mistake — arguments of the wrong count or wrong shape — reaches
/// the kernel, the kernel's own guard yields `Value::Undef`, and the sweep
/// reports the row `Vacuous` for a reason that has nothing to do with the row's
/// signature. The harness would then be accusing the registry of a fault in
/// this file.
///
/// The "same number of values as types" check this test used to open with is
/// gone, deliberately: [`Probe`] is a list of PAIRS, so that invariant is now
/// carried by the type and cannot be violated.
#[test]
fn representative_probes_are_well_formed_for_every_row() {
    for (id, row) in eval_builtin_rows() {
        let probes = representative_probes(id);

        // A row with no probe is never executed at all, and its parity
        // assertion would pass without ever running.
        assert!(
            !probes.is_empty(),
            "{id:?}: representative_probes registered no probe, so this row \
             would be swept at no argument shape whatsoever"
        );

        let probe_count = probes.len();
        for (i, probe) in probes.iter().enumerate() {
            let at = format!("{id:?} probe {} of {probe_count}", i + 1);

            // (a) the kernel must actually run. `helpers::{unary,binary}`
            // return Value::Undef on the wrong argc, so a mis-counted probe
            // reads as Vacuous with no bearing on the row.
            assert!(
                row.arity.matches(probe.len()),
                "{at}: {} argument(s) do not match the row's declared arity \
                 {:?} — the kernel would short-circuit to Value::Undef and \
                 the sweep would blame the row for this file's mistake",
                probe.len(),
                row.arity
            );

            // (b) one probe argument per declared slot.
            assert_eq!(
                probe.len(),
                row.arg_slots.len(),
                "{at}: {} argument(s) against {} declared arg_slots",
                probe.len(),
                row.arg_slots.len()
            );

            // (c) each pair is internally consistent under the SAME oracle the
            // verdict uses, so a value typed as something it is not cannot
            // make a row look like it diverges.
            for (slot, (value, ty)) in probe.iter().enumerate() {
                assert!(
                    crate::value_type_kind_matches(value, ty, None),
                    "{at}: argument {slot} is mis-paired — its Value does not \
                     satisfy the Type this probe claims for it ({value:?} vs \
                     {ty:?})"
                );
            }

            // (d) the row's own resolver must accept the probe's static types.
            // An ArgAware resolver answering None means the probe is
            // mis-shaped for the row, which would otherwise surface as an
            // unexplained skip.
            let types: Vec<Type> = probe.iter().map(|(_, ty)| ty.clone()).collect();
            assert!(
                row.result.resolve(&types).is_some(),
                "{at}: the row's ResultSpec declined these arg types \
                 {types:?}, so this probe cannot produce a declared type to \
                 compare against"
            );
        }
    }
}

/// A row's reported probe is its MOST SEVERE one, ties broken to the first.
///
/// Driven over synthetic [`ProbeOutcome`]s rather than real rows, in the same
/// dialect as the synthetic-table adjudication tests below and for the same
/// reason: every α row registers exactly ONE probe, so the aggregation rule
/// would otherwise be unobservable — a rule that silently reported the row's
/// BEST probe would pass the whole suite today and hollow the harness out the
/// moment a τ row registered a second shape.
#[test]
fn worst_probe_reports_the_most_severe_and_breaks_ties_to_the_first() {
    let outcome = |index, verdict| ProbeOutcome {
        index,
        observed: Value::Undef,
        declared: Type::String,
        verdict,
    };

    // A shape that matches does NOT cover a shape that was never really
    // compared — this is the whole reason the rule is "worst", not "any".
    let worst = worst_probe(vec![
        outcome(1, ParityVerdict::Matches),
        outcome(2, ParityVerdict::Vacuous),
    ])
    .expect("non-empty");
    assert_eq!(worst.verdict, ParityVerdict::Vacuous);
    assert_eq!(
        worst.index, 2,
        "the failure line must point at the probe that actually failed"
    );

    // Diverges outranks Vacuous.
    assert_eq!(
        worst_probe(vec![
            outcome(1, ParityVerdict::Vacuous),
            outcome(2, ParityVerdict::Diverges),
        ])
        .expect("non-empty")
        .verdict,
        ParityVerdict::Diverges
    );

    // Ties report the first, so the reported line is stable across runs.
    assert_eq!(
        worst_probe(vec![
            outcome(1, ParityVerdict::Diverges),
            outcome(2, ParityVerdict::Diverges),
        ])
        .expect("non-empty")
        .index,
        1
    );

    // No probe at all is not a pass. `observe_row` panics on this rather than
    // manufacturing a verdict for a row it never executed.
    assert!(worst_probe(vec![]).is_none());
}

// ── the exemption ledger ────────────────────────────────────────────────────

/// One accepted non-[`ParityVerdict::Matches`] row.
///
/// Keyed on `EvalBuiltinId`, **not** on a name string (house heuristic 12 —
/// structured data, never meaningful strings): a row deleted by a later τ turns
/// its entry into a compile error rather than a dead line nobody notices, and
/// there is no string to typo.
///
/// `expected` carries the verdict the row is exempted AT, not merely the fact of
/// an exemption. A row ledgered as `Vacuous` that begins genuinely diverging is
/// then reported rather than silently absorbed — the exemption covers one known
/// disposition, not the row forever.
struct ExemptionEntry {
    /// The row this entry exempts.
    id: EvalBuiltinId,
    /// The verdict this row is known to produce, and is exempted at.
    expected: ParityVerdict,
    /// Which leaf retires this entry, and why ψ cannot.
    why: &'static str,
}

/// Every row whose parity assertion cannot be made non-vacuously today, one
/// entry each. **Empty at α**, and that is the measured state, not a stub.
///
/// # Measured at α: all seven seed rows classify `Matches`
///
/// Observed by running the sweep, not derived by reading the table:
///
/// | row | observed `Value` | declared `Type` |
/// |---|---|---|
/// | `parse_length` | `Option(Some(Scalar{LENGTH}))` | `Option(Scalar{LENGTH})` |
/// | `parse_length_r` | `Enum{type_name:"Result"}` | `Enum("Result")` |
/// | `von_mises` | `Scalar{PRESSURE}` | `Scalar{PRESSURE}` |
/// | `max_shear` | `Scalar{PRESSURE}` | `Scalar{PRESSURE}` |
/// | `principal_stresses` | `List([Scalar{PRESSURE}; 3])` | `List(Scalar{PRESSURE})` |
/// | `safety_factor` | `Real(2.5)` | `Scalar{DIMENSIONLESS}` |
/// | `stress_invariants` | `StructureInstance{type_name:"StressInvariants"}` | `StructureRef("StressInvariants")` |
///
/// Two of those pairings are worth naming because they look like mismatches and
/// are not. `safety_factor` returns a bare `Value::Real` while its resolver
/// answers `Type::dimensionless_scalar()`, which IS
/// `Type::Scalar{DIMENSIONLESS}` (`crates/reify-core/src/ty.rs`) — there is
/// no `Type::Real` variant, and the matcher's `Value::Real` arm accepts
/// `Type::Scalar{..}`. `stress_invariants` returns a registry-free
/// `StructureInstance` whose `type_id` is a sentinel, and the matcher's
/// `Type::StructureRef` arm compares `type_name` only, which is why
/// `registry: None` suffices (see [`classify`]).
///
/// # Why the PRD's named seed entry cannot be here yet
///
/// PRD §4 / §9 name the `piecewise_polynomial` stub as the ledger's first
/// entry. It CANNOT be present: that name has no registry row, so it is not in
/// `eval_builtin_rows()` at all and the sweep never reaches it. It is still
/// answered by the surviving legacy string arm at
/// `crates/reify-stdlib/src/trajectory/mod.rs`
/// (`"piecewise_polynomial" => Some(Value::Undef)`), and it joins this ledger —
/// as a `Vacuous` entry, which is why that verdict class must exist — when
/// τ-mechanism/trajectory migrates the family.
///
/// # The dialect
///
/// One WHY sentence per entry naming the leaf that retires it, exactly as
/// `SEED_STRING_DISPATCH_LEDGER`
/// (`crates/reify-builtins/tests/i_reg_1_seed_string_dispatch_gate.rs`)
/// and `registry_drift_tests`' `QUERY_CALL_LEDGER` do. The point is that the
/// residue is COUNTED, not that it is acceptable. An empty ledger is only
/// meaningful because it is enforced in BOTH directions — an unledgered
/// non-`Matches` row fails, and so does a stale entry — so it cannot be padded
/// with entries that assert nothing.
const PARITY_EXEMPTION_LEDGER: &[ExemptionEntry] = &[];

// ── adjudication: one rule, both directions ─────────────────────────────────

/// One way a row's observed verdict fails to agree with the ledger.
///
/// Carries the row and the verdicts involved as DATA, so the human message is
/// rendered at the point of failure by [`describe_failure`] rather than being
/// pre-formatted into a string here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// The row does not pass and nothing accepted that.
    Unledgered {
        id: EvalBuiltinId,
        observed: ParityVerdict,
    },
    /// The row PASSES but still carries an exemption. The entry must be
    /// deleted — this is the direction that makes the ledger a ratchet.
    Stale {
        id: EvalBuiltinId,
        ledgered: ParityVerdict,
    },
    /// The row still does not pass, but at a different disposition than the one
    /// it was exempted at, so the exemption no longer describes it.
    VerdictChanged {
        id: EvalBuiltinId,
        observed: ParityVerdict,
        ledgered: ParityVerdict,
    },
}

impl Failure {
    /// The row this failure is about, for joining back to its observation.
    fn id(&self) -> EvalBuiltinId {
        match self {
            Failure::Unledgered { id, .. }
            | Failure::Stale { id, .. }
            | Failure::VerdictChanged { id, .. } => *id,
        }
    }
}

/// The single adjudication rule, in both directions.
///
/// Pure over its two arguments so the real sweep and the synthetic-table tests
/// share ONE implementation (SPOT): there is no second copy of this rule to
/// drift, and the staleness direction is proven without mutating either the
/// real ledger or a real row.
///
/// A ledger entry naming an id absent from `observed` needs no variant: every
/// `EvalBuiltinId` variant is present in `eval_builtin_rows()` — pinned by
/// [`sweep_covers_every_eval_builtin_row`]'s cardinality assertion — and a row
/// a later τ deletes or re-homes to another `BindingKind` takes its variant with
/// it, making its entry a compile error rather than a dead line. That is the
/// point of keying on the id.
fn adjudicate(
    observed: &[(EvalBuiltinId, ParityVerdict)],
    ledger: &[ExemptionEntry],
) -> Vec<Failure> {
    observed
        .iter()
        .filter_map(|&(id, verdict)| {
            let entry = ledger.iter().find(|entry| entry.id == id);
            match (verdict, entry) {
                (ParityVerdict::Matches, None) => None,
                (ParityVerdict::Matches, Some(entry)) => Some(Failure::Stale {
                    id,
                    ledgered: entry.expected,
                }),
                (observed, None) => Some(Failure::Unledgered { id, observed }),
                (observed, Some(entry)) if entry.expected == observed => None,
                (observed, Some(entry)) => Some(Failure::VerdictChanged {
                    id,
                    observed,
                    ledgered: entry.expected,
                }),
            }
        })
        .collect()
}

/// What the sweep observed at ONE probe of one row.
struct ProbeOutcome {
    /// 1-based position in the row's probe list, so a failure line points at
    /// the shape that actually failed.
    index: usize,
    observed: Value,
    declared: Type,
    verdict: ParityVerdict,
}

/// The probe a row is reported at: its most severe, ties broken to the first.
///
/// A row is only as certified as its WEAKEST probe (see
/// [`ParityVerdict::severity`]), so a row with one `Vacuous` shape is reported
/// `Vacuous` and needs a ledger entry even when its other shapes match. Ties
/// resolve to the first so the reported line is stable across runs.
///
/// `None` only for an empty probe list, which [`observe_row`] rejects outright:
/// a row swept at no shape has not been checked, and must not read as a pass.
fn worst_probe(outcomes: Vec<ProbeOutcome>) -> Option<ProbeOutcome> {
    outcomes.into_iter().reduce(|worst, next| {
        if next.verdict.severity() > worst.verdict.severity() {
            next
        } else {
            worst
        }
    })
}

/// What the sweep observed for one row — the evidence a [`Failure`] is rendered
/// against, taken from the row's [`worst_probe`].
struct RowObservation {
    id: EvalBuiltinId,
    name: &'static str,
    /// 1-based index of the reported probe, and how many the row was swept at.
    probe_index: usize,
    probe_count: usize,
    observed: Value,
    declared: Type,
    verdict: ParityVerdict,
}

/// Sweep one row at every probe it registers, and report its most severe.
fn observe_row(id: EvalBuiltinId, row: &'static BuiltinRow<BuiltinId>) -> RowObservation {
    let probes = representative_probes(id);
    let probe_count = probes.len();

    let outcomes: Vec<ProbeOutcome> = probes
        .into_iter()
        .enumerate()
        .map(|(i, probe)| {
            let (values, types): (Vec<Value>, Vec<Type>) = probe.into_iter().unzip();

            let declared = row
                .result
                .resolve(&types)
                .expect("probe well-formedness (d) guarantees the resolver answers");

            // The public path. `row.name` is a VARIABLE — see the sweep's doc.
            let observed = reify_stdlib::eval_builtin(row.name, &values);
            let verdict = classify(&observed, &declared);

            ProbeOutcome {
                index: i + 1,
                observed,
                declared,
                verdict,
            }
        })
        .collect();

    let worst = worst_probe(outcomes).unwrap_or_else(|| {
        panic!(
            "{id:?}: no representative probe, so this row would be swept at no \
             argument shape at all — see \
             representative_probes_are_well_formed_for_every_row, which reports \
             the same fault with the remedy"
        )
    });

    RowObservation {
        id,
        name: row.name,
        probe_index: worst.index,
        probe_count,
        observed: worst.observed,
        declared: worst.declared,
        verdict: worst.verdict,
    }
}

/// The stated reason of the ledger entry a [`Failure`] concerns.
///
/// [`Failure::Stale`] and [`Failure::VerdictChanged`] are constructed by
/// [`adjudicate`] only on the `Some(entry)` branches, so the lookup cannot
/// fail. The `expect` states that invariant rather than inventing a placeholder
/// string for a branch no test could ever reach.
fn ledgered_why(id: EvalBuiltinId, ledger: &[ExemptionEntry]) -> &'static str {
    ledger
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.why)
        .expect("Stale / VerdictChanged are adjudicated only from a row that carries an entry")
}

/// Render one [`Failure`] as a reader-facing line, against the observation that
/// produced it and the ledger entry it concerns.
///
/// Takes THE observation rather than the whole list: every `Failure` is
/// adjudicated from an entry of that list, so a per-call search would be a
/// lookup that cannot miss, and its miss branch would be unreachable control
/// flow no test could exercise. The sweep resolves the join once and hands the
/// result in.
///
/// The observed value is printed in full rather than through a local
/// variant-name table: the leading token of a Rust enum's `Debug` output IS its
/// discriminant, and `reify_ir` owns the variant list — a name table here would
/// be a silently drifting second copy of it (SPOT). For a `Vacuous` verdict this
/// prints the bare `Undef`; for `Diverges` it prints the payload, which is
/// exactly what the reader needs.
fn describe_failure(
    failure: &Failure,
    observation: &RowObservation,
    ledger: &[ExemptionEntry],
) -> String {
    let id = failure.id();
    let name = observation.name;

    match failure {
        Failure::Unledgered { observed, .. } => format!(
            "  UNLEDGERED {id:?} ({name:?}) — verdict {observed:?} at probe \
             {} of {}; observed {:?}, declared {:?}",
            observation.probe_index,
            observation.probe_count,
            observation.observed,
            observation.declared
        ),
        Failure::Stale { ledgered, .. } => format!(
            "  STALE LEDGER ENTRY {id:?} ({name:?}) — the row now \
             classifies Matches but is still exempted at {ledgered:?}. \
             Delete the entry; its stated reason was: {}",
            ledgered_why(id, ledger)
        ),
        Failure::VerdictChanged {
            observed, ledgered, ..
        } => format!(
            "  VERDICT CHANGED {id:?} ({name:?}) — exempted at {ledgered:?} \
             but now {observed:?}. Right row, wrong disposition: fix the \
             divergence or update the entry, whose stated reason was: {}",
            ledgered_why(id, ledger)
        ),
    }
}

// ── the executed sweep: the harness's headline assertion ────────────────────

/// **The headline assertion** (PRD §3 decision 12 / §7.2 I-REG-4 / §8 row 9).
///
/// For every `BindingKind::EvalBuiltin` row: synthesize representative args,
/// evaluate them through the PUBLIC `reify_stdlib::eval_builtin`, resolve the
/// row's declared result type from the same args' static types, and classify
/// the pair. Only [`ParityVerdict::Matches`] is a pass; anything else must be
/// named in [`PARITY_EXEMPTION_LEDGER`].
///
/// # Why the public path, not α's id-keyed shim
///
/// `reify_stdlib::eval_builtin` runs the full 26-arm dispatch chain with the
/// registry hoisted to its front (`registry_dispatch::try_dispatch`), so it
/// asserts strictly more than `__registry_dispatch_for_test` would: a later
/// family arm that shadowed a registered name — a live migration hazard while
/// the great majority of builtin names are still string-matched — surfaces here
/// as a kind mismatch. The shim takes an `EvalBuiltinId` and therefore bypasses
/// name resolution by construction, as its own doc-comment says.
///
/// # I-REG-1 is satisfied twice over
///
/// `row.name` is passed as a **variable**, derived from `reify_builtins::rows()`
/// — no builtin-name literal appears anywhere in this file, so the seed
/// string-dispatch gate
/// (`crates/reify-builtins/tests/i_reg_1_seed_string_dispatch_gate.rs`) has
/// nothing to find even before its `#[cfg(test)]`-block masking applies. No
/// ledger entry there is needed or wanted.
///
/// # Every row is reported, not just the first
///
/// Verdicts are accumulated across the whole table before asserting. A τ
/// migration lands several rows at once, and a first-failure abort would make
/// the reader re-run the harness once per broken row.
#[test]
fn every_eval_builtin_row_agrees_with_its_executed_kind() {
    let observations: Vec<RowObservation> = eval_builtin_rows()
        .into_iter()
        .map(|(id, row)| observe_row(id, row))
        .collect();

    // ONE adjudication rule, shared with the synthetic-table tests below.
    let verdicts: Vec<(EvalBuiltinId, ParityVerdict)> = observations
        .iter()
        .map(|obs| (obs.id, obs.verdict))
        .collect();
    let failures = adjudicate(&verdicts, PARITY_EXEMPTION_LEDGER);

    let report: Vec<String> = failures
        .iter()
        .map(|failure| {
            let observation = observations
                .iter()
                .find(|obs| obs.id == failure.id())
                .expect("every Failure is adjudicated from an observation in this same list");
            describe_failure(failure, observation, PARITY_EXEMPTION_LEDGER)
        })
        .collect();

    assert!(
        report.is_empty(),
        "static-vs-runtime parity: {} EvalBuiltin row(s) disagree with \
         PARITY_EXEMPTION_LEDGER.\n\n{}\n\n\
         A `Diverges` verdict means the row's declared `result` and its eval \
         body disagree about the KIND of the returned value — fix whichever is \
         wrong. A `Vacuous` verdict means one side of the pair was a \
         trivial-accept sentinel — the row evaluated to `Value::Undef`, or its \
         resolver answered `Type::Error` — either of which \
         `value_type_kind_matches` accepts unconditionally, so the parity \
         assertion certifies NOTHING about the row; check \
         `representative_probes` first, since a mis-shaped probe produces \
         exactly this. The line names the probe it is about. If the \
         divergence genuinely belongs to a later leaf, add an entry to \
         PARITY_EXEMPTION_LEDGER naming that leaf. A STALE or VERDICT CHANGED \
         line is the opposite problem: the ledger no longer describes the row, \
         so the entry must be deleted or updated.",
        report.len(),
        report.join("\n")
    );
}

// ── the ledger's second direction, tested over synthetic tables ─────────────
//
// These five tests drive [`adjudicate`] over LOCAL tables rather than over the
// real rows. That is deliberate, and it is what lets the real ledger be
// legitimately EMPTY while the ledger MECHANISM is still proven: the
// divergence, staleness and verdict-change paths are all exercised without
// committing a single mutating test against `PARITY_EXEMPTION_LEDGER` or
// against any registry row.
//
// The staleness direction is what gives an empty ledger teeth. Without it the
// ledger could be padded with entries that assert nothing, and a divergence
// fixed by a later τ would leave a permanent false exemption behind.

/// A synthetic ledger entry, so the tests below never touch the real one.
fn synthetic(id: EvalBuiltinId, expected: ParityVerdict) -> ExemptionEntry {
    ExemptionEntry {
        id,
        expected,
        why: "synthetic — belongs to this test only, never to the real ledger",
    }
}

/// (a) A divergence nobody accepted must be reported.
#[test]
fn adjudicate_flags_an_unledgered_divergence() {
    let observed = [(EvalBuiltinId::ParseLength, ParityVerdict::Diverges)];
    assert_eq!(
        adjudicate(&observed, &[]),
        vec![Failure::Unledgered {
            id: EvalBuiltinId::ParseLength,
            observed: ParityVerdict::Diverges,
        }]
    );
}

/// (b) A divergence with a matching entry is accepted — the residue is counted,
/// not forbidden.
#[test]
fn adjudicate_accepts_a_ledgered_divergence() {
    let observed = [(EvalBuiltinId::ParseLength, ParityVerdict::Diverges)];
    let ledger = [synthetic(EvalBuiltinId::ParseLength, ParityVerdict::Diverges)];
    assert_eq!(adjudicate(&observed, &ledger), vec![]);
}

/// (c) **The staleness direction.** A row that now passes must not keep its
/// exemption: the entry has to be DELETED, and the ledger must SHRINK.
#[test]
fn adjudicate_flags_a_stale_entry_whose_row_now_passes() {
    let observed = [(EvalBuiltinId::ParseLength, ParityVerdict::Matches)];
    let ledger = [synthetic(EvalBuiltinId::ParseLength, ParityVerdict::Diverges)];
    assert_eq!(
        adjudicate(&observed, &ledger),
        vec![Failure::Stale {
            id: EvalBuiltinId::ParseLength,
            ledgered: ParityVerdict::Diverges,
        }]
    );
}

/// (d) Right row, wrong disposition. A row ledgered as `Diverges` that is now
/// `Vacuous` (or vice versa) is still not passing, but the exemption no longer
/// describes it — so it is reported rather than absorbed. This is the case an
/// id-only ledger could not express.
#[test]
fn adjudicate_flags_a_ledgered_row_whose_verdict_changed() {
    let observed = [(EvalBuiltinId::ParseLength, ParityVerdict::Vacuous)];
    let ledger = [synthetic(EvalBuiltinId::ParseLength, ParityVerdict::Diverges)];
    assert_eq!(
        adjudicate(&observed, &ledger),
        vec![Failure::VerdictChanged {
            id: EvalBuiltinId::ParseLength,
            observed: ParityVerdict::Vacuous,
            ledgered: ParityVerdict::Diverges,
        }]
    );
}

/// (e) Nothing observed, nothing ledgered, nothing to report — the degenerate
/// case, pinned so the adjudicator cannot manufacture a failure from thin air.
#[test]
fn adjudicate_is_silent_on_empty_tables() {
    assert_eq!(adjudicate(&[], &[]), vec![]);
}
