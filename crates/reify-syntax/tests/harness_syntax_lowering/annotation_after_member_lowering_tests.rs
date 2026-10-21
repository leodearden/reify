//! Task #8300: an `@annotation` line after a value-ended member lowers onto the
//! FOLLOWING member, not into the preceding member's expression.

use reify_ast::*;
use reify_core::ModulePath;

// The three repros are the CLI e2e fixtures, shared so every layer reads the
// same source.
const AFTER_VALUED_PARAM: &str =
    include_str!("../../../reify-cli/tests/fixtures/annotation_after_valued_param.ri");
const AFTER_LET_CATALOG: &str =
    include_str!("../../../reify-cli/tests/fixtures/annotation_after_let_catalog.ri");
const AFTER_CONSTRAINT: &str =
    include_str!("../../../reify-cli/tests/fixtures/annotation_after_constraint.ri");

const AFTER_SUB: &str = "structure S {
    sub s = Foo()
    @solver_hint(\"discrete_set\", standard_bolt_lengths)
    param b : Length = auto
}
";

fn members(source: &str) -> Vec<MemberDecl> {
    let module = reify_syntax::parse(source, ModulePath::single("m"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    match module.declarations.into_iter().next() {
        Some(Declaration::Structure(s)) => s.members,
        other => panic!("expected a structure, got {other:?}"),
    }
}

fn param<'a>(members: &'a [MemberDecl], name: &str) -> &'a ParamDecl {
    members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Param(p) if p.name == name => Some(p),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no param `{name}` in {members:?}"))
}

fn assert_b_carries_the_hint(members: &[MemberDecl], collection: &str) {
    let annotations = &param(members, "b").annotations;
    assert_eq!(annotations.len(), 1, "param b annotations: {annotations:?}");
    assert_eq!(annotations[0].name, "solver_hint");
    match &annotations[0].args.get(1).map(|e| &e.kind) {
        Some(ExprKind::Ident(name)) => assert_eq!(name, collection),
        other => panic!("expected Ident({collection:?}) as 2nd arg, got {other:?}"),
    }
}

#[test]
fn annotation_after_a_valued_param_attaches_to_the_next_param() {
    let members = members(AFTER_VALUED_PARAM);
    assert_b_carries_the_hint(&members, "standard_bolt_lengths");
    assert!(param(&members, "a").annotations.is_empty());
}

#[test]
fn annotation_after_a_let_catalog_attaches_to_the_next_param() {
    let members = members(AFTER_LET_CATALOG);
    assert_b_carries_the_hint(&members, "sizes");
    let sizes = members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Let(l) if l.name == "sizes" => Some(l),
            _ => None,
        })
        .expect("no let `sizes`");
    assert!(sizes.annotations.is_empty(), "{:?}", sizes.annotations);
}

#[test]
fn annotation_after_a_constraint_attaches_to_the_next_param() {
    let members = members(AFTER_CONSTRAINT);
    assert_b_carries_the_hint(&members, "standard_bolt_lengths");
    assert!(param(&members, "a").annotations.is_empty());
}

#[test]
fn annotation_after_a_sub_attaches_to_the_next_param() {
    let members = members(AFTER_SUB);
    assert_b_carries_the_hint(&members, "standard_bolt_lengths");
}
