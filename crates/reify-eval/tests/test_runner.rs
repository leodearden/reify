use reify_constraints::SimpleConstraintChecker;
use reify_eval::run_tests;
use reify_types::{ModulePath, Severity};

fn parse_and_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test_runner_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    compiled
}

#[test]
fn run_tests_on_module_with_no_tests_returns_empty_vec() {
    let compiled = parse_and_compile("structure def B { param y : Real\n constraint y > 0 }");
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert!(
        results.is_empty(),
        "expected empty Vec for module with no @test templates, got {} results",
        results.len()
    );
}
