//! Which `(name, arity)` call forms a builtin's arity gate rejects.

use reify_core::{Diagnostic, DiagnosticLabel, Severity, SourceSpan};
use reify_test_support::compile_source_with_stdlib;

/// The centralised label every `arg_check.rs` arity rejection carries
/// (`crates/reify-compiler/src/arg_check.rs:72`). NOT universal — see
/// [`arg_count_rejections`].
const ARG_COUNT_LABEL: &str = "wrong number of arguments";

/// Every diagnostic in `diagnostics` that is an argument-count rejection of
/// `name` at `arity`.
fn arg_count_rejections<'a>(
    _diagnostics: &'a [Diagnostic],
    _name: &str,
    _arity: usize,
) -> Vec<&'a Diagnostic> {
    Vec::new()
}

/// One call form a builtin rejects on arity, with the compiler's message.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ArityRejection {
    pub(crate) name: String,
    pub(crate) arity: usize,
    pub(crate) message: String,
}

/// Every form in `forms` that a builtin's arity gate rejects, sorted.
pub(crate) fn arity_rejections(_forms: &[(String, usize)]) -> Vec<ArityRejection> {
    Vec::new()
}

/// Anti-vacuity control for every arity gate built on [`arg_count_rejections`]:
/// it pins that the matcher really does see a builtin's arity rejection, on a
/// HARDCODED probe (never read from any chunk) whose rejected and accepted
/// arities are both known.
///
/// Without this, a matcher that silently stopped recognising arity diagnostics
/// — or a compiler that stopped arity-gating `rotate` at all — would make every
/// live-chunk gate pass trivially, which is the one failure a guard of this
/// kind must never have.
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

    let rejected = arg_count_rejections(&compiled.diagnostics, "rotate", 3);
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
        arg_count_rejections(&compiled.diagnostics, "rotate", 2).is_empty(),
        "arity 2 is one of `rotate`'s accepted arities, so the matcher must report nothing for \
         it — a matcher that fires here would make every gate built on it unconditionally red"
    );
}

/// The matcher's two layers, on hand-built diagnostics: a readable `, got {N}`
/// tail is attributed to exactly that arity; an unreadable one falls back to the
/// label.
#[test]
fn arg_count_rejections_reads_the_tail_first_and_the_label_only_as_a_fallback() {
    let labelled = |message: &str| {
        Diagnostic::error(message)
            .with_label(DiagnosticLabel::new(SourceSpan::empty(0), ARG_COUNT_LABEL))
    };

    let twelve = [labelled("rotate() expects 2 or 5 arguments, got 12")];
    assert!(
        arg_count_rejections(&twelve, "rotate", 1).is_empty(),
        "the tail is read whole from the right, so `, got 1` is never read out of `, got 12`"
    );
    assert_eq!(
        arg_count_rejections(&twelve, "rotate", 12).len(),
        1,
        "a readable tail is attributed to exactly the arity it reports"
    );

    let unreadable = "rotate() expects 2 or 5 arguments, got several";
    for arity in [0, 1, 3, 12] {
        assert_eq!(
            arg_count_rejections(&[labelled(unreadable)], "rotate", arity).len(),
            1,
            "an unreadable tail carrying the arg-count label is attributed to EVERY arity \
             (arity {arity}) — a false red forces a human look, a false green hides a defect"
        );
        assert!(
            arg_count_rejections(&[Diagnostic::error(unreadable)], "rotate", arity).is_empty(),
            "an unreadable tail WITHOUT the label is attributed to no arity (arity {arity})"
        );
    }

    assert!(
        arg_count_rejections(&[labelled("box() expects 3 arguments, got 2")], "rotate", 2)
            .is_empty(),
        "another builtin's rejection is never attributed to `rotate`, even at its arity"
    );
}

#[test]
fn arity_rejections_reports_exactly_the_forms_a_builtin_rejects_on_arity() {
    let forms: Vec<(String, usize)> = [
        ("rotate", 3),
        ("rotate", 2),
        ("box", 2),
        ("box", 3),
        ("align", 3),
        ("auto", 1),
    ]
    .into_iter()
    .map(|(name, arity)| (name.to_string(), arity))
    .collect();

    assert_eq!(
        arity_rejections(&forms),
        vec![
            ArityRejection {
                name: "box".to_string(),
                arity: 2,
                message: "box() expects 3 arguments, got 2".to_string(),
            },
            ArityRejection {
                name: "rotate".to_string(),
                arity: 3,
                message: "rotate() expects 2 or 5 arguments, got 3".to_string(),
            },
        ],
        "exactly the rejected arities come back: rotate/3 and box/2. Not rotate/2 or box/3 \
         (accepted arities), not align/3 (no builtin of that name), and not auto/1 — `auto` is a \
         keyword, so `let v = auto(1mm)` does not parse; it must be left unprobed rather than \
         panic the one compile every other form shares"
    );
}
