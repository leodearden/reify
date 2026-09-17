//! Locating the real fault inside a tree-sitter `ERROR` node.
//!
//! INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
//! tree-sitter recovery answers "something below here is broken" with a single `ERROR` node
//! whose span is the whole collapsed construct — often several sibling declarations. A
//! diagnostic built from that node names a blob, not a defect: it spans text the author did
//! not write wrong, and its position lands on a line that is merely downstream of the break.
//!
//! This module turns one such node into the facts a diagnostic needs, and nothing else:
//!
//! - [`faults_strictly_inside`] — the INNERMOST fault nodes in the subtree, in source order,
//!   so the report is anchored to the narrow break rather than the enclosing blob.
//! - [`LetAnchor`] / [`collect_let_anchors`] — every `let` keyword in the subtree, each
//!   classified as a function-body binding (the only kind the grammar requires a `;` after) by
//!   brace scope rather than by a latched "an `fn` appeared earlier" flag.
//! - [`last_let_anchor_before`] — the nearest preceding anchor for a given fault, by binary
//!   search over that source-ordered table.
//! - [`MAX_DIAGNOSTICS`] — the per-`ERROR`-node budget those reports are spent against.
//!
//! Everything here takes a bare `tree_sitter::Node` and reads no lowering state, so the
//! policy that consumes it — which fault earns which message, and how the budget is spent —
//! stays with `Lowering::diagnose_error_node`, its sole caller.

/// Upper bound on diagnostics from a single `ERROR` node, so a badly broken file cannot bury
/// its first real error under recovery noise. Truncation is always announced — never silent.
///
/// Spent by [`Lowering::diagnose_error_node`](super::Lowering::diagnose_error_node), which
/// gives located reports first claim on it. Kept `pub(super)`, the narrowest scope both users
/// share; the integration corpus in `tests/harness_syntax/fn_body_separator_ambiguity_tests.rs`
/// still mirrors the value, because `ts_parser` is a private module of this crate and an
/// integration test can name nothing inside it.
pub(super) const MAX_DIAGNOSTICS: usize = 8;

/// Collect every INNERMOST fault (`ERROR` / `MISSING`) node strictly inside `node`'s subtree,
/// in source order.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392:
/// tree-sitter's recovery routinely collapses several independently-broken declarations into a
/// SINGLE enclosing `ERROR` node. Measured on two sibling fns that each omit their `;`, the
/// whole file becomes one `(ERROR [0,0]-[7,1])` whose second declaration — keyword, params,
/// its own `let`, and its own fault — is nested one level deeper inside an
/// `(ERROR [51..91])`. One collapsed node is therefore not one fault, and reporting only the
/// outermost leaves the later declaration's break entirely undiagnosed.
///
/// "Innermost" is what makes the result usable: an enclosing `ERROR`'s span is precisely the
/// whole-declaration blob position these diagnostics exist to get away from, so a fault is
/// recorded only once the walk can descend no further. Equivalently, the walk descends
/// wherever `has_error()` reports something broken below — INCLUDING through fault nodes,
/// since recovery debris can contain an entire further declaration — and records a fault only
/// at the point of no further descent. Clean subtrees are pruned in O(1) by the same
/// `has_error()` test, so the cost is proportional to the broken part of the tree.
///
/// Descending through a fault node is speculative, because `has_error()` is true for a fault
/// node itself and says nothing about its children: recovery routinely produces an `ERROR`
/// whose children are all well-formed recovered tokens (measured on
/// `structure S {\n  let a = 1 1\n}`, which yields
/// `(ERROR [1,12]-[1,13] (number_literal [1,12]-[1,13]))`). "Innermost" must therefore be
/// decided on the way BACK UP: a fault node below which the walk found nothing broken is
/// itself the innermost fault and is recorded then. Testing `is_error()` only at the point of
/// no further descent would skip every such node, and where it was the ONLY fault the caller
/// would fall back to the whole enclosing node — reinstating the blob span.
///
/// Iterative, matching the tree-walk pattern used elsewhere in this file: recursion here would
/// be bounded only by CST depth.
///
/// Returns an empty vector when no fault exists strictly below `node` — including the common
/// case where `node` is itself an `ERROR` leaf.
pub(super) fn faults_strictly_inside(node: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return out;
    }
    // Fault nodes the walk has descended INTO, each paired with `out.len()` at that moment.
    // On the ascent an unchanged length proves nothing broken lies below, making the node
    // itself innermost. Depth-bounded, and only fault nodes are ever pushed.
    let mut descended_faults: Vec<(tree_sitter::Node<'_>, usize)> = Vec::new();
    loop {
        let cur = cursor.node();
        let cur_is_fault = cur.is_error() || cur.is_missing();
        if cur.has_error() && cursor.goto_first_child() {
            // Something MAY be broken deeper down — keep narrowing.
            if cur_is_fault {
                descended_faults.push((cur, out.len()));
            }
            continue;
        }
        if cur_is_fault {
            out.push(cur); // innermost: no children to descend into
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return out;
            }
            let parent = cursor.node();
            if let Some(&(fault, mark)) = descended_faults.last()
                && fault == parent
            {
                descended_faults.pop();
                if out.len() == mark {
                    // Descended into a fault and found nothing broken below it: this node
                    // IS the innermost fault. Source order holds — every entry recorded
                    // after `mark` would have come from inside it, and there are none.
                    out.push(parent);
                }
            }
            if parent == node {
                return out;
            }
        }
    }
}

/// One anonymous `let` keyword found inside an `ERROR` subtree, with the two facts a
/// separator diagnostic needs about it.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
#[derive(Clone, Copy)]
pub(super) struct LetAnchor {
    /// Byte offset of the `let` keyword — the span start of the diagnostic it anchors.
    pub(super) start_byte: usize,
    /// Row of the `let` keyword. A missing `;` fuses two LINES, so a fault on the same row as
    /// its `let` is not a separator omission (see [`Lowering::diagnose_error_node`]).
    pub(super) row: usize,
    /// Is this `let` a FUNCTION-BODY binding — the only kind the grammar requires a `;` after?
    ///
    /// A structure/module member `let` is newline-separated and takes no `;`, so anchoring
    /// "missing ';' after `let` binding in function body" to one would advise an edit the
    /// grammar rejects, about a construct that is not a function body. Recovery flattens both
    /// kinds to bare tokens under an `ERROR`, so the classification is: parent kind when the
    /// binding survived intact, otherwise whether this `let` sits inside the still-open braces
    /// of a preceding `fn` keyword within the same debris — see [`collect_let_anchors`], which
    /// tracks that scope rather than latching a "an `fn` appeared somewhere earlier" flag.
    pub(super) in_fn_body: bool,
}

/// Collect every anonymous `let` keyword inside `node`'s subtree, in source order, each
/// classified by [`LetAnchor::in_fn_body`].
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
/// When a missing `;` makes tree-sitter fuse a `let` RHS with the following statement, the
/// recovered fault node sits on the ABSORBED line — one row after the line that actually
/// needs editing. The `let` keyword is the absorbing line's anchor, so it, not the fault, is
/// what a diagnostic must point at.
///
/// Collected ONCE per `ERROR` node rather than re-walked per fault. The subtree cannot be
/// pruned by `has_error()` here (a `let` keyword usually sits in a perfectly clean part of the
/// debris), so a per-fault walk costs O(faults × nodes); on a badly broken file that is the
/// whole point of the bound, and `reify_syntax::parse` runs on the LSP's per-keystroke path
/// where a transiently malformed large file is the normal state. One pass plus a binary search
/// per fault (see [`Lowering::diagnose_error_node`]) makes it O(nodes + faults × log n).
///
/// The search is confined to `node`'s subtree, so an unrelated well-formed `let` elsewhere in
/// the file can never be blamed for a fault it does not enclose. Pre-order DFS visits nodes in
/// source order, so the result is sorted by `start_byte` and binary-searchable.
///
/// The [`LetAnchor::in_fn_body`] fallback is BRACE-SCOPED, not latched. Recovery routinely
/// collapses a function AND the structure members that follow it into one `ERROR`; measured on
/// `structure T {\n  fn g() -> Int { let y = (1 }\n  let a = 3\n}`, tree-sitter emits a single
/// `ERROR` holding `fn … { let y … }` followed by the member `let a` as bare tokens. A
/// monotonic "an `fn` keyword was seen" flag classifies that member `let` as a function-body
/// binding, which is exactly the misclassification `in_fn_body` exists to prevent — and the
/// `fault.row > let.row` guard in [`Lowering::diagnose_error_node`] does not close it, because
/// a member `let` whose RHS ran onto the next line satisfies that too. So the walk tracks `{`
/// / `}` nesting and treats a `let` as a function-body binding only while the braces opened
/// after the most recent `fn` keyword are still open.
pub(super) fn collect_let_anchors(node: tree_sitter::Node<'_>) -> Vec<LetAnchor> {
    let mut out: Vec<LetAnchor> = Vec::new();
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return out;
    }
    // Brace nesting relative to `node`, and the depth at which the most recent `fn` keyword
    // appeared. Together they answer "is this `let` inside a function body?" for a `let` whose
    // own binding node recovery destroyed: `fn_scope` is armed by an `fn` keyword and DISARMED
    // by the `}` that closes the body it opened, so a member `let` following a collapsed
    // function is not swept up by it.
    let mut brace_depth: usize = 0;
    let mut fn_scope: Option<usize> = None;
    loop {
        let cur = cursor.node();
        if !cur.is_named() {
            match cur.kind() {
                "fn" => fn_scope = Some(brace_depth),
                "{" => brace_depth += 1,
                "}" => {
                    // Saturating: an `ERROR` can begin mid-body, so the first `}` in the
                    // subtree may have no `{` to match.
                    brace_depth = brace_depth.saturating_sub(1);
                    if fn_scope.is_some_and(|d| brace_depth <= d) {
                        fn_scope = None;
                    }
                }
                "let" => {
                    let in_fn_body = match cur.parent().map(|p| p.kind()) {
                        // The binding survived recovery intact: its kind is decisive.
                        Some("fn_let_binding") => true,
                        Some("let_declaration") => false,
                        // Recovery debris — fall back to positional evidence. The `let` must be
                        // strictly INSIDE the braces opened after the `fn` keyword: a header so
                        // mangled that no `{` survived (`fn g(` followed by member `let`s) is
                        // not evidence of a function body.
                        _ => fn_scope.is_some_and(|d| brace_depth > d),
                    };
                    out.push(LetAnchor {
                        start_byte: cur.start_byte(),
                        row: cur.start_position().row,
                        in_fn_body,
                    });
                }
                _ => {}
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() || cursor.node() == node {
                return out;
            }
        }
    }
}

/// The LAST anchor in `anchors` starting strictly before byte offset `before`, or `None`.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
/// `anchors` is in source order (see [`collect_let_anchors`]), so this is a binary search.
/// Deliberately returns the nearest preceding `let` WHATEVER its classification, leaving the
/// `in_fn_body` decision to the caller: skipping a non-fn `let` to blame an earlier fn one
/// would report a fault against a `let` that does not even enclose it.
pub(super) fn last_let_anchor_before(anchors: &[LetAnchor], before: usize) -> Option<LetAnchor> {
    let idx = anchors.partition_point(|a| a.start_byte < before);
    idx.checked_sub(1).map(|i| anchors[i])
}

/// Parse `source` and hand the root node to `f`. Deliberately tolerates a broken parse:
/// every caller feeds malformed source on purpose.
///
/// Test-only, and deliberately at module scope rather than inside [`mod tests`](self::tests):
/// the `snippet` tests in the parent module need the same parse-and-hand-me-the-root step, and
/// a second copy there would be two helpers to keep in agreement.
#[cfg(test)]
pub(super) fn with_root<R>(source: &str, f: impl FnOnce(tree_sitter::Node) -> R) -> R {
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    let tree = ts_parser.parse(source, None).expect("Failed to parse");
    f(tree.root_node())
}

/// Unit coverage for the fault-location primitives behind the fn-body separator diagnostics.
///
/// INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392.
/// The properties pinned here — "innermost" fault selection, fn-vs-member `let`
/// classification, nearest-preceding-anchor search — are contracts of the helpers themselves,
/// not of any one malformed fixture, so they are asserted directly rather than through
/// `parse`. The end-to-end behaviour they add up to is pinned separately, by the corpus in
/// `tests/harness_syntax/fn_body_separator_ambiguity_tests.rs`.
#[cfg(test)]
mod tests {
    use super::*;

    /// An `ERROR` node whose children are all well-formed is itself the innermost fault.
    ///
    /// INV-SF-7, task #5392. Measured shape: `structure S {\n  let a = 1 1\n}` yields
    /// `(ERROR [1,12]-[1,13] (number_literal [1,12]-[1,13]))` — an `ERROR` whose only child is
    /// a clean recovered token. `has_error()` is true for the `ERROR` itself and says nothing
    /// about its children, so a walk that tests for a fault only where it can descend no
    /// further walks straight past this node and records nothing. With no fault recorded,
    /// `diagnose_error_node` falls back to the whole enclosing node — the blob span this
    /// class of diagnostic exists to eliminate.
    #[test]
    fn faults_strictly_inside_records_an_error_whose_children_are_clean() {
        let source = "structure S {\n  let a = 1 1\n}\n";
        // The stray trailing literal, located by search rather than a hard-coded offset.
        let stray = source.rfind('1').expect("fixture must contain a stray '1'");

        with_root(source, |root| {
            let faults = faults_strictly_inside(root);
            assert_eq!(
                faults.len(),
                1,
                "expected exactly the one ERROR-with-clean-children to be recorded, got {:?}",
                faults
                    .iter()
                    .map(|f| (f.kind(), f.start_byte(), f.end_byte()))
                    .collect::<Vec<_>>(),
            );
            let fault = faults[0];
            assert!(
                fault.is_error(),
                "recorded fault should be the ERROR node, got kind {:?}",
                fault.kind(),
            );
            assert_eq!(
                (fault.start_byte(), fault.end_byte()),
                (stray, stray + 1),
                "recorded fault must be the narrow ERROR around the stray literal, not the \
                 enclosing declaration",
            );
        });
    }

    /// The same walk still narrows to the DEEPEST fault when one exists, and never records an
    /// enclosing fault that has a broken descendant.
    ///
    /// INV-SF-7, task #5392. The t9 shape collapses the WHOLE file into one top-level `ERROR`.
    /// Measured inside it: `g`'s break is an `(ERROR (ERROR))` pair at the absorbed token,
    /// while `h`'s entire declaration becomes a nested `ERROR [51..91]` whose header tokens are
    /// each wrapped in their own leaf `ERROR` — seven innermost faults in total. Both
    /// properties asserted here are what make that usable: no recorded fault is the enclosing
    /// blob, and each broken declaration contributes at least one, so a caller that reported
    /// only the first would leave `h` entirely undiagnosed.
    #[test]
    fn faults_strictly_inside_prefers_the_innermost_fault() {
        let source = "fn g(i: Int) -> Real {\n  let a = 2\n  a * sgn(i, 0)\n}\nfn h(i: Int) -> Real {\n  let b = 3\n  b + 1\n}\n";
        let fn_h = source.find("fn h(").expect("fixture must contain 'fn h('");

        with_root(source, |root| {
            let top: Vec<_> = {
                let mut cursor = root.walk();
                root.children(&mut cursor)
                    .filter(|c| c.is_error())
                    .collect()
            };
            assert_eq!(
                top.len(),
                1,
                "fixture should collapse into ONE top-level ERROR"
            );
            let faults = faults_strictly_inside(top[0]);
            let rendered: Vec<_> = faults
                .iter()
                .map(|f| (f.kind(), f.start_byte(), f.end_byte()))
                .collect();

            for fault in &faults {
                assert!(
                    faults_strictly_inside(*fault).is_empty(),
                    "recorded fault {:?} still has a broken descendant, so it is not innermost",
                    (fault.kind(), fault.start_byte(), fault.end_byte()),
                );
                assert!(
                    fault.end_byte() - fault.start_byte() < source.len(),
                    "recorded fault {:?} spans the whole file — the blob span again",
                    (fault.kind(), fault.start_byte(), fault.end_byte()),
                );
            }

            // One collapsed ERROR node is not one fault: BOTH declarations are represented.
            assert!(
                faults.iter().any(|f| f.start_byte() < fn_h),
                "no fault recorded inside `g` (bytes 0..{fn_h}); got {rendered:?}",
            );
            assert!(
                faults.iter().any(|f| f.start_byte() >= fn_h),
                "no fault recorded inside `h` (bytes {fn_h}..); got {rendered:?}",
            );
        });
    }

    /// A structure-member `let` must NOT be classified as a function-body binding.
    ///
    /// INV-SF-7, task #5392. Only `fn_let_binding` requires a `;`; a structure member `let` is
    /// newline-separated. Anchoring "missing ';' after `let` binding in function body" to a
    /// member `let` would name a construct that is not a function body and demand an edit the
    /// grammar rejects.
    #[test]
    fn let_anchors_distinguish_fn_bindings_from_member_declarations() {
        with_root("structure S {\n  let a = 1\n}\n", |root| {
            let anchors = collect_let_anchors(root);
            assert_eq!(anchors.len(), 1, "fixture has exactly one `let`");
            assert!(
                !anchors[0].in_fn_body,
                "a structure member `let` is not a function-body binding",
            );
        });

        with_root(
            "fn f(x: Int) -> Int {\n  let y = 2;\n  y + x\n}\n",
            |root| {
                let anchors = collect_let_anchors(root);
                assert_eq!(anchors.len(), 1, "fixture has exactly one `let`");
                assert!(
                    anchors[0].in_fn_body,
                    "a `fn_let_binding`'s `let` IS a function-body binding",
                );
            },
        );

        // Recovery debris: the collapse destroys `fn_let_binding`, so the classification falls
        // back to "this `let` is inside the still-open braces of a preceding `fn` keyword".
        with_root(
            "fn g(i: Int) -> Real {\n  let a = 2\n  a * b\n}\n",
            |root| {
                let anchors = collect_let_anchors(root);
                assert_eq!(anchors.len(), 1, "fixture has exactly one `let`");
                assert!(
                    anchors[0].in_fn_body,
                    "a `let` inside the wreckage of a function header is a function-body binding",
                );
            },
        );

        // The only genuinely risky combination, and the one the fallback used to get wrong: a
        // MEMBER `let` in debris that FOLLOWS an `fn` keyword. A latched "an `fn` was seen"
        // flag classifies it as a function-body binding and advises a `;` the grammar rejects.
        //
        // Fixture measured on tree-sitter-reify: the unterminated `(` in `let y = (1` collapses
        // the function AND the member `let a` that follows it into ONE `ERROR` whose children
        // are bare tokens — `fn` `g` `(` `)` `->` Int `{` `let` `y` `=` `(` 1 `}` `let` `a` `=`
        // 3 — so neither `let` has a `fn_let_binding` / `let_declaration` parent to be decided
        // by, and both fall to the fallback. Only the brace scope separates them: `let y` is
        // inside the `{`, `let a` is after the `}` that closed it.
        with_root(
            "structure T {\n  fn g() -> Int { let y = (1 }\n  let a = 3\n}\n",
            |root| {
                let anchors = collect_let_anchors(root);
                assert_eq!(
                    anchors.len(),
                    2,
                    "fixture has exactly two `let`s (one fn-body, one structure member)",
                );
                assert!(
                    anchors[0].in_fn_body,
                    "`let y` sits inside the function's braces and IS a function-body binding",
                );
                assert!(
                    !anchors[1].in_fn_body,
                    "`let a` is a structure member that merely FOLLOWS a collapsed function; \
                     classifying it as a function-body binding would emit \"missing ';' after \
                     `let` binding in function body\" against a construct that is not a \
                     function body, advising an edit the grammar rejects",
                );
            },
        );
    }

    /// `last_let_anchor_before` returns the NEAREST preceding anchor, whatever its class.
    #[test]
    fn last_let_anchor_before_picks_the_nearest_preceding_anchor() {
        let anchors = [
            LetAnchor {
                start_byte: 10,
                row: 1,
                in_fn_body: true,
            },
            LetAnchor {
                start_byte: 30,
                row: 3,
                in_fn_body: false,
            },
        ];
        assert!(
            last_let_anchor_before(&anchors, 10).is_none(),
            "strictly before"
        );
        assert_eq!(
            last_let_anchor_before(&anchors, 11).map(|a| a.start_byte),
            Some(10)
        );
        assert_eq!(
            last_let_anchor_before(&anchors, 40).map(|a| a.start_byte),
            Some(30)
        );
        assert!(
            last_let_anchor_before(&anchors, 40).is_some_and(|a| !a.in_fn_body),
            "the nearest anchor is returned even when it is a member `let`; skipping it to \
             blame the earlier fn `let` would report a fault against a binding that does not \
             enclose it",
        );
        assert!(last_let_anchor_before(&[], 5).is_none());
    }
}
