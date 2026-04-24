//! Tests for the `ten_bool_guarded_groups` shared fixture helper.
//!
//! Include `tests/common/mod.rs` via `mod common;`. This test file fails to
//! compile until `common/mod.rs` exists and `ten_bool_guarded_groups` is
//! defined (step-2). That compile failure is the "red" state required by TDD.

mod common;

use common::ten_bool_guarded_groups;
use reify_test_support::parse_and_compile;

/// The canonical 21-line 10-group fixture with group 3 guard = `u3`.
///
/// Copied verbatim from guard_eval.rs:2201-2222 / edit_source.rs:2326-2347.
const FIXTURE_U3: &str = r#"structure S {
    param u0: Bool = true
    param u1: Bool = true
    param u2: Bool = true
    param u3: Bool = true
    param u4: Bool = true
    param u5: Bool = true
    param u6: Bool = true
    param u7: Bool = true
    param u8: Bool = true
    param u9: Bool = true
    where u0 { let x0 = 1mm }
    where u1 { let x1 = 1mm }
    where u2 { let x2 = 1mm }
    where u3 { let x3 = 1mm }
    where u4 { let x4 = 1mm }
    where u5 { let x5 = 1mm }
    where u6 { let x6 = 1mm }
    where u7 { let x7 = 1mm }
    where u8 { let x8 = 1mm }
    where u9 { let x9 = 1mm }
}"#;

/// The canonical 21-line 10-group fixture with group 3 guard = `u3 && true`.
///
/// Copied verbatim from edit_source.rs:2353-2374.
const FIXTURE_U3_AND_TRUE: &str = r#"structure S {
    param u0: Bool = true
    param u1: Bool = true
    param u2: Bool = true
    param u3: Bool = true
    param u4: Bool = true
    param u5: Bool = true
    param u6: Bool = true
    param u7: Bool = true
    param u8: Bool = true
    param u9: Bool = true
    where u0 { let x0 = 1mm }
    where u1 { let x1 = 1mm }
    where u2 { let x2 = 1mm }
    where u3 && true { let x3 = 1mm }
    where u4 { let x4 = 1mm }
    where u5 { let x5 = 1mm }
    where u6 { let x6 = 1mm }
    where u7 { let x7 = 1mm }
    where u8 { let x8 = 1mm }
    where u9 { let x9 = 1mm }
}"#;

/// Byte-exact assertion: `ten_bool_guarded_groups("u3")` must equal the
/// canonical fixture literal with `where u3 { let x3 = 1mm }`.
#[test]
fn helper_u3_is_byte_exact() {
    let got = ten_bool_guarded_groups("u3");
    assert_eq!(
        got.as_str(),
        FIXTURE_U3,
        "ten_bool_guarded_groups(\"u3\") must be byte-exact"
    );
}

/// Byte-exact assertion: `ten_bool_guarded_groups("u3 && true")` must equal
/// the canonical fixture literal with `where u3 && true { let x3 = 1mm }`.
#[test]
fn helper_u3_and_true_is_byte_exact() {
    let got = ten_bool_guarded_groups("u3 && true");
    assert_eq!(
        got.as_str(),
        FIXTURE_U3_AND_TRUE,
        "ten_bool_guarded_groups(\"u3 && true\") must be byte-exact"
    );
}

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
