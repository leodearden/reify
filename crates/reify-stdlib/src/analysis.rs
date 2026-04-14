//! Stress analysis builtins: von_mises, principal_stresses, max_shear, safety_factor.

use reify_types::Value;

/// Evaluate a stress-analysis builtin by name.
///
/// Returns `Some(value)` if the name is a recognised analysis function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_analysis(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "von_mises" | "principal_stresses" | "max_shear" | "safety_factor" => {
            let _ = args;
            Value::Undef // stub — implementations added in subsequent steps
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_function_returns_none() {
        assert!(eval_analysis("foo", &[]).is_none());
    }

    #[test]
    fn known_function_returns_some() {
        assert!(eval_analysis("von_mises", &[]).is_some());
        assert!(eval_analysis("principal_stresses", &[]).is_some());
        assert!(eval_analysis("max_shear", &[]).is_some());
        assert!(eval_analysis("safety_factor", &[]).is_some());
    }
}
