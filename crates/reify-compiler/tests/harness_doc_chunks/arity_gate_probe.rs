//! Which `(name, arity)` call forms a builtin's arity gate rejects.
//!
//! Every form is probed by calling it BARE with `1mm` in every slot. Placeholder
//! fillers suffice because a builtin dispatches on argument COUNT before it
//! resolves any argument, so a wrong arity is reported whatever the fillers are;
//! `probe_lowering_accepted_arities` in `crates/reify-compiler/src/builtin_signatures.rs`
//! probes with the same shape. All forms share ONE module and one compile.
//!
//! # What is NOT read
//!
//! - **A type error drawn by a filler.** A correctly documented form draws one
//!   (`rotate: angle argument expects Angle, got Scalar[m]`), so only the
//!   argument-count message shape is read — see [`arg_count_rejections`].
//! - **A stdlib-`.ri` function's rejection.** It reports
//!   `no matching overload for NAME(types), candidates: …`, which mixes arity
//!   with type; telling them apart would mean parsing the candidate list out of
//!   a message, an ad-hoc parser of a meaningful string (heuristic 12).
//! - **A form whose bare call does not parse as a call of that name and arity**
//!   — a keyword callee such as `auto`. The parser decides, never a kept list.

use std::collections::BTreeSet;

use reify_ast::{Declaration, ExprKind, MemberDecl};
use reify_compiler::parse_with_stdlib;
use reify_core::{Diagnostic, DiagnosticLabel, ModulePath, Severity, SourceSpan};
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

/// Every diagnostic in `diagnostics` that is an argument-count rejection of
/// `name` at `arity`.
///
/// # Why the match is on the MESSAGE SHAPE, and the label is only a fallback
///
/// `arg_check.rs` does centralise the [`ARG_COUNT_LABEL`] wording, but the label
/// is NOT universal: `crates/reify-compiler/src/builtin_signatures.rs`
/// (`probe_lowering_accepted_arities`, and the doc block above it) records the
/// measurement that `geometry.rs`'s `extrude` arm pushes an arg-count error
/// carrying no label at all. A label-ONLY matcher therefore has UNSAFE polarity
/// — an unlabelled arity rejection reads as "arity accepted" and yields a false
/// GREEN, the exact silent pass an arity gate exists to prevent. So the label
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
    diagnostics: &'a [Diagnostic],
    name: &str,
    arity: usize,
) -> Vec<&'a Diagnostic> {
    let prefix = format!("{name}() expects");
    diagnostics
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

/// One call form a builtin rejects on arity, with the compiler's message.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ArityRejection {
    pub(crate) name: String,
    pub(crate) arity: usize,
    pub(crate) message: String,
}

/// Every form in `forms` that a builtin's arity gate rejects, sorted and
/// deduped — one entry per rejecting message.
///
/// Only the forms whose bare call parses as a call of that name and arity are
/// probed ([`parses_as_call`]), so the one shared compile can never meet a
/// parse error.
pub(crate) fn arity_rejections(forms: &[(String, usize)]) -> Vec<ArityRejection> {
    let probed: BTreeSet<(&str, usize)> = forms
        .iter()
        .map(|(name, arity)| (name.as_str(), *arity))
        .filter(|(name, arity)| parses_as_call(name, *arity))
        .collect();

    let mut source = String::from("module arity_gate_probe\n\nstructure def ArityProbe {\n");
    for (i, (name, arity)) in probed.iter().enumerate() {
        source.push_str(&format!("    let v{i} = {}\n", probe_call(name, *arity)));
    }
    source.push_str("}\n");
    let compiled = compile_source_with_stdlib(&source);

    let mut rejections: Vec<ArityRejection> = probed
        .iter()
        .flat_map(|(name, arity)| {
            arg_count_rejections(&compiled.diagnostics, name, *arity)
                .into_iter()
                .map(|d| ArityRejection {
                    name: name.to_string(),
                    arity: *arity,
                    message: d.message.clone(),
                })
        })
        .collect();
    rejections.sort();
    rejections.dedup();
    rejections
}

/// `name` called bare with `arity` `1mm` fillers.
fn probe_call(name: &str, arity: usize) -> String {
    format!("{name}({})", vec!["1mm"; arity].join(", "))
}

/// Whether `name`'s probe call parses, on its own, as a call of `name` with
/// `arity` arguments — the parser's word on what is a call, so a keyword
/// callee (`auto`) is never probed.
fn parses_as_call(name: &str, arity: usize) -> bool {
    let source = format!(
        "structure def ArityProbe {{\n    let v = {}\n}}\n",
        probe_call(name, arity)
    );
    let parsed = parse_with_stdlib(&source, ModulePath::single("arity_gate_probe"));
    if !parsed.errors.is_empty() {
        return false;
    }
    let [Declaration::Structure(structure)] = parsed.declarations.as_slice() else {
        return false;
    };
    let [MemberDecl::Let(binding)] = structure.members.as_slice() else {
        return false;
    };
    matches!(
        &binding.value.kind,
        ExprKind::FunctionCall { name: called, args, .. } if called == name && args.len() == arity
    )
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
