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
//! Applied only to a tree that parsed cleanly (see "Only clean parses"
//! below), for each *member* `M` — a direct named child of a member-list
//! container, lying between the container's `{` and `}`:
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
//! `prj/printer_v01/printer.ri`,
//! `docs/prds/v0_6/fixtures/discrete_balance_*.ri`). The standing sweep, not a
//! count written here, is what keeps that constituency intact.
//!
//! Clause 3's inspect-then-apply order decides the two cases that matter:
//!
//! - A row-leading `(` — REPRO 2 — is inspected at `d == 0`, because its own
//!   increment has not applied yet, so it IS reported. It has to be: a `(`
//!   opening a line is precisely how a call silently swallows the next line.
//! - A row-leading `)` or `}` that closes a group `M` itself opened is
//!   inspected while still inside that group, i.e. at `d >= 1`, so it is
//!   skipped. This is the everyday layout of a multi-row argument list or a
//!   `where cond { … }` block, and it accounts for every location the
//!   depth-free rule reported across tracked `.ri` source. The standing sweep
//!   `no_tracked_ri_source_trips_the_member_continuation_check`
//!   (`crates/reify-syntax/tests/harness_syntax/member_continuation_ambiguity_tests.rs`)
//!   is the live check on that claim; it re-measures on every run, which a
//!   number written here would not.
//!
//! # Only clean parses
//!
//! The check runs only when the tree holds no `ERROR` or `MISSING` node
//! anywhere. INV-SF-7 is about the SILENT join, and silence is precisely what
//! a clean parse means: once the grammar has spoken, the invariant is already
//! upheld and a second diagnostic can only mislead.
//!
//! It WOULD mislead, measured two different ways:
//!
//! - The offending member is itself broken (`let a = 5mm @@` absorbs the
//!   following `- 3mm` row), so the rule advises indenting a line whose real
//!   defect is the garbage above it.
//! - The offending member is CLEAN and something adjacent is broken. An
//!   incomplete `sub a :` takes the next line's `let` as its type name,
//!   leaving an error-free `sub_declaration` and an `ERROR` sibling — a
//!   per-member cleanliness test would let that one through, which is why the
//!   gate is the whole tree, not the member.
//!
//! This is also what keeps the check off the GUI's parse-while-typing path,
//! where half-written source is the normal state. Pinned by
//! `a_broken_parse_gets_no_continuation_diagnostic`.
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
//!
//! # Severity: a hard failure on every entry path (measured, #7094 step-10)
//!
//! These entries go into `ParsedModule::errors`, and INV-SF-7 forbids the
//! quiet pick — so "reported" is not enough, the compile has to STOP. Every
//! path that compiles a file a user named does stop, and none of them was
//! changed by #7094:
//!
//! - `reify_compiler::module_dag` — the project/import entry. Both
//!   `ModuleDag::compile_module` and `compile_project_with_entry_source_cfg`
//!   map a non-empty `parsed.errors` to `Diagnostic::error` and return `Err`
//!   before any compilation phase runs. Pinned by
//!   `crates/reify-compiler/tests/harness_langcore/member_continuation_hard_error_tests.rs`,
//!   which asserts `Severity::Error` explicitly so a downgrade cannot pass.
//! - `reify-cli`'s single-file `parse_and_compile` — prints each
//!   `parsed.errors` entry and returns `ExitCode::FAILURE` before it calls
//!   `compile_with_stdlib_checked`.
//!
//! `compile_builder::pre_pass::forward_parse_errors` DOES downgrade every
//! `ParseError` to a `Diagnostic::warning` (deliberately: it runs after a
//! best-effort parse, and its blanket behaviour is not #7094's to change).
//! That downgrade is only reachable when a caller hands a `ParsedModule`
//! straight to `compile_with_prelude_*` without checking `errors` first,
//! which is what the test helpers do — both entry paths above pre-check, so
//! no member-continuation entry reaches it on a user-facing compile. If a
//! future production caller skips that pre-check, the fix is a structural
//! marker on the entry (not a message-prefix match, which would be a
//! substring hack under INV-SF-6). The tests that must single this diagnostic
//! out of a mixed list do match on text, but through exactly one exported
//! predicate — [`is_member_continuation_message`] — so that marker, when it
//! arrives, replaces one function body rather than N scattered `contains`
//! calls.

use reify_core::SourceSpan;

/// Member-list container node kinds this check covers.
///
/// Derived from the member-repeat sites in `tree-sitter-reify/grammar.js` —
/// every rule with a brace-delimited `repeat(...)` body whose items are
/// members. Each entry names the grammar rule and the repeat it owns.
///
/// Public so the contract is readable and checkable from outside the crate:
/// this list plus [`MEMBER_LIST_CONTAINER_EXCLUSIONS`] is a total accounting of
/// the grammar's member-list bodies, and the two together are what the
/// grammar-drift guard verifies.
///
/// TRIPWIRE: `grammar.js` has no pointer back to this list, so a member-list
/// body added there is covered only if someone also adds it HERE. Dropping an
/// entry, or adding a grammar rule without adding it, silently un-enforces
/// INV-SF-7 at that site — every member-continuation test keeps passing,
/// because the container is simply never visited. Both directions are caught
/// by:
///   - `member_list_containers_cover_every_member_repeat_in_the_grammar`
///     (`crates/reify-syntax/tests/harness_syntax/member_continuation_ambiguity_tests.rs`),
///     which re-derives the sites from `grammar.js` and requires each to be
///     covered here or excluded below with a reason.
///   - a per-container test in the same file for EVERY entry below, which
///     catches a removed one. Each is a must-error fixture, except the two
///     containers where no join can form: `keyed_member_block` (every entry
///     ends in `}`) and `match_arm_decl_block` (`,`-separated arms with no
///     expression tail) instead pin that fact as a negative — and
///     `keyed_member_block`'s also asserts a row-split entry IS reported, so
///     "no diagnostic" cannot be confused with "never visited".
pub const MEMBER_LIST_CONTAINERS: &[&str] = &[
    // repeat($._member) — grammar.js:512
    "structure_definition",
    // repeat($._member) — grammar.js:525
    "occurrence_definition",
    // repeat($.trait_member) — grammar.js:291
    "trait_declaration",
    // repeat($.purpose_member) — grammar.js:375
    "purpose_declaration",
    // repeat($._constraint_def_body_item) — grammar.js:407
    "constraint_definition",
    // repeat($._guard_member), twice (the `where` body and the `else` body)
    // — grammar.js:598, :600
    "guarded_block",
    // repeat($.relation_member) — grammar.js:727
    "relate_block",
    // repeat($.relation_member) — grammar.js:748
    "sub_relate_block",
    // repeat($.relation_member) — grammar.js:800
    "joint_body",
    // repeat(choice($.param_assignment, $._member)) — grammar.js:945
    "specialization_body",
    // repeat1($.keyed_member_entry) — grammar.js:970
    "keyed_member_block",
    // repeat(choice($.param_declaration, $.let_declaration, …)) — grammar.js:1015.
    // A port body holds full `let` members, so it carries REPRO 1 verbatim:
    // `port p : in Flow { let x = 5mm ⏎ - 3mm }` parses as one joined
    // `binary_expression` (measured with `tree-sitter parse`).
    "port_body",
    // repeat($.field_config_entry) — grammar.js:341.
    // `field_config_entry` is `key = <expression>` (grammar.js:364-368), so its
    // trailing expression absorbs the next entry's line exactly as a `let` does:
    // `sampled { resolution = 5mm ⏎ - 3mm }` joins (measured).
    "field_source_sampled",
    // repeat($.field_config_entry) — grammar.js:361. Same body shape and same
    // join as `field_source_sampled`; a separate grammar rule, so a separate
    // entry (measured).
    "field_source_imported",
    // seq($.match_arm_decl_arm, repeat(seq(',', $.match_arm_decl_arm)), …)
    // — grammar.js:1404. Carried deliberately even though no arm shape can
    // trip the rule today: arms are `,`-separated and `match_arm_sub_decl`
    // ends in a plain identifier with no expression tail, so the grammar
    // already rejects the join with an ERROR node (measured). grammar.js
    // (~line 1415) defers the arm-body form `sub name : T { … }` to task
    // #3569; carrying the container now means that widening arrives covered.
    // Pinned by `match_arm_decl_block_rejects_the_join_at_the_grammar_level_already`.
    "match_arm_decl_block",
    // repeat(choice($.derived_param_assignment, $.keep_disposition,
    // $.exclude_disposition, $.let_declaration, $.constraint_declaration))
    // — grammar.js:1054. The derived-sub body added by task #6615, which landed
    // on main after this list was first written — the TRIPWIRE above firing for
    // real rather than in theory. It admits full `let` members, so it carries
    // REPRO 1 verbatim: `sub b = mirror of a across P { let x = 5mm ⏎ - 3mm }`
    // joins into one `binary_expression` (measured).
    "derived_body",
];

/// Grammar rules that own a separator-free `repeat(...)` body which this check
/// deliberately does **not** cover — `(rule_name, reason)`.
///
/// The grammar-drift guard treats [`MEMBER_LIST_CONTAINERS`] and this table as
/// a partition: every separator-free repeat in `grammar.js` must appear in one
/// or the other. That is the point of writing the exclusions down rather than
/// letting the guard's scan quietly skip them — an omission and a decision look
/// identical from the outside, and only a stated reason tells them apart.
///
/// TRIPWIRE: an entry added here silences the drift guard for that rule
/// forever. Add one only when the join provably cannot form (or has no member
/// column to anchor against), and say which, so the next reader can re-check
/// the claim instead of inheriting it.
pub const MEMBER_LIST_CONTAINER_EXCLUSIONS: &[(&str, &str)] = &[
    (
        "source_file",
        "repeat($._declaration) — grammar.js:126. Not brace-delimited: a \
         top-level declaration list has no `{` and therefore no member column \
         for clause 5 to compare against, so the rule has no anchor here. A \
         top-level join is a different defect with a different fix and is not \
         #7094's seam.",
    ),
    (
        "fn_body",
        "repeat($.fn_let_binding) — grammar.js:241. The separator is real but \
         lives INSIDE the item rather than in the repeat: `fn_let_binding` ends \
         in a REQUIRED `;` (grammar.js:248-255), so consecutive bindings can \
         never be adjacent and no silent join can form. What CAN go wrong here \
         is a MISSING `;`, which is a parse error the grammar already demands — \
         anchoring that error to the right line is task #5392's seam, not this \
         one.",
    ),
    (
        "interpolated_string",
        "repeat(choice(alias($._string_content, $.string_chunk), \
         $.interpolation)) — grammar.js:1774. Not a member list at all: the \
         repeat runs over string CONTENT between `\"` delimiters. A chunk has \
         no expression tail to absorb a following line with, and there is no \
         member column, so clauses 1 and 5 are both undefined here.",
    ),
];

/// Scan `root` for member-continuation ambiguities.
///
/// Returns `(span, message)` pairs in source order, each span covering exactly
/// the offending row-leading token — never the whole member, which would bury
/// the boundary the author actually needs to see.
pub(crate) fn check_member_continuations(root: tree_sitter::Node<'_>) -> Vec<(SourceSpan, String)> {
    // A tree that did not parse cleanly has no member boundaries worth reading
    // (see "Only clean parses" above): under error recovery, spans and start
    // columns are the parser's guesses, not the author's layout.
    if root.has_error() {
        return Vec::new();
    }

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

/// The stable head of every member-continuation diagnostic, and the ONLY text
/// [`is_member_continuation_message`] matches on.
///
/// Written once, used by both the constructor and the recogniser, so the
/// wording and the discriminator cannot drift apart.
const MESSAGE_HEAD: &str = "ambiguous member continuation:";

/// The diagnostic wording.
///
/// Names BOTH readings and BOTH fixes: an author who meant a continuation and
/// an author who meant a new member each need to be told which edit expresses
/// their intent. Single-line by house norm — parse diagnostics do not echo
/// source blocks.
fn continuation_message(col: usize, c0: usize) -> String {
    format!(
        "{MESSAGE_HEAD} this line starts at column {col}, at or left of \
         the enclosing member's start column {c0}, but the grammar joins it onto that \
         member's expression rather than starting a new member; indent it past column \
         {c0} to continue the expression, or separate the members"
    )
}

/// Does `message` identify a member-continuation ambiguity?
///
/// THE discriminator, exported so every caller — in this crate and in
/// `reify-compiler`'s test suites — asks the same question of the same text.
/// Three independent substring predicates previously stood in for this, one
/// per test file; reword the diagnostic and each would have drifted separately
/// (SPOT).
///
/// This is still text matching, and text matching is the wrong shape: a
/// structured kind or code on `ParseError` would make the discriminator DATA.
/// Keeping the match here rather than at each call site is what makes that
/// later change a one-function edit.
///
/// `contains`, not `starts_with`, because a caller may read the message after
/// a renderer has prefixed it with a location (`reify_compiler::module_dag`
/// maps `ParsedModule::errors` into `Diagnostic`s that way).
pub fn is_member_continuation_message(message: &str) -> bool {
    message.contains(MESSAGE_HEAD)
}
