use reify_core::ModulePath;

use crate::new_file_template::new_file_template;

/// Parse `src` the way the engine does for a file named `<stem>.ri`, and
/// return the parse errors rendered against it alongside the module-path
/// diagnostic (`W_MODULE_DECL_MISSING` / `E_MODULE_PATH_MISMATCH`), if any.
fn parse_as(src: &str, stem: &str) -> (Vec<String>, Option<String>) {
    let parsed = reify_compiler::parse_with_stdlib(src, ModulePath::single(stem));
    let errors = parsed.errors.iter().map(|e| format!("{e:?}")).collect();
    let module_diag =
        reify_compiler::check_module_path_decl(parsed.declared_module_path.as_ref(), &parsed.path)
            .map(|d| format!("{d:?}"));
    (errors, module_diag)
}

fn assert_has_starter_body(src: &str) {
    assert!(
        src.contains("structure def "),
        "the New… template must carry a `structure def`, got:\n{src}"
    );
    assert!(
        src.lines().any(|l| l.trim_start().starts_with("let ")),
        "the New… template must carry a geometry `let` so a new file renders, got:\n{src}"
    );
}

#[test]
fn declarable_stem_gets_a_matching_module_line() {
    let src = new_file_template("bracket_two");
    assert_eq!(
        src.lines().next(),
        Some("module bracket_two"),
        "a declarable stem must open with its own module line, got:\n{src}"
    );
    let (errors, module_diag) = parse_as(&src, "bracket_two");
    assert!(errors.is_empty(), "template must parse cleanly: {errors:?}\n{src}");
    assert_eq!(
        module_diag, None,
        "the module line must satisfy check_module_path_decl (no missing/mismatch diagnostic)"
    );
    assert_has_starter_body(&src);
}

#[test]
fn undeclarable_stems_omit_the_module_line_and_still_parse() {
    for stem in ["my-part", "structure"] {
        let src = new_file_template(stem);
        assert!(
            !src.lines().any(|l| l.trim_start().starts_with("module")),
            "stem {stem:?} cannot be declared, so the template must carry no module line, got:\n{src}"
        );
        let (errors, _) = parse_as(&src, stem);
        assert!(
            errors.is_empty(),
            "template for stem {stem:?} must parse cleanly: {errors:?}\n{src}"
        );
        assert_has_starter_body(&src);
    }
}
