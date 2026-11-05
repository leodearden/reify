//! Reading the findings array out of a `reify-audit` BINARY run's stderr.
//!
//! The binary writes diagnostic lines (breadcrumbs, git warnings) to stderr
//! BEFORE the JSON findings array, so the array is located as the LAST block
//! opening with `[` at the start of a line. Both directions — "an array was
//! emitted" and "no array was emitted" — share one definition of that
//! boundary, so they cannot drift apart.

/// Byte offset where the findings array begins, or `None` if there is none.
///
/// The LAST `\n[` rather than the first, so an earlier diagnostic line that
/// happens to contain `[` (a path with brackets, `[detached HEAD]`) cannot
/// corrupt the boundary.
#[allow(dead_code)]
fn find_findings_array_start(stderr: &str) -> Option<usize> {
    match stderr.rfind("\n[") {
        Some(pos) => Some(pos + 1),
        None => stderr.starts_with('[').then_some(0),
    }
}

/// The findings array on `stderr`; panics, echoing the stderr, when there is
/// none or it does not parse.
#[allow(dead_code)]
pub fn parse_findings_from_stderr(stderr: &str) -> Vec<serde_json::Value> {
    let json_start = find_findings_array_start(stderr)
        .unwrap_or_else(|| panic!("no JSON array found in stderr; full stderr:\n{stderr}"));
    serde_json::from_str(&stderr[json_start..]).unwrap_or_else(|e| {
        panic!("stderr does not contain valid JSON after '[': {e}\nstderr:\n{stderr}")
    })
}

/// Whether stderr carries a parseable findings array — non-panicking, for
/// asserting the ABSENCE of one. This is the property the `/audit` skill's
/// exit-125 disambiguator reads: a run emits an array, an error path never
/// does.
#[allow(dead_code)]
pub fn stderr_has_parseable_findings_array(stderr: &str) -> bool {
    find_findings_array_start(stderr).is_some_and(|start| {
        serde_json::from_str::<Vec<serde_json::Value>>(&stderr[start..]).is_ok()
    })
}
