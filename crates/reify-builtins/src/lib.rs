//! Enum-keyed builtin-signature registry — the single source of truth for
//! builtin names, arities, arg slots, result types, and the `BuiltinId` keys
//! that dispatch is routed on.
//!
//! Implements `docs/prds/v0_6/builtin-signature-registry.md` task α (the seed
//! crate; parse + analysis families).
//!
//! # Row shape (PRD §7.1)
//!
//! Every registered builtin is one [`BuiltinRow`]:
//!
//! ```text
//! { name, id, family, binding, arity, arg_slots, result, basis }
//! ```
//!
//! with `result: ResultSpec = Const(Type) | ArgAware(fn(&[Type]) -> Option<Type>)`
//! and `basis: Basis = Ruling | Physics | Doc | Artifact`. Same-name arity
//! overloads are DISTINCT rows in one name-group, disambiguated by
//! [`lookup`]`(name, argc)`.
//!
//! # Invariants this crate owns (PRD §7.2)
//!
//! - **I-REG-1**: [`lookup`] is the only string→builtin resolution in the
//!   workspace. No `"name" =>` string dispatch on a builtin name — and no
//!   `eval_builtin("name", …)` call — may live outside this crate.
//!   Enforced by `tests/i_reg_1_seed_string_dispatch_gate.rs`, which is
//!   **seed-scoped and ledgered**: it sweeps the names [`rows`] actually
//!   registers (so each later τ migration is covered automatically, never a
//!   restated list) across `crates/*/src/`, and the residue α cannot re-home
//!   is COUNTED entry-by-entry in that file's `SEED_STRING_DISPATCH_LEDGER`,
//!   each entry naming the leaf that owns it — five today, all in
//!   `reify-expr` (four `Value::Field` intercepts awaiting
//!   [`BindingKind::ExprIntercept`] in τ-fea/analysis, one re-entrant call
//!   awaiting `CompiledExpr` carrying a `BuiltinId` in leaf β). An
//!   UNLEDGERED site fails. The **workspace-wide** grep gate — every builtin
//!   name, not just the seeds — arrives at task ω.
//! - **I-REG-2**: every `BuiltinId` is bound exactly once in its
//!   [`BindingKind`]'s owning dispatcher, via an exhaustive match with no `_`
//!   arm. The per-kind sub-enums this crate generates (`EvalBuiltinId`, …) are
//!   what give that requirement teeth — see the negative-test proof recipe on
//!   `reify-stdlib`'s `registry_dispatch` module.
//! - **I-REG-7**: every row carries a `basis`; the `Artifact` count is a
//!   visible, reviewed ratchet toward zero (see [`artifact_basis_rows`]).
//!
//! # Crate boundary (PRD decision 3)
//!
//! This crate depends on **`reify-core` only** — locked structurally by
//! `tests/dag_invariant.rs`. It holds **no `Value`** and **no fn pointers into
//! eval**: the row table is pure signature data, and eval binding is the
//! exhaustive `match` in the owning crate (`reify-stdlib`, `reify-expr`,
//! `reify-eval`). The `ArgAware` fn pointers are `fn(&[Type]) -> Option<Type>`
//! — compile-time type algebra over `reify_core::Type`, never eval.

// Mirrors the reify-core lint-attribute prelude for parity across the
// core stack; reify-builtins itself has no current trigger.
#![allow(clippy::mutable_key_type)]

pub mod macros;
pub mod registry;
pub mod resolvers;
pub mod row;

// ── flat root re-exports ─────────────────────────────────────────────────────
// Flat re-export so consumers write `reify_builtins::BuiltinRow` (etc.)
// alongside the module-path form `reify_builtins::row::BuiltinRow`.
pub use registry::{
    BuiltinId, EvalBuiltinId, artifact_basis_rows, artifact_row_count, lookup, name_group, row,
    rows,
};
pub use resolvers::scalar_or_real;
pub use row::{Arity, ArgSlot, Basis, BindingKind, BuiltinRow, Family, ResultSpec};
