//! Generic list helpers exposed via function-call form: `single`, etc.
//!
//! These helpers complement the method-call helpers (`count`, `sum`, `map`,
//! …) dispatched by `reify-expr::eval_method_call`. The PRD §worked-examples
//! fixture (`examples/topology_selectors/fillet_top_edges.ri`) uses the
//! function-call form, so they must be reachable through `eval_builtin`.
//!
//! Convention (matches the rest of stdlib): silently return `Value::Undef`
//! on type errors, wrong arg counts, or otherwise ill-formed inputs. No
//! diagnostic emission — runtime poison propagates as `Undef` through the
//! evaluation graph.

use reify_types::Value;

/// Evaluate a list-helper builtin by name. Returns `None` if the name is not
/// a list helper, signalling `eval_builtin` to fall through to the next
/// per-domain dispatcher. Returns `Some(Value::Undef)` for ill-typed inputs
/// (the stdlib convention is "claim the name, return Undef on error").
pub(crate) fn eval_list(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_types::Value;

    #[test]
    fn single_one_element_returns_inner() {
        let result = eval_builtin("single", &[Value::List(vec![Value::Int(42)])]);
        match result {
            Value::Int(42) => {}
            other => panic!("expected Int(42), got {:?}", other),
        }
    }

    #[test]
    fn single_empty_list_returns_undef() {
        let result = eval_builtin("single", &[Value::List(vec![])]);
        assert!(
            result.is_undef(),
            "single([]) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn single_multi_element_list_returns_undef() {
        let result = eval_builtin(
            "single",
            &[Value::List(vec![Value::Int(1), Value::Int(2)])],
        );
        assert!(
            result.is_undef(),
            "single([1, 2]) should be Undef, got {:?}",
            result
        );
    }
}
