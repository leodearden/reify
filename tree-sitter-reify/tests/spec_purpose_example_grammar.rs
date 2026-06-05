//! Parse-guard tests for the purpose declaration examples in reify-language-spec.md.
//!
//! Task 4018 (η): reconcile §9.5 so it demonstrates multi-ref params, let bindings,
//! and guarded blocks. These tests guard the spec's §9.5 and §4.4 example fences by
//! extracting them directly from the live doc (include_str!) and parsing via the
//! real tree-sitter grammar — making them live guards on doc content.
//!
//! Test A `spec_9_5_purpose_example_parses_with_required_constructs` (RED driver):
//!   - Parses the §9.5 example fence with no ERROR nodes.
//!   - Asserts >=2 purpose_param + >=1 let_declaration + >=1 guarded_block nodes.
//!     RED before step-2 (old §9.5 example has 1 param, no let, no guarded block).
//!
//! Test B `spec_4_4_purpose_example_parses` (regression baseline):
//!   - Parses the §4.4 example fence (after "Example:" anchor) with no ERROR nodes.
//!     GREEN now; stays GREEN after step-3 replaces the §4.4 example.

use tree_sitter_reify::language;

const SPEC: &str = include_str!("../../docs/reify-language-spec.md");

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Count all nodes of `kind` in the subtree rooted at `node` (depth-first).
fn count_kind(node: tree_sitter::Node, kind: &str) -> usize {
    let mut count = if node.kind() == kind { 1 } else { 0 };
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            count += count_kind(cursor.node(), kind);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    count
}

/// Extract the first ``` ... ``` code fence appearing after `heading` (a `### …` line)
/// and before the next `### ` heading.
fn extract_section_fence(md: &str, heading: &str) -> String {
    let after = md
        .split_once(heading)
        .unwrap_or_else(|| panic!("heading {:?} not found in spec", heading))
        .1;
    // Trim to the section — stop at the next `### ` heading.
    let section = match after.split_once("\n### ") {
        Some((before_next, _)) => before_next,
        None => after,
    };
    extract_first_fence(section)
        .unwrap_or_else(|| panic!("no code fence found in section {:?}", heading))
}

/// Extract the first ``` ... ``` code fence appearing after `anchor` text within
/// the section beginning at `heading`.
fn extract_fence_after_anchor(md: &str, heading: &str, anchor: &str) -> String {
    let after_heading = md
        .split_once(heading)
        .unwrap_or_else(|| panic!("heading {:?} not found in spec", heading))
        .1;
    let section = match after_heading.split_once("\n### ") {
        Some((before_next, _)) => before_next,
        None => after_heading,
    };
    let after_anchor = section
        .split_once(anchor)
        .unwrap_or_else(|| panic!("anchor {:?} not found in section {:?}", anchor, heading))
        .1;
    extract_first_fence(after_anchor)
        .unwrap_or_else(|| panic!("no code fence found after {:?} in {:?}", anchor, heading))
}

/// Return the content inside the first ``` ... ``` block in `text`, or None.
fn extract_first_fence(text: &str) -> Option<String> {
    // Find opening ``` (at start of line or with optional language tag)
    let open = text.find("\n```")?;
    let after_open = &text[open + 1..]; // skip the leading newline
    // Skip the ``` line itself (may have a language tag like ```reify)
    let content_start = after_open.find('\n')? + 1;
    let content = &after_open[content_start..];
    let close = content.find("\n```")?;
    Some(content[..close].to_string())
}

// ── Test A (RED driver — turns GREEN after step-2) ───────────────────────────

/// Parse the §9.5 Purposes example fence from the live spec.
///
/// Asserts:
/// 1. No ERROR nodes (tree-sitter parse exit-0 equivalent).
/// 2. >=2 `purpose_param` nodes (multi-ref params).
/// 3. >=1 `let_declaration` node (let binding).
/// 4. >=1 `guarded_block` node (guarded `where {}` block).
///
/// RED before step-2: the old §9.5 example (`manufacturing_ready`) has 1 param,
/// no let binding, and no guarded block — assertions 2–4 all fail.
#[test]
fn spec_9_5_purpose_example_parses_with_required_constructs() {
    let example = extract_section_fence(SPEC, "### 9.5 Purposes");
    let mut parser = make_parser();
    let source = example.as_bytes();
    let tree = parser.parse(source, None).expect("parse returned None");
    let root = tree.root_node();

    let purpose_params = count_kind(root, "purpose_param");
    let let_decls = count_kind(root, "let_declaration");
    let guarded_blocks = count_kind(root, "guarded_block");

    assert!(
        !root.has_error(),
        "§9.5 purpose example must parse with no ERROR nodes;\n\
         source:\n{example}"
    );
    assert!(
        purpose_params >= 2,
        "§9.5 purpose example must have >=2 purpose_param nodes (multi-ref params), \
         found {purpose_params};\nsource:\n{example}"
    );
    assert!(
        let_decls >= 1,
        "§9.5 purpose example must have >=1 let_declaration node, \
         found {let_decls};\nsource:\n{example}"
    );
    assert!(
        guarded_blocks >= 1,
        "§9.5 purpose example must have >=1 guarded_block node, \
         found {guarded_blocks};\nsource:\n{example}"
    );
}

// ── Test B (regression baseline — GREEN before and after step-3) ─────────────

/// Parse the §4.4 Purpose Declarations example fence (after the "Example:" anchor).
///
/// Asserts: no ERROR nodes.
/// GREEN now with the existing §4.4 example; stays GREEN after step-3
/// replaces it with the canonical fits_within example.
#[test]
fn spec_4_4_purpose_example_parses() {
    let example = extract_fence_after_anchor(SPEC, "### 4.4 Purpose Declarations", "Example:");
    let mut parser = make_parser();
    let source = example.as_bytes();
    let tree = parser.parse(source, None).expect("parse returned None");
    let root = tree.root_node();
    assert!(
        !root.has_error(),
        "§4.4 purpose example must parse with no ERROR nodes;\n\
         source:\n{example}"
    );
}
