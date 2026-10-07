//! The `check_and_lower!` diagnostic is located at the refused child's FIRST fault.
//!
//! INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), tasks #5392 and
//! #6156: a parse error a user cannot locate is only marginally better than silence.
//!
//! `check_and_lower!` is the ONLY diagnostic a user sees for a fault inside a guarded block, a
//! port body or a connect body. Those bodies' own `ERROR` arms are shadowed: the macro refuses
//! the enclosing node before its body is ever lowered. Its span is therefore what locates those
//! faults, so it must point at the fault rather than at the start of the construct.
//!
//! The message is unchanged: `invalid <label>: ` plus a bounded excerpt of the CHILD, the
//! contract `check_and_lower_snippet_bound_tests` pins. The excerpt says WHICH construct; the
//! span says WHERE.
//!
//! Asserted through the public `reify_syntax::parse` API, on both the byte offset (derived with
//! `str::find`, never hard-coded) and the rendered `line:col: ` prefix a user actually reads.

use reify_ast::ParseError;

use crate::parse_error_lookup::{only_error_starting_with, parse_errors};

/// Assert `error` starts at byte `offset` and renders at `line_col`.
#[track_caller]
fn assert_located(error: &ParseError, source: &str, offset: usize, line_col: &str) {
    assert_eq!(
        error.span.start as usize, offset,
        "expected the diagnostic to start at byte {offset}, got: {error:?}"
    );
    let rendered = error.render(source);
    assert!(
        rendered.starts_with(&format!("{line_col}: ")),
        "expected the diagnostic rendered at {line_col}, got: {rendered:?}"
    );
}

#[test]
fn guarded_block_fault_is_located_at_the_missing_value() {
    let source = "structure S {\n  param x: Real = 1\n  where x > 0 {\n    let y =\n  }\n}\n";
    let errors = parse_errors(source);
    let error = only_error_starting_with(&errors, "invalid guarded block: ");
    // The fault is a zero-width MISSING value directly after the `=`.
    let missing_value = source.find("let y =").unwrap() + "let y =".len();
    assert_located(error, source, missing_value, "4:12");
}

#[test]
fn port_body_fault_is_located_at_the_junk_not_the_port() {
    let source = "structure S {\n  port a : in T {\n    >= )\n    <= (\n  }\n}\n";
    let errors = parse_errors(source);
    let error = only_error_starting_with(&errors, "invalid port: ");
    assert_located(error, source, source.find(">= )").unwrap(), "3:5");
    assert_eq!(error.message, "invalid port: port a : in T {…");
}

#[test]
fn connect_body_fault_is_located_at_the_junk_not_the_connect() {
    let source = "structure S {\n  port a : out T\n  port b : in T\n  connect a -> b : BoltSet {\n    >= )\n    <= (\n  }\n}\n";
    let errors = parse_errors(source);
    let error = only_error_starting_with(&errors, "invalid connect: ");
    assert_located(error, source, source.find(">= )").unwrap(), "5:5");
}

#[test]
fn malformed_port_mapping_is_located_at_the_missing_target() {
    let source = "structure S { port a : out T port b : in T connect a -> b { shaft -> } }";
    let errors = parse_errors(source);
    let error = only_error_starting_with(&errors, "invalid connect: ");
    // The mapping has a "from" but no "to": the fault is a MISSING identifier after the `->`.
    let missing_target = source.find("shaft ->").unwrap() + "shaft ->".len();
    assert_located(error, source, missing_target, "1:69");
    assert_eq!(
        error.message,
        "invalid connect: connect a -> b { shaft -> }"
    );
}
