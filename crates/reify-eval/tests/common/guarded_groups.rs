//! The canonical 10-group guarded-group fixture, shared by the `edit_source` and
//! `guard_eval` test binaries.
//!
//! A module cannot be shared across integration-test crate roots, so each declares
//! `#[path = "common/guarded_groups.rs"] mod guarded_groups;`, following the
//! `eval_gate_support.rs` / `differential.rs` convention.

use std::fmt::Write as _;

/// Build the canonical 10-group guarded-group fixture source string.
///
/// Produces a `structure S` with:
/// - 10 params: `param u0: Bool = true` … `param u9: Bool = true`
/// - 10 guarded groups: `where uN { let xN = 1mm }` for N ≠ 3;
///   group 3 uses `group3_guard_expr` as its guard expression.
///
/// The fixture uses 4-space indentation and LF line endings; the final `}` has
/// no trailing newline. Passing `"u3"` reproduces the canonical fixture used by
/// most callers.
///
/// # Usage
///
/// ```rust,ignore
/// #[path = "common/guarded_groups.rs"]
/// mod guarded_groups;
/// use guarded_groups::ten_bool_guarded_groups;
///
/// let src = ten_bool_guarded_groups("u3");
/// let module = parse_and_compile(&src);
/// ```
pub fn ten_bool_guarded_groups(group3_guard_expr: &str) -> String {
    let mut s = String::from("structure S {\n");
    for n in 0..10u32 {
        writeln!(s, "    param u{}: Bool = true", n).unwrap();
    }
    for n in 0..10u32 {
        if n == 3 {
            writeln!(s, "    where {} {{ let x{} = 1mm }}", group3_guard_expr, n).unwrap();
        } else {
            writeln!(s, "    where u{} {{ let x{} = 1mm }}", n, n).unwrap();
        }
    }
    s.push('}');
    s
}
