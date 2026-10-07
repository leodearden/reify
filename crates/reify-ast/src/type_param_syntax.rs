//! Surface-syntax rendering of one declared type parameter.

use std::fmt;

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
    let bounds: Vec<&str> = bounds.into_iter().collect();
    if !bounds.is_empty() {
        rendered.push_str(": ");
        rendered.push_str(&bounds.join(" + "));
    }
    if let Some(default) = default {
        rendered.push_str(&format!(" = {default}"));
    }
    rendered
}
