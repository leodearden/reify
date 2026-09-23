//! Constraint definition tests.
//!
//! Tests for `constraint def Name { params, predicate_lines }` declarations.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("constraint_def_test"),
    );
    (module.declarations, module.errors)
}

/// The one error whose message starts with `prefix`, or a failure naming every error emitted.
#[track_caller]
fn only_error_starting_with<'a>(errors: &'a [ParseError], prefix: &str) -> &'a ParseError {
    let mut matching = errors.iter().filter(|e| e.message.starts_with(prefix));
    let error = matching
        .next()
        .unwrap_or_else(|| panic!("expected an error starting with {prefix:?}, got: {errors:?}"));
    assert!(
        matching.next().is_none(),
        "expected exactly one {prefix:?} diagnostic, got: {errors:?}"
    );
    error
}

// ── Step 1: basic constraint def ─────────────────────────────────

#[test]
fn parse_basic_constraint_def() {
    let source = "constraint def Foo { x > 0 }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    match &decls[0] {
        Declaration::Constraint(c) => {
            assert_eq!(c.name, "Foo");
        }
        other => panic!("expected Declaration::Constraint, got {:?}", other),
    }
}

// ── Step 3: predicate expression content ─────────────────────────

#[test]
fn parse_constraint_def_predicate_expr() {
    let source = "constraint def Check { a > 5 }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert_eq!(cd.predicates.len(), 1, "expected 1 predicate");

    // Verify the predicate is a BinOp with op '>' and left Ident('a')
    match &cd.predicates[0].kind {
        ExprKind::BinOp { op, left, .. } => {
            assert_eq!(op, ">", "expected op '>'");
            match &left.kind {
                ExprKind::Ident(name) => assert_eq!(name, "a"),
                other => panic!("expected Ident('a'), got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}

// ── Step 5: params extraction ─────────────────────────────────────

#[test]
fn parse_constraint_def_with_params() {
    let source = "constraint def MinWall {
        param wall : Length
        param process : Process
        wall >= process.min_wall
    }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert_eq!(cd.params.len(), 2, "expected 2 params");
    assert_eq!(cd.params[0].name, "wall");
    assert_eq!(cd.params[1].name, "process");
    assert_eq!(cd.predicates.len(), 1, "expected 1 predicate");
}

// ── Step 7: multiple predicates ───────────────────────────────────

#[test]
fn parse_constraint_def_multiple_predicates() {
    let source = "constraint def Multi {
        param x : Length
        x > 0
        x < 100
        x != 50
    }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert_eq!(
        cd.predicates.len(),
        3,
        "expected 3 predicates (conjunction)"
    );
}

// ── Step 9: pub constraint def ────────────────────────────────────

#[test]
fn parse_pub_constraint_def() {
    let source = "pub constraint def Visible { x > 0 }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert!(cd.is_pub, "expected is_pub == true");
    assert_eq!(cd.name, "Visible");
}

// ── Step 11: type parameters ──────────────────────────────────────

#[test]
fn parse_constraint_def_with_type_params() {
    let source = "constraint def Aligned<T : Rigid> { param t : T  t.aligned }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert_eq!(cd.type_params.len(), 1, "expected 1 type param");
    assert_eq!(cd.type_params[0].name, "T");
}

// ── Step 13: complex integration test ────────────────────────────

#[test]
fn parse_constraint_def_complex() {
    // A realistic constraint def from the spec style (DFM-like)
    let source = "pub constraint def MinWallThickness<M : ManufacturingProcess> {
        param wall : Length
        param process : M
        wall >= process.min_wall_thickness
        wall > 0
    }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let cd = match &decls[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    assert_eq!(cd.name, "MinWallThickness");
    assert!(cd.is_pub);
    assert_eq!(cd.type_params.len(), 1);
    assert_eq!(cd.type_params[0].name, "M");
    assert_eq!(cd.params.len(), 2);
    assert_eq!(cd.params[0].name, "wall");
    assert_eq!(cd.params[1].name, "process");
    assert_eq!(cd.predicates.len(), 2);
}

// ── Step 15: syntax error in constraint body ──────────────────────

#[test]
fn parse_constraint_def_body_syntax_error() {
    // `>= }` is not a valid body item — no left operand, no right operand.
    // Tree-sitter produces an ERROR node as a direct child of constraint_definition.
    // With current `_ => {}` catch-all this is silently swallowed (no error reported).
    // After step-16 fix, an explicit "ERROR" arm emits 'syntax error in constraint body'.
    let source = "constraint def Bad { >= }";
    let (decls, errors) = parse_decls(source);
    assert!(
        !errors.is_empty(),
        "expected parse errors for invalid syntax inside constraint body, got none"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("syntax error in constraint body")),
        "expected an error message containing 'syntax error in constraint body', got: {:?}",
        errors
    );
    // INV-SF-7, task #6156: an excerpt already inside the snippet bound is reproduced verbatim.
    assert_eq!(
        only_error_starting_with(&errors, "syntax error in constraint body").message,
        "syntax error in constraint body: >="
    );
    // The constraint def should still be constructed (with empty predicates).
    assert_eq!(
        decls.len(),
        1,
        "expected constraint decl to be constructed despite body error"
    );
    match &decls[0] {
        Declaration::Constraint(c) => {
            assert_eq!(c.name, "Bad");
            assert!(
                c.predicates.is_empty(),
                "expected no predicates due to body error"
            );
        }
        other => panic!("expected Declaration::Constraint, got {:?}", other),
    }
}

// ── Step 17: error param in constraint body ───────────────────────

#[test]
fn parse_constraint_def_error_param() {
    // `param wall : Box<,>` — a type_arg_list with a leading comma forces
    // tree-sitter to insert a MISSING type-arg node inside type_args, making the
    // param_declaration node have `has_error() == true`.
    //
    // (An empty `Box<>` no longer suffices: once single-sided range arms `<expr`/
    // `>expr` were added to the expression grammar, the parser prefers reading
    // `Box` as a bare named type and the trailing `<...` as a range expression,
    // so `Box<>` parses with no MISSING node. `Box<,>` still has no valid parse
    // other than the MISSING-type-arg recovery, so it remains a clean probe for
    // the `check_and_lower!` has_error path.)
    //
    // Without `check_and_lower!` (before step-18), `self.lower_param()` is called
    // directly: it succeeds (name "wall" is found) and silently adds the malformed
    // param to params with no diagnostic pushed.
    //
    // After step-18 fix, `check_and_lower!` detects `has_error()`, emits
    // 'invalid constraint param: ...', and skips the param entirely.
    let source = "constraint def Bad { param wall : Box<,>  x > 0 }";
    let (decls, errors) = parse_decls(source);
    assert!(
        !errors.is_empty(),
        "expected parse errors for malformed param_declaration (Box<,> has MISSING type arg), got none"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("invalid constraint param")),
        "expected an error message containing 'invalid constraint param', got: {:?}",
        errors
    );
    // The constraint should still be constructed: 0 params (bad param skipped), 1 valid predicate.
    assert_eq!(
        decls.len(),
        1,
        "expected constraint decl to be constructed despite param error"
    );
    match &decls[0] {
        Declaration::Constraint(c) => {
            assert_eq!(c.name, "Bad");
            assert_eq!(
                c.params.len(),
                0,
                "expected 0 params (malformed param skipped by check_and_lower!)"
            );
            assert_eq!(c.predicates.len(), 1, "expected 1 valid predicate (x > 0)");
        }
        other => panic!("expected Declaration::Constraint, got {:?}", other),
    }
}

// ── A body ERROR is one line, located at its first unexpected token ──
//
// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #6156.

/// A multi-line recovery blob is reported as ONE line, starting at line 3's `) (`.
///
/// The location assertion pins the flat-body policy. Recovery nests only LATER debris (line 5's
/// `( (`) in inner `ERROR`s, so an innermost-fault walk would report only on line 5 and drop
/// line 3's break.
#[test]
fn multi_line_body_error_is_one_line_at_its_first_unexpected_token() {
    let source = "constraint def Eq {\n  param x: Length\n  x > 0 ) (\n    x < 10mm\n    x != 3mm ( (\n  x > 1mm\n}\n";
    let (_, errors) = parse_decls(source);
    let error = only_error_starting_with(&errors, "syntax error in constraint body");
    assert!(
        !error.message.contains('\n'),
        "expected a single-line diagnostic, got: {errors:?}"
    );
    assert_eq!(error.message, "syntax error in constraint body: ) (…");
    assert_eq!(error.span.start as usize, source.find(") (").unwrap());
}

/// Recovery folds line 4's `x` into the `ERROR` begun by line 3's `param = =`; the report still
/// belongs where that `ERROR` starts, not at the absorbed `x`.
#[test]
fn body_error_is_located_at_its_start_not_at_absorbed_debris() {
    let source = "constraint def Eq {\n  param x: Length\n  param = =\n  x > 0\n}\n";
    let (_, errors) = parse_decls(source);
    let error = only_error_starting_with(&errors, "syntax error in constraint body");
    assert!(
        !error.message.contains('\n'),
        "expected a single-line diagnostic, got: {errors:?}"
    );
    assert_eq!(error.message, "syntax error in constraint body: param = =…");
    assert_eq!(error.span.start as usize, source.find("param = =").unwrap());
}

// ── A faulty predicate is refused and diagnosed; a faulty let is diagnosed ──
//
// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #6156: a CST
// fault inside a constraint def must never lower into a predicate the source does not state, nor
// vanish without a diagnostic.

/// The single constraint definition `decls` must consist of.
#[track_caller]
fn sole_constraint(decls: &[Declaration]) -> &ConstraintDef {
    match decls {
        [Declaration::Constraint(c)] => c,
        other => panic!("expected exactly one constraint def, got {other:?}"),
    }
}

/// A stray `)` must not fuse a predicate with the next line's. Measured before the guard: zero
/// diagnostics, and a second predicate lowered as `(x < 10mm) > 1mm`.
#[test]
fn faulty_predicate_is_refused_and_diagnosed() {
    let source = "constraint def Eq {\n  param x: Length\n  x > 0\n  x < 10mm )\n  x > 1mm\n}\n";
    let (decls, errors) = parse_decls(source);
    let error = only_error_starting_with(&errors, "invalid constraint predicate");
    assert_eq!(error.message, "invalid constraint predicate: x < 10mm )…");
    let stray_paren = source.find("10mm )").unwrap() + "10mm ".len();
    assert_eq!(error.span.start as usize, stray_paren);
    assert_eq!(
        sole_constraint(&decls).predicates.len(),
        1,
        "only `x > 0` may lower, got: {decls:?}"
    );
}

/// An unclosed call must not absorb the next line as an argument. Measured before the guard:
/// zero diagnostics, and one predicate `x > f(1, x < 10mm)` closed by a MISSING `)`.
#[test]
fn unclosed_call_predicate_is_refused_and_diagnosed() {
    let source = "constraint def Eq {\n  param x: Length\n  x > f(1,\n  x < 10mm\n}\n";
    let (decls, errors) = parse_decls(source);
    let error = only_error_starting_with(&errors, "invalid constraint predicate");
    assert_eq!(error.message, "invalid constraint predicate: x > f(1,…");
    assert!(
        sole_constraint(&decls).predicates.is_empty(),
        "no predicate may lower, got: {decls:?}"
    );
}

/// Lets are ignored in a constraint def, but a faulty one is still reported. Measured before the
/// guard: zero diagnostics, while the `x > 0` predicate vanished into the let's recovery.
#[test]
fn faulty_let_is_diagnosed() {
    let source = "constraint def Eq {\n  param x: Length\n  let y = )\n  x > 0\n}\n";
    let (_, errors) = parse_decls(source);
    let error = only_error_starting_with(&errors, "invalid constraint let");
    assert_eq!(error.message, "invalid constraint let: let y = )…");
    assert_eq!(error.span.start as usize, source.find("= )").unwrap() + 2);
}

/// Control: a well-formed let and well-formed predicates stay silent and lower in full.
#[test]
fn well_formed_let_and_predicates_stay_silent() {
    let source =
        "constraint def Eq {\n  param x: Length\n  let y = x * 2\n  x > 0\n  y < 10mm\n}\n";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    let constraint = sole_constraint(&decls);
    assert_eq!(constraint.params.len(), 1);
    assert_eq!(constraint.predicates.len(), 2);
}
