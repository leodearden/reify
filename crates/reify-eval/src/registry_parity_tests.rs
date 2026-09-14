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
