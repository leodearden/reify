//! Structured reads over a committed PRD-gate probe-set
//! (`tests/prd-gate/*-probe-set.json`), the format `scripts/prd-capability-check.py`
//! owns and documents in its module docstring.
//!
//! Nothing here runs a probe or re-validates the format. Whether a probe PASSES
//! is the job of the infra gate that runs the checker over the same file; this
//! module answers what a probe-set ASSERTS, for a Rust-side invariant that
//! depends on it.

use std::collections::BTreeSet;

use serde_json::Value;

/// The fixtures `probe_set_json` asserts `reify check` REJECTS, as the
/// repo-relative paths the probes name.
///
/// A probe asserts rejection when it is a `check` probe expecting the observation
/// `present` for a match that requires `exit_code: 1`. The checker's verdict on
/// such a probe is PASS only if `reify check <fixture>` exits 1. Every other
/// shape is excluded, including three that a text search over the file cannot
/// tell apart from a real one:
///
/// * a path that appears only in a `capability` string;
/// * a probe expecting exit 0;
/// * a probe expecting the match `absent`, which PASSES exactly when the file
///   does NOT reject.
///
/// Errors if the text is not JSON or has no top-level `probes` array. A
/// malformed individual probe asserts nothing and is skipped, because the
/// checker, not this reader, owns validating the format.
pub fn fixtures_asserted_to_reject(probe_set_json: &str) -> Result<BTreeSet<String>, String> {
    let probe_set: Value = serde_json::from_str(probe_set_json)
        .map_err(|e| format!("probe-set is not valid JSON: {e}"))?;
    let probes = probe_set["probes"]
        .as_array()
        .ok_or("probe-set has no top-level `probes` array")?;
    Ok(probes
        .iter()
        .filter(|probe| asserts_check_rejection(probe))
        .filter_map(|probe| probe["fixture"].as_str().map(str::to_owned))
        .collect())
}

fn asserts_check_rejection(probe: &Value) -> bool {
    let expected = &probe["expected"];
    probe["probe_kind"] == "check"
        && expected["observation"] == "present"
        && expected["match"]["exit_code"] == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A probe-set holding the single probe `probe`.
    fn one_probe(probe: &str) -> String {
        format!(r#"{{ "probes": [ {probe} ] }}"#)
    }

    const REJECTING: &str = r#"{
        "capability": "rejects",
        "probe_kind": "check",
        "fixture": "tests/prd-gate/fixtures/rejects.ri",
        "expected": { "observation": "present", "match": { "exit_code": 1 } }
    }"#;

    #[test]
    fn a_check_probe_expecting_exit_1_present_asserts_rejection() {
        let fixtures = fixtures_asserted_to_reject(&one_probe(REJECTING)).expect("valid probe-set");
        assert_eq!(
            fixtures.into_iter().collect::<Vec<_>>(),
            ["tests/prd-gate/fixtures/rejects.ri"]
        );
    }

    /// The case a text search over the file cannot tell apart from a real probe.
    #[test]
    fn a_path_named_only_in_a_capability_string_is_not_asserted() {
        let probe = r#"{
            "capability": "mentions tests/prd-gate/fixtures/mentioned.ri in prose only",
            "probe_kind": "check",
            "fixture": "tests/prd-gate/fixtures/other.ri",
            "expected": { "observation": "present", "match": { "exit_code": 0 } }
        }"#;
        let fixtures = fixtures_asserted_to_reject(&one_probe(probe)).expect("valid probe-set");
        assert!(fixtures.is_empty(), "got: {fixtures:?}");
    }

    #[test]
    fn a_probe_expecting_exit_0_does_not_assert_rejection() {
        let probe = REJECTING.replace(r#""exit_code": 1"#, r#""exit_code": 0"#);
        let fixtures = fixtures_asserted_to_reject(&one_probe(&probe)).expect("valid probe-set");
        assert!(fixtures.is_empty(), "got: {fixtures:?}");
    }

    /// `absent` inverts the match: the probe PASSES exactly when the file does NOT
    /// exit 1, so it asserts the opposite of rejection.
    #[test]
    fn a_probe_expecting_the_match_absent_does_not_assert_rejection() {
        let probe = REJECTING.replace(r#""present""#, r#""absent""#);
        let fixtures = fixtures_asserted_to_reject(&one_probe(&probe)).expect("valid probe-set");
        assert!(fixtures.is_empty(), "got: {fixtures:?}");
    }

    /// An `ir` or `value` probe runs `reify eval`, not `reify check`.
    #[test]
    fn a_non_check_probe_does_not_assert_rejection() {
        let probe = REJECTING.replace(r#""check""#, r#""ir""#);
        let fixtures = fixtures_asserted_to_reject(&one_probe(&probe)).expect("valid probe-set");
        assert!(fixtures.is_empty(), "got: {fixtures:?}");
    }

    /// A match on stderr alone says nothing about the exit code.
    #[test]
    fn a_match_without_an_exit_code_does_not_assert_rejection() {
        let probe = REJECTING.replace(r#""exit_code": 1"#, r#""stderr_contains": "error""#);
        let fixtures = fixtures_asserted_to_reject(&one_probe(&probe)).expect("valid probe-set");
        assert!(fixtures.is_empty(), "got: {fixtures:?}");
    }

    #[test]
    fn a_probe_set_that_is_not_json_or_has_no_probes_array_is_an_error() {
        assert!(fixtures_asserted_to_reject("not json").is_err());
        assert!(fixtures_asserted_to_reject(r#"{ "probe": [] }"#).is_err());
    }
}
