//! Structural contract tests for `common::ten_bool_guarded_groups`.
//!
//! Exists to lock the helper's template against accidental drift (e.g. a
//! change to the `param uN: Bool = true` / `where uN { let xN = 1mm }` shape
//! would silently invalidate the perf-lock fixtures in `edit_source.rs` and
//! `guard_eval.rs`). Kept minimal — end-to-end behavior is already covered by
//! those perf-lock callers via counter-pinning.

mod common;

use common::ten_bool_guarded_groups;

#[test]
fn injects_custom_guard_only_at_group_3() {
    let src = ten_bool_guarded_groups("u3 && true");

    assert!(
        src.contains("    where u3 && true { let x3 = 1mm }"),
        "group 3 should carry the custom guard expression, got:\n{src}"
    );
    for n in [0u32, 1, 2, 4, 5, 6, 7, 8, 9] {
        let expected = format!("    where u{n} {{ let x{n} = 1mm }}");
        assert!(
            src.contains(&expected),
            "group {n} should retain the default `where u{n}` guard, got:\n{src}"
        );
    }
    for n in 0..10u32 {
        let expected = format!("    param u{n}: Bool = true");
        assert!(
            src.contains(&expected),
            "expected param declaration for u{n}, got:\n{src}"
        );
    }
}
