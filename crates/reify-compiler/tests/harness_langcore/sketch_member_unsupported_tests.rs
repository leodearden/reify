//! The loud-failure contract α owes for `sketch { … }` members
//! (constrained-2d-sketch α, task 5506; PRD `docs/prds/v0_6/constrained-2d-sketch.md`).
//!
//! α lands the grammar (`sketch_block`), the AST variant (`MemberDecl::Sketch`)
//! and the CST → AST lowering. It deliberately lands NO compile semantics — the
//! `SketchTemplate` classification, the sketch-local scope and the coded
//! `E_SKETCH_*` diagnostics are constrained-2d-sketch task γ.
//!
//! That gap is exactly where a construct silently becomes a no-op: the parser
//! accepts `sketch profile { … }`, the compiler's member loop has no arm for it,
//! and the user's constraints evaporate with no diagnostic at all. INV-SF-1 /
//! PRD §5 D14 forbid that. So α owes a LOUD rejection — an Error-severity
//! diagnostic naming sketch blocks as not yet supported, spanning the block — and
//! these tests are what hold α to it.
//!
//! ## Precedent
//!
//! The wording and shape mirror the `MemberDecl::Relate` "not yet supported"
//! arms landed by geometric-relations δ (task 4384) at
//! `crates/reify-compiler/src/guards.rs:407` and the purpose-body arm at
//! `crates/reify-compiler/src/traits.rs:987`. Same failure mode, same answer.
//!
//! ## Status
//!
//! RED at step-11 (does not compile: `MemberDecl::Sketch` does not exist yet, so
//! `reify-ast` itself fails to build). GREEN at step-12, which adds the variant
//! together with every exhaustive-match arm it forces.
//!
//! ## Expected to change, deliberately
//!
//! constrained-2d-sketch γ REPLACES this rejection with real compile semantics.
//! When it does, these tests must be rewritten to assert the new coded
//! diagnostics — not deleted, and never weakened into "compiles clean", which
//! would re-open the silent-no-op hole from the other side.

use reify_core::Severity;
use reify_test_support::{compile_source, errors_only};

// ── helpers ──────────────────────────────────────────────────────────────────

/// The diagnostics this task owns: Error-severity, and naming `sketch` blocks
/// as not yet supported.
///
/// Matched on the message rather than on a `DiagnosticCode` because α
/// deliberately introduces no `E_SKETCH_*` code — γ owns the coded surface, and
/// inventing a code here would strand it as a name γ has to migrate off.
fn sketch_unsupported_errors(
    module: &reify_compiler::CompiledModule,
) -> Vec<&reify_core::Diagnostic> {
    errors_only(module)
        .into_iter()
        .filter(|d| {
            let m = d.message.to_lowercase();
            m.contains("sketch") && (m.contains("not yet supported") || m.contains("not supported"))
        })
        .collect()
}

/// Byte range of `needle` within `haystack`, as `(start, end)` — used to assert
/// a label span actually covers the offending construct rather than merely
/// pointing somewhere inside the file.
fn byte_range(haystack: &str, needle: &str) -> (u32, u32) {
    let start = haystack
        .find(needle)
        .unwrap_or_else(|| panic!("test source does not contain {needle:?}"));
    (start as u32, (start + needle.len()) as u32)
}

// ── the contract ─────────────────────────────────────────────────────────────

/// A `sketch { … }` member at structure level is rejected LOUDLY: at least one
/// Error-severity diagnostic, never a silent no-op.
#[test]
fn structure_level_sketch_block_is_rejected_loudly() {
    let source = r#"structure def T {
    sketch profile {
        let a = point(0mm, 0mm)
        fix(a)
    }
}"#;
    let module = compile_source(source);
    let found = sketch_unsupported_errors(&module);

    assert!(
        !found.is_empty(),
        "a sketch block must produce a loud `not yet supported` error, not a silent no-op; \
         all diagnostics were: {:?}",
        module.diagnostics
    );
    assert!(
        found.iter().all(|d| d.severity == Severity::Error),
        "the rejection must be Error severity, not a warning: {found:?}"
    );
}

/// The rejection's label must cover the sketch block itself, so the user is
/// pointed at the construct that was dropped rather than at the enclosing
/// structure or at an arbitrary offset.
#[test]
fn the_rejection_label_covers_the_sketch_block() {
    let source = r#"structure def T {
    sketch profile {
        let a = point(0mm, 0mm)
        fix(a)
    }
}"#;
    let module = compile_source(source);
    let found = sketch_unsupported_errors(&module);
    let diag = found
        .first()
        .unwrap_or_else(|| panic!("no sketch rejection; diagnostics: {:?}", module.diagnostics));

    let (block_start, block_end) = byte_range(source, "sketch profile {");
    // The block's true end is its closing brace; assert containment of the
    // HEADER range rather than exact equality, so a span that legitimately
    // covers the whole block (header through `}`) also passes. What is pinned
    // is that the label is anchored ON the sketch block, not that it has one
    // exact width — the latter would be a span-drift trap of the kind
    // `imaginary_literal_grammar_tests.rs:202-226` warns against.
    assert!(
        !diag.labels.is_empty(),
        "the rejection must carry a label so the user sees WHERE: {diag:?}"
    );
    let label = &diag.labels[0];
    assert!(
        label.span.start <= block_start && label.span.end >= block_end,
        "label span {:?} must cover the sketch block header at ({block_start}, {block_end}); \
         diagnostic: {diag:?}",
        label.span
    );
}

/// A sketch block nested inside a `where { … }` guarded block is rejected too.
///
/// The grammar admits it there because `sketch_block` was added to
/// `commonMembers()`, which feeds `_guard_member` — exactly as `relate_block`
/// is. `guards.rs` is a SEPARATE member loop from `entity.rs`, so a rejection
/// wired only into the latter would leave this position silent. That is the
/// hole this test exists to keep shut.
#[test]
fn sketch_block_inside_a_guarded_block_is_rejected_loudly() {
    let source = r#"structure def T {
    param active : Bool = true
    where active {
        sketch profile {
            let a = point(0mm, 0mm)
            fix(a)
        }
    }
}"#;
    let module = compile_source(source);
    let found = sketch_unsupported_errors(&module);

    assert!(
        !found.is_empty(),
        "a sketch block inside `where {{ }}` must also be rejected loudly; \
         all diagnostics were: {:?}",
        module.diagnostics
    );
}

/// Anti-cascade: ONE diagnostic per sketch block, however many members the body
/// holds.
///
/// Matches the `Relate` precedent — the arm rejects the block as a unit and
/// never walks into it, so body size cannot inflate the diagnostic count.
#[test]
fn exactly_one_diagnostic_per_sketch_block() {
    let source = r#"structure def T {
    sketch profile {
        let a = point(0mm, 0mm)
        let b = point(10mm, 0mm)
        let ab = line(a, b)
        fix(a)
        horizontal(ab)
    }
}"#;
    let module = compile_source(source);
    let found = sketch_unsupported_errors(&module);

    assert_eq!(
        found.len(),
        1,
        "a five-member sketch body must still draw exactly ONE rejection, got {}: {:?}",
        found.len(),
        found
    );
}

/// Two sketch blocks draw two diagnostics — one each.
///
/// The companion to the anti-cascade test above: together they pin the count to
/// the number of BLOCKS, so neither over-reporting (per body member) nor
/// under-reporting (first offender only, then silence) can pass.
#[test]
fn two_sketch_blocks_draw_two_diagnostics() {
    let source = r#"structure def T {
    sketch first {
        fix(a)
    }
    sketch second {
        fix(b)
    }
}"#;
    let module = compile_source(source);
    let found = sketch_unsupported_errors(&module);

    assert_eq!(
        found.len(),
        2,
        "one rejection per sketch block — two blocks, two diagnostics, got {}: {:?}",
        found.len(),
        found
    );
}
