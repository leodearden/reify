//! CST→AST lowering tests for qualified references through an import binding
//! (`pp.Pulley`) — task 5495 μ, PRD `docs/prds/v0_6/stdlib-namespace.md`
//! §3.3 NS-Q2 / D-7.
//!
//! **Encoding contract pinned here.** μ carries the qualified path as a
//! DOT-JOINED string in the EXISTING `String` name slots — `TypeExprKind::Named
//! { name }`, `SubDecl::structure_name`, `ExprKind::FunctionCall { name }` — and
//! introduces NO new `TypeExprKind` / `ExprKind` variant.  `.` is not a legal
//! identifier character, so `name.contains('.')` is an unambiguous discriminator
//! for the resolution-phase fixup (task ν, PRD §3.3 NS-Q1/Q3).  This mirrors
//! resolution-unification D-9: the parse leaves an under-specified form and
//! resolution rewrites it, exactly as `Foo.Bar` enum access already does.  It is
//! also this AST's existing convention for module paths (`ImportDecl::path`).
//!
//! Step-5 RED: `lower_type_expr_node` and `lower_sub` produce the right string
//! only by ACCIDENT (a `node_text` fallback), which is why
//! `qualified_type_whitespace_is_normalised` — `pp . Pulley` → `"pp.Pulley"` —
//! is here: it fails against raw source text and forces the explicit,
//! field-joined branch that step-6 adds.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("namespaced_test"));
    (module.declarations, module.errors)
}

/// Locate the single structure declaration.
///
/// LOCATES rather than indexing `decls[0]`: every case below that declares an
/// `import` puts a `Declaration::Import` ahead of the structure, and an
/// order-independence case puts it after. Indexing position 0 would panic on
/// the former and silently pass on neither.
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

/// Unwrap the first member as a `param`, returning its type annotation.
fn first_param_type(source: &str) -> TypeExpr {
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let structure = only_structure(&decls);
    match &structure.members[0] {
        MemberDecl::Param(p) => p
            .type_expr
            .clone()
            .unwrap_or_else(|| panic!("param has no type annotation in `{source}`")),
        other => panic!("expected MemberDecl::Param, got {other:?}"),
    }
}

/// Unwrap the first member as a `sub`.
fn first_sub(source: &str) -> SubDecl {
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let structure = only_structure(&decls);
    match &structure.members[0] {
        MemberDecl::Sub(s) => s.clone(),
        other => panic!("expected MemberDecl::Sub, got {other:?}"),
    }
}

/// Unwrap a `TypeExprKind::Named` arm, panicking on mismatch.
fn as_named(te: &TypeExpr) -> (&str, &[TypeExpr]) {
    match &te.kind {
        TypeExprKind::Named { name, type_args } => (name.as_str(), type_args.as_slice()),
        other => panic!("expected TypeExprKind::Named, got {other:?}"),
    }
}

// ── Type position ───────────────────────────────────────────────────────────

/// `param p : pp.Pulley` → `Named { name: "pp.Pulley", type_args: [] }`.
///
/// No new `TypeExprKind` variant: the qualifier rides in the existing `name`
/// slot for ν's `name.contains('.')` fixup.
#[test]
fn qualified_type_lowers_to_dot_joined_named() {
    let te = first_param_type("structure def S { param p : pp.Pulley }");
    let (name, type_args) = as_named(&te);
    assert_eq!(name, "pp.Pulley");
    assert!(
        type_args.is_empty(),
        "a qualified type carries no type args in μ (qualified generics are out of scope); \
         got {type_args:?}"
    );
}

/// `param q : List<pp.Pulley>` → the qualified name nests as a type ARGUMENT,
/// unchanged in form.
#[test]
fn qualified_type_as_type_argument() {
    let te = first_param_type("structure def S { param q : List<pp.Pulley> }");
    let (name, type_args) = as_named(&te);
    assert_eq!(name, "List");
    assert_eq!(type_args.len(), 1, "expected one type arg; got {type_args:?}");
    let (inner, inner_args) = as_named(&type_args[0]);
    assert_eq!(inner, "pp.Pulley");
    assert!(inner_args.is_empty());
}

/// `param r : pp . Pulley` → `"pp.Pulley"`, NOT the raw source text
/// `"pp . Pulley"`.
///
/// This is the assertion that forces an explicit, field-joined lowering branch
/// rather than relying on the incidental `node_text` fallback: ν's
/// `name.contains('.')` discriminator would otherwise have to cope with
/// arbitrary interior whitespace.
#[test]
fn qualified_type_whitespace_is_normalised() {
    let te = first_param_type("structure def S { param r : pp . Pulley }");
    let (name, _) = as_named(&te);
    assert_eq!(
        name, "pp.Pulley",
        "the dotted name must be joined from the `binding`/`name` CST fields, \
         not read as raw source text"
    );
}

/// Negative control: an unqualified type annotation is unchanged.
#[test]
fn bare_type_still_lowers_to_plain_named() {
    let te = first_param_type("structure def S { param s : Steel }");
    let (name, type_args) = as_named(&te);
    assert_eq!(name, "Steel");
    assert!(type_args.is_empty());
    assert!(
        !name.contains('.'),
        "an unqualified name must not acquire a dot — that is ν's discriminator"
    );
}

// ── `sub` structure_name — all three grammar arms ───────────────────────────

/// Instantiation arm: `sub p = pp.Pulley()` → `structure_name == "pp.Pulley"`.
#[test]
fn sub_instantiation_lowers_dot_joined_structure_name() {
    let sub = first_sub("structure def S { sub p = pp.Pulley() }");
    assert_eq!(sub.structure_name, "pp.Pulley");
    assert!(!sub.is_collection);
}

/// Specialization arm: `sub h : pp.Pulley` → `structure_name == "pp.Pulley"`.
#[test]
fn sub_specialization_lowers_dot_joined_structure_name() {
    let sub = first_sub("structure def S { sub h : pp.Pulley }");
    assert_eq!(sub.structure_name, "pp.Pulley");
    assert!(!sub.is_collection);
}

/// Collection arm: `sub i : List<pp.Pulley>` → `structure_name == "pp.Pulley"`
/// with `is_collection == true` (the `List` keyword is still consumed as the
/// collection marker, not as the structure name).
#[test]
fn sub_collection_lowers_dot_joined_structure_name() {
    let sub = first_sub("structure def S { sub i : List<pp.Pulley> }");
    assert_eq!(sub.structure_name, "pp.Pulley");
    assert!(
        sub.is_collection,
        "`List<...>` must still lower to the collection form"
    );
}

/// Whitespace normalisation applies at `sub` position too.
#[test]
fn sub_structure_name_whitespace_is_normalised() {
    let sub = first_sub("structure def S { sub k = pp . Pulley() }");
    assert_eq!(sub.structure_name, "pp.Pulley");
}

/// Specialization arm with a type-argument tail: `sub h : pp.Pulley<T>`.
///
/// THREE-SURFACE PARITY PIN. The specialization arm keeps its own
/// `optional(field('type_args', …))` slot AFTER the widened `structure_name`
/// (grammar.js:895), so widening that slot to `namespaced_name` made the
/// qualified-plus-specialized form parse and lower cleanly — a form neither
/// surface accepted before μ. The tail belongs to the `sub` arm, NOT to
/// `namespaced_name` itself: a qualified generic in TYPE position (`param p :
/// pp.Box<T>`) is still rejected on every surface, and
/// `qualified_type_lowers_to_dot_joined_named` pins that `type_args` stays
/// empty there.
///
/// Pinned here so the compiler's accepted language is recorded; the matching
/// grammar-surface pins are `sub_specialization_arm_admits_a_type_arg_tail`
/// (tree-sitter-reify/tests/qualified_ref_grammar_tests.rs) and the
/// `sub h : pp.Pulley<T>` case in `reifyGrammarQualifiedRef.test.ts`. The GUI
/// editor rejecting what the compiler accepts is the silent degradation those
/// files exist to prevent.
#[test]
fn sub_specialization_with_type_args_lowers_dot_joined_structure_name() {
    let sub = first_sub("structure def S { sub h : pp.Pulley<T> }");
    assert_eq!(sub.structure_name, "pp.Pulley");
    assert!(!sub.is_collection);
    assert_eq!(
        sub.type_args.len(),
        1,
        "the arm's own type_args slot must still bind; got {:?}",
        sub.type_args
    );
    let (arg, arg_args) = as_named(&sub.type_args[0]);
    assert_eq!(arg, "T");
    assert!(arg_args.is_empty());
}

/// Negative control: an unqualified structure name is unchanged.
#[test]
fn bare_sub_structure_name_unchanged() {
    let sub = first_sub("structure def S { sub j = Plain() }");
    assert_eq!(sub.structure_name, "Plain");
    assert!(!sub.structure_name.contains('.'));
}

// ── Expression position — the qualified CALL form ───────────────────────────
//
// Step-7 RED: `namespaced_call` has no `lower_expr` dispatch arm, so
// `pp.Pulley()` lowers to nothing.
// Step-8 GREEN: `lower_namespaced_call` emits
// `ExprKind::FunctionCall { name: "pp.Pulley", .. }` — the same variant the
// unqualified path uses, with the qualifier carried in the dot-joined `name`.

/// Unwrap the first member as a `let`, returning its value expression.
fn first_let_value(source: &str) -> Expr {
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let structure = only_structure(&decls);
    match &structure.members[0] {
        MemberDecl::Let(l) => l.value.clone(),
        other => panic!("expected MemberDecl::Let, got {other:?}"),
    }
}

/// Unwrap an `ExprKind::FunctionCall` arm, panicking on mismatch.
fn as_function_call(expr: &Expr) -> (&str, &[Expr], &[Option<String>]) {
    match &expr.kind {
        ExprKind::FunctionCall {
            name,
            args,
            arg_names,
        } => (name.as_str(), args.as_slice(), arg_names.as_slice()),
        other => panic!("expected ExprKind::FunctionCall, got {other:?}"),
    }
}

/// `let f = pp.Pulley()` → `FunctionCall { name: "pp.Pulley", args: [], .. }`.
///
/// The `import parts as pp` is LOAD-BEARING, not decoration: `first_let_value`
/// asserts `errors.is_empty()`, so without a declared binding this case would
/// pin the undeclared-qualifier hole as intended behaviour (which, until the
/// import-binding gate below, is exactly what it did).
#[test]
fn nullary_qualified_call_lowers_to_dot_joined_function_call() {
    let value = first_let_value("import parts as pp\nstructure def S { let f = pp.Pulley() }");
    let (name, args, arg_names) = as_function_call(&value);
    assert_eq!(name, "pp.Pulley");
    assert!(args.is_empty(), "expected no args; got {args:?}");
    assert!(arg_names.is_empty());
}

/// `let g = pp.compute(1, scale: 2)` → named/positional handling is at exact
/// parity with the bare `function_call` path (both walk the same shared
/// argument-lowering helper, so they cannot drift).
#[test]
fn qualified_call_named_and_positional_args_at_parity() {
    let value =
        first_let_value("import parts as pp\nstructure def S { let g = pp.compute(1, scale: 2) }");
    let (name, args, arg_names) = as_function_call(&value);
    assert_eq!(name, "pp.compute");
    assert_eq!(args.len(), 2, "expected two args; got {args:?}");
    assert_eq!(
        arg_names,
        &[None, Some("scale".to_string())],
        "arg_names must be parallel to args, `None` for positional"
    );
}

/// Whitespace normalisation applies to the call form too.
#[test]
fn qualified_call_whitespace_is_normalised() {
    let value = first_let_value("import parts as pp\nstructure def S { let f = pp . Pulley() }");
    let (name, _, _) = as_function_call(&value);
    assert_eq!(name, "pp.Pulley");
}

/// OUT-OF-SCOPE GUARD (D-7 / PRD §9): bare full-path qualification.
///
/// The grammar accepts a 3+-segment callee because `namespaced_call`'s callee
/// is a full `member_access` (restricting it inline would collide with
/// `member_access` as a reduce-reduce ambiguity). Lowering must therefore
/// reject it with a specific diagnostic — NOT panic, and NOT silently fabricate
/// a 3-segment name.
#[test]
fn three_segment_callee_is_rejected_with_a_diagnostic() {
    let (_decls, errors) = parse_decls("structure def S { let h = a.b.c() }");
    assert!(
        !errors.is_empty(),
        "a 3-segment callee must produce a ParseError, not lower silently"
    );
    let joined = errors
        .iter()
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("binding.Name("),
        "the diagnostic must name the supported form `binding.Name(...)`; got: {joined}"
    );
    assert!(
        joined.contains("full-path qualification"),
        "the diagnostic must say full-path qualification is out of scope; got: {joined}"
    );
}

/// The REAL post-state of a rejected callee: the member is DROPPED, and the
/// diagnostic points at the callee.
///
/// `lower_namespaced_call` returns `None`, `lower_binding_value` propagates it,
/// and `lower_let` drops the whole member — so the structure ends up with no
/// members at all. Asserting that is what catches a future change that starts
/// lowering a fabricated 3-segment name: a `!rendered.contains("a.b.c")` check
/// alone is vacuous here, because an empty member list cannot contain the
/// string no matter what the name-joining code does.
#[test]
fn three_segment_callee_drops_the_member_and_spans_the_callee() {
    let source = "structure def S { let h = a.b.c() }";
    let (decls, errors) = parse_decls(source);

    let structure = only_structure(&decls);
    assert!(
        structure.members.is_empty(),
        "a rejected callee must drop the enclosing member, not half-build it; got {:?}",
        structure.members
    );

    let callee_start = source.find("a.b.c").expect("callee in source") as u32;
    let callee_end = callee_start + "a.b.c".len() as u32;
    assert!(
        errors
            .iter()
            .any(|e| e.span.start == callee_start && e.span.end == callee_end),
        "the diagnostic must span the callee `a.b.c` ({callee_start}..{callee_end}); \
         got {errors:?}"
    );

    // Secondary guard: no fabricated dotted name reaches the AST.
    let rendered = format!("{decls:?}");
    assert!(
        !rendered.contains("a.b.c"),
        "lowering must not fabricate a 3-segment name; got: {rendered}"
    );
}

/// A callee object that is not dotted at all still reaches the same guard,
/// because `member_access.object` is a full `_expression` (grammar.js:1621) —
/// so `arr[0].g()` reduces to `namespaced_call` with an `index_access` object.
///
/// The diagnostic must fit the input: it names the required `binding.Name(...)`
/// shape and the offending object, and must NOT tell a user who wrote
/// `arr[0].g()` that "full-path qualification is out of scope" — advice with no
/// relation to their code. That sentence is reserved for the dotted-path case
/// pinned above.
#[test]
fn non_dotted_callee_object_is_rejected_with_a_fitting_diagnostic() {
    for source in [
        "structure def S { let x = arr[0].g() }",
        "structure def S { let x = f(1).g() }",
    ] {
        let (decls, errors) = parse_decls(source);
        assert!(
            only_structure(&decls).members.is_empty(),
            "a rejected callee must drop the enclosing member in `{source}`"
        );
        let joined = errors
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("binding.Name("),
            "the diagnostic must name the required form `binding.Name(...)`; got: {joined}"
        );
        assert!(
            !joined.contains("full-path qualification"),
            "the full-path sentence must not fire for a non-dotted callee object \
             in `{source}`; got: {joined}"
        );
    }
}

// ── The import-binding gate (expression position only) ──────────────────────
//
// `namespaced_call` is `prec(12, seq(field('callee', $.member_access),
// callTail($)))`, so it captures EVERY two-segment `ident.ident(args)` — not
// only the import-qualified ones. Measured on this branch before the gate:
// `obj.width()` and `self.w()` went from `Parse error … exit 1` (pre-μ) to a
// bare "cannot infer return type" warning plus `All constraints satisfied.`
// exit 0, and `totally.undefined_thing(1, 2)` to NO diagnostic at all — because
// the compiler has no unknown-function error behind `ExprKind::FunctionCall`.
// μ must not turn a hard parse error into silence, so lowering — the first
// layer that knows the import set — rejects a qualifier that is not a declared
// import binding.
//
// EXPRESSION POSITION ONLY. The other two positions μ widened are already loud
// and need no second diagnostic for the same mistake: `param p : obj.width`
// answers `error: unresolved type: obj.width` (exit 1) and `sub s =
// obj.width()` answers `error: sub-component "s" references unknown structure
// "obj.width"` (exit 1).

/// The kind-INDEPENDENT half of a qualifier rejection, shared by every
/// rejection case: the enclosing member is DROPPED, one diagnostic spans
/// exactly the callee, the message names the offending qualifier, refers to
/// this file's imports, and says Reify has no method-call syntax (the
/// overwhelmingly likely intent behind `obj.width()`).
///
/// Returns the joined diagnostic text so each caller can add the expectation
/// that fits ITS `ImportKind`. The REMEDY is not kind-independent: "no import
/// binds this name at all" is fixed by declaring one, while "an import binds
/// it, but as an entity name" is not — so folding both into a single helper
/// would either weaken the remedy assertion for the cases it still covers or
/// assert a remedy that is actively wrong for the others.
///
/// The qualifier is matched BACKTICK-QUOTED so the assertion is not satisfied
/// incidentally by the echoed callee text: `` `obj` `` does not appear inside
/// `` `obj.width(...)` ``.
fn assert_qualifier_rejection_core(source: &str, callee: &str, qualifier: &str) -> String {
    let (decls, errors) = parse_decls(source);
    assert!(
        only_structure(&decls).members.is_empty(),
        "a rejected qualifier must drop the enclosing member, not half-build it, \
         in `{source}`; got {:?}",
        only_structure(&decls).members
    );

    let joined = errors
        .iter()
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains(&format!("`{qualifier}`")),
        "the diagnostic must name the offending qualifier `{qualifier}` in `{source}`; \
         got: {joined}"
    );
    assert!(
        joined.contains("import"),
        "every rejection must refer to this file's imports — that is where a qualifier \
         comes from; got: {joined}"
    );
    assert!(
        joined.contains("method-call"),
        "the diagnostic must say Reify has no method-call syntax — that is the likely \
         intent behind `{callee}(...)`; got: {joined}"
    );
    assert!(
        !joined.contains("full-path qualification"),
        "the full-path sentence belongs to the callee-SHAPE guard and must not fire for \
         a two-segment callee in `{source}`; got: {joined}"
    );

    let start = source
        .find(callee)
        .unwrap_or_else(|| panic!("callee `{callee}` not found in `{source}`"))
        as u32;
    let end = start + callee.len() as u32;
    assert!(
        errors
            .iter()
            .any(|e| e.span.start == start && e.span.end == end),
        "the diagnostic must span the callee `{callee}` ({start}..{end}); got {errors:?}"
    );

    joined
}

/// A qualifier NO import binds. The remedy is to declare one, so the message
/// must carry today's `import <path> as <q>` form unchanged.
fn assert_qualifier_rejected(source: &str, callee: &str, qualifier: &str) {
    let joined = assert_qualifier_rejection_core(source, callee, qualifier);
    assert!(
        joined.contains(&format!("import <path> as {qualifier}")),
        "an UNBOUND qualifier's remedy IS to declare an import, so the message must \
         still offer `import <path> as {qualifier}` in `{source}`; got: {joined}"
    );
}

/// A qualifier an import DOES bind — but as an ENTITY name
/// (`ImportKind::Entity`, `EntityAliased`, `Destructured`) rather than as a
/// module namespace.
///
/// The unbound remedy is WRONG here, and demonstrably so: `import a.b.Widget` +
/// `Widget.mk()` would be told to "declare one as `import <path>.Widget`" — the
/// line the user already wrote. So the message must instead say the import
/// binds an entity rather than a module namespace, and must NOT echo either
/// import form back as advice.
fn assert_entity_qualifier_rejected(source: &str, callee: &str, qualifier: &str) {
    let joined = assert_qualifier_rejection_core(source, callee, qualifier);
    assert!(
        joined.contains("entity"),
        "the diagnostic must say the import binds an ENTITY name in `{source}`; \
         got: {joined}"
    );
    assert!(
        joined.contains("module namespace"),
        "the diagnostic must contrast that entity binding with the module namespace a \
         qualifier needs in `{source}`; got: {joined}"
    );
    for already_written in [
        format!("import <path> as {qualifier}"),
        format!("import <path>.{qualifier}"),
    ] {
        assert!(
            !joined.contains(&already_written),
            "the diagnostic must not suggest `{already_written}` — for an entity-bound \
             qualifier that is the import the user already wrote in `{source}`; \
             got: {joined}"
        );
    }
}

/// An accepted qualified call lowers to the dot-joined `FunctionCall` with no
/// diagnostic at all.
fn assert_qualified_call_accepted(source: &str, expected_name: &str) {
    let value = first_let_value(source);
    let (name, _, _) = as_function_call(&value);
    assert_eq!(name, expected_name, "in `{source}`");
}

/// The three forms measured as silently accepted before the gate. Each is a
/// two-segment call whose qualifier is not a declared import binding.
#[test]
fn undeclared_qualifier_is_rejected_with_a_diagnostic_naming_it() {
    assert_qualifier_rejected(
        "structure def S { let x = obj.width() }",
        "obj.width",
        "obj",
    );
    assert_qualifier_rejected("structure def S { let g = self.w() }", "self.w", "self");
    assert_qualifier_rejected(
        "structure def S { let g = totally.undefined_thing(1, 2) }",
        "totally.undefined_thing",
        "totally",
    );
}

/// A declared binding does NOT rescue an unrelated qualifier — the gate checks
/// the specific name, not merely that some import exists.
#[test]
fn an_unrelated_import_does_not_bind_another_qualifier() {
    assert_qualifier_rejected(
        "import parts as pp\nstructure def S { let x = obj.width() }",
        "obj.width",
        "obj",
    );
}

// ── Which import kinds bind a namespace (D-7) ───────────────────────────────

/// `import a.b as pp` → `ImportKind::Aliased`, binding the ALIAS.
#[test]
fn aliased_import_binds_the_alias() {
    assert_qualified_call_accepted(
        "import a.b as pp\nstructure def S { let f = pp.Thing() }",
        "pp.Thing",
    );
}

/// `import a.b` → `ImportKind::Module`, binding the FINAL PATH SEGMENT.
#[test]
fn module_import_binds_the_final_path_segment() {
    assert_qualified_call_accepted(
        "import a.b\nstructure def S { let f = b.Thing() }",
        "b.Thing",
    );
}

/// A single-segment module import binds that segment.
#[test]
fn single_segment_module_import_binds_that_segment() {
    assert_qualified_call_accepted(
        "import parts\nstructure def S { let f = parts.Thing() }",
        "parts.Thing",
    );
}

/// ORDER INDEPENDENCE: an import written AFTER the structure that uses it still
/// binds, because the collector runs in `lower_source_file`'s first pass — the
/// same pass that already seeds `known_enums` for exactly this guarantee.
#[test]
fn import_after_the_structure_still_binds() {
    assert_qualified_call_accepted(
        "structure def S { let f = pp.Pulley() }\nimport parts as pp",
        "pp.Pulley",
    );
}

/// `import a.b.Widget` → `ImportKind::Entity`, which binds an ENTITY name, not
/// a module namespace. `Widget.mk()` is a method call on an entity — syntax
/// Reify does not have, and a parse error before μ — so accepting it would
/// reopen a narrower version of the same hole.
///
/// Rejected via `assert_entity_qualifier_rejected`: the name IS bound, so the
/// "declare an import" remedy would hand the user back `import <path>.Widget`,
/// which is character-for-character the line already at the top of the file.
#[test]
fn entity_import_does_not_bind_a_namespace() {
    assert_entity_qualifier_rejected(
        "import a.b.Widget\nstructure def S { let f = Widget.mk() }",
        "Widget.mk",
        "Widget",
    );
}

/// `import a.b.Widget as W` → `ImportKind::EntityAliased`, which binds the
/// ALIAS as an entity name.
///
/// This is the fifth and last `ImportKind` arm, and until now the only one with
/// no test: `collect_import_bindings` folds it into the same `None` arm as
/// `Entity`, so a future split that flipped it to `Some(alias)` would reopen the
/// gate hole for `import a.b.Widget as W` + `W.mk()` with every other test in
/// this file still green.
#[test]
fn entity_aliased_import_does_not_bind_a_namespace() {
    assert_entity_qualifier_rejected(
        "import a.b.Widget as W\nstructure def S { let f = W.mk() }",
        "W.mk",
        "W",
    );
}

/// `import a.b.{C, D}` → `ImportKind::Destructured`, likewise entity names.
#[test]
fn destructured_import_does_not_bind_a_namespace() {
    assert_entity_qualifier_rejected(
        "import a.b.{C, D}\nstructure def S { let f = C.mk() }",
        "C.mk",
        "C",
    );
}

/// RESIDUAL HOLE, pinned rather than assumed closed (ν / task 5505).
///
/// With a DECLARED binding, an unknown member still lowers to a dot-joined
/// `FunctionCall` with no parse diagnostic: whether `parts` actually exports
/// `thing` is resolution work, not parsing. It is not silent end-to-end when
/// the module is absent — `import parts as pp` + `pp.thing()` answers `error:
/// module 'parts' not found` (exit 1), measured — so the one genuinely silent
/// case is: declared binding, module resolves, member does not.
#[test]
fn declared_binding_with_unknown_member_is_left_to_resolution() {
    let source = "import parts as pp\nstructure def S { let f = pp.thing() }";
    let (decls, errors) = parse_decls(source);
    assert!(
        errors.is_empty(),
        "an unknown MEMBER is ν's to resolve, not a parse error; got {errors:?}"
    );
    let structure = only_structure(&decls);
    let value = match &structure.members[0] {
        MemberDecl::Let(l) => l.value.clone(),
        other => panic!("expected MemberDecl::Let, got {other:?}"),
    };
    let (name, _, _) = as_function_call(&value);
    assert_eq!(name, "pp.thing");
}

// ── Expression-position negative controls ───────────────────────────────────

/// `let r = pp.FitClass.Clearance` still lowers to nested `MemberAccess` — the
/// enum-access half of NS-Q2 stays on ν's D-9 resolution-fixup path.
#[test]
fn call_less_dotted_chain_stays_nested_member_access() {
    let value = first_let_value("structure def S { let r = pp.FitClass.Clearance }");
    match &value.kind {
        ExprKind::MemberAccess { object, member } => {
            assert_eq!(member, "Clearance");
            match &object.kind {
                ExprKind::MemberAccess { object, member } => {
                    assert_eq!(member, "FitClass");
                    assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "pp"));
                }
                other => panic!("expected a nested MemberAccess, got {other:?}"),
            }
        }
        other => panic!("expected ExprKind::MemberAccess, got {other:?}"),
    }
}

/// `let s = obj.width` still lowers to `MemberAccess`.
#[test]
fn plain_member_access_lowering_unchanged() {
    let value = first_let_value("structure def S { let s = obj.width }");
    match &value.kind {
        ExprKind::MemberAccess { object, member } => {
            assert_eq!(member, "width");
            assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "obj"));
        }
        other => panic!("expected ExprKind::MemberAccess, got {other:?}"),
    }
}

/// `let t = plain(1)` still lowers to an unqualified `FunctionCall`.
#[test]
fn unqualified_call_lowering_unchanged() {
    let value = first_let_value("structure def S { let t = plain(1) }");
    let (name, args, _) = as_function_call(&value);
    assert_eq!(name, "plain");
    assert_eq!(args.len(), 1);
    assert!(
        !name.contains('.'),
        "an unqualified callee must not acquire a dot — that is ν's discriminator"
    );
}

/// An in-file enum still lowers `Direction.In` to `EnumAccess` — the
/// `known_enums` path in `lower_member_access` is untouched by μ.
#[test]
fn enum_access_lowering_unchanged() {
    let source = "enum Direction { In, Out }\nstructure def S { let d = Direction.In }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let structure = match decls.iter().find(|d| matches!(d, Declaration::Structure(_))) {
        Some(Declaration::Structure(s)) => s,
        _ => panic!("expected a structure declaration in {decls:?}"),
    };
    let value = match &structure.members[0] {
        MemberDecl::Let(l) => &l.value,
        other => panic!("expected MemberDecl::Let, got {other:?}"),
    };
    match &value.kind {
        ExprKind::EnumAccess { type_name, variant } => {
            assert_eq!(type_name, "Direction");
            assert_eq!(variant, "In");
        }
        other => panic!("expected ExprKind::EnumAccess, got {other:?}"),
    }
}
