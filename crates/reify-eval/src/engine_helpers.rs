//! Helper utilities shared across engine_eval and engine_edit.

use reify_types::{Value, ValueCellId, ValueMap};

/// Build a synthetic per-member list value for a collection sub.
///
/// Iterates `idx in 0..n`, looks up `<parent>.<sub>[idx].<member>` in `values`,
/// and collects the results into a `Value::List`.
///
/// # Parameters
/// - `values`: the current live `ValueMap`
/// - `parent`: entity name that owns the collection sub (e.g. `"Widget"`)
/// - `sub`: sub-entity name (e.g. `"bolts"`)
/// - `member`: value-cell member name within each child instance (e.g. `"grade"`)
/// - `n`: number of child instances (`0..n` is the index range)
///
/// # Returns
/// `Value::List` whose `idx`-th element is the value of
/// `<parent>.<sub>[idx].<member>` from `values`, or `Value::Undef` if the cell
/// is absent.
pub(crate) fn collect_member_list(
    values: &ValueMap,
    parent: &str,
    sub: &str,
    member: &str,
    n: i64,
) -> Value {
    let items: Vec<Value> = (0..n)
        .map(|idx| {
            let scoped_id = ValueCellId::new(format!("{}.{}[{}]", parent, sub, idx), member);
            values.get(&scoped_id).cloned().unwrap_or(Value::Undef)
        })
        .collect();
    Value::List(items)
}

#[cfg(test)]
mod tests {
    use reify_types::{Value, ValueCellId, ValueMap};

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

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "child cell not yet evaluated")]
    fn collect_member_list_panics_in_debug_when_child_missing() {
        collect_member_list(&ValueMap::default(), "Parent", "bolts", "grade", 2);
    }
}
