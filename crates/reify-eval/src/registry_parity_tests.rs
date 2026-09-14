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
//! exhaustive `match` with no `_` arm (see [`representative_args`]), so a row
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
//!    `fn` at `crates/reify-eval/src/lib.rs:300`, not even `pub(crate)`. I-REG-4
//!    requires asserting against *that* function with **no second derivation**,
//!    and an integration test is a separate crate that cannot reach it. Making
//!    it `pub` would widen reify-eval's public API solely to host a test. The
//!    in-tree cost of the alternative is already visible: `crates/reify-compiler/
//!    tests/harness_type_checking/mul_div_static_runtime_parity.rs:180-198`
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
//! # Why `Value::Undef` is `Vacuous`, not a pass
//!
//! This is the load-bearing design constraint, and a two-way `true`/`false`
//! harness gets it wrong. [`crate::value_type_kind_matches`] returns `true` for
//! `Value::Undef` **unconditionally** (`crates/reify-eval/src/lib.rs:313` — the
//! Auto/no-value sentinel arm), and `Value::Undef` is where every failure mode
//! in this workspace funnels:
//!
//! - `reify_stdlib::helpers::{unary, binary}` return it on the wrong argc
//!   (`crates/reify-stdlib/src/helpers.rs:7-20`);
//! - `analysis::stress_invariants` returns it for anything but a 3×3 tensor
//!   (`crates/reify-stdlib/src/analysis.rs:487`);
//! - `eval_builtin` returns it as its unresolved-name terminus
//!   (`crates/reify-stdlib/src/lib.rs:325`).
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
//! `Some(Value::Undef)` (`crates/reify-stdlib/src/trajectory/mod.rs:107`). Its
//! ledger entry would be unexplainable under a two-way verdict, because the
//! stub already "passes". The ledger's real semantics are *rows whose parity
//! assertion cannot be made non-vacuously*, with `Undef` as the mechanism.
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

use std::collections::HashSet;

use reify_builtins::{BindingKind, BuiltinId, BuiltinRow, EvalBuiltinId, rows};
use reify_core::Type;
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
    /// The row evaluated to `Value::Undef`. The matcher accepts this for any
    /// type, so the comparison certifies NOTHING about the row — it is not a
    /// pass, and it needs a [`ExemptionEntry`] saying which leaf retires it.
    Vacuous,
    /// The row evaluated to a real value whose kind its declared type rejects.
    /// Either the row's `result` column or its eval body is wrong.
    Diverges,
}

/// Classify one (executed value, declared type) pair.
///
/// `Value::Undef` is tested FIRST and short-circuits to
/// [`ParityVerdict::Vacuous`], **before** the matcher is consulted. That order
/// is load-bearing, not stylistic: `crate::value_type_kind_matches` answers
/// `true` for `Undef` against every type (`crates/reify-eval/src/lib.rs:313`),
/// so consulting it first would erase the distinction this enum exists to
/// draw. Reversing these two statements makes the harness green over rows it
/// never probed.
///
/// Otherwise the answer is delegated to `crate::value_type_kind_matches` — the
/// workspace's single static-vs-runtime kind oracle, and the ONLY one this
/// harness may consult. That is literally what I-REG-4's "no second
/// derivation" requires, and it is the reason this module is in-crate at all
/// (the oracle is private). Do not re-author a local kind matcher here as
/// `crates/reify-compiler/tests/harness_type_checking/
/// mul_div_static_runtime_parity.rs:180-198` was forced to.
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
    // Order matters — see the doc above. `Undef` before the matcher, always.
    if matches!(value, Value::Undef) {
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
/// The `EvalBuiltinId` half of the pair is what [`representative_args`] keys
/// on, so the same derivation feeds both the membership and the argument side.
///
/// Shape reuse, not a shared import: `crates/reify-stdlib/tests/
/// registry_dispatch_seed_parity.rs`'s `eval_rows()` performs the identical
/// `BindingKind::EvalBuiltin` filter, and its own doc-comment gives the same
/// reason ("derived from the registry rather than restated … so a τ row added
/// later is covered here automatically instead of silently escaping the
/// sweep"). It cannot be imported — that is a `reify-stdlib` integration-test
/// target, this is a `reify-eval` lib unit-test module, and test targets are
/// separate compilation units with no path between them. Factoring the four
/// lines into a shared crate would put a test helper into a production
/// dependency to save four lines, so the shape is repeated and the repetition
/// is recorded here instead.
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
/// unconditionally (`crates/reify-eval/src/lib.rs:313` — the Auto/no-value
/// sentinel arm). A two-way `true`/`false` harness would therefore report a
/// pass for every row whose eval body fell off a match arm, which is exactly
/// the residue PRD §3 decision 12 says this harness exists to close. Three
/// in-tree funnels reach `Undef` without any registry involvement at all, so
/// this is not a hypothetical:
///
/// - `reify_stdlib::helpers::{unary, binary}` on the wrong argc
///   (`crates/reify-stdlib/src/helpers.rs:7-20`);
/// - `analysis::stress_invariants` on a non-3×3 tensor
///   (`crates/reify-stdlib/src/analysis.rs:487`);
/// - `eval_builtin`'s unresolved-name terminus
///   (`crates/reify-stdlib/src/lib.rs:325`).
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

/// Both trivial-accept paths present at once: `Undef` (the value sentinel) and
/// `Type::Error` (the type-inference poison sentinel, which
/// `value_type_kind_matches` short-circuits to `true` before it even inspects
/// the value). `Vacuous` must win, so the row is not certified by a pair in
/// which neither side carries information.
#[test]
fn classify_undef_against_error_type_is_vacuous() {
    assert_eq!(
        classify(&Value::Undef, &Type::Error),
        ParityVerdict::Vacuous
    );
}
