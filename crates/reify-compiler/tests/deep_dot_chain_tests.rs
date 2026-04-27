//! Integration tests for the deep-dot-chain lint (spec §5.7).
//!
//! The lint walks left-to-right `MemberAccess` chains in the parsed AST and
//! emits a Warning diagnostic with [`DiagnosticCode::DeepDotChain`] when the
//! chain length exceeds [`crate::compile_builder::dot_chain_lint::DEEP_DOT_CHAIN_THRESHOLD`]
//! (= 4). `a.b.c.d` (length 4) is at-threshold and does not warn;
//! `a.b.c.d.e` (length 5) warns.
//!
//! These integration tests use the public `compile_source` / `warnings_only`
//! helpers from `reify-test-support`, mirroring the style of
//! `import_warning_tests.rs` and `diagnostic_coverage_checkpoint.rs`.

use reify_test_support::{compile_source, warnings_only};
use reify_types::DiagnosticCode;

/// A chain at exactly the threshold (length 4 — `a.b.c.d`) must not warn.
///
/// This is a regression lock: the gate is `> THRESHOLD`, not `>= THRESHOLD`,
/// so at-threshold chains are explicitly OK per spec §5.7's example.
///
/// The test also includes a deeper chain `a.b.c.d.e.f` (length 6) in the
/// same module as a positive control. If the lint pass were silently
/// disabled (e.g. early-exit added before our phase, or our phase removed
/// from the orchestrator), the at-threshold negative would pass for the
/// wrong reason. The positive control fails loudly in that scenario, so
/// the threshold-boundary assertion only passes when the pass is actually
/// running.
#[test]
fn chain_at_threshold_does_not_warn() {
    // Source contains TWO chains:
    //   * `a.b.c.d` (length 4) — at threshold, MUST NOT warn.
    //   * `a.b.c.d.e.f` (length 6) — above threshold, MUST warn (positive control).
    // The structure body just establishes a scope for the chains to live in;
    // the lint only cares about AST shape, not name resolution.
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d
    let y = a.b.c.d.e.f
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    // Exactly one warning total — proves both that the threshold-boundary
    // chain does NOT trip and that the lint pass IS running (positive
    // control). A `0` count would mean the pass is disabled; a `2` count
    // would mean the boundary is `>= THRESHOLD` instead of `> THRESHOLD`.
    assert_eq!(
        deep_dot_chain_warnings.len(),
        1,
        "expected exactly 1 DeepDotChain warning (only `a.b.c.d.e.f`, NOT \
         `a.b.c.d`), got {} warnings: {:?}",
        deep_dot_chain_warnings.len(),
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // The single warning must be the deeper chain, not the at-threshold one.
    let warning = deep_dot_chain_warnings[0];
    assert!(
        warning.message.contains("a.b.c.d.e.f"),
        "expected the single DeepDotChain warning to be for `a.b.c.d.e.f` \
         (the only above-threshold chain), got message: {:?}",
        warning.message
    );
    assert!(
        !warning.message.contains("a.b.c.d ") && !warning.message.ends_with("a.b.c.d"),
        "the at-threshold chain `a.b.c.d` must NOT have produced a warning, \
         got message: {:?}",
        warning.message
    );
}

/// A chain just above threshold (length 5 — `a.b.c.d.e`) emits exactly one
/// Warning whose `code == Some(DiagnosticCode::DeepDotChain)`.
#[test]
fn chain_above_threshold_emits_one_warning_with_deep_dot_chain_code() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    assert_eq!(
        deep_dot_chain_warnings.len(),
        1,
        "expected exactly 1 DeepDotChain warning for above-threshold chain `a.b.c.d.e`, \
         got: {:?}",
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// The DeepDotChain warning's message contains the rendered chain text
/// (`a.b.c.d.e`) so that humans reading the diagnostic see the offending
/// chain inline without needing the source span.
#[test]
fn chain_warning_message_contains_full_chain_text() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warning = warnings
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .expect("expected one DeepDotChain warning");

    assert!(
        deep_dot_chain_warning.message.contains("a.b.c.d.e"),
        "expected DeepDotChain warning message to contain `a.b.c.d.e`, got: {:?}",
        deep_dot_chain_warning.message
    );
}

/// The DeepDotChain warning has at least one DiagnosticLabel whose span
/// equals the outermost MemberAccess.span — i.e. starts at byte offset of
/// root identifier `a` and ends after final member `e`.
#[test]
fn chain_warning_has_label_covering_full_chain_span() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b.c.d.e
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warning = warnings
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .expect("expected one DeepDotChain warning");

    assert!(
        !deep_dot_chain_warning.labels.is_empty(),
        "expected at least one label on the DeepDotChain warning, got: {:?}",
        deep_dot_chain_warning
    );

    // Compute the expected chain span by locating "a.b.c.d.e" in the source.
    let needle = "a.b.c.d.e";
    let start = source
        .find(needle)
        .expect("test source must contain `a.b.c.d.e` literally") as u32;
    let end = start + needle.len() as u32;

    let has_full_span_label = deep_dot_chain_warning
        .labels
        .iter()
        .any(|l| l.span.start == start && l.span.end == end);

    assert!(
        has_full_span_label,
        "expected a label whose span covers the full chain (bytes {start}..{end}), \
         got labels: {:?}",
        deep_dot_chain_warning.labels
    );
}

/// `a.b[0].c.d.e.f` parses as the outer chain `<IndexAccess>.c.d.e.f`
/// (root = `IndexAccess` of `a.b` indexed at `[0]`, then 4 hops). Chain length
/// is 5, so EXACTLY ONE DeepDotChain warning fires. The inner `a.b` chain has
/// length 2 (under threshold), so it does not warn. The warning's message must
/// NOT contain `a.b.c.d.e.f` because the IndexAccess naturally breaks the
/// chain.
#[test]
fn index_access_resets_chain_root_emits_one_warning_post_index() {
    let source = r#"
structure S {
    param a: Real = 0
    let x = a.b[0].c.d.e.f
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    assert_eq!(
        deep_dot_chain_warnings.len(),
        1,
        "expected exactly 1 DeepDotChain warning for `a.b[0].c.d.e.f`, got: {:?}",
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let warning = deep_dot_chain_warnings[0];
    assert!(
        !warning.message.contains("a.b.c.d.e.f"),
        "DeepDotChain warning must not flatten across the IndexAccess — \
         expected message to NOT contain `a.b.c.d.e.f`, got: {:?}",
        warning.message
    );

    // Positive control on the rendered chain text. The IndexAccess root is
    // not a bare Ident or EnumAccess, so render_chain_text intentionally
    // substitutes the literal `<expr>` placeholder for the root segment in
    // v0.1 (see the doc on `render_chain_text`). The diagnostic span still
    // anchors the squiggle correctly in editor output, but bare CLI
    // renderings will see the placeholder. This assertion pins that
    // contract so future authors who change the placeholder know to update
    // the test (or to extend the root-rendering arm with prettier output).
    assert!(
        warning.message.contains("<expr>.c.d.e.f"),
        "DeepDotChain warning for an IndexAccess-rooted chain must render \
         the chain text with `<expr>` standing in for the IndexAccess root \
         (v0.1 contract — see `render_chain_text` doc), got: {:?}",
        warning.message
    );
}

/// `Direction.In.a.b.c` parses as `MA(MA(MA(EnumAccess(Direction, In), "a"),
/// "b"), "c")`. Chain length is 4 (root = EnumAccess single segment + 3
/// hops `.a.b.c`) = threshold → NO warn. The lint counts `.field` hops only;
/// enum-variant access is structurally distinct in the AST.
#[test]
fn enum_access_root_within_threshold_does_not_warn() {
    let source = r#"
enum Direction { In, Out }

structure S {
    let x = Direction.In.a.b.c
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();

    assert!(
        deep_dot_chain_warnings.is_empty(),
        "expected no DeepDotChain warnings for `Direction.In.a.b.c` (length 4 = threshold), \
         got: {:?}",
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// --- Walker-coverage cluster -----------------------------------------------
//
// The walker must visit every expression-bearing position in the AST. The
// previous omnibus test bundled all positions into one fixture and asserted
// a single count — when it failed, the maintainer had no way to bisect which
// position regressed. The cluster below splits per-position, one focused test
// per `Declaration` / `MemberDecl` slot, so a regression in (say) `Port.frame_expr`
// fails localised to `walker_visits_port_frame_expr` and tells the maintainer
// exactly where to look.
//
// Each test compiles a minimal source that puts an intentionally-deep chain
// (`a.b.c.d.e`, length 5) in exactly one position and asserts that the
// expected number of DeepDotChain warnings fired. The shared helper
// `assert_deep_chain_warning_count` keeps the boilerplate down.

#[track_caller]
fn assert_deep_chain_warning_count(source: &str, expected: usize, position_label: &str) {
    let module = compile_source(source);
    let warnings = warnings_only(&module);
    let deep_dot_chain_warnings: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeepDotChain))
        .collect();
    assert_eq!(
        deep_dot_chain_warnings.len(),
        expected,
        "[{position_label}] expected {expected} DeepDotChain warning(s), got {}: {:#?}",
        deep_dot_chain_warnings.len(),
        deep_dot_chain_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Position 1: `MemberDecl::Param.default`.
#[test]
fn walker_visits_param_default() {
    let source = r#"
structure S {
    param p: Real = a.b.c.d.e
}
"#;
    assert_deep_chain_warning_count(source, 1, "Param.default");
}

/// Position 2: `MemberDecl::Let.value`.
#[test]
fn walker_visits_let_value() {
    let source = r#"
structure S {
    let v = a.b.c.d.e
}
"#;
    assert_deep_chain_warning_count(source, 1, "Let.value");
}

/// Position 3: `MemberDecl::Constraint.expr` (anonymous bare-expression form).
#[test]
fn walker_visits_constraint_expr() {
    let source = r#"
structure S {
    constraint a.b.c.d.e > 0
}
"#;
    assert_deep_chain_warning_count(source, 1, "Constraint.expr");
}

/// Position 4: `MemberDecl::Sub.args`.
#[test]
fn walker_visits_sub_args() {
    let source = r#"
structure def Connector { param p: Real = 0 }
structure S {
    sub s = Connector(p: a.b.c.d.e)
}
"#;
    assert_deep_chain_warning_count(source, 1, "Sub.args");
}

/// Position 5: `MemberDecl::Minimize.expr`.
#[test]
fn walker_visits_minimize_expr() {
    let source = r#"
structure S {
    minimize a.b.c.d.e
}
"#;
    assert_deep_chain_warning_count(source, 1, "Minimize.expr");
}

/// Position 6: `MemberDecl::Maximize.expr`.
#[test]
fn walker_visits_maximize_expr() {
    let source = r#"
structure S {
    maximize a.b.c.d.e
}
"#;
    assert_deep_chain_warning_count(source, 1, "Maximize.expr");
}

/// Position 7a: `MemberDecl::GuardedGroup.condition` (the `where ... { }` head).
#[test]
fn walker_visits_guarded_group_condition() {
    let source = r#"
structure S {
    where a.b.c.d.e > 0 {
        let g = 0
    }
}
"#;
    assert_deep_chain_warning_count(source, 1, "GuardedGroup.condition");
}

/// Position 7b: `MemberDecl::GuardedGroup.members` — nested member chain.
///
/// The walker MUST recurse into the `where { ... }` body, otherwise nested
/// chains escape the lint. `let g = a.b.c.d.e` inside the guarded group
/// fires exactly one warning.
#[test]
fn walker_recurses_into_guarded_group_members() {
    let source = r#"
structure S {
    where 1 > 0 {
        let g = a.b.c.d.e
    }
}
"#;
    assert_deep_chain_warning_count(source, 1, "GuardedGroup.members (nested Let.value)");
}

/// Position 8: `MemberDecl::Port.frame_expr`.
#[test]
fn walker_visits_port_frame_expr() {
    let source = r#"
trait MyPort { param diameter: Real }
structure S {
    port p : out MyPort {
        param diameter: Real = 1
        frame = a.b.c.d.e
    }
}
"#;
    assert_deep_chain_warning_count(source, 1, "Port.frame_expr");
}

/// Position 9: `MemberDecl::Connect.params`.
#[test]
fn walker_visits_connect_params() {
    let source = r#"
trait MyPort { param diameter: Real }
structure def Connector { param p: Real = 0 }
structure def SubTarget {
    port out_port : out MyPort { param diameter: Real = 1 }
    port in_port  : in  MyPort { param diameter: Real = 1 }
}
structure S {
    sub st1 = SubTarget()
    sub st2 = SubTarget()
    connect st1.out_port -> st2.in_port : Connector { p = a.b.c.d.e }
}
"#;
    assert_deep_chain_warning_count(source, 1, "Connect.params");
}

/// Position 10: `MemberDecl::Chain.elements`.
///
/// The chain statement's second element `st2.in_port.x.y.z.w.v` is a
/// 7-segment chain (length 7 > 4) → one warning. The first element
/// `st1.out_port` has length 2 and does not trip.
#[test]
fn walker_visits_chain_elements() {
    let source = r#"
trait MyPort { param diameter: Real }
structure def SubTarget {
    port out_port : out MyPort { param diameter: Real = 1 }
    port in_port  : in  MyPort { param diameter: Real = 1 }
}
structure S {
    sub st1 = SubTarget()
    sub st2 = SubTarget()
    chain st1.out_port -> st2.in_port.x.y.z.w.v
}
"#;
    assert_deep_chain_warning_count(source, 1, "Chain.elements");
}

/// Position 11: `Declaration::Field` → `FieldSource::Analytical.expr`
/// (the lambda body inside an `analytical { |p| ... }` block).
#[test]
fn walker_visits_field_analytical_source() {
    let source = r#"
field def my_field : Real -> Real { source = analytical { |p| a.b.c.d.e } }
"#;
    assert_deep_chain_warning_count(source, 1, "FieldSource::Analytical.expr");
}

/// Position 12: `Declaration::Constraint` (named definition) → `predicates`.
#[test]
fn walker_visits_named_constraint_def_predicates() {
    let source = r#"
constraint def MinX {
    param x: Real
    a.b.c.d.e > 0
}
"#;
    assert_deep_chain_warning_count(source, 1, "ConstraintDef.predicates");
}

/// Position 13: `Declaration::Unit.conversion`.
#[test]
fn walker_visits_unit_conversion() {
    let source = r#"
unit my_unit : Length = a.b.c.d.e
"#;
    assert_deep_chain_warning_count(source, 1, "UnitDecl.conversion");
}

/// Position 14a: `Declaration::Function` body's `let_bindings[*].value`.
#[test]
fn walker_visits_function_body_let_value() {
    let source = r#"
fn my_fn(w: Real) -> Real {
    let z = a.b.c.d.e;
    z
}
"#;
    assert_deep_chain_warning_count(source, 1, "FnBody.let_bindings.value");
}

/// Position 14b: `Declaration::Function` body's `result_expr`.
#[test]
fn walker_visits_function_body_result_expr() {
    let source = r#"
fn my_fn(w: Real) -> Real {
    a.b.c.d.e
}
"#;
    assert_deep_chain_warning_count(source, 1, "FnBody.result_expr");
}

/// Position 15: `Declaration::*.annotations[*].args` — deep chain inside a
/// top-level annotation arg (e.g. `@deprecated(a.b.c.d.e)`).
///
/// The structure body deliberately contains NO deep chain so the asserted
/// count of 1 isolates the warning to the annotation-arg position. If
/// `walk_declaration` silently skips annotations (the coverage hole), the
/// count is 0 and this test FAILS — which is what we want pre-impl.
#[test]
fn walker_visits_top_level_declaration_annotation_arg() {
    let source = r#"
@deprecated(a.b.c.d.e)
structure S {
    param x: Real = 0
}
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::Structure.annotations[*].args",
    );
}

/// Position 16: `MemberDecl::Param.annotations[*].args` — deep chain inside a
/// member-level annotation arg on a `param`.
///
/// The param body (default = 0) deliberately contains NO deep chain so the
/// asserted count of 1 isolates the warning to the annotation-arg position.
/// If `walk_members` silently skips Param annotations (the coverage hole
/// for this arm), the count is 0 and this test FAILS — expected pre-impl.
#[test]
fn walker_visits_param_member_annotation_arg() {
    let source = r#"
structure S {
    @solver_hint(a.b.c.d.e)
    param x: Real = 0
}
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "MemberDecl::Param.annotations[*].args",
    );
}

/// Position 17: `MemberDecl::Let.annotations[*].args` — deep chain inside a
/// member-level annotation arg on a `let`.
///
/// Tested separately from Param because the two are independent `match` arms
/// in `walk_members`; a regression that wires `walk_annotations` into only
/// one of them would silently lose coverage on the other.
///
/// The let value (= 0) deliberately contains NO deep chain so the asserted
/// count of 1 isolates the warning to the annotation-arg position.
#[test]
fn walker_visits_let_member_annotation_arg() {
    let source = r#"
structure S {
    @solver_hint(a.b.c.d.e)
    let v = 0
}
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "MemberDecl::Let.annotations[*].args",
    );
}

/// Position 18: `Declaration::Function.annotations[*].args` — deep chain
/// inside an annotation arg on a function declaration.
///
/// The function body deliberately contains NO deep chain (result_expr = `w`,
/// a single ident) so the asserted count of 1 isolates the warning to the
/// annotation-arg position. Covers the `Function` arm in `walk_declaration`.
#[test]
fn walker_visits_function_decl_annotation_arg() {
    let source = r#"
@deprecated(a.b.c.d.e)
fn my_fn(w: Real) -> Real { w }
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::Function.annotations[*].args",
    );
}

/// Position 19: `Declaration::Field.annotations[*].args` — deep chain inside
/// an annotation arg on a field definition.
///
/// The field source (`analytical { |p| p }`) deliberately contains NO deep
/// chain (lambda body = single ident `p`) so the asserted count of 1 isolates
/// the warning to the annotation-arg position. Covers the `Field` arm in
/// `walk_declaration`.
#[test]
fn walker_visits_field_decl_annotation_arg() {
    let source = r#"
@deprecated(a.b.c.d.e)
field def my_field : Real -> Real { source = analytical { |p| p } }
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::Field.annotations[*].args",
    );
}

/// Position 20: `Declaration::Unit.annotations[*].args` — deep chain inside
/// an annotation arg on a unit declaration.
///
/// The unit has no conversion expression so the asserted count of 1 isolates
/// the warning to the annotation-arg position. Covers the `Unit` arm in
/// `walk_declaration`.
#[test]
fn walker_visits_unit_decl_annotation_arg() {
    let source = r#"
@deprecated(a.b.c.d.e)
unit meter : Length
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::Unit.annotations[*].args",
    );
}

/// Position 21: `Declaration::Enum.annotations[*].args` — deep chain inside
/// an annotation arg on an enum declaration.
///
/// Enum bodies carry no embedded expressions; the asserted count of 1
/// isolates the warning to the annotation-arg position. This was a
/// previously no-op arm in `walk_declaration` (no body expressions) that now
/// calls `walk_annotations`. Covers the `Enum` arm in `walk_declaration`.
#[test]
fn walker_visits_enum_decl_annotation_arg() {
    let source = r#"
@deprecated(a.b.c.d.e)
enum Dir { In, Out }
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::Enum.annotations[*].args",
    );
}

/// Position 22: `Declaration::Import.annotations[*].args` — deep chain
/// inside an annotation arg on an import declaration.
///
/// Import declarations carry no embedded expressions; the asserted count of 1
/// isolates the warning to the annotation-arg position. This was a previously
/// no-op arm in `walk_declaration` that now calls `walk_annotations`. Covers
/// the `Import` arm in `walk_declaration`.
#[test]
fn walker_visits_import_decl_annotation_arg() {
    let source = r#"@deprecated(a.b.c.d.e) import std.math"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::Import.annotations[*].args",
    );
}

/// Position 23: `Declaration::TypeAlias.annotations[*].args` — deep chain
/// inside an annotation arg on a type alias declaration.
///
/// The alias body (`Force / Area`) is a BinOp over two idents with no deep
/// chain, so the asserted count of 1 isolates the warning to the annotation-
/// arg position. This was a previously no-op arm in `walk_declaration` that
/// now calls `walk_annotations`. Covers the `TypeAlias` arm in
/// `walk_declaration`.
#[test]
fn walker_visits_type_alias_decl_annotation_arg() {
    let source = r#"@deprecated(a.b.c.d.e) type Pressure = Force / Area"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::TypeAlias.annotations[*].args",
    );
}

/// Position 24: `Declaration::Occurrence.annotations[*].args` — deep chain
/// inside an annotation arg on an occurrence declaration.
///
/// The occurrence body (`param p: Real = 0`) contains only a literal default
/// with no deep chain, so the asserted count of 1 isolates the warning to the
/// annotation-arg position. If `walk_declaration`'s Occurrence arm dropped its
/// `walk_annotations` call (dot_chain_lint.rs:93), the count would be 0 and
/// this test would fail — which is the regression this test guards against.
#[test]
fn walker_visits_occurrence_decl_annotation_arg() {
    let source = r#"
@deprecated(a.b.c.d.e)
occurrence def Op {
    param p: Real = 0
}
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::Occurrence.annotations[*].args",
    );
}

/// Position 25: `Declaration::Trait.annotations[*].args` — deep chain inside
/// an annotation arg on a trait declaration.
///
/// The trait body (`param p: Real`) has no default expression and therefore no
/// embedded chain, so the asserted count of 1 isolates the warning to the
/// annotation-arg position. If `walk_declaration`'s Trait arm dropped its
/// `walk_annotations` call (dot_chain_lint.rs:97), the count would be 0 and
/// this test would fail — which is the regression this test guards against.
#[test]
fn walker_visits_trait_decl_annotation_arg() {
    let source = r#"
@deprecated(a.b.c.d.e)
trait MyTrait {
    param p: Real
}
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::Trait.annotations[*].args",
    );
}

/// Position 26: `Declaration::Purpose.annotations[*].args` — deep chain inside
/// an annotation arg on a purpose declaration.
///
/// The purpose body uses `let v = 0` rather than `param` because the grammar's
/// `purpose_member` rule does not include `param_declaration` (only
/// `constraint_declaration`, `let_declaration`, etc.). The let-value `0` is a
/// NumberLiteral with no MemberAccess chain, so the asserted count of 1
/// isolates the warning to the annotation-arg position. If `walk_declaration`'s
/// Purpose arm dropped its `walk_annotations` call (dot_chain_lint.rs:101),
/// the count would be 0 and this test would fail.
#[test]
fn walker_visits_purpose_decl_annotation_arg() {
    let source = r#"
@deprecated(a.b.c.d.e)
purpose my_purpose(subject: Structure) {
    let v = 0
}
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::Purpose.annotations[*].args",
    );
}

/// Position 27: `Declaration::Constraint.annotations[*].args` — deep chain
/// inside an annotation arg on a constraint definition.
///
/// The constraint-def body (`param x: Real`) has no default expression, no
/// predicates, and therefore no embedded chain. The asserted count of 1
/// isolates the warning to the annotation-arg position. If `walk_declaration`'s
/// Constraint arm dropped its `walk_annotations` call (dot_chain_lint.rs:129),
/// the count would be 0 and this test would fail — which is the regression
/// this test guards against.
#[test]
fn walker_visits_constraint_decl_annotation_arg() {
    let source = r#"
@deprecated(a.b.c.d.e)
constraint def MyConstraint {
    param x: Real
}
"#;
    assert_deep_chain_warning_count(
        source,
        1,
        "Declaration::Constraint.annotations[*].args",
    );
}
