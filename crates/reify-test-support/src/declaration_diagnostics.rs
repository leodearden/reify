//! Assertions on compile diagnostics anchored at a declaration site (`let x`, `param x`, ...).

use reify_core::DiagnosticCode;

use crate::helpers::{compile_source, errors_only};

/// Compile `source` (stdlib-less [`compile_source`]) and assert it yields exactly one
/// Error-severity diagnostic. That diagnostic must carry `code`, its message must quote
/// `'name'`, and its first label must cover `<keyword> <name>`. Returns the message for
/// callers that pin rendered types.
///
/// Pins code identity, the quoted name and the anchor, never other message prose
/// (docs/prds/v0_6/dimensioned-construction-strictness.md D4-8).
#[track_caller]
pub fn assert_single_declaration_error(
    source: &str,
    code: DiagnosticCode,
    keyword: &str,
    name: &str,
) -> String {
    let module = compile_source(source);
    let errors = errors_only(&module);
    let shapes: Vec<_> = errors.iter().map(|d| (&d.message, &d.code)).collect();
    let declaration = format!("{keyword} {name}");

    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error for '{declaration}', got: {shapes:?}"
    );
    let diag = errors[0];
    assert_eq!(
        diag.code,
        Some(code),
        "expected {code:?} for '{declaration}', got: {shapes:?}"
    );
    assert!(
        diag.message.contains(&format!("'{name}'")),
        "message must quote '{name}', got: {:?}",
        diag.message
    );
    let Some(label) = diag.labels.first() else {
        panic!("'{declaration}': diagnostic has no label: {diag:?}")
    };
    let span = label.span;
    let Some(covered) = source.get(span.start as usize..span.end as usize) else {
        panic!("'{declaration}': label span {span:?} is outside the source")
    };
    assert!(
        covered.contains(&declaration),
        "label must cover '{declaration}', covers: {covered:?}"
    );
    diag.message.clone()
}
