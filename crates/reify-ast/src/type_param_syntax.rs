//! Surface-syntax rendering of one declared type parameter.

use std::fmt::{self, Write};

/// Render one type parameter as declared: `T`, `Q: Dimension`, `T: A + B`,
/// `V: A + B = Int`. Bounds keep their given order.
///
/// The default is rendered through the caller's own `Display`, so the caller
/// chooses between the author's spelling (a [`crate::TypeExpr`]) and a
/// resolved type.
pub fn render_type_param<'a>(
    name: &str,
    bounds: impl IntoIterator<Item = &'a str>,
    default: Option<impl fmt::Display>,
) -> String {
    let mut rendered = name.to_owned();
    let mut separator = ": ";
    for bound in bounds {
        rendered.push_str(separator);
        rendered.push_str(bound);
        separator = " + ";
    }
    if let Some(default) = default {
        let _ = write!(rendered, " = {default}");
    }
    rendered
}
