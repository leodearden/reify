//! Doc-truth gate for the example signatures served by the `functions`
//! language-reference chunk (`crates/reify-mcp/src/tools/chunks/functions.md`),
//! which `language_chunks.rs` `include_str!`s and `reify_language_reference`
//! hands to design agents verbatim.
//!
//! # The hazard
//!
//! A chunk example declares a signature and shows its call form. A reader who
//! copies the CALL FORM without the surrounding declaration gets whatever the
//! compiler resolves that bare name to — and if the name collides with a
//! builtin that rejects the documented arity, the served documentation is
//! asserting a call form the compiler refuses. That is what the `## Overloading`
//! fence did with `rotate(geometry, axis, angle)`: the builtin `rotate` accepts
//! arities 2 and 5 only, so the copied 3-argument form drew
//! `rotate() expects 2 or 5 arguments, got 3` (task #6890).
//!
//! # What is enforced
//!
//! Every `fn`-declaration signature written anywhere in the chunk, called BARE
//! at the arity written, draws no argument-count diagnostic — plus the
//! structural companion that the Overloading section still shows one name at two
//! distinct arities, so "delete the offending line" cannot pass as a fix.
//!
//! # What is deliberately NOT established
//!
//! - **Argument types and dimensions.** The probe fills every slot with the
//!   placeholder `1mm`; a documented `Angle` parameter passed a `Length` is
//!   invisible here.
//! - **Argument order.** A permutation of two parameters satisfies everything
//!   asserted below.
//! - **Anything about prose.** The scan reads structure (a declared name and its
//!   parameter count) and checks it against compiler behaviour. No wording,
//!   heading text, docstring or fence tag is pinned, so this is not a
//!   doc-content meta-test.
//! - **That a documented name EXISTS.** An unknown call name in a `structure def`
//!   body compiles silently — no diagnostic at any severity (measured; also
//!   recorded at `stdlib_chunk_geometry_ops_smoke.rs`'s property 2). This module
//!   is the complement of that sibling's name-existence guard: it asks whether a
//!   name that DOES resolve accepts the documented arity, not whether it
//!   resolves at all.

use reify_compiler::CompiledModule;
use reify_core::{Diagnostic, Severity};
use reify_test_support::compile_source_with_stdlib;

/// The centralised label every `arg_check.rs` arity rejection carries
/// (`crates/reify-compiler/src/arg_check.rs:72`). NOT universal — see
/// [`arg_count_rejections`].
const ARG_COUNT_LABEL: &str = "wrong number of arguments";

/// The arity a `"…, got {N}"` arg-count message reports, or `None` when its tail
/// is not a bare count this matcher can read.
///
/// The split is from the RIGHT and the parse is whole-tail, so `", got 1"` is
/// never read out of `", got 12"`.
fn reported_arity(message: &str) -> Option<usize> {
    message.rsplit_once(", got ")?.1.parse().ok()
}

/// Every diagnostic in `compiled` that is an argument-count rejection of `name`
/// at `arity`.
///
/// # Why the match is on the MESSAGE SHAPE, and the label is only a fallback
///
/// `arg_check.rs` does centralise the [`ARG_COUNT_LABEL`] wording, but the label
/// is NOT universal: `crates/reify-compiler/src/builtin_signatures.rs`
/// (`probe_lowering_accepted_arities`, and the doc block above it) records the
/// measurement that `geometry.rs`'s `extrude` arm pushes an arg-count error
/// carrying no label at all. A label-ONLY matcher therefore has UNSAFE polarity
/// — an unlabelled arity rejection reads as "arity accepted" and yields a false
/// GREEN, the exact silent pass this module exists to prevent. So the label
/// cannot be required.
///
/// Nor can it be a plain alternative to the arity tail: a rejection carrying the
/// label would then match at EVERY arity, and the matcher would stop
/// discriminating the one thing it is asked about. The two signals are therefore
/// layered rather than OR'd —
///
/// 1. a `"{name}() expects"` message whose tail [`reported_arity`] can read is
///    attributed to exactly that arity;
/// 2. a `"{name}() expects"` message whose tail it CANNOT read is attributed to
///    every arity if it carries the label, and to none otherwise.
///
/// Layer 2 is what keeps the failure polarity safe: an arity message this
/// matcher does not recognise can only produce a false RED that forces a human
/// look, never a false GREEN. The message shape `"{name}() expects …, got {N}"`
/// held for all 34 observable names across all three emit sites when the
/// builtin_signatures ledger was measured, so layer 2 is expected to stay
/// unreached.
fn arg_count_rejections<'a>(
    compiled: &'a CompiledModule,
    name: &str,
    arity: usize,
) -> Vec<&'a Diagnostic> {
    let prefix = format!("{name}() expects");
    compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.message.starts_with(&prefix)
                && match reported_arity(&d.message) {
                    Some(reported) => reported == arity,
                    None => d.labels.iter().any(|l| l.message == ARG_COUNT_LABEL),
                }
        })
        .collect()
}

/// Anti-vacuity control for every later assertion in this module: it pins that
/// [`arg_count_rejections`] really does see a builtin's arity rejection, on a
/// HARDCODED probe (never read from any chunk) whose rejected and accepted
/// arities are both known.
///
/// Without this, a matcher that silently stopped recognising arity diagnostics
/// — or a compiler that stopped arity-gating `rotate` at all — would make the
/// live-chunk gate below pass trivially, which is the one failure a guard of
/// this kind must never have.
#[test]
fn arg_count_rejection_is_detected_for_the_builtin_rotate_arity_gate() {
    let compiled = compile_source_with_stdlib(
        "module functions_chunk_probe\n\
         \n\
         structure def ArgCountControl {\n\
         \x20   let bad = rotate(1mm, 1mm, 1mm)\n\
         \x20   let good = rotate(1mm, 1mm)\n\
         }\n",
    );

    let rejected = arg_count_rejections(&compiled, "rotate", 3);
    assert_eq!(
        rejected.len(),
        1,
        "the builtin `rotate` rejects arity 3 (geometry_transform.rs's `n =>` arm), so exactly \
         one arg-count diagnostic must be matched; got {rejected:#?}"
    );
    assert_eq!(
        rejected[0].message, "rotate() expects 2 or 5 arguments, got 3",
        "the arity rejection's wording is the matcher's anchor — see arg_count_rejections"
    );
    assert_eq!(
        rejected[0].severity,
        Severity::Error,
        "an arity rejection is emitted through Diagnostic::error (arg_check.rs:71)"
    );
    assert!(
        rejected[0]
            .labels
            .iter()
            .any(|label| label.message == ARG_COUNT_LABEL),
        "this arm routes through push_labeled_arg_count_error, so it must carry the centralised \
         label (crates/reify-compiler/src/arg_check.rs:72); got {:#?}",
        rejected[0].labels
    );

    assert!(
        arg_count_rejections(&compiled, "rotate", 2).is_empty(),
        "arity 2 is one of `rotate`'s accepted arities, so the matcher must report nothing for \
         it — a matcher that fires here would make every gate below unconditionally red"
    );
}
