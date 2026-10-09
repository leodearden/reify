//! Behaviour of `reify_ast::render_type_param`, the surface-syntax renderer
//! for one declared type parameter: `name`, then `": "` + bounds joined with
//! `" + "`, then `" = "` + the caller-rendered default.

use std::fmt;

use reify_ast::render_type_param;

#[test]
fn name_only_renders_bare_name() {
    assert_eq!(render_type_param("T", [] as [&str; 0], None::<&str>), "T");
}

#[test]
fn single_bound_follows_colon() {
    assert_eq!(
        render_type_param("Q", ["Dimension"], None::<&str>),
        "Q: Dimension"
    );
}

#[test]
fn multiple_bounds_join_with_plus_in_declared_order() {
    assert_eq!(render_type_param("T", ["A", "B"], None::<&str>), "T: A + B");
}

#[test]
fn default_without_bounds_follows_equals() {
    assert_eq!(
        render_type_param("T", [] as [&str; 0], Some("Real")),
        "T = Real"
    );
}

#[test]
fn bounds_precede_default() {
    assert_eq!(
        render_type_param("V", ["A", "B"], Some("Int")),
        "V: A + B = Int"
    );
}

/// A default whose `Display` output is computed from its field rather than
/// stored, standing in for a resolved type such as `reify_core::Type`.
struct LengthPower(i32);

impl fmt::Display for LengthPower {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Scalar[m^{}]", self.0)
    }
}

#[test]
fn non_string_default_renders_through_its_own_display() {
    assert_eq!(
        render_type_param("Q", ["Dimension"], Some(LengthPower(2))),
        "Q: Dimension = Scalar[m^2]"
    );
}
