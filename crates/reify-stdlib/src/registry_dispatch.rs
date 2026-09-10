//! The eval-side binding of the builtin-signature registry (task #6001 α,
//! `docs/prds/v0_6/builtin-signature-registry.md` §7.3(3)).
//!
//! `reify-stdlib` owns [`BindingKind::EvalBuiltin`]. PRD decision 4 puts
//! exhaustiveness enforcement in the kind's OWNING crate, and this module is
//! where that lands: [`dispatch`] is an exhaustive `match` on
//! `reify_builtins::EvalBuiltinId` with **no `_` arm**, and the absence of
//! that arm is the entire mechanism behind invariant I-REG-2.
//!
//! # Negative-test proof recipe (PRD §8 boundary #6)
//!
//! Verified by temporary MUTATION-THEN-REVERT (task #6001 α, step-16), and
//! RE-verified the same way at step-30 after main merged in. The cited line
//! numbers are the fragile part — they move whenever anything above the
//! `match` shifts, INCLUDING this comment — so the recipe is re-run and the
//! numbers re-read off rustc, never hand-adjusted. The merge itself moved
//! neither cite; re-writing this paragraph moved the first one, which is
//! exactly the drift that makes re-running the rule. No mutating test is
//! committed: the point of this recipe is precisely that the property needs
//! no test, because a violation is UNREPRESENTABLE and fails the BUILD. The
//! error text below was observed, not guessed.
//!
//! **The property.** A registry row whose `BindingKind` is `EvalBuiltin` but
//! which has no arm in [`dispatch`] cannot exist in a compiling tree.
//!
//! 1. **Edit.** Add an eighth row to the `EvalBuiltin` group in
//!    `crates/reify-builtins/src/registry.rs`, without adding its arm here:
//!
//!    ```text
//!    ProbeEighthRow {
//!        name: "probe_eighth_row",
//!        family: Parse,
//!        arity: Exact(1),
//!        arg_slots: [Any],
//!        result: Const(Type::dimensionless_scalar()),
//!        basis: Artifact
//!    },
//!    ```
//!
//! 2. **Command.** `cargo check -p reify-stdlib`
//!
//! 3. **Expected failure.** The build FAILS. The head of the error, verbatim
//!    (rustc also prints the macro-invocation and `help:` frames, elided here):
//!
//!    ```text
//!    error[E0004]: non-exhaustive patterns: `EvalBuiltinId::ProbeEighthRow` not covered
//!      --> crates/reify-stdlib/src/registry_dispatch.rs:87:11
//!       |
//!    87 |     match id {
//!       |           ^^ pattern `EvalBuiltinId::ProbeEighthRow` not covered
//!       |
//!    note: `EvalBuiltinId` defined here
//!      --> crates/reify-builtins/src/macros.rs:99:13
//!       = note: the matched value is of type `EvalBuiltinId`
//!    ```
//!
//!    rustc even names the fix (`EvalBuiltinId::ProbeEighthRow => todo!()`).
//!    Note WHAT this proves: registration drift is caught by the COMPILER, at
//!    the point of the omission, not by a test that has to remember to look.
//!
//! 4. **Revert.** Delete the added row.
//!
//! **The converse direction needs no recipe at all.** An eval arm with no row
//! has no variant to match on — `EvalBuiltinId::WhateverYouMeant` simply does
//! not exist — so it cannot be written in the first place. The two directions
//! together are what make "every `BuiltinId` is bound exactly once in its
//! kind's owning dispatcher" a property of the type system rather than a
//! convention.
//!
//! **What would break it.** Adding a `_ => …` arm to [`dispatch`], or marking
//! `EvalBuiltinId` `#[non_exhaustive]` (which would force downstream crates to
//! write one). Neither is present, deliberately.
//!
//! [`BindingKind::EvalBuiltin`]: reify_builtins::BindingKind::EvalBuiltin

use reify_builtins::{BuiltinId, EvalBuiltinId};
use reify_ir::Value;

use crate::{analysis, parse};

/// Evaluate the builtin a [`EvalBuiltinId`] names.
///
/// Exhaustive over the sub-enum — see the module docs for why there is no
/// `_` arm. Every arm calls the family kernel unchanged; this function
/// routes, it does not compute.
pub(crate) fn dispatch(id: EvalBuiltinId, args: &[Value]) -> Value {
    match id {
        EvalBuiltinId::ParseLength => parse::parse_length(args),
        EvalBuiltinId::ParseLengthR => parse::parse_length_r(args),
        EvalBuiltinId::VonMises => analysis::von_mises(args),
        EvalBuiltinId::MaxShear => analysis::max_shear(args),
        EvalBuiltinId::PrincipalStresses => analysis::principal_stresses(args),
        EvalBuiltinId::SafetyFactor => analysis::safety_factor(args),
        EvalBuiltinId::StressInvariants => analysis::stress_invariants(args),
    }
}

/// Resolve a builtin name to its registry row and evaluate it, or decline.
///
/// `None` means "not a registered `EvalBuiltin` row at this argument count",
/// which is the same "this sub-dispatcher declines" signal every other
/// `eval_*` family member returns — so `eval_builtin`'s chain falls through
/// exactly as before.
///
/// Resolution goes through [`reify_builtins::lookup`] (argc-keyed), NOT
/// [`reify_builtins::name_group`] (argc-independent): eval genuinely IS
/// argc-keyed, unlike the compiler ladder, whose family arms are name-only
/// today (`reify-compiler`'s `builtin_registry::registry_result_type` says so
/// explicitly). A same-name arity overload must reach a DIFFERENT kernel here,
/// and `lookup` is what expresses that.
pub(crate) fn try_dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let id = reify_builtins::lookup(name, args.len())?;
    Some(dispatch(BuiltinId::as_eval_builtin(id)?, args))
}
