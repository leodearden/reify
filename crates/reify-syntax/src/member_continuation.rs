//! Member-continuation ambiguity check — INV-SF-7 `parse-is-value-faithful`
//! (`docs/legibility/design-invariants.md` §INV-SF-7), task #7094.
//!
//! # The problem
//!
//! `extras: [/\s/, ...]` (`tree-sitter-reify/grammar.js:88`) makes the grammar
//! wholly newline-insensitive, and every member-list body is a bare
//! `repeat(...)` with no separator token. A member whose tail is an expression
//! therefore greedily absorbs the start of the next line whenever that line
//! *can* continue it:
//!
//! ```text
//! structure S {
//!   let d = 5mm
//!   - 3mm          // joined: `d == 2mm`, silently, with zero diagnostics
//! }
//! ```
//!
//! INV-SF-7 forbids the quiet pick. This module reports the join; it does
//! **not** change which reading the grammar produces (the grammar is untouched
//! by #7094).
//!
//! # The rule (normative)
//!
//! For each *member* `M` — a direct named child of a member-list container,
//! lying between the container's `{` and `}`:
//!
//! 1. **`c0`** = `M.start_position().column`, the member's own start column.
//! 2. Walk `M`'s non-extra leaf tokens in source order. Extras (comments) are
//!    skipped entirely: the rule is about where a member's *code* resumes, and
//!    a comment can neither join an expression nor mask the token that does.
//! 3. **`d`** = bracket-nesting depth *relative to `M`* — 0 at `M`'s first
//!    token. Every leaf is inspected BEFORE its own bracket effect applies:
//!    `(`, `[` and `{` increment `d` after inspection, and `)`, `]` and `}`
//!    decrement `d` after inspection. Equivalently, `d` at leaf `t` counts the
//!    brackets opened strictly before `t` and not yet closed. This
//!    inspect-then-apply order is load-bearing in both directions (see below).
//! 4. A leaf `t` is **row-leading** when it is the first non-extra leaf of `M`
//!    on a row strictly greater than `M`'s start row.
//! 5. A row-leading `t` is **reported** when `d == 0` at the moment `t` is
//!    inspected AND `t.start_position().column <= c0`.
//!
//! Clause 5's column test says: the continuation begins at or to the LEFT of
//! the member it continues, which is exactly the shape a reader parses as a
//! new member. Indentation past `c0` is the author's signal that the line is a
//! deliberate continuation, and stays legal — that shape is real, tracked
//! reify source (`designs/litter_tray/bottom_deck.ri:65`,
//! `prj/printer_v01/printer.ri`, `docs/prds/v0_6/fixtures/discrete_balance_*.ri`,
//! ~28 sites).
//!
//! Clause 3's inspect-then-apply order decides the two cases that matter:
//!
//! - A row-leading `(` — REPRO 2 — is inspected at `d == 0`, because its own
//!   increment has not applied yet, so it IS reported. It has to be: a `(`
//!   opening a line is precisely how a call silently swallows the next line.
//! - A row-leading `)` or `}` that closes a group `M` itself opened is
//!   inspected while still inside that group, i.e. at `d >= 1`, so it is
//!   skipped. This is the everyday layout of a multi-row argument list or a
//!   `where cond { … }` block, and it accounts for all 255 locations the
//!   depth-free rule reported across the 677 tracked `.ri` files.
//!
//! NOTE for anyone reconciling this against #7094's plan text: the plan's
//! prose said closers decrement *before* inspection, but its own worked
//! examples require `d >= 1` for a closing `)`/`}` at the member column — and
//! "decrement before" yields `d == 0` there, reporting every such row. The
//! uniform inspect-then-apply order implemented here is what satisfies every
//! constituency the plan validated, including the repo-wide sweep.
//!
//! Because `d` is computed from real CST leaf tokens rather than raw bytes,
//! brackets inside string literals and comments cannot fool it: their contents
//! live inside `string_chunk` / `line_comment` / `block_comment` leaves, never
//! as bare bracket tokens.
//!
//! # Why post-parse and not a grammar change
//!
//! A newline/indent-sensitive external scanner token would have to re-derive
//! the 5.8 MB `parser.c` and put all 38 corpus files and 16 grammar-test
//! binaries at risk, to police a rule that is purely about layout. Keeping the
//! check here also keeps the merge surface against the pending 801-line
//! `ts_parser.rs` change on `task/5392` down to the single call site.

use reify_core::SourceSpan;

/// Member-list container node kinds this check covers.
///
/// Step-2 scope: `structure_definition` only. Widened in step-6.
const MEMBER_LIST_CONTAINERS: &[&str] = &["structure_definition"];

/// Scan `root` for member-continuation ambiguities.
///
/// Returns `(span, message)` pairs in source order, each span covering exactly
/// the offending row-leading token — never the whole member, which would bury
/// the boundary the author actually needs to see.
pub(crate) fn check_member_continuations(
    root: tree_sitter::Node<'_>,
    _source: &str,
) -> Vec<(SourceSpan, String)> {
    let mut out = Vec::new();

    // Iterative pre-order walk (no recursion: member bodies can nest
    // arbitrarily deep). Containers nest too — a `structure_definition` can
    // hold a `guarded_block` — so a match never prunes the descent.
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if MEMBER_LIST_CONTAINERS.contains(&node.kind()) {
            for member in members_of(node) {
                check_member(member, &mut out);
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }

    // The stack walk visits siblings in reverse; sort so diagnostics land in
    // source order regardless of traversal shape.
    out.sort_by_key(|(span, _)| (span.start, span.end));
    out
}

/// The members of a container: its direct named children lying strictly
/// between the container's `{` and `}` anonymous children.
///
/// Deliberately kind-agnostic — ANY named, non-extra child inside the braces
/// is a member. Enumerating member KINDS instead would silently drift as
/// `commonMembers()` in `grammar.js` grows.
fn members_of(container: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let mut cursor = container.walk();
    let children: Vec<_> = container.children(&mut cursor).collect();

    let open = children.iter().position(|c| c.kind() == "{");
    let close = children.iter().rposition(|c| c.kind() == "}");
    let (Some(open), Some(close)) = (open, close) else {
        // No brace-delimited body (e.g. a forward declaration): there is no
        // member column to anchor the rule to, so there is nothing to check.
        return Vec::new();
    };
    if close <= open {
        return Vec::new();
    }

    children[open + 1..close]
        .iter()
        .copied()
        .filter(|c| c.is_named() && !c.is_extra())
        .collect()
}

/// `M`'s leaf tokens in source order, extras excluded.
fn leaves_in_order(member: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let mut out = Vec::new();
    let mut stack = vec![member];
    while let Some(node) = stack.pop() {
        if node.is_extra() {
            continue;
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        if children.is_empty() {
            out.push(node);
        } else {
            for child in children.into_iter().rev() {
                stack.push(child);
            }
        }
    }
    out
}

/// Apply the rule to one member, appending any diagnostics to `out`.
///
/// Implements clauses 1–5 of the module-level contract; the numbered comments
/// below name the clause each line discharges.
fn check_member(member: tree_sitter::Node<'_>, out: &mut Vec<(SourceSpan, String)>) {
    // (1)
    let c0 = member.start_position().column;
    let start_row = member.start_position().row;
    let mut last_row = start_row;
    // (3) depth relative to M, so M's own first token sits at 0.
    let mut depth: i32 = 0;

    // (2)
    for leaf in leaves_in_order(member) {
        let row = leaf.start_position().row;
        // (4) first non-extra leaf of M on a row past M's start row.
        if row > last_row {
            last_row = row;
            let col = leaf.start_position().column;
            // (5) `depth` here is still pre-effect for THIS leaf: a row-leading
            // `(` reads 0 and is reported; a row-leading `)`/`}` closing a group
            // M opened reads >= 1 and is skipped.
            if depth == 0 && col <= c0 {
                out.push((
                    SourceSpan::new(leaf.start_byte() as u32, leaf.end_byte() as u32),
                    continuation_message(col, c0),
                ));
            }
        }

        // (3) apply this leaf's own bracket effect only after inspecting it.
        match leaf.kind() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            _ => {}
        }
    }
}

/// The diagnostic wording.
///
/// Names BOTH readings and BOTH fixes: an author who meant a continuation and
/// an author who meant a new member each need to be told which edit expresses
/// their intent. Single-line by house norm — parse diagnostics do not echo
/// source blocks.
fn continuation_message(col: usize, c0: usize) -> String {
    format!(
        "ambiguous member continuation: this line starts at column {col}, at or left of \
         the enclosing member's start column {c0}, but the grammar joins it onto that \
         member's expression rather than starting a new member; indent it past column \
         {c0} to continue the expression, or separate the members"
    )
}
