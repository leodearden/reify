//! α's loud "not yet supported" contract for `MemberDecl::Sketch`
//! (constrained-2d-sketch α, task 5506).
//!
//! α lands the `sketch { … }` grammar, AST and lowering but no compile
//! semantics, so every position that admits a sketch block must reject it with
//! an Error rather than drop it silently (INV-SF-1 / PRD §5 D14). This module is
//! the single home of that rejection: [`diagnostic`] is the one message, used
//! by the structure-level member loop (entity.rs), the guarded-member loop
//! (guards.rs) and [`validate_module`], which covers the positions neither loop
//! enters — specialization bodies and keyed override blocks.
//!
//! Deletion boundary: constrained-2d-sketch γ (#5508) replaces the rejection
//! with real compile lowering and removes this module together with its call
//! sites.

use reify_ast::{MemberDecl, ParsedModule};
use reify_core::{Diagnostic, DiagnosticLabel, SourceSpan};

use super::specialization_scope_check::for_each_specialization_member;

/// The rejection for one sketch block, labelled across `span`.
pub(crate) fn diagnostic(span: SourceSpan) -> Diagnostic {
    Diagnostic::error( // pdiag:allow — α mints no code; γ (#5508) owns the E_SKETCH_* surface and deletes this module
        "sketch blocks are not yet supported \
         (compile lowering lands in constrained-2d-sketch task γ)",
    )
    .with_label(DiagnosticLabel::new(span, "not yet supported"))
}

/// Reject every sketch block that sits inside a specialization scope.
///
/// Scope roots and their traversal come from the §8.7 check's own walker, so
/// both passes see exactly the same members. That walker never descends into a
/// sketch body, so a block is reported once, as a unit.
pub(crate) fn validate_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
    for_each_specialization_member(parsed, &mut |member| {
        if let MemberDecl::Sketch(sketch) = member {
            diagnostics.push(diagnostic(sketch.span));
        }
    });
}
