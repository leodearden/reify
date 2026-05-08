//! Mock-kernel and pure-Rust unit tests for the v0.2 selector vocabulary
//! (`reify_eval::selector_vocabulary_v2`), task 2658 (PRD task 10).
//!
//! These tests are always-on (no OCCT runtime required) and complement the
//! OCCT-backed integration tests in `selector_vocabulary_v2_e2e.rs` which
//! skip at runtime when OCCT is unavailable.
//!
//! Convention: handle id=1 is the parent solid, id=2..N are the sub-shape
//! (edge / face) handles returned by the configured extraction. This
//! mirrors `topology_filtered_selectors_mock.rs`.

use reify_eval::selector_vocabulary_v2::{complement, except, intersect, union};
use reify_types::GeometryHandleId;

// ─────────────────────────────────────────────────────────────────────────────
// intersect — set intersection over Vec<GeometryHandleId>
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn intersect_keeps_left_encounter_order_for_common_elements() {
    let a = vec![
        GeometryHandleId(10),
        GeometryHandleId(20),
        GeometryHandleId(30),
        GeometryHandleId(40),
    ];
    let b = vec![
        GeometryHandleId(40),
        GeometryHandleId(20),
        GeometryHandleId(99),
    ];

    // Both 20 and 40 are in both; order is the LEFT operand's order
    // (so 20 before 40, not 40 before 20 as `b` would suggest).
    assert_eq!(
        intersect(&a, &b),
        vec![GeometryHandleId(20), GeometryHandleId(40)],
        "intersect must preserve left-operand encounter order"
    );
}

#[test]
fn intersect_dedupes_duplicates_in_left_operand() {
    // The left operand contains duplicates; intersect must emit each
    // common element at most once, at its first encounter position.
    let a = vec![
        GeometryHandleId(10),
        GeometryHandleId(20),
        GeometryHandleId(20),
        GeometryHandleId(30),
        GeometryHandleId(10),
    ];
    let b = vec![GeometryHandleId(10), GeometryHandleId(20)];

    assert_eq!(
        intersect(&a, &b),
        vec![GeometryHandleId(10), GeometryHandleId(20)],
        "intersect must dedupe on first-seen even when LHS has duplicates"
    );
}

#[test]
fn intersect_with_disjoint_inputs_is_empty() {
    let a = vec![GeometryHandleId(1), GeometryHandleId(2)];
    let b = vec![GeometryHandleId(3), GeometryHandleId(4)];
    assert!(intersect(&a, &b).is_empty());
}

#[test]
fn intersect_with_empty_inputs_is_empty() {
    let a: Vec<GeometryHandleId> = vec![];
    let b = vec![GeometryHandleId(1)];
    assert!(intersect(&a, &b).is_empty());
    assert!(intersect(&b, &a).is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// union — set union with left-then-right encounter order
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn union_returns_left_then_right_only_new_elements() {
    let a = vec![
        GeometryHandleId(10),
        GeometryHandleId(20),
        GeometryHandleId(30),
    ];
    let b = vec![
        GeometryHandleId(20), // already in a; skip
        GeometryHandleId(40), // new
        GeometryHandleId(10), // already in a; skip
        GeometryHandleId(50), // new
    ];

    assert_eq!(
        union(&a, &b),
        vec![
            GeometryHandleId(10),
            GeometryHandleId(20),
            GeometryHandleId(30),
            GeometryHandleId(40),
            GeometryHandleId(50),
        ],
        "union returns a in encounter order, then elements of b not in a in encounter order"
    );
}

#[test]
fn union_dedupes_duplicates_within_either_operand() {
    let a = vec![
        GeometryHandleId(1),
        GeometryHandleId(1),
        GeometryHandleId(2),
    ];
    let b = vec![
        GeometryHandleId(2),
        GeometryHandleId(3),
        GeometryHandleId(3),
    ];

    assert_eq!(
        union(&a, &b),
        vec![GeometryHandleId(1), GeometryHandleId(2), GeometryHandleId(3)],
        "union must dedupe on first-seen even when either operand has duplicates"
    );
}

#[test]
fn union_with_empty_left_returns_dedupe_of_right() {
    let a: Vec<GeometryHandleId> = vec![];
    let b = vec![
        GeometryHandleId(1),
        GeometryHandleId(2),
        GeometryHandleId(1),
    ];
    assert_eq!(
        union(&a, &b),
        vec![GeometryHandleId(1), GeometryHandleId(2)],
    );
}

#[test]
fn union_with_empty_right_returns_dedupe_of_left() {
    let a = vec![
        GeometryHandleId(1),
        GeometryHandleId(2),
        GeometryHandleId(1),
    ];
    let b: Vec<GeometryHandleId> = vec![];
    assert_eq!(
        union(&a, &b),
        vec![GeometryHandleId(1), GeometryHandleId(2)],
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// complement — set difference (universe \ exclude), preserving universe order
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn complement_returns_universe_elements_not_in_exclude() {
    let universe = vec![
        GeometryHandleId(10),
        GeometryHandleId(20),
        GeometryHandleId(30),
        GeometryHandleId(40),
    ];
    let exclude = vec![GeometryHandleId(20), GeometryHandleId(40)];
    assert_eq!(
        complement(&universe, &exclude),
        vec![GeometryHandleId(10), GeometryHandleId(30)],
        "complement must return universe elements not in exclude, in universe order"
    );
}

#[test]
fn complement_empty_universe_is_empty() {
    let universe: Vec<GeometryHandleId> = vec![];
    let exclude = vec![GeometryHandleId(1)];
    assert!(complement(&universe, &exclude).is_empty());
}

#[test]
fn complement_empty_exclude_returns_dedupe_of_universe() {
    let universe = vec![
        GeometryHandleId(1),
        GeometryHandleId(2),
        GeometryHandleId(1), // duplicate — should dedupe to first
        GeometryHandleId(3),
    ];
    let exclude: Vec<GeometryHandleId> = vec![];
    assert_eq!(
        complement(&universe, &exclude),
        vec![
            GeometryHandleId(1),
            GeometryHandleId(2),
            GeometryHandleId(3)
        ],
        "with empty exclude, complement = universe with dedup-on-first-seen"
    );
}

#[test]
fn complement_full_overlap_is_empty() {
    let universe = vec![GeometryHandleId(1), GeometryHandleId(2)];
    let exclude = vec![GeometryHandleId(1), GeometryHandleId(2)];
    assert!(complement(&universe, &exclude).is_empty());
}

#[test]
fn complement_partial_overlap_with_duplicates_dedupes() {
    let universe = vec![
        GeometryHandleId(1),
        GeometryHandleId(2),
        GeometryHandleId(2),
        GeometryHandleId(3),
        GeometryHandleId(1),
    ];
    let exclude = vec![GeometryHandleId(2), GeometryHandleId(2)];
    assert_eq!(
        complement(&universe, &exclude),
        vec![GeometryHandleId(1), GeometryHandleId(3)],
        "complement dedupes universe duplicates and tolerates exclude duplicates"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// except — alias for complement (PRD line 79 names them distinctly)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn except_matches_complement_on_simple_inputs() {
    let a = vec![
        GeometryHandleId(1),
        GeometryHandleId(2),
        GeometryHandleId(3),
    ];
    let b = vec![GeometryHandleId(2)];
    assert_eq!(
        except(&a, &b),
        complement(&a, &b),
        "except is currently identical to complement"
    );
    assert_eq!(
        except(&a, &b),
        vec![GeometryHandleId(1), GeometryHandleId(3)],
    );
}

#[test]
fn except_with_full_overlap_is_empty() {
    let a = vec![GeometryHandleId(1), GeometryHandleId(2)];
    let b = vec![GeometryHandleId(2), GeometryHandleId(1)];
    assert!(except(&a, &b).is_empty());
}
