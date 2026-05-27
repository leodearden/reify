//! Helper utilities shared across engine_eval and engine_edit.

use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};

/// Build a synthetic per-member list value for a collection sub.
///
/// Iterates `idx in 0..n`, looks up `<parent>.<sub>[idx].<member>` in `values`,
/// and collects the results into a `Value::List`.
///
/// # Eval-order invariant
///
/// All child cells `<parent>.<sub>[0..(n-1)].<member>` **must** already be
/// present in `values` before this helper is called. In debug/test builds a
/// `debug_assert!` fires immediately if any child cell is absent, naming the
/// missing cell and the violated invariant. In release builds the fallback
/// `Value::Undef` is returned for absent cells — this preserves the historical
/// behaviour of the three inline closures this helper replaces; the
/// `debug_assert!` surfaces any invariant violation early in dev/test builds,
/// while release preserves the historical Undef-fallback behaviour.
///
/// This mirrors the sibling pattern in `reapply_guard_deactivations_post_wave2`
/// (see `engine_edit.rs`).
///
/// # Parameters
/// - `values`: the current live `ValueMap`
/// - `parent`: entity name that owns the collection sub (e.g. `"Widget"`)
/// - `sub`: sub-entity name (e.g. `"bolts"`)
/// - `member`: value-cell member name within each child instance (e.g. `"grade"`)
/// - `n`: number of child instances (`0..n` is the index range).
///   In debug/test builds a `debug_assert!` fires on negative `n`, surfacing
///   upstream arithmetic bugs early. In release builds negative values are
///   clamped to 0 (returns an empty list), matching the historical behaviour
///   of the inline `0..count` loops this helper replaces.
///
/// # Returns
/// `Value::List` whose `idx`-th element is the value of
/// `<parent>.<sub>[idx].<member>` from `values`, or `Value::Undef` if the cell
/// is absent (release-mode fallback; in dev/test builds the `debug_assert!`
/// surfaces the invariant violation before this fallback is reached).
pub(crate) fn collect_member_list(
    values: &ValueMap,
    parent: &str,
    sub: &str,
    member: &str,
    n: i64,
) -> Value {
    debug_assert!(
        n >= 0,
        "collect_member_list: negative count n={n} for {parent}.{sub}"
    );
    let n = n.max(0); // Release-only safety net (debug_assert above panics first in dev/test).
    let items: Vec<Value> = (0..n)
        .map(|idx| {
            let scoped_id = ValueCellId::new(format!("{}.{}[{}]", parent, sub, idx), member);
            debug_assert!(
                values.contains(&scoped_id),
                "child cell not yet evaluated: {} (collect_member_list eval-order invariant violated)",
                scoped_id
            );
            values.get(&scoped_id).cloned().unwrap_or(Value::Undef)
        })
        .collect();
    Value::List(items)
}

#[cfg(test)]
mod tests {
    use reify_core::ValueCellId;
    use reify_ir::{Value, ValueMap};

    use super::collect_member_list;

    #[test]
    fn collect_member_list_builds_list_in_index_order() {
        let mut values = ValueMap::default();
        for i in 0..3_i64 {
            let id = ValueCellId::new(format!("Parent.bolts[{}]", i), "grade");
            values.insert(id, Value::Int(i + 1));
        }

        let result = collect_member_list(&values, "Parent", "bolts", "grade", 3);
        assert_eq!(
            result,
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
    }

    #[test]
    fn collect_member_list_returns_empty_for_zero_n() {
        // n=0 is a valid boundary: collection subs whose count resolves to 0
        // must produce an empty list without touching `values` at all.
        let result = collect_member_list(&ValueMap::default(), "Parent", "bolts", "grade", 0);
        assert_eq!(result, Value::List(vec![]));
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn collect_member_list_returns_empty_for_negative_n() {
        // Pre-refactor inline 0..count loops silently produced empty list for
        // negative counts in release; the clamp restores that contract for release builds.
        // Debug builds get a debug_assert! instead (see sibling test below).
        let result = collect_member_list(&ValueMap::default(), "Parent", "bolts", "grade", -3);
        assert_eq!(result, Value::List(vec![]));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "collect_member_list: negative count")]
    fn collect_member_list_panics_in_debug_for_negative_n() {
        // In debug builds the debug_assert!(n >= 0) fires before the clamp,
        // surfacing upstream arithmetic bugs early rather than silently clamping.
        collect_member_list(&ValueMap::default(), "Parent", "bolts", "grade", -3);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "child cell not yet evaluated")]
    fn collect_member_list_panics_in_debug_when_child_missing() {
        collect_member_list(&ValueMap::default(), "Parent", "bolts", "grade", 2);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn collect_member_list_returns_undef_fallback_in_release_when_children_missing() {
        // Release-mode contract: when child cells are absent, the helper returns
        // Value::Undef placeholders rather than panicking — preserves the historical
        // behaviour of the inline closures this helper replaces. Debug builds catch
        // this via the sibling debug_assert! on the values.contains(&scoped_id) check
        // (see the debug-only test above).
        let result = collect_member_list(&ValueMap::default(), "Parent", "bolts", "grade", 2);
        assert_eq!(result, Value::List(vec![Value::Undef, Value::Undef]));
    }
}
