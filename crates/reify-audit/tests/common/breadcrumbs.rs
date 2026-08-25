//! The jcodemunch PER-CALL fail-soft breadcrumb literals, in ONE place.
//!
//! Every one of these strings is a copy of a `eprintln!` in
//! `src/jcodemunch_client.rs`. Two test binaries consume them, for two
//! DIFFERENT and complementary reasons:
//!
//! - `tests/cli.rs` (`freshness_gate::per_call_fail_soft_*`) asserts the
//!   literal is PRESENT in the real binary's stderr, hermetically, on the
//!   ordinary merge gate. That is the production-side lock: reword the
//!   `eprintln!` and these tests go red.
//! - `tests/jcodemunch_live.rs` (`assert_live_leg`) asserts the literal is
//!   ABSENT from a live leg's stderr. That is the vacuity check: a per-call
//!   error happens AFTER a successful handshake, so exit is 0, the
//!   construction breadcrumb is silent, the §4.3 gate has already admitted,
//!   and `[]` is perfectly well-formed — the breadcrumb is the ONLY signal
//!   that the run produced nothing because the serve refused the call.
//!
//! # Why they live here rather than in either consumer
//!
//! The absence direction is only meaningful against a string the binary can
//! actually emit. When the two binaries each spelled their own copy, nothing
//! but a prose paragraph bound them: a maintainer who reworded an `eprintln!`
//! would see only the cli.rs failure, fix the cli.rs literal, get a green
//! build, and leave the live capstone asserting the absence of a string that
//! can no longer appear — an unconditionally-true check. That is PRD §2.4's
//! failure mode displaced one file over, and it is exactly what a shared
//! constant makes unrepresentable: one reword, one edit, both consumers move.
//!
//! # How it is wired in
//!
//! Declared by each consuming test binary as
//! `#[path = "common/breadcrumbs.rs"] mod breadcrumbs;` rather than through
//! `common/mod.rs`. Cargo only promotes top-level `tests/*.rs` files (and
//! subdirectories carrying a `main.rs`) to test targets, so a plain module
//! file in `tests/common/` compiles into its consumers and never becomes a
//! test binary of its own.

/// `RealJCodemunchOps::get_dead_code`'s `Err` arm
/// (`src/jcodemunch_client.rs:1135`).
///
/// Reached by `--pattern PDEAD`, whose single jcodemunch op is
/// `src/pdead_dead_code.rs:35`.
#[allow(dead_code)]
pub const PDEAD_GET_DEAD_CODE: &str = "jcodemunch get_dead_code_v2:";

/// `RealJCodemunchOps::get_changed_symbols`'s `Err` arm
/// (`src/jcodemunch_client.rs:1074`).
///
/// Runs unconditionally on every P1 leg (`src/p1_producer_orphan.rs:131`), so
/// it is the load-bearing half of the P1 pair: its silence is evidence.
#[allow(dead_code)]
pub const P1_GET_CHANGED_SYMBOLS: &str = "jcodemunch get_changed_symbols:";

/// `RealJCodemunchOps::find_references`'s `Err` arm
/// (`src/jcodemunch_client.rs:1117`).
///
/// CONDITIONALLY reachable: `src/p1_producer_orphan.rs:146` sits inside the
/// `for symbol in ... get_changed_symbols(...)` loop opened at `:131`, so it
/// runs once per RETURNED symbol. If `get_changed_symbols` legitimately
/// returns zero symbols this breadcrumb is unreachable and its absence
/// carries no information — which is a statement about the live leg's
/// interpretation, not about whether the literal is pinned. It IS pinned:
/// `cli.rs::freshness_gate::per_call_fail_soft_on_p1s_second_call` drives a
/// DISPATCHING mock that answers `get_changed_symbols` with a real symbol row
/// and errors only `find_references`, reaching this arm in the real binary.
#[allow(dead_code)]
pub const P1_FIND_REFERENCES: &str = "jcodemunch find_references(";

/// Every per-call breadcrumb `--pattern PDEAD` can emit.
///
/// Exactly one entry, because PDEAD reaches exactly one jcodemunch op.
#[allow(dead_code)]
pub const PDEAD_CALL: &[&str] = &[PDEAD_GET_DEAD_CODE];

/// Every per-call breadcrumb `--pattern P1` can emit.
///
/// Deliberately NOT listed here: `get_untested_symbols` /
/// `get_layer_violations` (`src/jcodemunch_client.rs:1152`, `:1168`). Neither
/// op is reachable from PDEAD or P1, so asserting their breadcrumbs would be
/// decorative — a check that can never fire reads like coverage while
/// providing none.
#[allow(dead_code)]
pub const P1_CALL: &[&str] = &[P1_GET_CHANGED_SYMBOLS, P1_FIND_REFERENCES];
