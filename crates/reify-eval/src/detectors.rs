//! Shared post-pass detector registry (task λ, #5043).
//!
//! PRD `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.7, INV-EVAL-3.
//!
//! Today's eval-only post-passes (the `MassProperties` PSD inertia
//! validation and the annotation-args materialization driver, both inline
//! in `Engine::eval`) run on the cold `eval()` path but NOT on
//! `eval_cached()` — a cold-only detector asymmetry — and their relative
//! ordering is encoded only in scattered "must run before …" / "MUST run
//! AFTER …" convention comments (e.g. `engine_eval.rs:6312`,
//! `structural_query.rs:531,610`, `significance_filter.rs:1025,1032`).
//!
//! This module provides the REGISTRY MECHANISM that replaces both: a single
//! shared post-pass detector registry any eval path can run identically,
//! where registration order IS run order — one owner, all paths.
//!
//! **Scope of this task**: the registry mechanism only. Wiring it into
//! `Engine::eval` / `Engine::eval_cached` / `Engine::edit_check`, plus
//! fast-path diagnostic replay, is task μ (#5044, depends on κ, λ, γ, δ) —
//! explicitly OUT of scope here.
//!
//! ## Known scope gaps for task μ
//!
//! - The annotation-args materialization driver (`engine_eval.rs:4400-4557`)
//!   needs heavy READ-ONLY `Engine` context (module/prelude/functions plus
//!   an `EvalContext`) that this module's post-pass state does not carry.
//!   Its per-path hand-off — especially on `edit_check` — is deferred to
//!   task μ (PRD §10 open-question #2).
//! - Per-node diagnostic attribution and `NodeCache` storage/replay
//!   (consuming task κ's per-node diagnostics vec) is also task μ's.
//! - The inline `MassProperties` PSD pass at `engine_eval.rs:4298-4397`
//!   stays in place until task μ removes it and wires this registry into
//!   the three eval paths (foundation-then-migrate, mirroring how task α
//!   shipped `commit_cell_result` ahead of its own migration leaves).
