//! End-to-end `reify eval` on task #8300's repros: an `@solver_hint` line after
//! a valued param, a `let` catalog, or a constraint is its own member, so the
//! file evaluates instead of failing on a joined ad-hoc selector.

use crate::common;
use reify_syntax::member_continuation::is_member_continuation_message;

/// Runs `reify eval` on `fixture` and asserts it exits 0, prints every cell in
/// `cells`, and reports no parse, selector, or member-continuation error.
fn assert_evaluates(fixture: &str, cells: &[&str]) {
    let (status, stdout, stderr) = common::run_subcommand("eval", &common::fixture_path(fixture));
    let context = format!("{fixture}\nstdout:\n{stdout}\nstderr:\n{stderr}");

    assert!(status.success(), "reify eval should exit 0: {context}");
    for cell in cells {
        let prefix = format!("S.{cell} = ");
        assert!(
            stdout.lines().any(|l| l.starts_with(&prefix)),
            "missing `{prefix}` line: {context}"
        );
    }
    for stream in [&stdout, &stderr] {
        assert!(!stream.contains("Parse error"), "{context}");
        assert!(!stream.contains("unknown selector kind"), "{context}");
        assert!(
            !stream.lines().any(is_member_continuation_message),
            "{context}"
        );
    }
}

#[test]
fn eval_annotation_after_a_valued_param() {
    assert_evaluates("annotation_after_valued_param.ri", &["a", "b"]);
}

#[test]
fn eval_annotation_after_a_let_catalog() {
    assert_evaluates("annotation_after_let_catalog.ri", &["sizes", "b"]);
}

#[test]
fn eval_annotation_after_a_constraint() {
    assert_evaluates("annotation_after_constraint.ri", &["a", "b"]);
}
