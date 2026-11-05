//! The source a design created by New… starts with (task 9730).

use reify_core::ModulePath;

/// The starter design: a parametric flange with one geometry `let`, so a new
/// file renders something the moment it opens. Its own copy of the
/// `examples/m5_geometry.ri` design rather than an `include_str!` of that
/// fixture, which other tests consume and may change.
const STARTER_BODY: &str = "\
structure def Flange {
    param radius : Length = 50mm
    param height : Length = 10mm
    param hole_count : Int = 6
    param bolt_circle_radius : Length = 35mm
    param hole_radius : Length = 3mm

    constraint radius > bolt_circle_radius
    constraint hole_count > 0

    let body = cylinder(radius, height)
}
";

/// The starter source for a new `<stem>.ri`, opened by `module <stem>` when the
/// parser accepts that declaration for a file of that stem. A stem that cannot
/// be declared (a hyphenated name, a keyword) gets the body alone, which costs
/// only a `W_MODULE_DECL_MISSING` warning where a bad module line would be an
/// error.
pub fn new_file_template(stem: &str) -> String {
    let declared = format!("module {stem}\n\n{STARTER_BODY}");
    if declares_cleanly(&declared, stem) {
        declared
    } else {
        STARTER_BODY.to_string()
    }
}

/// The parser is the one judge of a declarable stem: `source` parses with no
/// errors and its module line matches the path a `<stem>.ri` file resolves to.
fn declares_cleanly(source: &str, stem: &str) -> bool {
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(stem));
    parsed.errors.is_empty()
        && reify_compiler::check_module_path_decl(
            parsed.declared_module_path.as_ref(),
            &parsed.path,
        )
        .is_none()
}
