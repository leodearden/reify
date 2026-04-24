//! Shared test helpers for `reify-eval` integration test binaries.
//!
//! Include in a test binary with `mod common;` at the top of the file.
//! Helpers are `pub` so they are visible after `use common::{...}`.

/// Build the canonical 10-group guarded-group fixture source string.
///
/// Produces a `structure S` with:
/// - 10 params: `param u0: Bool = true` … `param u9: Bool = true`
/// - 10 guarded groups: `where uN { let xN = 1mm }` for N ≠ 3;
///   group 3 uses `group3_guard_expr` as its guard expression.
///
/// The returned `String` is byte-exact with the raw-string literals that were
/// previously inlined in `edit_source.rs` and `guard_eval.rs`; passing `"u3"`
/// reproduces the canonical fixture used by most callers.
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
        s.push_str(&format!("    param u{}: Bool = true\n", n));
    }
    for n in 0..10u32 {
        if n == 3 {
            s.push_str(&format!(
                "    where {} {{ let x{} = 1mm }}\n",
                group3_guard_expr, n
            ));
        } else {
            s.push_str(&format!("    where u{} {{ let x{} = 1mm }}\n", n, n));
        }
    }
    s.push('}');
    s
}
