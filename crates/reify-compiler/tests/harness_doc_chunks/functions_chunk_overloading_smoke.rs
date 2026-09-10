//! Doc-truth gate for the example signatures served by the `functions`
//! language-reference chunk (`crates/reify-mcp/src/tools/chunks/functions.md`).

use reify_core::Severity;
use reify_test_support::compile_source_with_stdlib;

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
            .any(|label| label.message == "wrong number of arguments"),
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
