use reify_core::Severity;
use reify_test_support::compile_source_with_stdlib;

#[test]
fn probe_generic_enum_return_type() {
    let source = r#"
fn probe3<T, E>(r: Result<T, E>) -> Result<T, E> { r }
"#;
    let module = compile_source_with_stdlib(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    eprintln!("DIAGNOSTICS: {:#?}", errors);
    let probe = module.functions.iter().find(|f| f.name == "probe3");
    eprintln!("RETURN TYPE: {:?}", probe.map(|f| &f.return_type));
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}
