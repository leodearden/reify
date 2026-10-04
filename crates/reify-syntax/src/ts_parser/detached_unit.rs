//! Recognising a detached unit suffix (`1000.0 N/m`) at a parse fault.
//!
//! PRD docs/prds/unit-expressions.md §3.1, the contiguity invariant: a unit must touch its
//! number, and that is what keeps `5kg * m` a binary expression. The grammar therefore rejects
//! `1000.0 N/m` on purpose, and the parser's job is to explain the fix rather than accept the
//! spelling (task #7190). This module recognises the shape at a fault, so the diagnostic can name
//! the unit, the binding it sits in and the attached spelling.
//!
//! Like its sibling `fault_diagnosis`, it reads only a bare `tree_sitter::Node` plus the source
//! text and no lowering state; wording the diagnostic stays with `Lowering`.

use tree_sitter::Node;

/// A unit word separated from its number by spaces or tabs.
pub(super) struct DetachedUnit<'t> {
    /// The `number_literal` the unit belongs to.
    pub(super) number: Node<'t>,
    /// The first unit-shaped word after the gap: the fault's first leaf.
    pub(super) unit: Node<'t>,
    /// The `name` of the nearest binding whose value holds `number`, if there is one.
    pub(super) binding_name: Option<Node<'t>>,
}

/// Recognise `fault` as a detached unit suffix: an `ERROR` whose first leaf is a unit-shaped word
/// preceded, across spaces or tabs only, by a `number_literal`.
///
/// A newline gap is deliberately not this shape. It is the absorbed-next-line shape, which has a
/// different cause, and a wrong remedy is worse than none.
pub(super) fn detached_unit_suffix<'t>(fault: Node<'t>, source: &str) -> Option<DetachedUnit<'t>> {
    if !fault.is_error() {
        return None;
    }
    let unit = first_leaf(fault);
    if !is_unit_word(unit, source) {
        return None;
    }
    let gap_start = horizontal_gap_start(source, unit.start_byte())?;
    let number = number_ending_at(unit, gap_start)?;
    Some(DetachedUnit {
        number,
        unit,
        binding_name: binding_name(number),
    })
}

fn first_leaf(node: Node<'_>) -> Node<'_> {
    let mut leaf = node;
    while let Some(child) = leaf.child(0) {
        leaf = child;
    }
    leaf
}

/// An `identifier` or `ERROR` leaf whose text is in the grammar's `unit_name` class,
/// `[A-Za-z_][A-Za-z0-9_]*` (`immediate_identifier`). An `ERROR` leaf's kind says nothing about
/// its text, so the class is checked on the text itself.
///
/// This is a SHAPE test, not a unit lookup: the syntax layer has no unit registry, so a
/// variable after a number (`2 x`, a missing operator) matches too. That is accepted here and
/// answered in the wording, which the caller makes conditional on the word being a unit.
fn is_unit_word(leaf: Node<'_>, source: &str) -> bool {
    if !(leaf.kind() == "identifier" || leaf.is_error()) {
        return false;
    }
    let mut chars = source[leaf.byte_range()].chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Where the run of spaces and tabs that ends at `word_start` begins, or `None` when the run is
/// empty or nothing precedes it.
fn horizontal_gap_start(source: &str, word_start: usize) -> Option<usize> {
    let gap_start = source[..word_start].trim_end_matches([' ', '\t']).len();
    (0 < gap_start && gap_start < word_start).then_some(gap_start)
}

/// The `number_literal` that ends exactly at `gap_start`, searched for from `unit`'s nearest
/// ancestor that starts before the gap.
fn number_ending_at(unit: Node<'_>, gap_start: usize) -> Option<Node<'_>> {
    let mut scope = unit;
    while scope.start_byte() >= gap_start {
        scope = scope.parent()?;
    }
    let number = scope.descendant_for_byte_range(gap_start - 1, gap_start)?;
    (number.kind() == "number_literal" && number.end_byte() == gap_start).then_some(number)
}

/// The `name` of the nearest ancestor of `number` that binds it, meaning one with a `name` field
/// and a `value` or `default` field spanning `number`. `function_call` has a `name` but no value,
/// so a number among call arguments names the binding the call is the value of.
fn binding_name(number: Node<'_>) -> Option<Node<'_>> {
    let mut ancestor = number.parent();
    while let Some(node) = ancestor {
        if let Some(name) = node.child_by_field_name("name")
            && binds(node, number)
        {
            return Some(name);
        }
        ancestor = node.parent();
    }
    None
}

fn binds(node: Node<'_>, number: Node<'_>) -> bool {
    ["value", "default"]
        .into_iter()
        .filter_map(|field| node.child_by_field_name(field))
        .any(|value| {
            value.start_byte() <= number.start_byte() && number.end_byte() <= value.end_byte()
        })
}
