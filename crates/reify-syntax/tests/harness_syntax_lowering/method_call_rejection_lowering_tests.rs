//! Reify has no method-call syntax (GR-040, docs/architecture-audit/gap-register.md).
//! The grammar's `namespaced_call` captures every `<receiver>.name(args)`, so lowering is
//! where it must be refused, and the refusal must name the rule (task #6406).

use reify_ast::*;

fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("method_call_test"));
    (module.declarations, module.errors)
}

fn only_structure(decls: &[Declaration]) -> &StructureDef {
    let mut found = decls.iter().filter_map(|d| match d {
        Declaration::Structure(s) => Some(s),
        _ => None,
    });
    let structure = found
        .next()
        .unwrap_or_else(|| panic!("expected a Declaration::Structure, got {decls:?}"));
    assert!(
        found.next().is_none(),
        "expected exactly one structure declaration, got {decls:?}"
    );
    structure
}

/// `source` declares a structure with exactly ONE member, whose value is a
/// `<receiver>.name(args)` call with callee text `callee`. The member must be
/// dropped, and exactly one diagnostic must span `callee` and name the rule.
fn assert_method_call_rejected(source: &str, callee: &str) {
    let (decls, errors) = parse_decls(source);
    let members = &only_structure(&decls).members;
    assert!(
        members.is_empty(),
        "a rejected method call must drop the enclosing member, not half-build it, \
         in `{source}`; got {members:?}"
    );
    assert_eq!(
        errors.len(),
        1,
        "a rejection is ONE diagnostic, not a cascade, in `{source}`; got {errors:?}"
    );
    let error = &errors[0];

    let start = source
        .find(callee)
        .unwrap_or_else(|| panic!("callee `{callee}` not found in `{source}`"))
        as u32;
    let end = start + callee.len() as u32;
    assert_eq!(
        (error.span.start, error.span.end),
        (start, end),
        "the diagnostic must span the callee `{callee}` in `{source}`; got {error:?}"
    );

    assert!(
        error.message.contains("method-call syntax"),
        "the diagnostic must name the no-method-call rule (GR-040) in `{source}`; \
         got: {}",
        error.message
    );
    assert!(
        !error.message.contains("  "),
        "the diagnostic must not contain a run of spaces (a collapsed line \
         continuation) in `{source}`; got: {:?}",
        error.message
    );
}

#[test]
fn identifier_receiver_in_let_is_rejected_naming_the_rule() {
    assert_method_call_rejected("structure def S { let ys = xs.map(|v| v + 1) }", "xs.map");
}

/// The valid constraint form is `constraint <expr>`; `constraint name: expr`
/// is not Reify, and fails as "invalid constraint" whatever the expression.
#[test]
fn identifier_receiver_in_constraint_is_rejected_naming_the_rule() {
    assert_method_call_rejected("structure def S { constraint xs.all(|v| v > 0) }", "xs.all");
}

#[test]
fn list_literal_receiver_in_let_is_rejected_naming_the_rule() {
    assert_method_call_rejected(
        "structure def S { let ys = [1, 2, 3].filter(|v| v > 1) }",
        "[1, 2, 3].filter",
    );
}

#[test]
fn list_literal_receiver_in_constraint_is_rejected_naming_the_rule() {
    assert_method_call_rejected(
        "structure def S { constraint [1, 2].any(|v| v > 0) }",
        "[1, 2].any",
    );
}

#[test]
fn parenthesized_receiver_is_rejected_naming_the_rule() {
    assert_method_call_rejected(
        "structure def S { let ys = (xs).map(|v| v + 1) }",
        "(xs).map",
    );
}

#[test]
fn member_chain_receiver_is_rejected_naming_the_rule() {
    assert_method_call_rejected(
        "structure def S { let ys = self.xs.filter(|v| v > 1) }",
        "self.xs.filter",
    );
}

#[test]
fn call_result_receiver_is_rejected_naming_the_rule() {
    assert_method_call_rejected(
        "structure def S { let y = f(1).fold(0, |a, v| a + v) }",
        "f(1).fold",
    );
}

#[test]
fn index_result_receiver_is_rejected_naming_the_rule() {
    assert_method_call_rejected(
        "structure def S { let ys = arr[0].generate(3, |i| i) }",
        "arr[0].generate",
    );
}
