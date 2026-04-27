//! Behavioural tests for the self-contained HTML formatter (`fmt_html`).
//!
//! Tests live in the integration `tests/` directory rather than `mod tests` inside
//! `fmt_html.rs` so that golden snapshots can be loaded from sibling
//! `tests/snapshots/` files without polluting the library binary.

use reify_doc::fmt_html::render_html;
use reify_doc::model::DocModel;

/// `render_html` on the default (empty) `DocModel` must produce a structurally
/// well-formed HTML5 document that is *self-contained*: no `<link>` / `<script>` /
/// `<iframe>` / `<img>` / `http(s)://` / `@import` references.
#[test]
fn empty_model_yields_self_contained_html5_skeleton() {
    let out = render_html(&DocModel::default(), None);

    // Positive structural assertions.
    assert!(
        out.starts_with("<!DOCTYPE html>"),
        "output must start with `<!DOCTYPE html>`, got: {out:?}"
    );
    assert!(
        out.contains("<html lang=\"en\">"),
        "output must contain `<html lang=\"en\">`, got:\n{out}"
    );
    assert!(out.contains("<head>"), "missing <head> in:\n{out}");
    assert!(
        out.contains("<meta charset=\"utf-8\">"),
        "missing <meta charset=\"utf-8\"> in:\n{out}"
    );
    assert!(out.contains("<title>"), "missing <title> in:\n{out}");
    assert!(out.contains("</title>"), "missing </title> in:\n{out}");
    assert!(out.contains("<style>"), "missing <style> in:\n{out}");
    assert!(out.contains("</style>"), "missing </style> in:\n{out}");
    assert!(out.contains("</head>"), "missing </head> in:\n{out}");
    assert!(out.contains("<body>"), "missing <body> in:\n{out}");
    assert!(out.contains("</body>"), "missing </body> in:\n{out}");
    assert!(out.contains("</html>"), "missing </html> in:\n{out}");

    // Negative self-containment assertions: no external resource references.
    let forbidden = [
        "<link ", "<script", "<iframe", "<img ",
        "http://", "https://", "@import",
    ];
    for s in &forbidden {
        assert!(
            !out.contains(s),
            "output must NOT contain `{s}` (self-contained guarantee), got:\n{out}"
        );
    }
}
