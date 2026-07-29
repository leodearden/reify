//! PDIAG — codes-mandatory ratchet for diagnostic construction sites.
//!
//! ## Purpose
//!
//! `INV-SF-6 diagnostics-carry-codes` (`docs/legibility/design-invariants.md`
//! §INV-SF-6) requires every *emitted* Warning/Error to carry a
//! `DiagnosticCode`. Code-less diagnostics cannot be gated, filtered, counted
//! or de-noised systematically, and force message-substring hacks downstream —
//! the CLI's `E_DFM_` message-prefix escalation exists only because its
//! co-resident Error diagnostics are code-less.
//!
//! This detector holds the line at the current backlog: it scans tracked Rust
//! source for `Diagnostic::error(...)` / `Diagnostic::warning(...)`
//! construction sites with no `.with_code(...)` attached, counts them per
//! file, and compares against a committed baseline manifest
//! (`crates/reify-audit/pdiag-baseline.txt`). Counts may only **decrease**.
//! Migrating the existing backlog is opportunistic, per PRD §6 decision 3 —
//! enforcement is for *new* sites.
//!
//! ## Heuristic
//!
//! Pure line scan over `&str` — no `syn`/AST, no `regex`, no `walkdir`. Their
//! absence from `crates/reify-audit/Cargo.toml` is a deliberate design
//! invariant this module inherits from `ptodo.rs` and `pdssentinel.rs`.
//!
//! Each occurrence of an anchor token (`Diagnostic::error(` /
//! `Diagnostic::warning(`, whitespace-tolerant before the paren) on a
//! non-comment line is one site. `Diagnostic::info(` is deliberately NOT an
//! anchor: INV-SF-6 scopes the rule to Warning/Error, and coding an `Info` is
//! welcome but not required.
//!
//! A site counts as **coded** when `.with_code(` appears at or after the
//! anchor position on the anchor line, or on any of the next
//! `PDIAG_CODE_WINDOW` non-comment lines. The probe is the bare token
//! `.with_code(` and NOT `.with_code(DiagnosticCode::` — real sites pass a
//! severity-dispatched variable (`crates/reify-eval/src/compute_targets/
//! fea_diagnostics.rs:53`).
//!
//! ## Why a bounded line window and not brace/paren matching
//!
//! Measured against the real corpus: only ~16% of sites fit on one line
//! (multi-line `format!` wrapping is the norm), and both mechanics were run
//! and diffed. A strict paren-depth chain scan and a 15-line window disagree
//! on 7 of 730 code-less sites — and the strict scan is *wrong* on two of
//! them: the `if {...} else {...}.with_code(code)` severity-dispatch shape
//! closes its constructor paren before the chain resumes, so paren-matching
//! declares a coded site code-less (a false RED). The remaining five are
//! `#[cfg(test)]` bodies this detector excludes anyway.
//!
//! ## Accepted residual imprecision
//!
//! Both directions are deliberate and both are *permissive* — the detector
//! never manufactures a false RED:
//!
//! - An anchor token inside a string literal on a code line is counted as a
//!   site (none observed in the corpus; `pdiag:allow` escapes it).
//! - An unrelated `.with_code(` inside a site's window can mark it coded —
//!   7/730 sites repo-wide, all in `#[cfg(test)]` code this detector excludes.
//!
//! ## Scope
//!
//! `crates/<name>/src/**.rs` and `gui/src-tauri/src/**.rs`, minus
//! `SCOPE_EXCLUDE_PREFIXES`, minus any `tests/`-segment path / `tests.rs` /
//! `*_tests.rs` file, minus `#[cfg(test)]` module bodies. INV-SF-6 governs
//! *emitted* diagnostics; test scaffolding that fabricates a `Diagnostic` to
//! assert on is out of scope by construction, not by exemption.
//!
//! ## Escape hatch
//!
//! A trailing `// pdiag:allow — reason` on the anchor line, or anywhere within
//! the site's window, suppresses the site. Only the substring `pdiag:allow` is
//! load-bearing — the reason prose is for humans, exactly as `ptodo:allow`
//! treats it (`ptodo.rs::line_escaped`). The spelling is fixed by PRD §3
//! Leg C as a deliberate mirror of `ptodo:allow`. The archetype of a
//! legitimate escape is `crates/reify-stdlib/src/dfm.rs:174-200`, whose
//! severity-parameterized `{I,W,E}_DFM_*` message-prefix convention is
//! documented as code-less by design.
//!
//! ## Severity posture
//!
//! Unlike PTODO's warn-first Medium lanes, a PDIAG ratchet violation is
//! **High** and therefore moves the process exit code (`high_severity_exit_code`
//! in `src/bin/reify-audit.rs`) — that is the hard gate PRD §8 boundary row 8
//! requires. Under-count and orphan-row verdicts are Medium (exit-neutral), so
//! an opportunistic fix never turns a diff RED.
//!
//! Reference: `docs/prds/v0_6/eradicate-silent-undef.md` §3 Leg C, §6.7-6.8,
//! §7 "PDIAG baseline", §8 row 8. Remediation recipe for a RED diff:
//! `docs/notes/diagnostic-severity-policy.md` §3.

use crate::{AuditContext, Finding};

/// PDIAG entry point — see the module header for the heuristic and scope.
///
/// Stub: the scanner, scope predicate, baseline parser and ratchet land in
/// task 5405 steps 1-12.
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let _ = ctx;
    Vec::new()
}
