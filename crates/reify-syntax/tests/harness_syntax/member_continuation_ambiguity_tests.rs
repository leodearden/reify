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

/// REPRO 1 from task #7094: leading-operator continuation inside a structure
/// member body.
const REPRO_ONE: &str = "structure S {\n  let d = 5mm\n  - 3mm\n}\n";

/// REPRO 2 from task #7094: `(`-led continuation inside a structure member
/// body.
const REPRO_TWO: &str = "structure S {\n  let x = a.b\n  (c)\n}\n";

/// Does this diagnostic message identify a member-continuation ambiguity?
///
/// Kept as one predicate so every test in this file agrees on what counts,
/// and so a wording change lands in exactly one place.
fn is_member_continuation_error(message: &str) -> bool {
    message.contains("continuation") && message.contains("member")
}

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
/// break — ~28 such sites exist in tracked reify source. Indentation past the
/// member column is the author's signal, and it stays legal.
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
