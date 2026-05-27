//! Multi-case load dispatcher — detects `MultiCaseResult`-shaped `Value::Map` cells
//! at the eval/commit boundary and extracts the set of available case names.
//!
//! Keyed on the same shape contract as `crates/reify-stdlib/src/fea.rs::extract_cases_map`
//! (fea.rs:703): outer `Map{"cases" -> Map<Value::String, ElasticResult-Map>}`.
//! Implements the engine-side detector for PRD §2.2 task η (`fea-case-changed`).

use reify_ir::Value;

/// Detected multi-case result from a `Value::Map` cell.
///
/// `active_case_id` is the lexicographically-smallest case name (deterministic
/// BTreeMap key order, matching stdlib `extract_cases_map`).
/// `available_cases` is the sorted list of all case names (BTreeMap iteration order).
pub struct DetectedCases {
    pub active_case_id: String,
    pub available_cases: Vec<String>,
}

/// Detect a `MultiCaseResult`-shaped value and return the set of case names.
///
/// Returns `None` if:
/// - `value` is not a `Value::Map`
/// - the map has no `"cases"` key
/// - the `"cases"` value is not a `Value::Map`
/// - the inner cases map is empty (nothing to switch)
///
/// When task 3026 (`solve_load_cases`) lands, this detector recognises real
/// `MultiCaseResult` values automatically — no rewiring needed here.
pub fn detect_multi_case_result(value: &Value) -> Option<DetectedCases> {
    let outer = match value {
        Value::Map(m) => m,
        _ => return None,
    };

    let cases_value = outer.get(&Value::String("cases".to_string()))?;

    let inner = match cases_value {
        Value::Map(m) => m,
        _ => return None,
    };

    let available_cases: Vec<String> = inner
        .keys()
        .filter_map(|k| match k {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect();

    if available_cases.is_empty() {
        return None;
    }

    // BTreeMap keys are already sorted; available_cases[0] is the lex-smallest.
    let active_case_id = available_cases[0].clone();

    Some(DetectedCases {
        active_case_id,
        available_cases,
    })
}

#[cfg(test)]
mod tests {
    use reify_test_support::values::multi_case_result_value;
    use reify_ir::Value;
    use std::collections::BTreeMap;

    use super::detect_multi_case_result;

    // (a) non-Map values → None
    #[test]
    fn non_map_value_returns_none() {
        assert!(detect_multi_case_result(&Value::Int(42)).is_none());
        assert!(detect_multi_case_result(&Value::String("hello".to_string())).is_none());
    }

    // (a) empty Map → None
    #[test]
    fn empty_map_returns_none() {
        let v = Value::Map(BTreeMap::new());
        assert!(detect_multi_case_result(&v).is_none());
    }

    // (a) Map without "cases" key → None
    #[test]
    fn map_without_cases_key_returns_none() {
        let mut m = BTreeMap::new();
        m.insert(Value::String("other".to_string()), Value::Int(1));
        let v = Value::Map(m);
        assert!(detect_multi_case_result(&v).is_none());
    }

    // (b) 3-case fixture: active_case_id == lex-smallest, available_cases == sorted keys
    #[test]
    fn three_case_fixture_returns_expected_active_and_available() {
        // "transport" / "operating" / "overload" → lex-smallest is "operating"
        let v = multi_case_result_value(&[
            ("transport", Value::Int(1)),
            ("operating", Value::Int(2)),
            ("overload", Value::Int(3)),
        ]);
        let result = detect_multi_case_result(&v).expect("should detect multi-case shape");
        assert_eq!(result.active_case_id, "operating");
        assert_eq!(
            result.available_cases,
            vec!["operating".to_string(), "overload".to_string(), "transport".to_string()]
        );
    }

    // (c) Map whose "cases" value is not a Value::Map → None
    #[test]
    fn cases_value_not_a_map_returns_none() {
        let mut m = BTreeMap::new();
        m.insert(Value::String("cases".to_string()), Value::Int(99));
        let v = Value::Map(m);
        assert!(detect_multi_case_result(&v).is_none());
    }

    // (d) {cases → empty Value::Map} → None (no cases = nothing to switch)
    #[test]
    fn cases_empty_inner_map_returns_none() {
        let v = multi_case_result_value(&[]);
        assert!(detect_multi_case_result(&v).is_none());
    }

    /// Shape-contract cross-crate invariant guard.
    ///
    /// `multi_case_result_value` (reify-test-support) builds exactly the same
    /// outer `Map{"cases" -> Map<Value::String, ...>}` shape that stdlib's private
    /// `extract_cases_map` (`crates/reify-stdlib/src/fea.rs:703`) produces.
    ///
    /// If either the stdlib wrapper shape or this detector's key string ("cases")
    /// diverges, this test fails — catching shape drift before task 3026 wires
    /// real `MultiCaseResult` values into `CheckResult.values`.
    #[test]
    fn detector_accepts_stdlib_produced_shape_contract() {
        let v = multi_case_result_value(&[
            ("case_a", Value::Int(1)),
            ("case_b", Value::Int(2)),
        ]);
        let result = detect_multi_case_result(&v).expect(
            "detect_multi_case_result must accept the shape produced by \
             multi_case_result_value (which mirrors stdlib extract_cases_map); \
             None here indicates shape drift between the detector and stdlib",
        );
        assert_eq!(result.active_case_id, "case_a");
        assert_eq!(
            result.available_cases,
            vec!["case_a".to_string(), "case_b".to_string()]
        );
    }
}
