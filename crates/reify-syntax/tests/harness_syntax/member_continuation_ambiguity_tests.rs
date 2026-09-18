//! INV-SF-7 (task #7094): a structure-member-body `let` silently absorbs the
//! following line.
//!
//! `extras: [/\s/, ...]` (tree-sitter-reify/grammar.js:88) makes the grammar
//! wholly newline-insensitive, and member-list bodies are bare `repeat(...)`
//! with no separator token. A member's trailing expression therefore greedily
//! joins whatever begins the next line whenever that line can continue it —
//! a leading binary operator (`- 3mm`) or a `(`-led call/argument list.
//!
//! These tests pin the DIAGNOSTIC, not the parse. The CST reading is
//! deliberately UNCHANGED by #7094 (the grammar is untouched); what changes is
//! that the join is now reported instead of being silently picked. See
//! `crates/reify-syntax/src/member_continuation.rs` for the normative rule.

use reify_core::ModulePath;
// The "is this a member-continuation diagnostic?" question is answered by the
// rule's own exported discriminator, never by a substring predicate restated
// here: the recogniser lives next to the message it recognises, so a reword
// lands in exactly one place for every caller in every crate.
use reify_syntax::member_continuation::is_member_continuation_message as is_member_continuation_error;

/// REPRO 1 from task #7094: leading-operator continuation inside a structure
/// member body.
const REPRO_ONE: &str = "structure S {\n  let d = 5mm\n  - 3mm\n}\n";

/// REPRO 2 from task #7094: `(`-led continuation inside a structure member
/// body.
const REPRO_TWO: &str = "structure S {\n  let x = a.b\n  (c)\n}\n";

/// Collect the member-continuation diagnostics from a parse, as
/// `(span_start, span_end, message)` triples.
fn member_continuation_errors(source: &str) -> Vec<(u32, u32, String)> {
    let parsed = reify_syntax::parse(source, ModulePath::single("m"));
    parsed
        .errors
        .iter()
        .filter(|e| is_member_continuation_error(&e.message))
        .map(|e| (e.span.start, e.span.end, e.message.clone()))
        .collect()
}

// ── (a) the two repros must be reported ──────────────────────────────────────

#[test]
fn leading_operator_continuation_is_rejected_at_the_operator() {
    let src = REPRO_ONE;
    let parsed = reify_syntax::parse(src, ModulePath::single("m"));
    assert_eq!(
        parsed.errors.len(),
        1,
        "expected exactly one ParseError for the leading-operator join, got {:?}",
        parsed.errors
    );
    let err = &parsed.errors[0];

    // The span must cover ONLY the offending row-leading `-`, not the whole
    // member: a member-wide span would bury the actual boundary.
    let minus = src.find("- 3mm").expect("REPRO_ONE must contain `- 3mm`") as u32;
    assert_eq!(
        (err.span.start, err.span.end),
        (minus, minus + 1),
        "span must cover exactly the `-` token at byte {minus}, got {:?} ({:?})",
        err.span,
        &src[err.span.start as usize..err.span.end as usize]
    );

    assert!(
        is_member_continuation_error(&err.message),
        "message must name the member-continuation ambiguity, got: {:?}",
        err.message
    );
}

/// Scoped to member-continuation diagnostics rather than to the whole error
/// list, because the joined reading of REPRO 2 is a `namespaced_call` whose
/// qualifier `a` is not bound by any import — so lowering ALSO emits a
/// pre-existing "unknown qualifier `a`" diagnostic (measured on this branch).
/// That co-diagnostic is incidental to this repro's exact identifier choice:
/// it disappears the moment `a` names an imported namespace, while the join
/// itself does not. Asserting on it would pin an unrelated seam.
#[test]
fn paren_led_continuation_is_rejected_at_the_open_paren() {
    let src = REPRO_TWO;
    let found = member_continuation_errors(src);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one member-continuation ParseError for the `(`-led join, \
         got {found:?} (all errors: {:?})",
        reify_syntax::parse(src, ModulePath::single("m")).errors
    );
    let (start, end, _message) = &found[0];

    let lparen = src.find("(c)").expect("REPRO_TWO must contain `(c)`") as u32;
    assert_eq!(
        (*start, *end),
        (lparen, lparen + 1),
        "span must cover exactly the `(` token at byte {lparen}, got {:?}",
        &src[*start as usize..*end as usize]
    );
}

// ── (b) the fix is LOUD, not silent ──────────────────────────────────────────

/// #7094 makes the join VISIBLE; it does not change which reading the grammar
/// picks. Pinning that distinction here means a later grammar-level fix (an
/// indent-sensitive external scanner token, say) cannot change the accepted
/// reading without turning this test red — which is exactly the review it
/// deserves.
#[test]
fn the_joined_reading_is_still_what_the_grammar_produces() {
    let parsed = reify_syntax::parse(REPRO_ONE, ModulePath::single("m"));

    let structure = parsed
        .declarations
        .iter()
        .find_map(|d| match d {
            reify_ast::Declaration::Structure(s) => Some(s),
            _ => None,
        })
        .expect("REPRO_ONE must lower to one structure declaration");

    assert_eq!(
        structure.members.len(),
        1,
        "the grammar still joins both lines into ONE member; got {:?}",
        structure.members
    );

    let let_decl = match &structure.members[0] {
        reify_ast::MemberDecl::Let(l) => l,
        other => panic!("expected the single member to be a Let, got {other:?}"),
    };
    assert_eq!(let_decl.name, "d");

    match &let_decl.value.kind {
        reify_ast::ExprKind::BinOp { op, .. } => {
            assert_eq!(
                op, "-",
                "the joined value is still `5mm - 3mm`; got op {op:?}"
            );
        }
        other => panic!(
            "the joined reading must still be a subtraction BinOp \
             (this fix reports the join, it does not re-parse it); got {other:?}"
        ),
    }
}

// ── (c) shared helper is exercised ───────────────────────────────────────────

#[test]
fn member_continuation_errors_helper_agrees_with_the_repro_assertions() {
    assert_eq!(
        member_continuation_errors(REPRO_ONE).len(),
        1,
        "REPRO_ONE must yield exactly one member-continuation diagnostic"
    );
    assert_eq!(
        member_continuation_errors(REPRO_TWO).len(),
        1,
        "REPRO_TWO must yield exactly one member-continuation diagnostic"
    );
}

// ── (d) negatives: legal multi-line shapes must stay clean ───────────────────

/// Assert `source` yields no member-continuation diagnostic, reporting the
/// full error list on failure so a regression is actionable.
fn assert_no_member_continuation_error(label: &str, source: &str) {
    let found = member_continuation_errors(source);
    assert!(
        found.is_empty(),
        "{label}: expected no member-continuation diagnostic, got {found:#?}"
    );
}

/// The `designs/litter_tray/bottom_deck.ri:64-65` shape, verbatim in form: a
/// leading-operator continuation indented PAST the member's start column.
///
/// This is the constituency a naive "a newline ends a member" rule would
/// break: deliberately-continued rows are ordinary in tracked reify source.
/// Indentation past the member column is the author's signal, and it stays
/// legal — `no_tracked_ri_source_trips_the_member_continuation_check` below is
/// what keeps every such site honest, re-measured on each run.
#[test]
fn deeper_indented_leading_operator_is_a_legal_continuation() {
    let source = concat!(
        "structure S {\n",
        "    let capacity = (ledge_z - floor_thickness) * inner_length * inner_width\n",
        "                 - 2.0 * 3.14159265 * pedestal_r * pedestal_r * pedestal_h\n",
        "}\n",
    );
    // Sanity: the fixture really does indent the continuation past the member
    // column — otherwise this test would pass for the wrong reason.
    let third = source.lines().nth(2).expect("source has a third line");
    let cont_col = third.len() - third.trim_start().len();
    assert!(
        cont_col > 4,
        "fixture must indent the continuation past the member column 4, got {cont_col}"
    );
    assert_no_member_continuation_error("deeper-indented leading operator", source);
}

/// The `examples/fea_multi_case_smoke.ri:47-52` shape: a call whose argument
/// list spans several rows, with the closing `)` back at the member's own
/// start column. Those rows are at bracket depth >= 1 relative to the member,
/// so the rule does not look at them.
#[test]
fn multi_line_argument_list_rows_at_or_left_of_the_member_column_are_clean() {
    let source = concat!(
        "structure S {\n",
        "    let self_weight = solve_elastic_static(\n",
        "        material, length, width, height,\n",
        "    [Gravity()],\n",
        "    )\n",
        "}\n",
    );
    // Sanity: rows 4 and 5 begin at or left of the member column 4, so the
    // fixture genuinely exercises the depth exclusion rather than the column
    // comparison.
    for row in [3usize, 4] {
        let line = source.lines().nth(row).expect("fixture row exists");
        let col = line.len() - line.trim_start().len();
        assert!(
            col <= 4,
            "fixture row {row} must start at or left of column 4 to exercise the \
             depth exclusion, got {col}"
        );
    }
    assert_no_member_continuation_error("multi-line argument list", source);
}

/// A closing `)` sitting at exactly the member's start column is depth 1, not
/// depth 0 — it closes a group the member itself opened, so it cannot be the
/// start of a new member.
#[test]
fn a_closing_delimiter_at_the_member_column_is_clean() {
    let source = "structure S {\n    let m = f(\n        1mm,\n    )\n}\n";
    assert_no_member_continuation_error("closing paren at the member column", source);
}

/// The check speaks only about trees that parsed cleanly: a source carrying
/// an `ERROR` or `MISSING` node anywhere gets no member-continuation
/// diagnostic at all.
///
/// Under error recovery a member's span and start column are the parser's
/// guesses, not the author's layout, so the rule's advice ("indent it past
/// column N") is aimed at a boundary nobody wrote. Both fixtures below were
/// MEASURED to report when the gate is removed, so this test is not vacuous —
/// and they fail in the two different ways that decide the gate's shape:
///
/// - `broken member`: the garbage tail leaves the ERROR INSIDE the
///   `let_declaration` that swallows the next row.
/// - `broken sibling`: the incomplete `sub a :` takes the following `let` as
///   its type name, so the reported member is itself ERROR-free and the ERROR
///   is a sibling. A per-member cleanliness test would let this one through;
///   only the whole-tree gate catches it.
///
/// This is also the property that keeps the check quiet on the GUI's
/// parse-while-typing path.
#[test]
fn a_broken_parse_gets_no_continuation_diagnostic() {
    let fixtures: &[(&str, &str)] = &[
        (
            "broken member",
            concat!(
                "structure def S {\n",
                "    let a = 5mm @@\n",
                "    - 3mm\n",
                "}\n",
            ),
        ),
        (
            "broken sibling",
            concat!(
                "structure def S {\n",
                "    sub a : \n",
                "    let z = 2mm\n",
                "}\n",
            ),
        ),
    ];

    for (label, source) in fixtures {
        // Sanity: each fixture really is a broken parse with a diagnostic of
        // its own, so this test cannot pass by being clean source with nothing
        // to report.
        let parsed = reify_syntax::parse(source, ModulePath::single("m"));
        assert!(
            parsed
                .errors
                .iter()
                .any(|e| !is_member_continuation_error(&e.message)),
            "{label}: fixture must still produce a syntax error of its own; got {:?}",
            parsed.errors
        );
        assert_no_member_continuation_error(label, source);
    }
}

/// Same for a member-level `where cond { ... }` whose `}` sits at the member's
/// own column — the everyday layout for a guarded block.
#[test]
fn a_guarded_block_closing_brace_at_the_member_column_is_clean() {
    let source = "structure S {\n    where enabled {\n        let a = 1mm\n    }\n}\n";
    assert_no_member_continuation_error("guarded-block closing brace", source);
}

// ── (e) repo-wide sweep: the standing guard ──────────────────────────────────

/// The workspace root, resolved from this crate's manifest dir
/// (`<root>/crates/reify-syntax` → up two).
fn workspace_root() -> std::path::PathBuf {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be <workspace>/crates/reify-syntax")
        .to_path_buf()
}

/// Every tracked-looking `*.ri` under `root`, skipping build output and
/// version-control metadata.
fn collect_ri_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                // `target/` is build output; `node_modules/` is vendored JS;
                // dot-dirs are VCS/tooling metadata. None hold tracked sources.
                if name == "target" || name == "node_modules" || name.starts_with('.') {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "ri") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Byte offset → 1-based (line, column), for actionable failure reporting.
fn line_col(source: &str, offset: u32) -> (usize, usize) {
    let offset = offset as usize;
    let before = &source[..offset.min(source.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
    (line, col)
}

/// THE STANDING GUARD: no `.ri` source in this repository may trip the
/// member-continuation check.
///
/// A hit here is a genuine finding, and it has exactly two dispositions: the
/// rule is wrong (fix `member_continuation.rs`), or the source really is
/// ambiguous (fix the source). It must never be silenced by loosening the
/// assertion.
#[test]
fn no_tracked_ri_source_trips_the_member_continuation_check() {
    let root = workspace_root();
    let files = collect_ri_files(&root);
    assert!(
        files.len() > 100,
        "sweep found only {} .ri files under {} — the walk is broken, not the repo",
        files.len(),
        root.display()
    );

    let mut hits: Vec<String> = Vec::new();
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        for (start, _end, message) in member_continuation_errors(&source) {
            let (line, col) = line_col(&source, start);
            let rel = path.strip_prefix(&root).unwrap_or(path);
            hits.push(format!("{}:{}:{}: {}", rel.display(), line, col, message));
        }
    }

    assert!(
        hits.is_empty(),
        "{} tracked .ri source location(s) trip the member-continuation check \
         (swept {} files):\n{}",
        hits.len(),
        files.len(),
        hits.join("\n")
    );
}

// ── (f) the other member-list bodies ─────────────────────────────────────────

/// Assert `source` yields exactly one member-continuation diagnostic whose
/// span covers exactly `token`, located by searching for `locator`.
///
/// `locator` and `token` are separate because the diagnostic is deliberately
/// token-precise: the offending row reads `- 3mm`, but the span must cover the
/// `-` alone. Passing the whole row as the locator keeps the fixture's intent
/// readable while still pinning the narrow span.
fn assert_one_member_continuation_error_at(
    label: &str,
    source: &str,
    locator: &str,
    token: &str,
) {
    assert!(
        locator.starts_with(token),
        "{label}: test bug — locator {locator:?} must begin with the expected \
         token {token:?}"
    );

    let found = member_continuation_errors(source);
    assert_eq!(
        found.len(),
        1,
        "{label}: expected exactly one member-continuation diagnostic, got {found:#?}"
    );
    let at = source
        .find(locator)
        .unwrap_or_else(|| panic!("{label}: fixture must contain {locator:?}")) as u32;
    let (start, end, _) = &found[0];
    assert_eq!(
        (*start, *end),
        (at, at + token.len() as u32),
        "{label}: span must cover exactly {token:?} at byte {at}, got {:?}",
        &source[*start as usize..*end as usize]
    );
}

/// THE DECIDED CASE. `tree-sitter-reify/test/corpus/namespaced_ref.txt`
/// (case "namespaced_ref item boundary: relate's relation_member repeat joins
/// the same way") deliberately COMMITS this join as the pinned CST reading.
/// #7094's task statement requires that case be decided explicitly rather than
/// broken by accident.
///
/// DECISION: reject. The pinned CST is unchanged — the grammar is untouched —
/// but the source is now reported at the syntax layer. A `relate` body's
/// members are bare expressions with no separator, so the join is exactly the
/// silent value change INV-SF-7 forbids.
#[test]
fn relate_body_item_boundary_join_is_rejected() {
    let source = "structure S {\n  relate {\n    a.b\n    (x)\n  }\n}\n";
    assert_one_member_continuation_error_at("relate body", source, "(x)", "(");
}

/// The same shape in a sub's inline `at <pose> where { … }` relate block,
/// which reuses `relation_member` verbatim (grammar.js:745-751).
#[test]
fn sub_relate_block_body_item_boundary_join_is_rejected() {
    let source = concat!(
        "structure S {\n",
        "  sub p : Part at origin where {\n",
        "    a.b\n",
        "    (x)\n",
        "  }\n",
        "}\n",
    );
    assert_one_member_continuation_error_at("sub_relate_block body", source, "(x)", "(");
}

/// `namespaced_ref.txt` case "namespaced_ref item boundary: a predicate ending
/// `.name` JOINS a following `(` predicate" — same decision as
/// `relate_body_item_boundary_join_is_rejected`.
#[test]
fn constraint_def_body_item_boundary_join_is_rejected() {
    let source = "constraint def C {\n  a.b\n  (x) > 0\n}\n";
    assert_one_member_continuation_error_at("constraint def body", source, "(x)", "(");
}

/// `namespaced_ref.txt`'s sibling control case, "item boundary control: a
/// predicate ending in a BARE ident joins the same way". The bare-ident form
/// joins into a `function_call` rather than a `namespaced_call`; the boundary
/// is identical and so is the decision.
#[test]
fn constraint_def_bare_ident_control_join_is_rejected() {
    let source = "constraint def C {\n  a\n  (x) > 0\n}\n";
    assert_one_member_continuation_error_at(
        "constraint def bare-ident control",
        source,
        "(x)",
        "(",
    );
}

#[test]
fn trait_body_leading_operator_continuation_is_rejected() {
    let source = "trait T {\n  let d = 5mm\n  - 3mm\n}\n";
    assert_one_member_continuation_error_at("trait body", source, "- 3mm", "-");
}

#[test]
fn purpose_body_leading_operator_continuation_is_rejected() {
    let source = "purpose P() {\n  let d = 5mm\n  - 3mm\n}\n";
    assert_one_member_continuation_error_at("purpose body", source, "- 3mm", "-");
}

#[test]
fn guarded_block_body_leading_operator_continuation_is_rejected() {
    let source = concat!(
        "structure S {\n",
        "  where enabled {\n",
        "    let d = 5mm\n",
        "    - 3mm\n",
        "  }\n",
        "}\n",
    );
    assert_one_member_continuation_error_at("guarded-block body", source, "- 3mm", "-");
}

/// `guarded_block` is the only container with TWO brace pairs (the `where`
/// body and the `else` body), and `members_of` slices between the FIRST `{`
/// and the LAST `}` — so both bodies' members fall out of one slice, and the
/// intervening `}`, `else`, `{` are anonymous and filtered away. That is the
/// subtlest line in `members_of`; this test pins it by putting the join in the
/// `else` body, which the no-`else` fixtures above never reach.
#[test]
fn guarded_block_else_body_leading_operator_continuation_is_rejected() {
    let source = concat!(
        "structure S {\n",
        "  where c {\n",
        "    let a = 1mm\n",
        "  } else {\n",
        "    let b = 2mm\n",
        "    - 3mm\n",
        "  }\n",
        "}\n",
    );
    assert_one_member_continuation_error_at("guarded-block else body", source, "- 3mm", "-");
}

/// `occurrence_definition` shares `repeat($._member)` with
/// `structure_definition` (grammar.js:512 and :525), so it is vulnerable to
/// exactly the same join.
#[test]
fn occurrence_body_leading_operator_continuation_is_rejected() {
    let source = "occurrence O : S {\n  let d = 5mm\n  - 3mm\n}\n";
    assert_one_member_continuation_error_at("occurrence body", source, "- 3mm", "-");
}

/// MEASURED FINDING, contra #7094's plan, which listed a must-error case here.
///
/// `match_arm_decl_block` (grammar.js:1400-1406) is the one brace-delimited
/// member list that CANNOT host this join, for two independent reasons:
/// its arms are `,`-separated, and `match_arm_sub_decl` ends in a plain
/// `structure_name: $.identifier` (grammar.js:1418-1423) with no expression
/// tail for a following line to attach to.
///
/// So the grammar itself already rejects the shape, loudly — a leading-operator
/// line after an arm yields an `ERROR` node rather than a silent join. That is
/// INV-SF-7-compliant on its own, and it is why no member-continuation
/// diagnostic is expected here.
///
/// The container is still carried in `MEMBER_LIST_CONTAINERS`: grammar.js
/// (~line 1415) defers the arm-body form `sub name : T { ... }` to task #3569,
/// and when that lands the coverage is already in place. This test goes red if
/// that widening ever makes the shape parse cleanly, which is exactly when
/// someone should re-decide this case.
#[test]
fn match_arm_decl_block_rejects_the_join_at_the_grammar_level_already() {
    let source = concat!(
        "structure S {\n",
        "  match m {\n",
        "    A => sub h : T\n",
        "    - 3mm\n",
        "  }\n",
        "}\n",
    );
    let parsed = reify_syntax::parse(source, ModulePath::single("m"));
    assert!(
        !parsed.errors.is_empty(),
        "the grammar must still reject this shape outright; got a clean parse"
    );
    assert!(
        member_continuation_errors(source).is_empty(),
        "no member-continuation diagnostic is expected — the join never forms. \
         If this fires, task #3569 widened the arm body and this case needs \
         re-deciding. Errors: {:?}",
        parsed.errors
    );
}

/// `port_body` (grammar.js:1013-1023) holds full `let` members, so REPRO 1
/// reproduces inside a port body verbatim. Surfaced as MISSING by the
/// grammar-drift guard in section (g), not by hand — which is the guard doing
/// exactly the job it was added for.
#[test]
fn port_body_leading_operator_continuation_is_rejected() {
    let source = concat!(
        "structure S {\n",
        "  port p : in Flow {\n",
        "    let x = 5mm\n",
        "    - 3mm\n",
        "  }\n",
        "}\n",
    );
    assert_one_member_continuation_error_at("port body", source, "- 3mm", "-");
}

/// `field_source_sampled` (grammar.js:338-343) repeats `field_config_entry`,
/// which is `key = <expression>` (grammar.js:364-368). The trailing expression
/// absorbs the next line exactly as a `let` does. Also surfaced by section (g).
#[test]
fn field_source_sampled_body_leading_operator_continuation_is_rejected() {
    let source = concat!(
        "field def f : Length -> Length {\n",
        "  source = sampled {\n",
        "    resolution = 5mm\n",
        "    - 3mm\n",
        "  }\n",
        "}\n",
    );
    assert_one_member_continuation_error_at("sampled field source", source, "- 3mm", "-");
}

/// `field_source_imported` (grammar.js:358-363) has the same body shape as
/// `field_source_sampled` but is a distinct grammar rule, so it needs its own
/// container entry and its own pin. Also surfaced by section (g).
#[test]
fn field_source_imported_body_leading_operator_continuation_is_rejected() {
    let source = concat!(
        "field def f : Length -> Length {\n",
        "  source = imported {\n",
        "    scale = 5mm\n",
        "    - 3mm\n",
        "  }\n",
        "}\n",
    );
    assert_one_member_continuation_error_at("imported field source", source, "- 3mm", "-");
}

/// `derived_body` (grammar.js:1052-1061) is the derived-sub body added by task
/// #6615, which landed on main AFTER the container list was first written — so
/// this is section (g)'s tripwire firing on real drift rather than in theory.
/// The body admits full `let` members, so it carries REPRO 1 verbatim.
#[test]
fn derived_body_leading_operator_continuation_is_rejected() {
    let source = concat!(
        "structure S {\n",
        "  sub b = mirror of a across P {\n",
        "    let x = 5mm\n",
        "    - 3mm\n",
        "  }\n",
        "}\n",
    );
    assert_one_member_continuation_error_at("derived body", source, "- 3mm", "-");
}

/// `joint_body`'s block arm repeats `relation_member` verbatim
/// (grammar.js:799-802), the same item `relate_block` uses — so it carries the
/// same join. Worth its own fixture rather than reasoning by analogy: the rule
/// has a brace-less alternative (`field('result', $._expression)`), and
/// `members_of` returns nothing for a body with no `{`/`}` children, so this is
/// exactly the kind of entry that could sit in `MEMBER_LIST_CONTAINERS` doing
/// nothing while every other test stayed green.
#[test]
fn joint_body_leading_operator_continuation_is_rejected() {
    let source = "joint J() with angle : Angle = {\n  a.b\n  - 3mm\n}\n";
    assert_one_member_continuation_error_at("joint body", source, "- 3mm", "-");
}

/// `specialization_body` (grammar.js:1121-1125) repeats
/// `choice($.param_assignment, $._member)`, and `param_assignment` is
/// `name = <expression>` — a trailing expression, so it absorbs the next line
/// exactly as a `let` does.
#[test]
fn specialization_body_leading_operator_continuation_is_rejected() {
    let source = concat!(
        "structure S {\n",
        "  sub a : T {\n",
        "    p = 1mm\n",
        "    - 3mm\n",
        "  }\n",
        "}\n",
    );
    assert_one_member_continuation_error_at("specialization body", source, "- 3mm", "-");
}

/// `keyed_member_block` (grammar.js:1146-1150) is covered but CANNOT host the
/// ordinary join, and that is a measured claim rather than an assumption: a
/// `keyed_member_entry` is `"key" => <specialization_body>`, so every entry
/// ends in `}` and has no trailing expression for the next line to attach to.
///
/// Both halves are asserted, because "no diagnostic" on its own is also what
/// an entry that is never visited looks like. The second half splits one entry
/// across two rows so its `=>` is row-leading at the entry's own column: that
/// IS reported, which proves the container is live rather than inert.
#[test]
fn keyed_member_block_entries_cannot_join_but_the_container_is_live() {
    let clean = concat!(
        "structure S {\n",
        "  sub a : T {\n",
        "    \"k\" => { p = 1mm }\n",
        "    \"j\" => { p = 2mm }\n",
        "  }\n",
        "}\n",
    );
    assert_no_member_continuation_error("adjacent keyed entries", clean);

    let split = concat!(
        "structure S {\n",
        "  sub a : T {\n",
        "    \"k\"\n",
        "    => { p = 1mm }\n",
        "  }\n",
        "}\n",
    );
    assert_one_member_continuation_error_at(
        "keyed entry split across rows",
        split,
        "=> { p = 1mm }",
        "=>",
    );
}

// ── (g) grammar-drift guard ─────────────────────────────────────────────────
//
// The container list in `member_continuation.rs` is a hand-written enumeration
// of grammar rules. Nothing in the grammar points back at it, so a member-list
// body added to `grammar.js` tomorrow would silently escape the check and
// reintroduce INV-SF-7 at the new site. This section closes that loop: it reads
// `grammar.js`, re-derives the member-repeat sites from it, and requires each
// one to be either covered or deliberately excluded with a stated reason.

/// A member-list body repeat found in `grammar.js`: `(line_number, rule_name)`.
///
/// **What counts.** A `repeat(...)` / `repeat1(...)` is a *member-list body*
/// when its argument
///
/// 1. contains at least one grammar-symbol reference (`$.x` / `$._x`), and
/// 2. contains no string-literal token (`'...'`).
///
/// Clause 2 is the discriminator, and it is the INV-SF-7 rule itself in
/// grammar terms: a repeat that carries a string literal carries a *separator*
/// or *terminator* token (`repeat(seq(',', $.match_arm))`,
/// `repeat(seq('+', $.trait_bound_entry))`), so consecutive items can never be
/// adjacent in the token stream and no join can form. A repeat with no string
/// literal at all puts items directly against each other — which is exactly the
/// shape that lets one member's trailing expression swallow the next member's
/// first token.
///
/// The scan runs on [`mask_noncode`]'s output, so comments cannot contribute a
/// site and no string or regex content can be mistaken for grammar. Only the
/// `rules: {` block is scanned, so the `commaSeparated` helper above it cannot
/// be attributed to a rule.
fn member_list_body_repeats(masked: &str) -> Vec<(usize, String)> {
    let mut sites = Vec::new();
    let mut current_rule: Option<String> = None;
    let mut in_rules_block = false;

    for (line_index, (line_start, line)) in line_spans(masked).into_iter().enumerate() {
        if !in_rules_block {
            in_rules_block = line.trim_start() == "rules: {";
            continue;
        }
        if let Some(name) = rule_header(line) {
            current_rule = Some(name.to_string());
        }

        for open in repeat_call_offsets(line) {
            let Some(arg) = balanced_arg(masked, line_start + open) else {
                panic!(
                    "grammar.js:{}: unbalanced `repeat(` — the drift-guard scanner \
                     cannot classify this site, so it cannot vouch for the grammar. \
                     Fix the scanner or the grammar before trusting this test.",
                    line_index + 1
                );
            };
            if !contains_symbol_reference(arg) || contains_string_literal(arg) {
                continue;
            }
            let rule = current_rule.clone().unwrap_or_else(|| {
                panic!(
                    "grammar.js:{}: member-list repeat with no enclosing grammar rule; \
                     `rule_header` failed to attribute it",
                    line_index + 1
                )
            });
            sites.push((line_index + 1, rule));
        }
    }
    sites
}

/// `(byte_offset_of_line_start, line_text)` for every line, newline excluded.
fn line_spans(src: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, _) in src.match_indices('\n') {
        out.push((start, src[start..i].trim_end_matches('\r')));
        start = i + 1;
    }
    if start < src.len() {
        out.push((start, &src[start..]));
    }
    out
}

/// A grammar-rule header line: exactly four spaces, `<name>`, `:`, `$ =>`.
///
/// Four spaces is the rule-definition indentation in `grammar.js`; anything
/// deeper is inside a rule body and must not re-anchor attribution.
fn rule_header(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("    ")?;
    if rest.starts_with(' ') {
        return None;
    }
    let (name, tail) = rest.split_once(':')?;
    if !tail.trim_start().starts_with("$ =>") {
        return None;
    }
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(name)
}

/// Byte offsets, within `line`, one past the `(` of each `repeat(`/`repeat1(`.
fn repeat_call_offsets(line: &str) -> Vec<usize> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    for (i, _) in line.match_indices("repeat") {
        // Reject `xrepeat(` — only a whole identifier counts.
        if i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        let after = &line[i + "repeat".len()..];
        if let Some(rest) = after.strip_prefix("1(") {
            out.push(line.len() - rest.len());
        } else if let Some(rest) = after.strip_prefix('(') {
            out.push(line.len() - rest.len());
        }
    }
    out
}

/// `grammar` with every comment, regex literal and string-literal *interior*
/// blanked to spaces — byte-for-byte the same length, so every offset computed
/// on the result also indexes the original.
///
/// Balancing parentheses on the raw text is not safe. `grammar.js` holds a
/// regex literal containing a bare `"` (`/[^"\\{}]/`, the `string_literal`
/// rule) and another containing a bare `/` inside a character class
/// (`/[^*]*\\*+([^/*][^*]*\\*+)*/`); a naive quote-aware scan desynchronises on
/// the first and stays wrong for the rest of the file. That is not
/// hypothetical — it is what this scanner did on its first run, and the
/// unbalanced-`repeat(` panic below is what caught it.
///
/// String literals keep their two delimiters so [`contains_string_literal`] can
/// still see that a separator token was present, without re-parsing it.
/// Comments and regex literals are blanked whole: neither is ever a separator.
/// Newlines are preserved everywhere so line numbering survives.
fn mask_noncode(grammar: &str) -> String {
    let src = grammar.as_bytes();
    let mut out = vec![b' '; src.len()];
    let mut i = 0usize;

    // Copy `src[i]` through and advance; used for every byte that is real code.
    macro_rules! keep {
        () => {{
            out[i] = src[i];
            i += 1;
        }};
    }
    // Blank `src[i]`, but never a newline — line numbering must survive.
    macro_rules! blank {
        () => {{
            if src[i] == b'\n' {
                out[i] = b'\n';
            }
            i += 1;
        }};
    }

    while i < src.len() {
        match src[i] {
            b'/' if src.get(i + 1) == Some(&b'/') => {
                while i < src.len() && src[i] != b'\n' {
                    blank!();
                }
            }
            b'/' if src.get(i + 1) == Some(&b'*') => {
                blank!();
                blank!();
                while i < src.len() && !(src[i] == b'*' && src.get(i + 1) == Some(&b'/')) {
                    blank!();
                }
                for _ in 0..2 {
                    if i < src.len() {
                        blank!();
                    }
                }
            }
            // Any remaining `/` in code position opens a regex literal:
            // `grammar.js` contains no division.
            b'/' => {
                blank!();
                let mut in_class = false;
                while i < src.len() {
                    match src[i] {
                        b'\\' => {
                            blank!();
                            if i < src.len() {
                                blank!();
                            }
                            continue;
                        }
                        b'[' => in_class = true,
                        b']' => in_class = false,
                        // A `/` inside `[...]` is content, not the terminator.
                        b'/' if !in_class => {
                            blank!();
                            break;
                        }
                        _ => {}
                    }
                    blank!();
                }
            }
            q @ (b'\'' | b'"') => {
                keep!();
                while i < src.len() && src[i] != q {
                    if src[i] == b'\\' {
                        blank!();
                        if i < src.len() {
                            blank!();
                        }
                        continue;
                    }
                    blank!();
                }
                if i < src.len() {
                    keep!();
                }
            }
            _ => keep!(),
        }
    }

    String::from_utf8(out).expect("masking only ever copies whole bytes or writes ASCII")
}

/// The balanced-parenthesis argument text of a call whose `(` ends at `open`
/// (a byte offset one past the paren), over [`mask_noncode`]'s output — where a
/// `'('` token has already been blanked and so cannot unbalance the scan.
fn balanced_arg(masked: &str, open: usize) -> Option<&str> {
    let bytes = masked.as_bytes();
    let mut depth = 1usize;
    for (i, &c) in bytes.iter().enumerate().skip(open) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&masked[open..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Does `arg` reference a grammar symbol (`$.name` / `$._name`)?
fn contains_symbol_reference(arg: &str) -> bool {
    arg.match_indices("$.").any(|(i, _)| {
        arg[i + 2..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    })
}

/// Does `arg` carry a string token — i.e. a separator?
///
/// `arg` comes from [`mask_noncode`], where a string literal is exactly its two
/// surviving delimiters, so the presence of a quote byte IS the presence of a
/// string token.
fn contains_string_literal(arg: &str) -> bool {
    arg.bytes().any(|c| c == b'\'' || c == b'"')
}

/// Every grammar-rule name defined in `grammar.js`'s `rules: {` block.
fn grammar_rule_names(masked: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_rules_block = false;
    for (_, line) in line_spans(masked) {
        if !in_rules_block {
            in_rules_block = line.trim_start() == "rules: {";
            continue;
        }
        if let Some(name) = rule_header(line) {
            out.push(name.to_string());
        }
    }
    out
}

#[test]
fn member_list_containers_cover_every_member_repeat_in_the_grammar() {
    let path = workspace_root().join("tree-sitter-reify/grammar.js");
    let grammar = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let masked = mask_noncode(&grammar);
    assert_eq!(
        masked.len(),
        grammar.len(),
        "masking must be length-preserving or every offset below is wrong"
    );

    let sites = member_list_body_repeats(&masked);

    // Non-vacuity: a scanner that silently matches nothing would make every
    // assertion below pass while vouching for nothing at all.
    assert!(
        sites.len() >= 15,
        "the grammar scan found only {} member-list repeats — it is broken, not \
         the grammar. Sites: {sites:?}",
        sites.len()
    );
    assert!(
        sites.iter().any(|(_, r)| r == "structure_definition"),
        "the scan missed `structure_definition`, the canonical member-list body \
         (grammar.js:512). Sites: {sites:?}"
    );

    let covered = reify_syntax::member_continuation::MEMBER_LIST_CONTAINERS;
    let excluded = reify_syntax::member_continuation::MEMBER_LIST_CONTAINER_EXCLUSIONS;

    // (i) every member-list body in the grammar is covered or excluded.
    let uncovered: Vec<_> = sites
        .iter()
        .filter(|(_, rule)| {
            !covered.contains(&rule.as_str()) && !excluded.iter().any(|(r, _)| r == rule)
        })
        .collect();
    assert!(
        uncovered.is_empty(),
        "grammar.js has member-list body repeats that the member-continuation \
         check neither covers nor deliberately excludes, so INV-SF-7 is \
         unenforced there: {uncovered:?}. Add each rule to \
         `MEMBER_LIST_CONTAINERS` (with a must-error test), or to \
         `MEMBER_LIST_CONTAINER_EXCLUSIONS` with the reason it cannot join."
    );

    // (ii) every exclusion states why.
    let unreasoned: Vec<_> = excluded
        .iter()
        .filter(|(_, reason)| reason.trim().is_empty())
        .map(|(rule, _)| *rule)
        .collect();
    assert!(
        unreasoned.is_empty(),
        "these exclusions carry no reason, so nobody can tell a decision from an \
         oversight: {unreasoned:?}"
    );

    // (iii) no dead entries: every name on either list is a real grammar rule.
    let rules = grammar_rule_names(&masked);
    let dead: Vec<_> = covered
        .iter()
        .copied()
        .chain(excluded.iter().map(|(r, _)| *r))
        .filter(|name| !rules.iter().any(|r| r == name))
        .collect();
    assert!(
        dead.is_empty(),
        "these names are not grammar rules in grammar.js — the grammar renamed or \
         removed them and the tables rotted: {dead:?}"
    );

    // (iv) the two tables must not contradict each other.
    let both: Vec<_> = excluded
        .iter()
        .map(|(r, _)| *r)
        .filter(|r| covered.contains(r))
        .collect();
    assert!(
        both.is_empty(),
        "these rules are both covered and excluded, which is incoherent: {both:?}"
    );
}
