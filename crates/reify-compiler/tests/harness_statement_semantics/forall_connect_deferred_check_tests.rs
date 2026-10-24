//! A `forall` connect over a deferred-count collection gets the endpoint checks
//! the resolved form gets. One fixture is compiled twice: with an undefaulted
//! count (`constraint vents.count == n`), which captures the body for runtime
//! re-elaboration, and with a literal count (`== 2`), which elaborates it per
//! element. Both must report the same undefined-port and port-direction errors;
//! the deferred form reports each once per declaration and names the bound
//! element (`vents[v]`) rather than any one element.

use reify_test_support::{compile_source, errors_only};
use std::collections::BTreeSet;

/// The count that leaves the collection deferred: `n` has no default.
const DEFERRED: &str = "n";
/// The count that resolves the collection at compile time.
const RESOLVED: &str = "2";

fn fixture(count: &str, connect_line: &str) -> String {
    format!(
        r#"
trait Air {{ param d : Length }}
structure def Vent {{
    port inlet : out Air {{ param d : Length = 5mm }}
    port outlet : out Air {{ param d : Length = 5mm }}
}}
structure def S {{
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == {count}
    port air_channel : in Air {{ param d : Length = 5mm }}
    port hub : bidi Air {{ param d : Length = 5mm }}
    forall v in vents: {connect_line}
}}
"#
    )
}

fn error_messages(source: &str) -> Vec<String> {
    errors_only(&compile_source(source))
        .into_iter()
        .map(|d| d.message.clone())
        .collect()
}

/// The Error messages with every element indexer erased, so the resolved
/// form's per-element names and the deferred form's bound-element name
/// compare equal.
fn endpoint_errors(source: &str) -> BTreeSet<String> {
    error_messages(source)
        .iter()
        .map(|message| erase_element_index(message))
        .collect()
}

/// Rewrite every `[<ascii digits>]` and every `[v]` to `[_]`.
fn erase_element_index(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        match after_open.find(']') {
            Some(close) if is_element_index(&after_open[..close]) => {
                out.push_str("[_]");
                rest = &after_open[close + 1..];
            }
            _ => {
                out.push('[');
                rest = after_open;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_element_index(inner: &str) -> bool {
    inner == "v" || (!inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()))
}

fn count_containing(messages: &[String], needle: &str) -> usize {
    messages.iter().filter(|m| m.contains(needle)).count()
}

#[test]
fn deferred_form_reports_the_endpoint_errors_the_resolved_form_reports() {
    let connect_lines = [
        "connect air_channel -> v.inlet",
        "connect v.inlet <- air_channel",
        "connect hub <-> v.inlet",
        "connect v.nosuch -> air_channel",
        "connect vents[0].outlet -> v.inlet",
    ];
    for connect_line in connect_lines {
        let resolved = endpoint_errors(&fixture(RESOLVED, connect_line));
        let deferred = endpoint_errors(&fixture(DEFERRED, connect_line));
        assert!(
            !resolved.is_empty(),
            "`{connect_line}`: the resolved form reports no error, so this row compares nothing"
        );
        assert_eq!(
            deferred, resolved,
            "`{connect_line}`: deferred form errors {deferred:?} differ from resolved form errors {resolved:?}"
        );
    }
}

#[test]
fn deferred_form_names_the_bound_element_in_an_undefined_port_error() {
    let messages = error_messages(&fixture(DEFERRED, "connect v.nosuch -> air_channel"));
    assert_eq!(
        count_containing(
            &messages,
            "undefined port 'vents[v].nosuch' in connect statement"
        ),
        1,
        "expected exactly one undefined-port error naming the bound element, got {messages:?}"
    );
}

#[test]
fn deferred_form_reports_a_direction_error_once_per_declaration() {
    let messages = error_messages(&fixture(DEFERRED, "connect air_channel -> v.inlet"));
    assert_eq!(
        count_containing(
            &messages,
            "incompatible port directions for connect: In -> Out"
        ),
        1,
        "expected exactly one direction error for the declaration, got {messages:?}"
    );
}

#[test]
fn deferred_compatible_connect_reports_no_error() {
    let messages = error_messages(&fixture(DEFERRED, "connect v.inlet -> air_channel"));
    assert!(
        messages.is_empty(),
        "a direction-compatible connect naming declared ports must compile clean, got {messages:?}"
    );
}
