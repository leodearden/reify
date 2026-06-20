//! Shared test helpers for `reify-eval` integration test binaries.
//!
//! Include in a test binary with `mod common;` at the top of the file.
//! Helpers are `pub` so they are visible after `use common::{...}`.

pub mod alloc_counter;
pub mod as_printed;

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
/// mod common;
/// use common::ten_bool_guarded_groups;
///
/// let src = ten_bool_guarded_groups("u3");
/// let module = parse_and_compile(&src);
/// ```
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
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
