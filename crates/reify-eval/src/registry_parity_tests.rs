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
//! representative args  ->  reify_expr::eval_expr(`row.name(args…)`)  ->  Value
//!                                                                          |
//!                      crate::value_type_kind_matches(value, declared, None)
//!                                                                          |
//!            declared = row.result.resolve(&arg types)                  verdict
//! ```
//!
//! The registry's `result` column is a STATIC claim; reify-expr's evaluator,
//! running the call exactly as the engine would, is the RUNTIME answer (see
//! [`evaluate_call`]). Everything α's own tests pin is one side or the other —
//! the row table is checked for internal consistency, and the eval kernels are
//! checked value-for-value against their legacy string path. Neither closes the
//! residue PRD decision 12 names: **a buggy eval body returning the wrong
//! kind**, which is invisible to a table-only test and invisible to a
//! value-only test whose expectation was authored from the same buggy body.
//! This module is the join — the only place the two sides meet under execution
//! — but it is a join over the argument shapes it actually probes. Read "What
//! this harness cannot see" below before treating a green sweep as closing
//! decision 12 for a whole row.
//!
//! A probe whose declared type is a `Type::Field` is compared twice (see
//! [`probe_legs`]): the call's result against the declared Field, then one
//! sample of that result, taken through reify-expr's `sample` intercept,
//! against the declared codomain — both by the same oracle. The first leg
//! alone cannot tell the analysis rows' Field claims apart, because the oracle
//! maps a `Value::Field` onto ANY `Type::Field` and those claims differ only in
//! codomain.
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
//!    reify-compiler, reify-expr and reify-stdlib; reify-builtins is a DEV-dep
//!    only (`crates/reify-eval/Cargo.toml`), so `cargo tree -e no-dev -p
//!    reify-eval` is unchanged and `engine_hash_closure.txt` needed no edit.
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
//! - **The oracle is kind-level.** [`crate::value_type_kind_matches`] compares
//!   a value's outer KIND against a type. Dimensions and inner element types
//!   are not compared, for concrete and Field shapes alike: a kernel answering
//!   `Scalar<LENGTH>` where `Scalar<PRESSURE>` is declared, a list of the
//!   wrong element type, or a `Value::Point` of the wrong length against a
//!   `Type::Point` domain (a 2-D sample point for a `Point3<Length>` Field,
//!   say) reads `Matches` — on the probe checks as on the verdict.
//! - **A Field result is sampled at one point.** The Sample leg (see
//!   [`probe_legs`]) executes the declared codomain at a single point of the
//!   domain, so a codomain that held there and changed kind elsewhere would not
//!   be seen.
//! - **Only the two backings `tensor_backing` admits are probed** — Callable
//!   (an `Analytical` lambda) and Grid (a `Sampled` grid), in
//!   `crates/reify-expr/src/analysis.rs`. reify-expr declines a field with any
//!   other backing (an `Imported` one, say), the call evaluates to
//!   `Value::Undef`, and the probe would read `Vacuous`. The resolvers work on
//!   types and cannot see a backing, the asymmetry `field_tensor_arg`
//!   (`crates/reify-builtins/src/resolvers.rs`) documents. The two backings
//!   also check different things: the Callable backing samples through the
//!   pointwise builtin, so its sample's kind does not depend on the codomain
//!   label reify-expr writes on the wrapper, while the Grid backing wraps its
//!   interpolated values per that label — so only the Grid probe joins the
//!   label against the registry (mutation Leg 5 below).
//! - **The call is stamped with the type it is judged against.**
//!   [`evaluate_call`] gives the call the row's DECLARED type as its
//!   `result_type`, as the compiler does in production. A row whose reify-expr
//!   arm reads `result_type` must be compared against an independently stamped
//!   type, or ledgered.
//! - **The residue is closed only for rows whose probes exist.** PRD §3
//!   decision 12's residue is closed for the `Type::Field` branch of
//!   `reduce_tensor_arg` (`von_mises`, `max_shear`, `principal_stresses`,
//!   `safety_factor`) because [`representative_probes`] registers a Callable
//!   and a Grid Field probe for each, beside the concrete one. A later
//!   resolver branch is covered only once its shape is registered there too.
//!
//! The resolvers' own `Type`-level Field contract is also pinned by
//! `reify_builtins::resolvers`' unit tests, ported from #6577 — a type-algebra
//! test on one side of the pair, which this module joins to execution.
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
//! (task 6013 ψ step 11, re-confirmed at the amendment pass; Legs 4–6 added
//! by #7563 for the Field half). **No mutating test is committed**, here or
//! anywhere: a committed mutation asserts the bug, not the contract. The
//! permanent guards are the tests in this module; this recipe is how you
//! re-confirm each one still fires when a τ migration makes you doubt it.
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
//! signal.** In the child module `registry_parity_tests/probes.rs`, split
//! [`representative_probes`]' shared `ParseLength | ParseLengthR` arm and give
//! `ParseLength` an argument-less probe (`vec![Probe::concrete(vec![])]`;
//! `Probe::concrete` is private to that module, where the edit is made). The
//! kernel's arity guard yields `Value::Undef` and TWO tests go RED: the sweep
//! reports `ParseLength` UNLEDGERED at verdict `Vacuous`, and
//! `representative_probes_are_well_formed_for_every_row` fails its check (a).
//! That pair is the intended double signal — the second says the fault is in
//! the harness's probe data rather than in the row, which is exactly the
//! misattribution it exists to prevent. Revert the arm.
//!
//! **Leg 4 — the eval side reduces a Field eagerly** (the Call leg). In
//! `crates/reify-expr/src/analysis.rs`, make `compute_max_shear` return
//! `Value::Real(0.0)` instead of the `wrap_tensor_field` wrapper. Expect the
//! sweep RED with `MaxShear` UNLEDGERED at verdict `Diverges`, at a Field
//! probe's Call leg. Revert the function.
//!
//! **Leg 5 — reify-expr writes the wrong codomain label** (the Grid probe's
//! Sample leg). In the same file, set `compute_principal_stresses`'
//! `result_codomain` to the bare scalar type instead of `Type::List(..)`.
//! Expect the sweep RED with `PrincipalStresses` at verdict `Diverges` at the
//! GRID probe's Sample leg, while its Callable probe's legs still match — the
//! split that shows the two backings check different things. Revert the line.
//!
//! **Leg 6 — the resolver claims an eager result for a Field** (the #6577 bug
//! class). In `crates/reify-builtins/src/resolvers.rs`, make
//! `reduce_tensor_arg`'s Field branch return `Some(codomain(dim))`. Expect the
//! sweep RED with all four analysis rows at verdict `Diverges` at a Field
//! probe's Call leg. `representative_probes_are_well_formed_for_every_row`'s
//! check (e) and the premise of
//! `a_field_probe_is_compared_at_its_call_and_at_a_sample_of_its_result` go RED
//! with it, since the probes name sample points for a type that is no longer a
//! Field. Revert the branch.
//!
//! Legs 1, 2, 4, 5 and 6 each transiently edit another crate, so `git status`
//! must be clean of `registry.rs`, `parse.rs`, reify-expr's `analysis.rs` and
//! reify-builtins' `resolvers.rs` before anything is committed.

use std::collections::HashSet;

use reify_builtins::{BindingKind, BuiltinId, BuiltinRow, EvalBuiltinId, rows_bound_by};
use reify_core::Type;
use reify_ir::{CompiledExpr, Value, ValueMap};
use strum::{EnumCount, IntoEnumIterator};

mod probes;
use probes::{Probe, representative_probes};

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
    /// really compared — so [`worst_outcome`] reports a row's most severe
    /// probe leg, and a row with one `Vacuous` shape needs a ledger entry even
    /// when its other shapes match.
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
/// Membership itself comes from `reify_builtins::rows_bound_by`, the single
/// derivation shared with reify-stdlib's `registry_dispatch_seed_parity` test.
fn eval_builtin_rows() -> Vec<(EvalBuiltinId, &'static BuiltinRow<BuiltinId>)> {
    rows_bound_by(BindingKind::EvalBuiltin)
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
        Type::point3(Type::length()),
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

/// Whether a probe's `value` is well-formed for the `ty` it is paired with:
/// exactly when the verdict would classify the pair [`ParityVerdict::Matches`].
/// The two sentinels the verdict reads as `Vacuous` — `Value::Undef` and a
/// `Type::Error` slot — therefore fail at the probe rather than being blamed on
/// the row.
fn matches_non_vacuously(value: &Value, ty: &Type) -> bool {
    classify(value, ty) == ParityVerdict::Matches
}

/// The first argument of `probe`, by slot, whose value is not well-formed for
/// the type the probe pairs it with.
fn first_ill_formed_argument(probe: &Probe) -> Option<(usize, &(Value, Type))> {
    probe
        .args
        .iter()
        .enumerate()
        .find(|(_, (value, ty))| !matches_non_vacuously(value, ty))
}

/// `probe`'s sample point with the domain it falls short of, when `declared` is
/// a `Type::Field` and the point is not well-formed for that domain. A
/// `Type::Error` domain accepts every point and so certifies none: it is
/// reported too.
fn ill_formed_sample_point<'a>(
    probe: &'a Probe,
    declared: &'a Type,
) -> Option<(&'a Value, &'a Type)> {
    match (&probe.sample_at, declared) {
        (Some(point), Type::Field { domain, .. }) if !matches_non_vacuously(point, domain) => {
            Some((point, domain))
        }
        _ => None,
    }
}

/// Every probe must be well-formed for the row it probes, checked five ways —
/// and every row must have at least one.
///
/// This test exists so that a defective probe fails AT THE PROBE instead of
/// being misattributed to the row under test. Without it, the most likely
/// authoring mistake — arguments of the wrong count or wrong shape — reaches
/// the kernel, the kernel's own guard yields `Value::Undef`, and the sweep
/// reports the row `Vacuous` for a reason that has nothing to do with the row's
/// signature. The harness would then be accusing the registry of a fault in
/// `registry_parity_tests/probes.rs`.
///
/// The "same number of values as types" check this test used to open with is
/// gone, deliberately: [`Probe::args`] is a list of PAIRS, so that invariant
/// is now carried by the type and cannot be violated.
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
                row.arity.matches(probe.args.len()),
                "{at}: {} argument(s) do not match the row's declared arity \
                 {:?} — the kernel would short-circuit to Value::Undef and \
                 the sweep would blame the row for a mistake in \
                 registry_parity_tests/probes.rs",
                probe.args.len(),
                row.arity
            );

            // (b) one probe argument per declared slot.
            assert_eq!(
                probe.args.len(),
                row.arg_slots.len(),
                "{at}: {} argument(s) against {} declared arg_slots",
                probe.args.len(),
                row.arg_slots.len()
            );

            // (c) each pair is internally consistent under the verdict's own
            // rule (`matches_non_vacuously`, i.e. `classify`): a value typed as
            // something it is not cannot make a row look like it diverges, and
            // an Undef argument is rejected here instead of being blamed on the
            // row as Vacuous.
            if let Some((slot, (value, ty))) = first_ill_formed_argument(probe) {
                panic!(
                    "{at}: argument {slot} is mis-paired — its Value does not \
                     satisfy the Type this probe claims for it ({value:?} vs \
                     {ty:?})"
                );
            }

            // (d) the row's own resolver must accept the probe's static types.
            // An ArgAware resolver answering None means the probe is
            // mis-shaped for the row, which would otherwise surface as an
            // unexplained skip.
            let types = probe.arg_types();
            let declared = row.result.resolve(&types).unwrap_or_else(|| {
                panic!(
                    "{at}: the row's ResultSpec declined these arg types \
                     {types:?}, so this probe cannot produce a declared type \
                     to compare against"
                )
            });

            // (e) a sample point exactly when the declared type is a Field.
            // A Field-shaped probe with no point would leave its codomain
            // claim unexecuted, and a point on any other probe names a Sample
            // leg that can never run.
            let declares_field = matches!(declared, Type::Field { .. });
            assert_eq!(
                probe.sample_at.is_some(),
                declares_field,
                "{at}: the probe {} a sample point but the row's resolver \
                 answers {declared:?} — a probe names a sample point exactly \
                 when its declared type is a Type::Field",
                if probe.sample_at.is_some() {
                    "names"
                } else {
                    "names no"
                }
            );

            // ...and that point satisfies the declared Field's domain, under the
            // same rule as (c). A point of the wrong kind, or a Value::Undef
            // one, would make `sample` yield Value::Undef, and the sweep would
            // report the row Vacuous for a fault in
            // `registry_parity_tests/probes.rs` — so it is caught here, not at
            // the Sample leg.
            if let Some((point, domain)) = ill_formed_sample_point(probe, &declared) {
                assert!(
                    !domain.is_error(),
                    "{at}: the declared Field's domain is Type::Error \
                     ({declared:?}), which accepts every point, so the probe's \
                     sample point ({point:?}) cannot be certified against it — \
                     the fault is in the row's resolver, or in a Field type a \
                     probe argument hands it, not in the point"
                );
                panic!(
                    "{at}: the probe's sample point is mis-built — it does not \
                     satisfy the declared Field's domain ({point:?} vs \
                     {domain:?}), so its Sample leg would read Vacuous and \
                     blame the row for a fault in \
                     registry_parity_tests/probes.rs"
                );
            }
        }
    }
}

/// An argument the verdict would read `Vacuous` — a `Value::Undef`, or a value
/// typed `Type::Error` — is rejected AT THE PROBE, not found by the sweep and
/// blamed on the row.
///
/// Drives [`first_ill_formed_argument`], the helper behind the sweep's check
/// (c), over synthetic probes, in the dialect of
/// [`classify_undef_is_vacuous_for_every_declared_type`]: every real probe is
/// well-formed, so the rule is otherwise unobservable. The raw
/// `crate::value_type_kind_matches` accepts both rejected pairs (the premises of
/// the two `classify_*` tests above), so a check built on it would let them
/// through.
#[test]
fn a_probe_argument_that_would_read_vacuous_is_rejected_at_the_probe() {
    let rejected_slot = |args: Vec<(Value, Type)>| {
        let probe = Probe {
            args,
            sample_at: None,
        };
        first_ill_formed_argument(&probe).map(|(slot, _)| slot)
    };
    let text = || Value::String("12mm".to_string());

    assert_eq!(
        rejected_slot(vec![(text(), Type::String)]),
        None,
        "control: a pair the verdict classifies Matches passes, so the \
         rejections below are the rule's and not a blanket refusal"
    );
    assert_eq!(
        rejected_slot(vec![(text(), Type::String), (Value::Undef, Type::String)]),
        Some(1),
        "a Value::Undef argument must be rejected at the probe, at its own \
         slot: the kernel would answer Undef, so the Call leg would read \
         Vacuous and blame the row for the probe's fault"
    );
    assert_eq!(
        rejected_slot(vec![(text(), Type::Error)]),
        Some(0),
        "a Type::Error slot accepts any value, so it never shows the \
         argument's Value consistent with the Type the resolver reads: the \
         pair must be rejected at the probe"
    );
}

/// A sample point the verdict would read `Vacuous` against the declared Field's
/// domain — a `Value::Undef`, or any point against a `Type::Error` domain — is
/// rejected AT THE PROBE, not found by the sweep and blamed on the row.
///
/// Drives [`ill_formed_sample_point`], the helper behind the sweep's
/// sample-point check, over synthetic probes, as
/// [`a_probe_argument_that_would_read_vacuous_is_rejected_at_the_probe`] does for
/// check (c).
#[test]
fn a_probe_sample_point_that_would_read_vacuous_is_rejected_at_the_probe() {
    let ill_formed = |point: Value, domain: Type| {
        let declared = Type::Field {
            domain: Box::new(domain),
            codomain: Box::new(Type::dimensionless_scalar()),
        };
        let probe = Probe {
            args: vec![],
            sample_at: Some(point),
        };
        ill_formed_sample_point(&probe, &declared).is_some()
    };
    let point = || Value::Point(vec![Value::length(0.5); 3]);
    let domain = || Type::point3(Type::length());

    assert!(
        !ill_formed(point(), domain()),
        "control: a point the verdict classifies Matches passes, so the \
         rejections below are the rule's and not a blanket refusal"
    );
    assert!(
        ill_formed(Value::Undef, domain()),
        "a Value::Undef sample point must be rejected at the probe: `sample` \
         would yield Undef, so the Sample leg would read Vacuous and blame the \
         row for the probe's fault"
    );
    assert!(
        ill_formed(point(), Type::Error),
        "a Type::Error domain accepts every point and so certifies none: it \
         must be reported, not passed"
    );
}

/// A row is reported at the MOST SEVERE leg of any of its probes, located at
/// that probe, ties broken to the first.
///
/// Driven over synthetic [`LegOutcome`]s rather than real rows, in the same
/// dialect as the synthetic-table adjudication tests below and for the same
/// reason: every real row matches on every probe and leg, so the aggregation
/// rule would otherwise be unobservable — a rule that silently reported the
/// row's BEST outcome, or kept only each probe's Call leg, would pass the whole
/// suite today and hollow the harness out the moment one shape, or one
/// codomain, stopped matching.
#[test]
fn worst_outcome_reports_the_most_severe_leg_of_any_probe_and_breaks_ties_to_the_first() {
    let leg = |leg: Leg, verdict: ParityVerdict| LegOutcome {
        leg,
        observed: Value::Undef,
        declared: Type::String,
        verdict,
    };
    let call = |verdict| leg(Leg::Call, verdict);
    let sample = |verdict| leg(Leg::Sample, verdict);
    let reported = |legs_per_probe| {
        let ProbeOutcome { index, outcome } = worst_outcome(legs_per_probe).expect("non-empty");
        (index, outcome.leg, outcome.verdict)
    };

    // A shape that matches does NOT cover a shape that was never really
    // compared — this is the whole reason the rule is "worst", not "any". The
    // index lets the failure line point at the probe that actually failed.
    assert_eq!(
        reported(vec![
            vec![call(ParityVerdict::Matches)],
            vec![call(ParityVerdict::Vacuous)],
        ]),
        (2, Leg::Call, ParityVerdict::Vacuous)
    );

    // A Field probe whose Call leg matches is still reported at a Sample leg
    // that diverges: the matching Call leg is Field-vs-Field and says nothing
    // about the codomain, which only the Sample leg executes.
    assert_eq!(
        reported(vec![
            vec![call(ParityVerdict::Matches)],
            vec![
                call(ParityVerdict::Matches),
                sample(ParityVerdict::Diverges)
            ],
        ]),
        (2, Leg::Sample, ParityVerdict::Diverges)
    );

    // Diverges outranks Vacuous.
    assert_eq!(
        reported(vec![
            vec![call(ParityVerdict::Vacuous)],
            vec![call(ParityVerdict::Diverges)],
        ]),
        (2, Leg::Call, ParityVerdict::Diverges)
    );

    // Ties report the first, so the reported line is stable across runs.
    assert_eq!(
        reported(vec![
            vec![call(ParityVerdict::Diverges)],
            vec![call(ParityVerdict::Diverges)],
        ]),
        (1, Leg::Call, ParityVerdict::Diverges)
    );

    // No probe at all is not a pass. `observe_row` panics on this rather than
    // manufacturing a verdict for a row it never executed.
    assert!(worst_outcome(vec![]).is_none());
}

// ── a Field probe is compared twice: at its call and at a sample ───────────

/// A Field probe is compared on TWO legs — the call's result against the
/// declared `Type::Field`, then one sample of that result against the declared
/// codomain — both through the same [`classify`].
///
/// Driven over REAL execution of the `VonMises` row's Callable Field probe.
/// Part (b) is the teeth: [`crate::value_type_kind_matches`] maps a
/// `Value::Field` onto ANY `Type::Field`, so the Call leg alone cannot tell a
/// right codomain from a wrong one — and the codomain is the only thing that
/// distinguishes the four analysis rows' Field claims. Only the Sample leg
/// executes it.
#[test]
fn a_field_probe_is_compared_at_its_call_and_at_a_sample_of_its_result() {
    let (_, row) = eval_builtin_rows()
        .into_iter()
        .find(|(id, _)| *id == EvalBuiltinId::VonMises)
        .expect("VonMises is a BindingKind::EvalBuiltin row");
    let field_probe = || {
        representative_probes(EvalBuiltinId::VonMises)
            .into_iter()
            .find(|probe| probe.sample_at.is_some())
            .expect("VonMises registers a Field probe that names a sample point")
    };
    let declared = row
        .result
        .resolve(&field_probe().arg_types())
        .expect("probe well-formedness (d) guarantees the resolver answers");
    let Type::Field { domain, codomain } = &declared else {
        panic!("premise: the resolver answers a Type::Field for a Field probe, got {declared:?}")
    };
    let legs_of = |outcomes: &[LegOutcome]| outcomes.iter().map(|o| o.leg).collect::<Vec<_>>();
    let verdicts_of =
        |outcomes: &[LegOutcome]| outcomes.iter().map(|o| o.verdict).collect::<Vec<_>>();

    // (a) the declared type the row really answers: both legs match, and the
    // Sample leg is judged against the Field's codomain.
    let outcomes = probe_legs(row.name, field_probe(), declared.clone());
    assert_eq!(legs_of(&outcomes), vec![Leg::Call, Leg::Sample]);
    assert_eq!(
        verdicts_of(&outcomes),
        vec![ParityVerdict::Matches, ParityVerdict::Matches],
        "the row's own Field claim must agree with its execution on both legs"
    );
    assert_eq!(
        outcomes[1].declared, **codomain,
        "the Sample leg must be judged against the declared Field's codomain"
    );

    // (b) a WRONG codomain: the Call leg still matches (Field-vs-Field is
    // shallow), and only the Sample leg sees the lie.
    let wrong = Type::Field {
        domain: domain.clone(),
        codomain: Box::new(Type::List(codomain.clone())),
    };
    let outcomes = probe_legs(row.name, field_probe(), wrong);
    assert_eq!(
        verdicts_of(&outcomes),
        vec![ParityVerdict::Matches, ParityVerdict::Diverges],
        "a wrong codomain must be caught by the Sample leg and only there"
    );

    // (c) no sample point: the Call leg alone.
    let mut unsampled = field_probe();
    unsampled.sample_at = None;
    assert_eq!(
        legs_of(&probe_legs(row.name, unsampled, declared)),
        vec![Leg::Call]
    );
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
/// entry each. **Empty at α, and still empty with the Field probes registered
/// (#7563)** — that is the measured state, not a stub.
///
/// # Measured at α: all seven seed rows classify `Matches` (concrete probes)
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
/// The Field probes (#7563) also classify `Matches` on both legs; their
/// per-leg observations are recorded in that task's commit history.
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

// ── the executor: the engine's own evaluator ────────────────────────────────

/// Execute one call of the builtin `name` the way the engine does: as a
/// `FunctionCall` expression evaluated by `reify_expr::eval_expr`, each argument
/// a typed literal.
///
/// `name` is always a VARIABLE taken from the row table, never a literal (see
/// the sweep's "I-REG-1 is satisfied twice over"). `result_type` is the type
/// the call is compiled at — the declared type, as the compiler would stamp it.
/// Only a few reify-expr arms read it, and for a row reaching one of them the
/// comparison is tautological (see the module header's "What this harness
/// cannot see"). No value cells are bound: every argument is a literal.
fn evaluate_call(name: &str, args: Vec<(Value, Type)>, result_type: Type) -> Value {
    let call = reify_test_support::std_call(
        name,
        args.into_iter()
            .map(|(value, ty)| CompiledExpr::literal(value, ty))
            .collect(),
        result_type,
    );
    let no_bindings = ValueMap::new();
    reify_expr::eval_expr(&call, &reify_expr::EvalContext::simple(&no_bindings))
}

/// reify-expr's field-sampling intercept, through which the Sample leg reads a
/// Field result at a point. Not a registry row, and never passed to
/// `eval_builtin`.
const FIELD_SAMPLE_CALL: &str = "sample";

/// Which comparison of a probe an outcome reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leg {
    /// The call's result against the row's declared type.
    Call,
    /// One sample of a Field result against the declared Field's codomain.
    /// [`crate::value_type_kind_matches`] maps a `Value::Field` onto ANY
    /// `Type::Field`, so this is the only leg that executes a Field claim's
    /// codomain — which is all that tells the analysis rows' Field claims
    /// apart.
    Sample,
}

/// One leg of one probe: what executed, what it was judged against, and the
/// verdict.
struct LegOutcome {
    leg: Leg,
    observed: Value,
    declared: Type,
    verdict: ParityVerdict,
}

/// Execute one probe of the builtin `name` and compare it on every leg it has.
///
/// The Call leg always runs: [`evaluate_call`] at `declared`, classified
/// against `declared`. The Sample leg runs only when the probe names a sample
/// point AND the call produced a `Value::Field` AND `declared` is a
/// `Type::Field`: it evaluates `sample(result, point)` through the same
/// [`evaluate_call`] and classifies the sample against the declared codomain
/// with the same [`classify`], so there is no second kind matcher (I-REG-4).
/// A Field result against a non-Field declared type has already diverged on
/// the Call leg, and a non-Field result has nothing to sample.
fn probe_legs(name: &str, probe: Probe, declared: Type) -> Vec<LegOutcome> {
    let Probe { args, sample_at } = probe;
    let observed = evaluate_call(name, args, declared.clone());
    let call = LegOutcome {
        leg: Leg::Call,
        verdict: classify(&observed, &declared),
        observed,
        declared,
    };

    let sample = match (sample_at, &call.observed, &call.declared) {
        (Some(point), Value::Field { .. }, Type::Field { domain, codomain }) => {
            let sampled = evaluate_call(
                FIELD_SAMPLE_CALL,
                vec![
                    (call.observed.clone(), call.declared.clone()),
                    (point, (**domain).clone()),
                ],
                (**codomain).clone(),
            );
            Some(LegOutcome {
                leg: Leg::Sample,
                verdict: classify(&sampled, codomain),
                observed: sampled,
                declared: (**codomain).clone(),
            })
        }
        _ => None,
    };

    std::iter::once(call).chain(sample).collect()
}

/// A [`LegOutcome`], located at the probe it came from.
struct ProbeOutcome {
    /// 1-based position in the row's probe list, so a failure line points at
    /// the shape that actually failed.
    index: usize,
    outcome: LegOutcome,
}

/// The outcome a row is reported at: the most severe leg of any of its probes,
/// ties broken to the first. `legs_per_probe` holds one list per probe, in
/// probe order, each that probe's legs in leg order.
///
/// A row is only as certified as its WEAKEST probe (see
/// [`ParityVerdict::severity`]), and a probe only as certified as its weaker
/// leg — so a row with one `Vacuous` shape, or one Field probe whose Sample leg
/// diverges, is reported at that outcome even when everything else matches.
/// Ties resolve to the first so the reported line is stable across runs.
///
/// The legs arrive grouped per probe rather than pre-flattened so that the fold
/// locating each leg at its probe belongs to this one pure, unit-tested rule,
/// not to [`observe_row`]'s execution code, where dropping a leg would go
/// unseen by every test while the real sweep is all-`Matches`.
///
/// `None` only when no probe produced a leg, which [`observe_row`] rejects
/// outright: a row swept at no shape has not been checked, and must not read as
/// a pass.
fn worst_outcome(legs_per_probe: Vec<Vec<LegOutcome>>) -> Option<ProbeOutcome> {
    legs_per_probe
        .into_iter()
        .enumerate()
        .flat_map(|(i, legs)| {
            legs.into_iter().map(move |outcome| ProbeOutcome {
                index: i + 1,
                outcome,
            })
        })
        .reduce(|worst, next| {
            if next.outcome.verdict.severity() > worst.outcome.verdict.severity() {
                next
            } else {
                worst
            }
        })
}

/// What the sweep observed for one row — the evidence a [`Failure`] is rendered
/// against.
struct RowObservation {
    id: EvalBuiltinId,
    name: &'static str,
    /// How many probes the row was swept at.
    probe_count: usize,
    /// The outcome the row is reported at: its [`worst_outcome`].
    reported: ProbeOutcome,
}

/// Sweep one row at every leg of every probe it registers, and report its most
/// severe.
fn observe_row(id: EvalBuiltinId, row: &'static BuiltinRow<BuiltinId>) -> RowObservation {
    let probes = representative_probes(id);
    let probe_count = probes.len();

    let legs_per_probe: Vec<Vec<LegOutcome>> = probes
        .into_iter()
        .map(|probe| {
            let declared = row
                .result
                .resolve(&probe.arg_types())
                .expect("probe well-formedness (d) guarantees the resolver answers");

            // `row.name` is a VARIABLE — see the sweep's doc.
            probe_legs(row.name, probe, declared)
        })
        .collect();

    let reported = worst_outcome(legs_per_probe).unwrap_or_else(|| {
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
        probe_count,
        reported,
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
    let ProbeOutcome { index, outcome } = &observation.reported;

    match failure {
        Failure::Unledgered { observed, .. } => format!(
            "  UNLEDGERED {id:?} ({name:?}) — verdict {observed:?} at probe \
             {index} of {}, {:?} leg; observed {:?}, declared {:?}",
            observation.probe_count, outcome.leg, outcome.observed, outcome.declared
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
/// evaluate the call through reify-expr's PUBLIC evaluator ([`evaluate_call`]),
/// resolve the row's declared result type from the same args' static types,
/// and classify the pair. Only [`ParityVerdict::Matches`] is a pass; anything
/// else must be named in [`PARITY_EXEMPTION_LEDGER`].
///
/// # Why reify-expr's evaluator, not `eval_builtin` or α's id-keyed shim
///
/// `reify_expr::eval_expr` is the path the engine evaluates a builtin call
/// through. For every argument shape reify-expr does not intercept, it reaches
/// `reify_stdlib::eval_builtin` through its catch-all arm, and that runs the
/// full dispatch chain with the registry hoisted to its front
/// (`registry_dispatch::try_dispatch`). So everything a direct `eval_builtin`
/// call asserted is still asserted: a later family arm that shadowed a
/// registered name — a live migration hazard while the great majority of
/// builtin names are still string-matched — surfaces here as a kind mismatch.
///
/// It additionally reaches what `eval_builtin` cannot: reify-expr's named
/// intercepts, which answer the four analysis rows over a `Value::Field`
/// argument by wrapping the field lazily (`crates/reify-expr/src/analysis.rs`).
/// Called directly, `eval_builtin` has no Field handling and answers those
/// shapes `Value::Undef`, so the resolvers' `Type::Field` branch would read
/// `Vacuous` and certify nothing.
///
/// α's `__registry_dispatch_for_test` shim asserts strictly less than either:
/// it takes an `EvalBuiltinId` and therefore bypasses name resolution by
/// construction, as its own doc-comment says.
///
/// # I-REG-1 is satisfied twice over
///
/// `row.name` is passed as a **variable**, derived from `reify_builtins::rows()`
/// — no registry-row name literal appears anywhere in this file or in its
/// `probes` child, so the seed string-dispatch gate
/// (`crates/reify-builtins/tests/i_reg_1_seed_string_dispatch_gate.rs`) has
/// nothing to find even before its `#[cfg(test)]`-block masking applies. No
/// ledger entry there is needed or wanted.
///
/// The one call-name literal is [`FIELD_SAMPLE_CALL`], reify-expr's `sample`
/// intercept. It has no registry row, so the gate, which sweeps registered
/// names only, cannot match it, and it is never passed to `eval_builtin`.
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
        .map(|obs| (obs.id, obs.reported.outcome.verdict))
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
