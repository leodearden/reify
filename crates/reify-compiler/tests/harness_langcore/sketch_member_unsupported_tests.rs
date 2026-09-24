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

// ── specialization scopes ────────────────────────────────────────────────────
//
// A `sub s : T { … }` specialization body and a keyed `"k" => { … }` override
// block are member lists too, so the grammar admits a sketch block there. The
// structure-level member loop (entity.rs) and the guarded-member loop
// (guards.rs) never enter them, so without a dedicated rejection such a block
// compiled with `diagnostics: []` — the INV-SF-1 silent no-op this file exists
// to keep shut.

const STRUCTURE_LEVEL_SKETCH: &str = r#"structure def T {
    sketch profile {
        let a = point(0mm, 0mm)
        fix(a)
    }
}"#;

const GUARDED_SKETCH: &str = r#"structure def T {
    param active : Bool = true
    where active {
        sketch profile {
            let a = point(0mm, 0mm)
            fix(a)
        }
    }
}"#;

const SPEC_BODY_SKETCH: &str = r#"structure def Inner {
    param w : Length = 10mm
}
structure def T {
    sub s : Inner {
        sketch profile {
            let a = point(0mm, 0mm)
            fix(a)
        }
    }
}"#;

const KEYED_OVERRIDE_SKETCH: &str = r#"structure def Inner {
    param w : Length = 10mm
}
structure def T {
    sub s : Keyed<Inner> {
        "k" => {
            w = 5mm
            sketch profile {
                let a = point(0mm, 0mm)
                fix(a)
            }
        }
    }
}"#;

const GUARDED_SPEC_BODY_SKETCH: &str = r#"structure def Inner {
    param w : Length = 10mm
}
structure def T {
    param flag : Length = 2mm
    sub s : Inner {
        where flag > 1mm {
            sketch profile {
                let a = point(0mm, 0mm)
                fix(a)
            }
        }
    }
}"#;

/// Byte range of the whole `sketch profile { … }` block in `source`: from the
/// `sketch` keyword through its matching closing brace.
fn sketch_block_range(source: &str) -> (u32, u32) {
    let (start, _) = byte_range(source, "sketch profile {");
    let mut depth = 0usize;
    for (offset, ch) in source[start as usize..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return (start, start + offset as u32 + 1);
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced sketch block in test source: {source:?}")
}

/// `source` with its sketch block cut out — the non-vacuity control that proves
/// the surrounding scope compiles clean, so any error seen with the block in
/// place is the block's.
fn without_sketch_block(source: &str) -> String {
    let (start, end) = sketch_block_range(source);
    format!("{}{}", &source[..start as usize], &source[end as usize..])
}

/// The single sketch rejection in `source`, asserting it is the ONLY error and
/// that the same source minus the block compiles clean.
fn only_error_is_one_sketch_rejection(position: &str, source: &str) -> reify_core::Diagnostic {
    let control = compile_source(&without_sketch_block(source));
    assert!(
        errors_only(&control).is_empty(),
        "NON-VACUITY ({position}): the source must compile clean without its sketch block, \
         got: {:?}",
        control.diagnostics
    );

    let module = compile_source(source);
    let found = sketch_unsupported_errors(&module);
    assert_eq!(
        found.len(),
        1,
        "a sketch block in a {position} must draw exactly ONE loud rejection, got {}; \
         all diagnostics were: {:?}",
        found.len(),
        module.diagnostics
    );
    let errors = errors_only(&module);
    assert_eq!(
        errors.len(),
        1,
        "the sketch rejection must be the only error in a {position}, got: {errors:?}"
    );
    found[0].clone()
}

/// A sketch block directly inside a `sub … { … }` specialization body is
/// rejected loudly, not dropped.
#[test]
fn sketch_block_inside_a_specialization_body_is_rejected_loudly() {
    let diag = only_error_is_one_sketch_rejection("specialization body", SPEC_BODY_SKETCH);
    assert_eq!(diag.severity, Severity::Error);
}

/// A sketch block inside a keyed `"k" => { … }` override block is rejected
/// loudly — the keyed entry's overrides are a specialization scope too.
#[test]
fn sketch_block_inside_a_keyed_override_block_is_rejected_loudly() {
    let diag = only_error_is_one_sketch_rejection("keyed override block", KEYED_OVERRIDE_SKETCH);
    assert_eq!(diag.severity, Severity::Error);
}

/// A sketch block in a `where { … }` inside a specialization body — the nesting
/// a non-recursive fix would miss.
#[test]
fn sketch_block_in_a_guarded_block_inside_a_specialization_body_is_rejected_loudly() {
    let diag = only_error_is_one_sketch_rejection(
        "guarded block inside a specialization body",
        GUARDED_SPEC_BODY_SKETCH,
    );
    assert_eq!(diag.severity, Severity::Error);
}

/// The specialization-body rejection's label spans exactly the sketch block.
#[test]
fn the_specialization_body_rejection_label_covers_the_sketch_block() {
    let diag = only_error_is_one_sketch_rejection("specialization body", SPEC_BODY_SKETCH);
    let label = diag.labels.first().unwrap_or_else(|| {
        panic!("the rejection must carry a label so the user sees WHERE: {diag:?}")
    });
    let (start, end) = sketch_block_range(SPEC_BODY_SKETCH);
    assert_eq!(
        (label.span.start, label.span.end),
        (start, end),
        "label must span the sketch block {:?}; diagnostic: {diag:?}",
        &SPEC_BODY_SKETCH[start as usize..end as usize]
    );
}

/// One block, one diagnostic, in every specialization-scope position — so a
/// pre-pass and a member loop can never both report the same block.
#[test]
fn exactly_one_diagnostic_per_sketch_block_in_a_specialization_body() {
    for (position, source) in [
        ("specialization body", SPEC_BODY_SKETCH),
        ("keyed override block", KEYED_OVERRIDE_SKETCH),
        (
            "guarded block inside a specialization body",
            GUARDED_SPEC_BODY_SKETCH,
        ),
    ] {
        let module = compile_source(source);
        let found = sketch_unsupported_errors(&module);
        assert_eq!(
            found.len(),
            1,
            "{position}: one sketch block must draw exactly one rejection, got {}: {found:?}",
            found.len()
        );
    }
}

/// An `occurrence def` body is a member list like a structure's, so a sketch
/// block there is rejected loudly as well.
#[test]
fn occurrence_level_sketch_block_is_rejected_loudly() {
    let source = r#"occurrence def O {
    sketch profile {
        let a = point(0mm, 0mm)
        fix(a)
    }
}"#;
    let diag = only_error_is_one_sketch_rejection("occurrence body", source);
    assert_eq!(diag.severity, Severity::Error);
}

/// Every position that rejects a sketch block says the same thing, byte for
/// byte — the drift detector for the rejection's single message.
#[test]
fn every_sketch_rejection_site_shares_one_message() {
    let rendered: Vec<(&str, String, Vec<String>)> = [
        ("structure level", STRUCTURE_LEVEL_SKETCH),
        ("guarded block", GUARDED_SKETCH),
        ("specialization body", SPEC_BODY_SKETCH),
        ("keyed override block", KEYED_OVERRIDE_SKETCH),
    ]
    .into_iter()
    .map(|(position, source)| {
        let module = compile_source(source);
        let found = sketch_unsupported_errors(&module);
        let diag = found.first().unwrap_or_else(|| {
            panic!(
                "{position}: no sketch rejection; diagnostics: {:?}",
                module.diagnostics
            )
        });
        let labels = diag.labels.iter().map(|l| l.message.clone()).collect();
        (position, diag.message.clone(), labels)
    })
    .collect();

    let (_, first_message, first_labels) = &rendered[0];
    for (position, message, labels) in &rendered[1..] {
        assert_eq!(
            (message, labels),
            (first_message, first_labels),
            "{position}'s sketch rejection must match the structure-level one byte for byte"
        );
    }
}
