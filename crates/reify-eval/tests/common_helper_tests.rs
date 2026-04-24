//! Tests for the `ten_bool_guarded_groups` shared fixture helper.
//!
//! Include `tests/common/mod.rs` via `mod common;`. The round-trip test
//! compiles helper output through `parse_and_compile` for all 3 guard
//! expressions used by callers, locking syntactic validity without
//! duplicating inline fixtures here.

mod common;

use common::ten_bool_guarded_groups;
use reify_test_support::parse_and_compile;

/// Round-trip validity: helper output parses and compiles successfully for all
/// 3 guard expressions actually used by callers ("u3", "u3 && true", "!u3").
#[test]
fn helper_round_trips_parse_and_compile() {
    for expr in ["u3", "u3 && true", "!u3"] {
        let src = ten_bool_guarded_groups(expr);
        // parse_and_compile panics on invalid source; success = valid syntax.
        let _ = parse_and_compile(&src);
    }
}
