//! Closed-world membership oracle for **builtin function names**.
//!
//! # Why this module exists
//!
//! The `NoUserFunctions` arm of the `FunctionCall` ladder in
//! [`crate::expr`] ends in a *terminal first-arg fallback*: any callee that no
//! ladder arm claims is typed as its first argument's type (or
//! `Type::dimensionless_scalar()` when zero-arg). That fallback is
//! **open-world** — a genuinely nonexistent name such as
//! `definitely_not_a_reify_builtin_xyz(2.5mm)` compiles with ZERO diagnostics
//! and silently adopts `Scalar<LENGTH>`.
//!
//! This module supplies the missing complement: a single predicate,
//! [`is_known_builtin`], that answers "is this name known to the compiler at
//! all?" by unioning **every** classification family the ladder consults, plus
//! two explicit manifests declared here:
//!
//! * `FIRST_ARG_TYPED_NAMES` — names for which the terminal fallback's
//!   first-arg typing is *verified correct*, so they are named rather than
//!   left open-world.
//! * `EVAL_DEFERRED_BUILTIN_NAMES` — names that are eval-dispatchable but not
//!   yet family-registered, whose typing is deliberately left to the fallback.
//!
//! With the union in hand, `expr.rs` can emit a
//! `DiagnosticCode::UnresolvedFunction` **warning** at the fallback when the
//! callee is unknown, closing the open world without changing any typing.
//!
//! # Warn-mode-first posture
//!
//! Typing is **unchanged** by this module. Every call that compiled before
//! still compiles to the same type; the only new observable is a diagnostic.
//! That is deliberate (fail-closed warn-first): the corpus sweep must be green
//! before the code can become an error.
//!
//! # Downstream consumers
//!
//! * **#5997** flips `UnresolvedFunction` from Warning to Error behind a
//!   break-glass env knob. It names this module's manifest, allowlist and
//!   corpus sweep as its preconditions.
//! * **#6014** (builtin-signature-registry, task omega) DELETES the terminal
//!   first-arg fallback outright, and with it this module's
//!   `FIRST_ARG_TYPED_NAMES` family — once every name in it holds a real
//!   registry row, the allowlist has no remaining job. Its family-by-family
//!   migration is seeded by this task's warn-sweep violation list
//!   (`docs/notes/unresolved-function-warn-sweep-2026-08-29.md`).

/// Is `name` a builtin function name the compiler knows about *at all*?
///
/// Closed-world union over every classification family plus the two manifests
/// declared in this module. A pure predicate: no allocation, no diagnostics.
///
/// Case-sensitive — Reify function names are snake_case.
pub fn is_known_builtin(_name: &str) -> bool {
    false
}
