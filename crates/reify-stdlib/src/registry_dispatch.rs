//! The eval-side binding of the builtin-signature registry (task #6001 α,
//! `docs/prds/v0_6/builtin-signature-registry.md` §7.3(3)).
//!
//! `reify-stdlib` owns [`BindingKind::EvalBuiltin`]. PRD decision 4 puts
//! exhaustiveness enforcement in the kind's OWNING crate, and this module is
//! where that lands: [`dispatch`] is an exhaustive `match` on
//! `reify_builtins::EvalBuiltinId` with **no `_` arm**, and the absence of
//! that arm is the entire mechanism behind invariant I-REG-2.
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
