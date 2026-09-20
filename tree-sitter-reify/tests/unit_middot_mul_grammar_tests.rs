//! Grammar integration tests for U+00B7 MIDDLE DOT as a unit-multiply operator.
//!
//! Task #5784 (angle-units leaf κ; `docs/prds/v0_6/angle-units-surface-convergence.md`
//! cluster C, ratified decision 7a).  `Display for DimensionVector` joins base-unit
//! parts with `·`, so `reify eval` prints strings such as `7850 kg·m^-3` that Reify
//! could not read back.  The external scanner's `UNIT_MUL_OP` now fires on ASCII `*`
//! OR U+00B7, both gated identically by `is_unit_start`.
//!
//! THIS FILE IS THE LIVE REGRESSION SIGNAL for that acceptance.  No gate anywhere
//! executes the tree-sitter corpus (`scripts/verify.sh` never invokes `tree-sitter
//! test`, and `tree-sitter-reify/package.json` has no scripts block), so the INV-SF-7
//! ambiguity obligation is discharged here, under `cargo nextest`.
//! `tree-sitter-reify/test/corpus/unit_expr.txt` keeps three illustrative U+00B7 rows
//! as documentation and defers every negative and boundary case to this file — it is
//! deliberately NOT a row-for-row mirror, because an unexecuted copy of these
//! assertions would diverge from the parser silently.
//!
//! Assertions follow the house restraint used by the 13 sibling `*_grammar_tests.rs`:
//! error PRESENCE via `has_error()`, never exact ERROR spans or the `UNEXPECTED`
//! pseudo-token; positive shape via node lookup and field access, never s-expression
//! string comparison.  All sources are wrapped in `structure S { let x = <expr> }` so
//! the grammar sees them in a valid declaration context.
//!
//! Note on the `op` field: `unit_expr`'s operator is `choice($._unit_mul_op,
//! $._unit_div_op)` — both HIDDEN external tokens, so `child_by_field_name("op")`
//! never resolves.  The operator is therefore read as the source slice between the
//! `left` and `right` byte ranges, which is exactly how `reify-syntax`'s
//! `lower_unit_expr` recovers it.

use tree_sitter_reify::language;

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Walk a tree and collect all node kinds (depth-first, including anonymous nodes).
fn collect_kinds(node: tree_sitter::Node) -> Vec<String> {
    let mut kinds = vec![node.kind().to_string()];
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            kinds.extend(collect_kinds(cursor.node()));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    kinds
}

/// Depth-first search for the first node with the given kind.
fn find_node_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            if let Some(found) = find_node_by_kind(cursor.node(), kind) {
                return Some(found);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

/// Count every node of the given kind in the tree (depth-first).
fn count_nodes_by_kind(node: tree_sitter::Node, kind: &str) -> usize {
    let mut n = usize::from(node.kind() == kind);
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            n += count_nodes_by_kind(cursor.node(), kind);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    n
}

/// Parse `source`, assert it is error-free, and return the outermost `unit_expr`.
fn parse_clean_unit_expr(source: &str) -> (tree_sitter::Tree, String) {
    let mut parser = make_parser();
    let bytes = source.as_bytes();
    let tree = parser.parse(bytes, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "`{source}` must parse cleanly; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let unit = find_node_by_kind(tree.root_node(), "unit_expr")
        .unwrap_or_else(|| panic!("`{source}`: expected a `unit_expr` node"));
    let text = unit
        .utf8_text(bytes)
        .expect("unit_expr text is valid UTF-8")
        .to_string();
    (tree, text)
}

/// Assert `source` parses with at least one error node.
fn assert_rejected(source: &str) {
    let mut parser = make_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "`{source}` must NOT parse cleanly — there is no general `·` operator in \
         Reify, and `·` must bind only as a unit-multiply between adjacent units; \
         got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

/// The `left` / `right` field of a `unit_expr`, as its source text.
fn field_text<'a>(node: tree_sitter::Node<'a>, field: &str, src: &'a [u8]) -> String {
    node.child_by_field_name(field)
        .unwrap_or_else(|| {
            panic!(
                "expected a `{field}` field on `{}`; got kinds: {:?}",
                node.kind(),
                collect_kinds(node)
            )
        })
        .utf8_text(src)
        .expect("field text is valid UTF-8")
        .to_string()
}

/// The operator token between a `unit_expr`'s `left` and `right` operands, read as
/// the source slice between their byte ranges (the `op` field is hidden — see the
/// module header).
fn op_slice<'a>(node: tree_sitter::Node<'a>, src: &'a [u8]) -> String {
    let left = node
        .child_by_field_name("left")
        .expect("mul/div unit_expr has a `left` field");
    let right = node
        .child_by_field_name("right")
        .expect("mul/div unit_expr has a `right` field");
    std::str::from_utf8(&src[left.end_byte()..right.start_byte()])
        .expect("operator slice is valid UTF-8")
        .to_string()
}

/// Assert the `left`/`right` operand texts and the operator slice of a `unit_expr`.
fn assert_binary(
    node: tree_sitter::Node,
    src: &[u8],
    expect_left: &str,
    expect_op: &str,
    expect_right: &str,
) {
    assert_eq!(
        field_text(node, "left", src),
        expect_left,
        "unexpected `left` operand"
    );
    assert_eq!(
        field_text(node, "right", src),
        expect_right,
        "unexpected `right` operand"
    );
    assert_eq!(op_slice(node, src), expect_op, "unexpected operator token");
}

// ── ACCEPT: `·` binds adjacent units exactly as `*` does ──────────────────────

/// `5N·m` — the canonical torque literal.  Must parse clean into a mul `unit_expr`
/// whose operands are the `unit_name`s `N` and `m`.
#[test]
fn accept_newton_middot_metre() {
    let source = "structure S { let x = 5N·m }";
    let (tree, text) = parse_clean_unit_expr(source);
    assert_eq!(text, "N·m", "unit_expr must span exactly `N·m`");
    let unit = find_node_by_kind(tree.root_node(), "unit_expr").unwrap();
    assert_binary(unit, source.as_bytes(), "N", "·", "m");
    // Both operands must bottom out in `unit_name`, not some recovery node.
    assert_eq!(
        count_nodes_by_kind(unit, "unit_name"),
        2,
        "`5N·m` must contain exactly two unit_name nodes; got kinds: {:?}",
        collect_kinds(unit)
    );
}

/// `5N·m/rad` — mixed `·` and `/`.  `/` is the outermost operator (left-assoc), so
/// the top-level slice is `/` and its `left` is the `N·m` mul.
#[test]
fn accept_newton_middot_metre_per_radian() {
    let source = "structure S { let x = 5N·m/rad }";
    let (tree, text) = parse_clean_unit_expr(source);
    assert_eq!(text, "N·m/rad");
    let unit = find_node_by_kind(tree.root_node(), "unit_expr").unwrap();
    let bytes = source.as_bytes();
    assert_binary(unit, bytes, "N·m", "/", "rad");
    let left = unit.child_by_field_name("left").unwrap();
    assert_binary(left, bytes, "N", "·", "m");
}

/// `7850kg·m^-3` — density: `·` against a `pow` right operand with a negative
/// exponent.  This is the literal `Display for DimensionVector` emits.
#[test]
fn accept_density_kg_middot_metre_pow_neg3() {
    let source = "structure S { let x = 7850kg·m^-3 }";
    let (tree, text) = parse_clean_unit_expr(source);
    assert_eq!(text, "kg·m^-3");
    let unit = find_node_by_kind(tree.root_node(), "unit_expr").unwrap();
    let bytes = source.as_bytes();
    assert_binary(unit, bytes, "kg", "·", "m^-3");
    let right = unit.child_by_field_name("right").unwrap();
    assert_eq!(field_text(right, "base", bytes), "m");
    assert_eq!(field_text(right, "exponent", bytes), "-3");
}

/// `9.81m·s^-2` — acceleration, with a decimal magnitude before the unit.
#[test]
fn accept_acceleration_metre_middot_second_pow_neg2() {
    let source = "structure S { let x = 9.81m·s^-2 }";
    let (tree, text) = parse_clean_unit_expr(source);
    assert_eq!(text, "m·s^-2");
    let unit = find_node_by_kind(tree.root_node(), "unit_expr").unwrap();
    assert_binary(unit, source.as_bytes(), "m", "·", "s^-2");
}

/// `5m^2·kg·s^-2·rad^-1` — the four-factor chain `Display` emits for torque.  Pins
/// LEFT-associativity: `((m^2 · kg) · s^-2) · rad^-1`.
#[test]
fn accept_four_factor_left_associative_chain() {
    let source = "structure S { let x = 5m^2·kg·s^-2·rad^-1 }";
    let (tree, text) = parse_clean_unit_expr(source);
    assert_eq!(text, "m^2·kg·s^-2·rad^-1");
    let unit = find_node_by_kind(tree.root_node(), "unit_expr").unwrap();
    let bytes = source.as_bytes();
    assert_binary(unit, bytes, "m^2·kg·s^-2", "·", "rad^-1");
    let l1 = unit.child_by_field_name("left").unwrap();
    assert_binary(l1, bytes, "m^2·kg", "·", "s^-2");
    let l2 = l1.child_by_field_name("left").unwrap();
    assert_binary(l2, bytes, "m^2", "·", "kg");
}

/// `5W·(m/K)` and `5W/(m·K)` — `·` adjacent to a PAREN GROUP on either side.
///
/// `is_unit_start` accepts `(`, so this is a live accepted path.  It is pinned at
/// the CST layer because the operator is NOT a field on the node: every consumer,
/// including `lower_unit_expr`, recovers it from the source slice between the
/// operands (`op_slice` here does the same), and a paren moves those boundaries.
#[test]
fn accept_middot_adjacent_to_paren_group() {
    let source = "structure S { let x = 5W·(m/K) }";
    let (tree, text) = parse_clean_unit_expr(source);
    assert_eq!(text, "W·(m/K)");
    let unit = find_node_by_kind(tree.root_node(), "unit_expr").unwrap();
    assert_binary(unit, source.as_bytes(), "W", "·", "(m/K)");

    // …and with the `·` INSIDE the group, under an outer `/`.
    let source = "structure S { let x = 5W/(m·K) }";
    let (tree, text) = parse_clean_unit_expr(source);
    assert_eq!(text, "W/(m·K)");
    let unit = find_node_by_kind(tree.root_node(), "unit_expr").unwrap();
    let bytes = source.as_bytes();
    assert_binary(unit, bytes, "W", "/", "(m·K)");
    // Descend past the paren-group node to the mul it wraps — the same walk
    // `lower_unit_expr` step 3 does (parens are anonymous tokens, not children),
    // so `find_node_by_kind` is not usable here: it would match the group itself.
    let group = unit.child_by_field_name("right").unwrap();
    let mut cursor = group.walk();
    let inner = group
        .named_children(&mut cursor)
        .find(|c| c.kind() == "unit_expr")
        .expect("paren group must wrap an inner unit_expr");
    assert_binary(inner, bytes, "m", "·", "K");
}

// ── REJECT: `·` is scoped to unit-multiply and nothing else ───────────────────
//
// All four fall out of the SINGLE `is_unit_start(lexer->lookahead)` post-check the
// UNIT_MUL_OP block already applied to `*`; no `·`-specific rejection logic exists,
// and these rows are what lock that in.

/// `5N·3` — a digit is not a unit-start character, so `·` must not be taken as
/// unit-multiply (mirrors the existing `5kg*1` guard for `*`).
#[test]
fn reject_digit_after_middot() {
    assert_rejected("structure S { let x = 5N·3 }");
}

/// `5N· m` — whitespace AFTER `·` breaks unit contiguity (PRD §3.1).
#[test]
fn reject_space_after_middot() {
    assert_rejected("structure S { let x = 5N· m }");
}

/// `5N · m` — whitespace BEFORE `·` breaks unit contiguity.
#[test]
fn reject_spaces_around_middot() {
    assert_rejected("structure S { let x = 5N · m }");
}

/// `5 · 3` — the B7 boundary row.  Reify has no general `·` binary operator, and
/// widening the scanner must not accidentally introduce one.
#[test]
fn reject_bare_middot_as_general_binary_operator() {
    assert_rejected("structure S { let x = 5 · 3 }");
}

// ── STATEMENT BOUNDARY (the INV-SF-7 ambiguity obligation) ────────────────────

/// A DANGLING `·` at end of line must NOT absorb the following statement: the parse
/// must fail rather than silently swallowing `let b` as the multiplicand.
#[test]
fn boundary_dangling_middot_does_not_absorb_next_statement() {
    assert_rejected("structure S {\n  let a = 5N·\n  let b = 3\n}");
}

/// Two adjacent `let`s — the first `·`-bearing — must parse as TWO independent
/// declarations, with the first binding's unit_expr spanning only `N·m`.  Adjacent
/// -token variation must not change what the first binding parses to.
#[test]
fn boundary_adjacent_lets_parse_independently() {
    let source = "structure S {\n  let a = 5N·m\n  let b = 3m\n}";
    let mut parser = make_parser();
    let bytes = source.as_bytes();
    let tree = parser.parse(bytes, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "two adjacent lets must parse cleanly; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    assert_eq!(
        count_nodes_by_kind(tree.root_node(), "let_declaration"),
        2,
        "expected exactly two `let_declaration` nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    // The FIRST unit_expr found depth-first belongs to `a` and must stop at `m`.
    let first_unit = find_node_by_kind(tree.root_node(), "unit_expr").unwrap();
    assert_eq!(
        first_unit.utf8_text(bytes).unwrap(),
        "N·m",
        "`a`'s unit_expr must span exactly `N·m` — the `·` must not reach across \
         the statement boundary"
    );
    assert_binary(first_unit, bytes, "N", "·", "m");
    // `b`'s value is a separate quantity_literal, not a continuation of `a`'s.
    assert_eq!(
        count_nodes_by_kind(tree.root_node(), "quantity_literal"),
        2,
        "each let must carry its own quantity_literal; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

// ── `*`/`·` equivalence at the CST level ─────────────────────────────────────

/// The two spellings must produce structurally identical trees — same node kinds in
/// the same depth-first order — differing only in the operator slice.  This is the
/// CST-level half of the task's "`5N·m` evaluates identically to `5N*m`" signal.
#[test]
fn middot_and_star_produce_identical_cst_shapes() {
    for (dot_src, star_src) in [
        ("structure S { let x = 5N·m }", "structure S { let x = 5N*m }"),
        (
            "structure S { let x = 5N·m/rad }",
            "structure S { let x = 5N*m/rad }",
        ),
        (
            "structure S { let x = 5m^2·kg·s^-2 }",
            "structure S { let x = 5m^2*kg*s^-2 }",
        ),
    ] {
        let (dot_tree, _) = parse_clean_unit_expr(dot_src);
        let (star_tree, _) = parse_clean_unit_expr(star_src);
        assert_eq!(
            collect_kinds(dot_tree.root_node()),
            collect_kinds(star_tree.root_node()),
            "`{dot_src}` and `{star_src}` must yield identical CST shapes"
        );
    }
}
