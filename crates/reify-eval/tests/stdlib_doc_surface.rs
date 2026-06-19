//! Integration test: stdlib doc surface (GR-042 / task-3565).
//!
//! Calls `build_stdlib_doc_model()` + `render_html_pages()` against the REAL
//! compiled stdlib and asserts the leaf observable: the index page contains
//! known stdlib symbols (ElasticMaterial, Bounded, Manifold) and per-symbol
//! pages exist.
//!
//! Library-level test (no binary subprocess) because reify-eval's test target
//! cannot resolve CARGO_BIN_EXE_reify.

#[test]
fn stdlib_doc_surface_index_lists_known_symbols() {
    let model = reify_doc_build::build_stdlib_doc_model();
    let pages = reify_doc::fmt_html::render_html_pages(&model, None);

    // (a) Multiple modules produced (the stdlib has many .ri files).
    assert!(
        model.modules.len() > 1,
        "expected multiple stdlib modules, got {}",
        model.modules.len()
    );

    // (b) Pages vec has more than just index.html.
    assert!(
        pages.len() > 1,
        "expected more than one page, got {}",
        pages.len()
    );

    // (c) First page is index.html and contains the known symbol names.
    let (ref idx_name, ref idx_body) = pages[0];
    assert_eq!(idx_name, "index.html", "first page must be index.html");
    assert!(
        idx_body.contains("ElasticMaterial"),
        "index.html must contain 'ElasticMaterial'; got (truncated):\n{}",
        &idx_body[..idx_body.len().min(4000)]
    );
    assert!(
        idx_body.contains("Bounded"),
        "index.html must contain 'Bounded'; got (truncated):\n{}",
        &idx_body[..idx_body.len().min(4000)]
    );
    assert!(
        idx_body.contains("Manifold"),
        "index.html must contain 'Manifold'; got (truncated):\n{}",
        &idx_body[..idx_body.len().min(4000)]
    );

    // (d) At least one per-symbol page filename ends with "trait-Bounded.html"
    //     and at least one ends with "trait-ElasticMaterial.html".
    let filenames: Vec<&str> = pages.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        filenames.iter().any(|f| f.ends_with("trait-Bounded.html")),
        "expected a page ending in 'trait-Bounded.html' in {:?}",
        &filenames[..filenames.len().min(30)]
    );
    assert!(
        filenames
            .iter()
            .any(|f| f.ends_with("trait-ElasticMaterial.html")),
        "expected a page ending in 'trait-ElasticMaterial.html' in {:?}",
        &filenames[..filenames.len().min(30)]
    );
}
