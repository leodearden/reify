//! Behavioural tests for the self-contained HTML formatter (`fmt_html`).
//!
//! Tests live in the integration `tests/` directory rather than `mod tests` inside
//! `fmt_html.rs` so that golden snapshots can be loaded from sibling
//! `tests/snapshots/` files without polluting the library binary.

use reify_doc::fmt_html::render_html;
use reify_doc::model::{
    AnnotationDoc, ConstraintDoc, DocModel, ItemDoc, ModuleDoc, ParamDoc, PortDoc,
};

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

/// A module with a multi-paragraph doc string must render its path as both the
/// `<title>` (in `<head>`) and the `<h1>` (in `<body>`), and its doc string
/// must be split on blank lines into one `<p>...</p>` per non-empty paragraph
/// in declaration order.  All-whitespace segments are skipped.
#[test]
fn module_header_and_doc_paragraphs_render() {
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "electronics.board".into(),
            doc: Some("Para one.\n\nPara two.\n\n   \n\nPara three.".into()),
            ..Default::default()
        }],
    };
    let out = render_html(&model, None);

    // Module path appears inside <title>.
    assert!(
        out.contains("<title>electronics.board</title>"),
        "missing module path in <title>; got:\n{out}"
    );

    // Module path appears as <h1> inside <body>.
    let h1_pos = out
        .find("<h1>electronics.board</h1>")
        .unwrap_or_else(|| panic!("missing <h1>electronics.board</h1>; got:\n{out}"));

    // Each non-empty paragraph appears as its own <p>...</p>.
    let p1_pos = out.find("<p>Para one.</p>")
        .unwrap_or_else(|| panic!("missing <p>Para one.</p>; got:\n{out}"));
    let p2_pos = out.find("<p>Para two.</p>")
        .unwrap_or_else(|| panic!("missing <p>Para two.</p>; got:\n{out}"));
    let p3_pos = out.find("<p>Para three.</p>")
        .unwrap_or_else(|| panic!("missing <p>Para three.</p>; got:\n{out}"));

    // Whitespace-only paragraph (the one with `   `) must NOT produce a <p></p>
    // entry — assert no empty <p>.
    assert!(
        !out.contains("<p></p>") && !out.contains("<p>   </p>") && !out.contains("<p> </p>"),
        "whitespace-only paragraph leaked into output:\n{out}"
    );

    // Positional ordering: <h1> precedes both <p>s, and paragraphs are in
    // declaration order.
    assert!(h1_pos < p1_pos, "<h1> must precede first <p>");
    assert!(p1_pos < p2_pos, "Para one must precede Para two");
    assert!(p2_pos < p3_pos, "Para two must precede Para three");
}

/// User-supplied content must be escaped before being inserted into HTML.
/// Asserts that `<`, `>`, `&`, `"`, `'` are translated to their entity
/// references in module path / doc strings.
///
/// Item-level escape coverage (Field name, type_repr) is exercised by the
/// snapshot test at step-31 once item bodies render through the same
/// `html_escape` helper introduced here.
#[test]
fn html_escape_handles_special_chars() {
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "x&y".into(),
            doc: Some("<script>alert('xss')&\"</script>".into()),
            ..Default::default()
        }],
    };
    let out = render_html(&model, None);

    // Module-level content escaping (path appears in <title> AND <h1>).
    assert!(
        out.contains("<title>x&amp;y</title>"),
        "module path must be HTML-escaped in <title>; got:\n{out}"
    );
    assert!(
        out.contains("<h1>x&amp;y</h1>"),
        "module path must be HTML-escaped in <h1>; got:\n{out}"
    );

    // The dangerous `<script>` substring from the doc must NOT appear literally.
    assert!(
        !out.contains("<script>alert"),
        "raw <script> escaped from doc must not appear; got:\n{out}"
    );
    // It must appear escaped instead.
    assert!(
        out.contains("&lt;script&gt;"),
        "doc must contain escaped `&lt;script&gt;`; got:\n{out}"
    );
    assert!(
        out.contains("&lt;/script&gt;"),
        "doc must contain escaped `&lt;/script&gt;`; got:\n{out}"
    );
    // Ampersand and double-quote escapes.
    assert!(out.contains("&amp;"), "expected &amp; for `&`; got:\n{out}");
    assert!(out.contains("&quot;"), "expected &quot; for `\"`; got:\n{out}");
    // Single-quote escape: accept either the named or numeric form.
    assert!(
        out.contains("&#x27;") || out.contains("&#39;"),
        "expected single-quote escape (`&#x27;` or `&#39;`); got:\n{out}"
    );
}

/// Build a single-item module model and render it.
fn render_one_item(item: ItemDoc) -> String {
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            items: vec![item],
            ..Default::default()
        }],
    };
    render_html(&model, None)
}

/// Each `ItemDoc` variant must render as `<section id="{name}">…<h2>…</h2>…`.
/// Verifies the H2 heading text matches the keyword/visibility/name
/// convention from `fmt_markdown::item_keyword`.
#[test]
fn item_section_h2_per_variant() {
    let cases: Vec<(ItemDoc, &str)> = vec![
        (
            ItemDoc::Structure {
                name: "Foo".into(), doc: None, is_pub: true,
                annotations: vec![], pragmas: vec![], params: vec![],
                ports: vec![], constraints: vec![], sub_components: vec![],
                realizations: vec![], meta: vec![],
            },
            "pub structure Foo",
        ),
        (
            ItemDoc::Occurrence {
                name: "Foo".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![], params: vec![],
                ports: vec![], constraints: vec![], sub_components: vec![],
                realizations: vec![], meta: vec![],
            },
            "occurrence Foo",
        ),
        (
            ItemDoc::Trait {
                name: "Foo".into(), doc: None, is_pub: true,
                annotations: vec![], pragmas: vec![], members: vec![],
            },
            "pub trait Foo",
        ),
        (
            ItemDoc::Function {
                name: "Foo".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                signature: "fn Foo()".into(),
            },
            "fn Foo",
        ),
        (
            ItemDoc::Field {
                name: "Foo".into(), doc: None, is_pub: true,
                annotations: vec![], pragmas: vec![],
                type_repr: "i32".into(), default_repr: None,
            },
            "pub let Foo",
        ),
        (
            ItemDoc::Purpose {
                name: "Foo".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                expr_repr: "x".into(), direction: "minimize".into(),
            },
            "purpose Foo",
        ),
        (
            ItemDoc::Enum {
                name: "Foo".into(), doc: None, is_pub: true,
                annotations: vec![], pragmas: vec![], variants: vec![],
            },
            "pub enum Foo",
        ),
        (
            ItemDoc::Unit {
                name: "Foo".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                base_unit: "Meter".into(), scale: "1.0".into(),
            },
            "unit Foo",
        ),
        (
            ItemDoc::TypeAlias {
                name: "Foo".into(), doc: None, is_pub: true,
                annotations: vec![], pragmas: vec![],
                type_repr: "f64".into(),
            },
            "pub type Foo",
        ),
        (
            ItemDoc::ConstraintDef {
                name: "Foo".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                expr_repr: "x > 0".into(),
            },
            "constraint Foo",
        ),
    ];

    for (item, expected_h2_text) in cases {
        let out = render_one_item(item);
        assert!(
            out.contains("<section id=\"Foo\">"),
            "missing wrapping <section id=\"Foo\"> for variant with H2 `{expected_h2_text}`; got:\n{out}"
        );
        let expected_h2 = format!("<h2>{expected_h2_text}</h2>");
        assert!(
            out.contains(&expected_h2),
            "missing `{expected_h2}` for variant; got:\n{out}"
        );
        assert!(
            out.contains("</section>"),
            "missing closing </section>; got:\n{out}"
        );
    }
}

/// Helper: build an `ItemDoc` of the given kind discriminant + name.
fn mk_item(kind: &str, name: &str) -> ItemDoc {
    match kind {
        "trait" => ItemDoc::Trait {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![], members: vec![],
        },
        "structure" => ItemDoc::Structure {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![], params: vec![],
            ports: vec![], constraints: vec![], sub_components: vec![],
            realizations: vec![], meta: vec![],
        },
        "occurrence" => ItemDoc::Occurrence {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![], params: vec![],
            ports: vec![], constraints: vec![], sub_components: vec![],
            realizations: vec![], meta: vec![],
        },
        "enum" => ItemDoc::Enum {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![], variants: vec![],
        },
        "function" => ItemDoc::Function {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![],
            signature: format!("fn {name}()"),
        },
        "field" => ItemDoc::Field {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![],
            type_repr: "i32".into(), default_repr: None,
        },
        _ => panic!("unknown kind: {kind}"),
    }
}

/// The TOC `<nav>` must contain a `<h2>Contents</h2>` plus per-group
/// `<h3>{Group}</h3>` headings (Traits → Structures → Occurrences → Enums →
/// Functions → Constants), each followed by an alphabetical `<ul>` of
/// `<li><a href="#name">name</a></li>` entries.  Empty groups are omitted.
/// The nav must appear after the module H1/doc and before the first item.
#[test]
fn toc_nav_renders_grouped_kinds_with_anchors() {
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            doc: Some("Top doc.".into()),
            items: vec![
                // Mixed kinds + within-group sort cases.
                mk_item("structure", "Bravo"),
                mk_item("structure", "Alpha"),
                mk_item("trait", "Iface"),
                mk_item("enum", "Color"),
                mk_item("function", "compute"),
                mk_item("field", "k"),
                mk_item("occurrence", "Inst"),
            ],
            ..Default::default()
        }],
    };
    let out = render_html(&model, None);

    assert!(out.contains("<nav>"), "missing <nav>; got:\n{out}");
    assert!(out.contains("</nav>"), "missing </nav>; got:\n{out}");
    let nav_start = out.find("<nav>").expect("nav start");
    let nav_end = out.find("</nav>").expect("nav end");
    let nav = &out[nav_start..nav_end];

    assert!(nav.contains("<h2>Contents</h2>"),
        "missing <h2>Contents</h2> in nav:\n{nav}");
    // Per-group headings present.
    for h3 in &["<h3>Traits</h3>", "<h3>Structures</h3>", "<h3>Occurrences</h3>",
                "<h3>Enums</h3>", "<h3>Functions</h3>", "<h3>Constants</h3>"] {
        assert!(nav.contains(h3), "missing {h3} in nav:\n{nav}");
    }
    // Fixed group ordering.
    let pos_traits = nav.find("<h3>Traits</h3>").unwrap();
    let pos_structures = nav.find("<h3>Structures</h3>").unwrap();
    let pos_occ = nav.find("<h3>Occurrences</h3>").unwrap();
    let pos_enums = nav.find("<h3>Enums</h3>").unwrap();
    let pos_fns = nav.find("<h3>Functions</h3>").unwrap();
    let pos_consts = nav.find("<h3>Constants</h3>").unwrap();
    assert!(pos_traits < pos_structures);
    assert!(pos_structures < pos_occ);
    assert!(pos_occ < pos_enums);
    assert!(pos_enums < pos_fns);
    assert!(pos_fns < pos_consts);

    // Anchor-link entries are <li><a href="#name">name</a></li>.
    assert!(nav.contains("<li><a href=\"#Alpha\">Alpha</a></li>"),
        "expected anchor for Alpha in nav:\n{nav}");
    assert!(nav.contains("<li><a href=\"#Bravo\">Bravo</a></li>"));
    assert!(nav.contains("<li><a href=\"#Iface\">Iface</a></li>"));
    assert!(nav.contains("<li><a href=\"#Color\">Color</a></li>"));
    assert!(nav.contains("<li><a href=\"#compute\">compute</a></li>"));
    assert!(nav.contains("<li><a href=\"#Inst\">Inst</a></li>"));
    assert!(nav.contains("<li><a href=\"#k\">k</a></li>"));

    // Within-group alphabetical sort: Alpha appears before Bravo in the nav.
    let pos_alpha = nav.find("<li><a href=\"#Alpha\">").unwrap();
    let pos_bravo = nav.find("<li><a href=\"#Bravo\">").unwrap();
    assert!(pos_alpha < pos_bravo, "within-group alphabetical sort failed");

    // Position: <h1> precedes <nav> precedes the first <section>.
    let h1_pos = out.find("<h1>m</h1>").expect("h1");
    let nav_pos = out.find("<nav>").expect("nav");
    let first_section = out.find("<section id=").expect("section");
    assert!(h1_pos < nav_pos, "<h1> must precede <nav>");
    assert!(nav_pos < first_section, "<nav> must precede first <section>");
}

/// Helper: build a Structure containing a single `params` list.
fn structure_with_params(name: &str, params: Vec<ParamDoc>) -> ItemDoc {
    ItemDoc::Structure {
        name: name.into(), doc: None, is_pub: true,
        annotations: vec![], pragmas: vec![],
        params, ports: vec![], constraints: vec![],
        sub_components: vec![], realizations: vec![],
        meta: vec![],
    }
}

/// Helper: build a Structure with the given ports list.
fn structure_with_ports(name: &str, ports: Vec<PortDoc>) -> ItemDoc {
    ItemDoc::Structure {
        name: name.into(), doc: None, is_pub: true,
        annotations: vec![], pragmas: vec![],
        params: vec![], ports, constraints: vec![],
        sub_components: vec![], realizations: vec![],
        meta: vec![],
    }
}

/// Helper: build a Structure with the given constraints list.
fn structure_with_constraints(name: &str, constraints: Vec<ConstraintDoc>) -> ItemDoc {
    ItemDoc::Structure {
        name: name.into(), doc: None, is_pub: true,
        annotations: vec![], pragmas: vec![],
        params: vec![], ports: vec![], constraints,
        sub_components: vec![], realizations: vec![],
        meta: vec![],
    }
}

/// Helper: build a Structure with the given meta list.
fn structure_with_meta(name: &str, meta: Vec<(String, String)>) -> ItemDoc {
    ItemDoc::Structure {
        name: name.into(), doc: None, is_pub: true,
        annotations: vec![], pragmas: vec![],
        params: vec![], ports: vec![], constraints: vec![],
        sub_components: vec![], realizations: vec![],
        meta,
    }
}

/// Parameters table renders with a 5-column header and one row per param.
/// Default cell uses an em-dash placeholder when `default_repr.is_none()`;
/// `solver_hint` annotation appends `<em>hint: …</em>` to the description.
#[test]
fn parameters_table_renders() {
    let item = structure_with_params(
        "Bolt",
        vec![
            ParamDoc {
                name: "length".into(),
                doc: Some("Bolt length.".into()),
                type_repr: "Length".into(),
                default_repr: Some("100 mm".into()),
                annotations: vec![],
            },
            ParamDoc {
                name: "diameter".into(),
                doc: None,
                type_repr: "Length".into(),
                default_repr: None,
                annotations: vec![AnnotationDoc {
                    name: "solver_hint".into(),
                    args: vec!["\"discrete\"".into()],
                }],
            },
        ],
    );
    let out = render_one_item(item);

    // Section header.
    assert!(out.contains("<h3>Parameters</h3>"), "missing <h3>Parameters</h3>; got:\n{out}");
    // Table header: 5 columns.
    assert!(out.contains("<table>"), "missing <table>; got:\n{out}");
    assert!(out.contains("<thead>"), "missing <thead>");
    assert!(out.contains("<th>Name</th>"));
    assert!(out.contains("<th>Type</th>"));
    assert!(out.contains("<th>Dimension</th>"));
    assert!(out.contains("<th>Default</th>"));
    assert!(out.contains("<th>Description</th>"));
    // Rows: name and type wrapped in <code>, dim placeholder, default Some/None handling.
    assert!(out.contains("<td><code>length</code></td>"),
        "expected length name in <code>; got:\n{out}");
    assert!(out.contains("<td><code>Length</code></td>"),
        "expected Length type in <code>; got:\n{out}");
    assert!(out.contains("<td><code>100 mm</code></td>"),
        "expected default for length; got:\n{out}");
    // diameter has no default — em-dash.
    let em = "—";
    assert!(out.contains(em), "expected em-dash placeholder; got:\n{out}");
    // solver_hint suffix on the diameter row.
    assert!(out.contains("<em>hint: discrete</em>"),
        "expected solver_hint suffix; got:\n{out}");
    // Description for length contains its doc text.
    assert!(out.contains("Bolt length."),
        "expected length doc in description cell; got:\n{out}");
}

/// Empty params list must produce no `<h3>Parameters</h3>` and no `<table>`.
#[test]
fn parameters_table_omitted_when_empty() {
    let item = structure_with_params("Bolt", vec![]);
    let out = render_one_item(item);
    assert!(!out.contains("<h3>Parameters</h3>"));
    assert!(!out.contains("<table>"));
}

/// Ports table: `<h3>Ports</h3>` plus a 5-column `<table>` with
/// Name | Kind | Role | Type | Description.  Kind is em-dash placeholder
/// (PortDoc has no kind field), Description joins members with ", " and
/// uses em-dash when empty.
#[test]
fn ports_table_renders() {
    let item = structure_with_ports(
        "Board",
        vec![
            PortDoc {
                name: "pwr_in".into(),
                direction: "in".into(),
                type_name: "Power".into(),
                members: vec!["voltage".into(), "current".into()],
            },
            PortDoc {
                name: "ant".into(),
                direction: "out".into(),
                type_name: "RF".into(),
                members: vec![],
            },
        ],
    );
    let out = render_one_item(item);

    assert!(out.contains("<h3>Ports</h3>"), "missing <h3>Ports</h3>; got:\n{out}");
    // 5-column header.
    assert!(out.contains("<th>Name</th>"));
    assert!(out.contains("<th>Kind</th>"));
    assert!(out.contains("<th>Role</th>"));
    assert!(out.contains("<th>Type</th>"));
    assert!(out.contains("<th>Description</th>"));
    // Name/Type wrapped in <code>.
    assert!(out.contains("<td><code>pwr_in</code></td>"),
        "expected pwr_in in <code>; got:\n{out}");
    assert!(out.contains("<td><code>Power</code></td>"));
    assert!(out.contains("<td><code>ant</code></td>"));
    assert!(out.contains("<td><code>RF</code></td>"));
    // Direction in Role column.
    assert!(out.contains("<td>in</td>"), "expected `in` Role cell; got:\n{out}");
    assert!(out.contains("<td>out</td>"), "expected `out` Role cell; got:\n{out}");
    // Members joined by ", " in Description; em-dash when empty.
    assert!(out.contains("voltage, current"),
        "expected joined members; got:\n{out}");
}

/// Empty ports list must produce no `<h3>Ports</h3>` and no table for ports.
#[test]
fn ports_table_omitted_when_empty() {
    let item = structure_with_ports("Board", vec![]);
    let out = render_one_item(item);
    assert!(!out.contains("<h3>Ports</h3>"));
}

/// Constraints section: `<h3>Constraints</h3>` plus a `<ul>` of `<li>`s.
/// Each entry has the form `[label: ]<code>{escaped-expr}</code>[ <em>(line N)</em>]`.
#[test]
fn constraints_section_renders() {
    let item = structure_with_constraints(
        "Bolt",
        vec![
            ConstraintDoc {
                label: None,
                expr_repr: "length >= diameter".into(),
                annotations: vec![],
                line: Some(42),
            },
            ConstraintDoc {
                label: Some("safe_v".into()),
                expr_repr: "v <= 5.5 V".into(),
                annotations: vec![],
                line: None,
            },
        ],
    );
    let out = render_one_item(item);

    // Section header and <ul> wrapper.
    assert!(
        out.contains("<h3>Constraints</h3>"),
        "missing <h3>Constraints</h3>; got:\n{out}"
    );
    assert!(out.contains("<ul>"), "missing <ul>; got:\n{out}");
    // First entry: labelless, escaped `>=`, line annotation.
    assert!(
        out.contains("<code>length &gt;= diameter</code>"),
        "expected labelless expr in <code> with `>=` escaped to `&gt;=`; got:\n{out}"
    );
    assert!(
        out.contains("<em>(line 42)</em>"),
        "expected `<em>(line 42)</em>` suffix on first entry; got:\n{out}"
    );
    // Second entry: labelled, escaped `<=`, no line suffix.
    assert!(
        out.contains("safe_v: <code>v &lt;= 5.5 V</code>"),
        "expected `safe_v: <code>v &lt;= 5.5 V</code>` for labelled entry; got:\n{out}"
    );
    // Confirm only ONE `(line ` substring exists (the labelless entry's),
    // proving the `None`-line entry has no line suffix.
    assert_eq!(
        out.matches("(line ").count(),
        1,
        "expected exactly one `(line ` substring (only the first entry has Some(42)); got:\n{out}"
    );
}

/// Empty constraints list must produce no `<h3>Constraints</h3>`.
#[test]
fn constraints_section_omitted_when_empty() {
    let item = structure_with_constraints("Bolt", vec![]);
    let out = render_one_item(item);
    assert!(!out.contains("<h3>Constraints</h3>"));
}

/// Meta section: `<h3>Meta</h3>` then `<dl>` with `<dt>{key}</dt><dd>{value}</dd>`
/// pairs sorted alphabetically by key, regardless of insertion order.
#[test]
fn meta_section_renders_alphabetical() {
    let item = structure_with_meta(
        "Bolt",
        vec![
            ("version".into(), "1.0".into()),
            ("alpha".into(), "yes".into()),
        ],
    );
    let out = render_one_item(item);

    assert!(
        out.contains("<h3>Meta</h3>"),
        "missing <h3>Meta</h3>; got:\n{out}"
    );
    assert!(out.contains("<dl>"), "missing <dl>; got:\n{out}");
    assert!(
        out.contains("<dt>alpha</dt><dd>yes</dd>"),
        "missing alpha pair; got:\n{out}"
    );
    assert!(
        out.contains("<dt>version</dt><dd>1.0</dd>"),
        "missing version pair; got:\n{out}"
    );
    // Verify alphabetical ordering: alpha must precede version.
    let alpha_pos = out
        .find("<dt>alpha</dt>")
        .expect("alpha key must be present");
    let version_pos = out
        .find("<dt>version</dt>")
        .expect("version key must be present");
    assert!(
        alpha_pos < version_pos,
        "expected alphabetical order (alpha before version); got alpha@{alpha_pos} version@{version_pos}\n{out}"
    );
}

/// Empty meta list must produce no `<h3>Meta</h3>` and no `<dl>`.
#[test]
fn meta_section_omitted_when_empty() {
    let item = structure_with_meta("Bolt", vec![]);
    let out = render_one_item(item);
    assert!(!out.contains("<h3>Meta</h3>"));
    assert!(!out.contains("<dl>"));
}

/// Empty module (no items) must produce no `<nav>` at all.
#[test]
fn toc_nav_omitted_when_no_items() {
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            ..Default::default()
        }],
    };
    let out = render_html(&model, None);
    assert!(!out.contains("<nav>"), "expected no <nav> for empty module; got:\n{out}");
}
