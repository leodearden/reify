//! The compiler's **single** entry point into the enum-keyed builtin-signature
//! registry (`reify-builtins`) — task #6001 α of
//! `docs/prds/v0_6/builtin-signature-registry.md`, §7.3(2).
//!
//! Every compile-time question the compiler asks the registry goes through
//! here, so the seam between `expr.rs`'s result-type ladder and the row table
//! is one module, not a scattering of `reify_builtins::` references.
//!
//! Argument **types** cross the boundary, not `CompiledExpr`s: `reify-builtins`
//! depends on `reify-core` only (PRD decision 3, locked by that crate's
//! `tests/dag_invariant.rs`) and cannot see `reify_ir`. The caller projects
//! `compiled_args` down to their `result_type`s — the same projection the
//! deleted `analysis_fn_result_type` did internally.
//!
//! # Arity-insensitivity is deliberate, not an oversight
//!
//! [`registry_result_type`] resolves through
//! [`name_group`](reify_builtins::name_group), NOT the argc-keyed
//! [`lookup`](reify_builtins::lookup), because the legacy ladder arms it
//! replaces (`is_parse_typed_fn` / `is_analysis_typed_fn`) gated on the NAME
//! only: an arity-mismatched call such as `safety_factor()` still received the
//! family's result type. Switching to `lookup` here would re-route mis-arity
//! calls through `expr.rs`'s terminal first-arg fallback — a behavioural change
//! α is not chartered to make (PRD §7.3(6): zero corrections). Real arity
//! diagnostics arrive with the first genuine overload in τ-numeric. Eval is the
//! other way round, and says so: `reify-stdlib`'s `registry_dispatch` is
//! argc-keyed because a same-name overload must reach a different kernel.

use reify_core::Type;

/// The registry's compile-time result type for a call to `name` with the given
/// argument types — `None` when the registry holds no answer.
///
/// `None` means "not mine": `expr.rs`'s ladder falls through to the surviving
/// legacy family arms and, finally, to the first-arg fallback. That is I-REG-3's
/// pre-ω carve-out — the registry is authoritative for the names it holds and
/// silent about every other, until task ω deletes the fallback path.
///
/// A name whose group holds more than one row (a genuine arity overload) also
/// yields `None`. α seeds no overloads, so that is unreachable today; it exists
/// so the first τ to register one gets a conservative fall-through rather than
/// an arbitrary first-row answer.
pub(crate) fn registry_result_type(name: &str, args: &[Type]) -> Option<Type> {
    reify_builtins::row_for(sole_row_id(name)?).result.resolve(args)
}

/// The one row that owns `name`, or `None` for an unregistered name (empty
/// group) or a multi-row group (an arity overload).
///
/// Both entry points are written in terms of this, which is what makes
/// [`registry_owns`] *definitionally* [`registry_result_type`]'s precondition
/// rather than a restatement that could drift.
fn sole_row_id(name: &str) -> Option<reify_builtins::BuiltinId> {
    match reify_builtins::name_group(name) {
        [id] => Some(*id),
        _ => None,
    }
}

/// Cheap **name-only** precheck: can the registry answer for `name` at all?
///
/// Hoisted so `expr.rs` can test it BEFORE paying to materialise the argument
/// types — that projection allocates a `Vec<Type>` and deep-clones every arg
/// (`Type` carries `Box<Type>` / `String` payloads) on a ladder arm reached by
/// nearly every call in a program, while answering for 7 names in α.
///
/// Guarding on it is behaviour-preserving because `false` here means
/// [`registry_result_type`] would have returned `None` anyway. The converse does
/// NOT hold — an `ArgAware` resolver may still decline the arguments it is
/// handed — so only the one direction is relied on, and
/// `tests/harness_builtin_registry/registry_seed_result_types.rs` pins it
/// row-derived over `rows()`.
pub(crate) fn registry_owns(name: &str) -> bool {
    sole_row_id(name).is_some()
}

/// Is `name` registered **at all** — under any arity?
///
/// The membership question, as distinct from [`registry_owns`]'s
/// answerability question. They differ on exactly one case, the arity
/// overload: a name whose group holds several rows is registered, so this
/// returns `true`, while `registry_owns` returns `false` because no single
/// row can be asked for a result type. α seeds no overload, so the two agree
/// today; τ1's two-arg `floor` is the first that will split them.
///
/// That case is why [`crate::unresolved_function::is_known_builtin`] must ask
/// this one. It unions the name vocabularies to decide whether a callee is
/// known to the compiler at all, and an overloaded builtin is emphatically
/// known — typing it through the terminal first-arg fallback is a separate
/// question from whether it exists. Asking `registry_owns` there would warn
/// `UnresolvedFunction` on a name the registry holds two rows for.
pub(crate) fn registry_knows_name(name: &str) -> bool {
    !reify_builtins::name_group(name).is_empty()
}
