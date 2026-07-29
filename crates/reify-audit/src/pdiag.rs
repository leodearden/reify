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

/// One `Diagnostic::error(...)` / `Diagnostic::warning(...)` construction site.
///
/// `coded` is the detector's whole verdict for the site: a site with
/// `coded == false` is what the per-file ratchet counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Site {
    /// 1-based line number of the constructor token.
    line: usize,
    /// A `.with_code(` attaches to this constructor.
    coded: bool,
}

/// The anchor constructor paths, whitespace-tolerant before the open paren.
///
/// `Diagnostic::info` is deliberately absent: INV-SF-6 scopes codes-mandatory
/// to Warning/Error (see the module header). The `Diagnostic::` qualifier is
/// part of the token, which is what makes the 37 `ErrorRef::new(..)` sites and
/// `ErrorRef::with_code` (`crates/reify-ir/src/value.rs`) harmless.
const ANCHOR_IDENTS: &[&str] = &["Diagnostic::error", "Diagnostic::warning"];

/// The code-attachment probe — the BARE method token, deliberately not
/// `.with_code(DiagnosticCode::`. Real sites pass a severity-dispatched
/// variable (`crates/reify-eval/src/compute_targets/fea_diagnostics.rs:53`),
/// and probing the enum path would miss them and manufacture a false RED.
const CODE_PROBE: &str = ".with_code(";

/// Byte offsets of every anchor constructor occurrence in `line`, ascending.
///
/// An occurrence counts only when the next non-whitespace character after the
/// path is `(` — so `Diagnostic::error_with_span(` and a bare `Diagnostic::error`
/// used as a function value are both correctly non-anchors, while the
/// `Diagnostic::error (msg)` spacing survives (this repo has no rustfmt gate,
/// see CLAUDE.md § Formatting).
fn anchor_positions(line: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for ident in ANCHOR_IDENTS {
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(ident) {
            let at = from + rel;
            let after = at + ident.len();
            if line[after..].trim_start().starts_with('(') {
                out.push(at);
            }
            from = after;
        }
    }
    out.sort_unstable();
    out
}

/// Per-file scan: one [`Site`] per anchor occurrence, in source order.
///
/// Single-line core — the bounded forward window, comment masking, the
/// `#[cfg(test)]` block skip and the `pdiag:allow` escape layer on in later
/// steps. Pure `&str` operations throughout: no `syn`, no `regex`.
// The scanner is exercised by this module's unit tests but is not yet reachable
// from `check`, which still returns an empty `Vec`. Seeding it as a live root
// keeps the plain `cargo clippy --all-targets -- -D warnings` lib target green
// (and transitively covers `Site`, `ANCHOR_IDENTS`, `CODE_PROBE` and
// `anchor_positions`). Dropped when `check` is implemented.
#[allow(dead_code)]
fn scan_file(content: &str) -> Vec<Site> {
    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        for at in anchor_positions(line) {
            // Probe from the anchor rightwards so a `.with_code(` belonging to
            // an EARLIER constructor on the same line cannot code a later one.
            let coded = line[at..].contains(CODE_PROBE);
            out.push(Site { line: i + 1, coded });
        }
    }
    out
}

/// PDIAG entry point — see the module header for the heuristic and scope.
///
/// Stub: the scope predicate, baseline parser and ratchet land in task 5405
/// steps 7-12.
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let _ = ctx;
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Terse view of a scan: one `(line, coded)` pair per site, in scan order.
    fn sites(content: &str) -> Vec<(usize, bool)> {
        scan_file(content).into_iter().map(|s| (s.line, s.coded)).collect()
    }

    /// `Vec<(usize, bool)>`-typed empty expectation (inference needs the hint).
    fn none() -> Vec<(usize, bool)> {
        Vec::new()
    }

    /// Join `lines` into a file body. Window-offset arithmetic stays legible
    /// when each source line is its own array element.
    fn file(lines: &[&str]) -> String {
        lines.join("\n")
    }

    /// A constructor followed by `n - 1` filler chain lines and then a
    /// `.with_code(` — i.e. the code lands exactly `n` lines below the anchor.
    fn with_code_at_offset(n: usize) -> Vec<(usize, bool)> {
        let mut lines = vec!["    let d = Diagnostic::error(msg)".to_string()];
        lines.extend((1..n).map(|_| "        .with_label(l)".to_string()));
        lines.push("        .with_code(c);".to_string());
        let body = lines.join("\n");
        sites(&body)
    }

    // -- single-line core -------------------------------------------------

    #[test]
    fn single_line_codeless_push_is_one_uncoded_site() {
        // Real shape: crates/reify-eval/src/geometry_ops.rs:313.
        let src = "        diagnostics.push(Diagnostic::warning(rej.message(&x, name)));";
        assert_eq!(sites(src), vec![(1, false)]);
    }

    #[test]
    fn same_line_with_code_is_coded() {
        // Real shape: crates/reify-eval/src/dispatcher.rs:376.
        let src = "    Diagnostic::error(message).with_code(DiagnosticCode::PinnedKernelMissing)";
        assert_eq!(sites(src), vec![(1, true)]);
    }

    #[test]
    fn with_code_probe_is_the_bare_token_not_the_enum_path() {
        // `crates/reify-eval/src/compute_targets/fea_diagnostics.rs:53` passes a
        // severity-dispatched *variable*, so probing `.with_code(DiagnosticCode::`
        // would miss it and manufacture a false RED — the worst failure mode for
        // a merge gate. The probe is deliberately the bare `.with_code(` token.
        let src = "    Diagnostic::error(msg).with_code(code)";
        assert_eq!(sites(src), vec![(1, true)]);
    }

    #[test]
    fn info_constructor_is_not_an_anchor() {
        // INV-SF-6 scopes codes-mandatory to Warning/Error; `Info` is the debug
        // tier (crates/reify-core/src/diagnostics.rs:3327) and is not
        // code-mandatory, so it yields no site at all.
        let src = "    let d = Diagnostic::info(msg);";
        assert_eq!(sites(src), none());
    }

    #[test]
    fn two_constructors_on_one_line_are_two_sites() {
        let src = "    let d = if bad { Diagnostic::error(m) } else { Diagnostic::warning(m) };";
        assert_eq!(sites(src), vec![(1, false), (1, false)]);
    }

    #[test]
    fn error_ref_receiver_is_not_an_anchor() {
        // `ErrorRef::with_code` (crates/reify-ir/src/value.rs:4387) is a
        // different receiver; anchoring on the `Diagnostic::` ctor token makes
        // the 37 `ErrorRef::new` sites harmless.
        let src = "    ErrorRef::new(span, msg).with_code(DiagnosticCode::Foo)";
        assert_eq!(sites(src), none());
    }

    #[test]
    fn whitespace_between_ident_and_paren_still_anchors() {
        // rustfmt is not a gate in this repo (see CLAUDE.md § Formatting), so
        // the anchor match tolerates whitespace before the open paren.
        let src = "    Diagnostic::error (msg)";
        assert_eq!(sites(src), vec![(1, false)]);
    }

    // -- bounded forward window -------------------------------------------

    #[test]
    fn multi_line_format_without_code_is_one_uncoded_site() {
        // The DOMINANT real shape (crates/reify-eval/src/geometry_ops.rs:183):
        // only ~16% of the corpus fits on one line, so a single-line-only
        // scanner would under-count by ~84%.
        let src = file(&[
            "            diagnostics.push(Diagnostic::warning(format!(",
            "                \"unit {} rejected: {}\",",
            "                name, why",
            "            )));",
        ]);
        assert_eq!(sites(&src), vec![(1, false)]);
    }

    #[test]
    fn with_code_three_lines_below_is_coded() {
        // Real shape: crates/reify-eval/src/geometry_ops.rs:122.
        let src = file(&[
            "        let d = Diagnostic::error(format!(",
            "            \"bad {}\",",
            "        ))",
            "        .with_code(DiagnosticCode::BadThing);",
        ]);
        assert_eq!(sites(&src), vec![(1, true)]);
    }

    #[test]
    fn worst_observed_offset_of_thirteen_lines_is_coded() {
        // crates/reify-compiler/src/expr.rs:5895 -> :5908 is the widest
        // constructor -> `.with_code(` gap in the whole corpus. It MUST be
        // coded, or the detector manufactures a false RED on landed code.
        assert_eq!(with_code_at_offset(13), vec![(1, true)]);
    }

    #[test]
    fn window_edge_is_pinned_in_both_directions() {
        // PDIAG_CODE_WINDOW = 15: the measured worst case is 13, so 15 covers
        // 100% of the corpus with two lines of headroom. Pinning BOTH sides
        // makes a future widening a deliberate, evidence-anchored edit rather
        // than an accident.
        assert_eq!(with_code_at_offset(15), vec![(1, true)], "offset 15 is the last in-window line");
        assert_eq!(with_code_at_offset(16), vec![(1, false)], "offset 16 is past the window");
    }

    #[test]
    fn if_else_severity_dispatch_shape_is_coded() {
        // crates/reify-eval/src/compute_targets/fea_diagnostics.rs:48-53. This
        // is the shape a paren-depth chain scan gets WRONG: both constructor
        // parens close before the chain resumes, so depth-matching declares a
        // coded site code-less. The line window handles it.
        let src = file(&[
            "    let mut diag = if failure.is_error() {",
            "        Diagnostic::error(m)",
            "    } else {",
            "        Diagnostic::warning(m)",
            "    }",
            "    .with_code(code);",
        ]);
        assert_eq!(sites(&src), vec![(2, true), (4, true)]);
    }

    #[test]
    fn with_label_in_the_window_does_not_code_the_site() {
        // crates/reify-compiler/src/arg_check.rs:24 — a labelled but code-less
        // diagnostic is exactly what the ratchet exists to count.
        let src = file(&[
            "    diagnostics.push(",
            "        Diagnostic::error(msg)",
            "            .with_label(DiagnosticLabel::new(span, msg)),",
            "    );",
        ]);
        assert_eq!(sites(&src), vec![(2, false)]);
    }

    // -- comment exclusion -------------------------------------------------

    #[test]
    fn line_comment_forms_yield_no_sites() {
        // ~86 doc/line-comment occurrences repo-wide would otherwise inflate
        // the counts (crates/reify-eval/src/engine_build.rs:3563 is exactly a
        // constructor quoted inside a `//` comment).
        let src = file(&[
            "// Diagnostic::error(m) — quoted in a line comment",
            "    /// Diagnostic::warning(m) — quoted in a doc comment",
            "//! Diagnostic::error(m) — quoted in an inner doc comment",
        ]);
        assert_eq!(sites(&src), none());
    }

    #[test]
    fn block_comment_region_yields_no_sites_and_ends_at_its_close() {
        let src = file(&[
            "/**",
            " * Diagnostic::error(m) — quoted in a block comment.",
            " * Diagnostic::warning(m) — likewise.",
            " */",
            "let real = Diagnostic::error(m);",
        ]);
        assert_eq!(sites(&src), vec![(5, false)]);
    }

    #[test]
    fn block_comment_opened_and_closed_inline_does_not_swallow_the_rest() {
        // The region tracker must return to "live" at `*/`, or every anchor
        // after an inline `/* note */` on a code line would be lost.
        let src = "    let x = 1; /* note */ diagnostics.push(Diagnostic::error(m));";
        assert_eq!(sites(src), vec![(1, false)]);
    }

    #[test]
    fn commented_out_with_code_does_not_code_the_site() {
        // The CHOSEN rule, asserted explicitly: comment lines are invisible to
        // the `.with_code(` probe as well as to anchoring. A code that has been
        // commented out is not attached, so the site still counts.
        let src = file(&[
            "    let d = Diagnostic::error(msg)",
            "        // .with_code(DiagnosticCode::WasHere) — removed, needs reinstating",
            "        .with_label(l);",
        ]);
        assert_eq!(sites(&src), vec![(1, false)]);
    }

    #[test]
    fn comment_lines_do_not_consume_the_window_budget() {
        // The window spans the next PDIAG_CODE_WINDOW NON-COMMENT lines, so an
        // interleaved doc-comment block cannot push a real `.with_code(` out of
        // reach. Errs permissive, never toward a false RED.
        let mut lines = vec!["    let d = Diagnostic::error(msg)".to_string()];
        lines.extend((0..10).map(|i| format!("        // filler note {i}")));
        lines.extend((0..14).map(|_| "        .with_label(l)".to_string()));
        lines.push("        .with_code(c);".to_string());
        // Physical offset 25, but only the 15th non-comment line — in window.
        assert_eq!(sites(&lines.join("\n")), vec![(1, true)]);
    }
}
