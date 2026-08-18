//! The compiler's **single** entry point into the enum-keyed builtin-signature
//! registry (`reify-builtins`) — task #6001 α of
//! `docs/prds/v0_6/builtin-signature-registry.md`, §7.3(2).
//!
//! Every compile-time question the compiler asks the registry goes through this
//! module, so the seam between "the compiler's `NoUserFunctions` result-type
//! ladder" and "the row table" is one function with one call site, not a
//! scattering of `reify_builtins::` references across `expr.rs`.
//!
//! # What crosses the boundary
//!
//! Argument **types**, not `CompiledExpr`s. `reify-builtins` depends on
//! `reify-core` only (PRD decision 3, locked by that crate's
//! `tests/dag_invariant.rs`) and therefore cannot see `reify_ir::CompiledExpr`.
//! The caller in `expr.rs` projects `compiled_args` down to their
//! `result_type`s at the call site — the same projection the deleted
//! `analysis_fn_result_type` performed internally via its `tensor_quantity`
//! helper.
//!
//! # Arity-insensitivity is deliberate, not an oversight
//!
//! The legacy ladder arms this replaces (`is_parse_typed_fn` /
//! `is_analysis_typed_fn`) gated on the **name only**: an arity-mismatched call
//! such as `safety_factor()` still received the family's result type. α
//! preserves that exactly — see
//! [`registry_result_type`]'s use of
//! [`name_group`](reify_builtins::name_group) rather than
//! [`lookup`](reify_builtins::lookup). Switching to the argc-keyed `lookup`
//! here would silently re-route mis-arity calls through `expr.rs`'s terminal
//! first-arg fallback, which is a behavioural change α is not chartered to
//! make (PRD §7.3(6): zero corrections). Real arity diagnostics arrive with
//! the first genuine overload in τ-numeric.
//!
//! `lookup(name, argc)` remains the I-REG-1 authority for the **eval** side,
//! where dispatch genuinely is argc-keyed (`reify-stdlib`'s
//! `registry_dispatch`).

use reify_core::Type;

/// The registry's compile-time result type for a call to `name` with the given
/// argument types — `None` when the registry holds no answer.
///
/// `None` means "not mine": `expr.rs`'s ladder falls through to the surviving
/// legacy family arms and, finally, to the first-arg fallback. This is I-REG-3's
/// pre-ω carve-out — the registry is authoritative for the names it holds and
/// silent about every other, until task ω deletes the fallback path entirely.
///
/// # Multi-row name groups
///
/// A name whose group holds more than one row (a genuine arity overload) also
/// yields `None`. α seeds no overloads — `reify-builtins`'
/// `every_seed_name_group_holds_exactly_one_row` pins that precondition — so
/// this arm is unreachable today. It exists so that the first τ to register an
/// overload gets a conservative fall-through rather than an arbitrary
/// first-row answer; picking the right row for an overloaded call needs the
/// argc-keyed [`lookup`](reify_builtins::lookup) plus the arity diagnostics
/// that arrive with it in τ-numeric (PRD §3 decision 5).
pub(crate) fn registry_result_type(name: &str, args: &[Type]) -> Option<Type> {
    match reify_builtins::name_group(name) {
        [id] => reify_builtins::row(*id).result.resolve(args),
        // Unregistered name (empty group) or a not-yet-possible overload.
        _ => None,
    }
}
