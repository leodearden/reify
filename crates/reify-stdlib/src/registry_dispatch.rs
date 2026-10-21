//! The eval-side binding of the builtin-signature registry (task #6001 α,
//! `docs/prds/v0_6/builtin-signature-registry.md` §7.3(3)).
//!
//! `reify-stdlib` owns [`BindingKind::EvalBuiltin`], and PRD decision 4 puts
//! exhaustiveness enforcement in the kind's OWNING crate. [`dispatch`]'s
//! missing `_` arm is that enforcement: invariant I-REG-2 — every `BuiltinId`
//! is bound exactly once in its kind's dispatcher — held by the compiler
//! rather than by a test. Adding a row to the `EvalBuiltin` group mints a
//! variant in `EvalBuiltinId` and stops this crate compiling until the arm
//! exists:
//!
//! ```text
//! error[E0004]: non-exhaustive patterns: `EvalBuiltinId::ProbeEighthRow` not covered
//! ```
//!
//! The converse direction needs no guard at all: an arm with no row has no
//! variant to name, so it cannot be written. Both halves die the moment a
//! `_ => …` arm appears here, or `EvalBuiltinId` becomes `#[non_exhaustive]`.
//!
//! That verbatim error IS PRD §8 boundary #6's negative test — observed by
//! adding an eighth row and reverting, at step-16 and again after main merged
//! in, never guessed. Nothing mutating is committed, because the whole point
//! is that the violation fails the BUILD rather than a test run.
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
/// `None` is the same "this sub-dispatcher declines" signal every other
/// `eval_*` family member returns, so `eval_builtin`'s chain falls through
/// exactly as before.
///
/// Resolution is [`reify_builtins::lookup`] (argc-keyed), NOT
/// [`reify_builtins::name_group`] (argc-independent) as the compiler seam
/// uses: a same-name arity overload must reach a DIFFERENT kernel here, which
/// is exactly what `lookup` expresses and `name_group` cannot.
pub(crate) fn try_dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let id = reify_builtins::lookup(name, args.len())?;
    Some(dispatch(BuiltinId::as_eval_builtin(id)?, args))
}
