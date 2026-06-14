//! Compliant-joint / flexure constructors (Compliant-Joints PRD, v0.3).
//!
//! This module hosts the Howell-style pseudo-rigid-body (PRB) flexure
//! constructors. Each builtin returns a joint `Value::Map` (kind
//! `"revolute"` / `"prismatic"`) extended with the flexure-specific keys
//! (`spring_rate`, `damping`, `neutral`, `pivot`) so a flexure plugs into the
//! mechanism / sweep / snapshot engines exactly like a plain joint — the
//! engines dispatch on the `kind` string and ignore the extra keys (PRD §8.2).
//!
//! Dispatch mirrors the other stdlib modules: [`eval_flexures`] returns
//! `Some(Value)` for a recognised flexure name (including `Some(Value::Undef)`
//! on validation failure) and `None` for any unknown name, so `eval_builtin`
//! can fall through to the next module.

use reify_ir::Value;

mod common;
mod beam;
mod notch;
mod hinge;
mod prismatic;
mod compound;
mod diagnostics;

#[cfg(test)]
mod test_util;

/// Re-export the PRB-constructor diagnostic classifier so the crate root can
/// surface it as `reify_stdlib::flexure_diagnose` (mirroring `stackup::diagnose`
/// → `stackup_diagnose`). reify-expr's `FunctionCall` arm calls it on the
/// builtin result to emit the §5.3 / §1 flexure diagnostics into the eval sink.
pub use diagnostics::flexure_diagnose;

/// Evaluate a flexure stdlib function by name.
///
/// Returns `Some(Value)` for known flexure constructors (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_flexures(name: &str, args: &[Value]) -> Option<Value> {
    // `__flexure_compliance_get` — the Rust intrinsic backing the PRD §4.2
    // `flexure_compliance(joint)` accessor. Routed here as a bare builtin name
    // (FunctionCall → eval_builtin → eval_flexures) from the retained DSL fn
    // body in `stdlib/flexures.ri`.
    if name == "__flexure_compliance_get" {
        return Some(flexure_compliance_get(args));
    }
    beam::eval_beam(name, args)
        .or_else(|| notch::eval_notch(name, args))
        .or_else(|| hinge::eval_hinge(name, args))
        .or_else(|| prismatic::eval_prismatic(name, args))
        .or_else(|| compound::eval_compound(name, args))
}

/// Surface the cached `FlexureCompliance` record from a PRB-emitting joint Map.
///
/// For a single `Value::Map` argument carrying the reserved hidden
/// `__flexure_compliance` key (every PRB ctor attaches one via
/// [`common::attach_compliance`]), return that populated record. For anything
/// else — a non-flexure joint, a non-Map argument, or wrong arity — return a
/// sentinel-zero default record so the accessor always yields a well-formed
/// `FlexureCompliance` (matching the structure-def defaults: no yield datum ⇒
/// `at_yield = false`, `max_stress = 0 Pa`) rather than `Value::Undef`.
fn flexure_compliance_get(args: &[Value]) -> Value {
    let cached = match args {
        [Value::Map(joint)] => joint
            .get(&Value::String("__flexure_compliance".to_string()))
            .cloned(),
        _ => None,
    };
    // Sentinel-zero default: effective_stiffness 0, stresses 0, no yield datum
    // (⇒ margin sentinel, not-at-yield), zero validity range, no parasitic.
    cached.unwrap_or_else(|| common::make_compliance_record(0.0, 0.0, 0.0, None, None, common::symmetric_angle_range(0.0)))
}
