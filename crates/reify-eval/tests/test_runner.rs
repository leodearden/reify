use reify_constraints::SimpleConstraintChecker;
use reify_eval::run_tests;
use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity};

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

// Step 11: single passing test
#[test]
fn run_tests_with_single_passing_structure_returns_pass() {
    let compiled = parse_and_compile(
        "@test structure TestA { param x : Length = 5mm\n constraint x > 0mm }",
    );
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(results.len(), 1, "expected 1 test result");
    assert_eq!(results[0].name, "TestA");
    assert_eq!(results[0].status, reify_eval::TestStatus::Pass);
    assert!(
        !results[0].constraint_results.is_empty(),
        "expected at least one constraint result"
    );
    for entry in &results[0].constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "expected all Satisfied"
        );
    }
}

// Step 12: single failing (violated) test
#[test]
fn run_tests_with_single_violated_structure_returns_fail() {
    let compiled = parse_and_compile(
        "@test structure TestA { param x : Length = -3mm\n constraint x > 0mm }",
    );
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "TestA");
    assert_eq!(results[0].status, reify_eval::TestStatus::Fail);
    assert!(
        results[0]
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Violated),
        "expected at least one Violated constraint"
    );
}

// Step 13: indeterminate test using MockConstraintChecker
#[test]
fn run_tests_with_indeterminate_constraint_returns_indeterminate() {
    let compiled = parse_and_compile(
        "@test structure TestA { param x : Length = 1mm\n constraint x > 0mm }",
    );
    let results = run_tests(&compiled, || {
        Box::new(MockConstraintChecker::new().with_default(Satisfaction::Indeterminate))
    });
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, reify_eval::TestStatus::Indeterminate);
}

// Step 14: multiple tests with mixed outcomes
#[test]
fn run_tests_with_multiple_tests_returns_mixed_results() {
    let source = "@test structure TestPass { param x : Length = 5mm\n constraint x > 0mm }\n@test structure TestFail { param y : Length = -3mm\n constraint y > 0mm }\nstructure Prod { param z : Length = 1mm\n constraint z > 0mm }";
    let compiled = parse_and_compile(source);
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(
        results.len(),
        2,
        "expected 2 test results (TestPass, TestFail); Prod must be excluded"
    );
    let by_name: std::collections::HashMap<&str, reify_eval::TestStatus> =
        results.iter().map(|r| (r.name.as_str(), r.status)).collect();
    assert_eq!(by_name.get("TestPass"), Some(&reify_eval::TestStatus::Pass));
    assert_eq!(by_name.get("TestFail"), Some(&reify_eval::TestStatus::Fail));
    assert!(
        !by_name.contains_key("Prod"),
        "non-test template must not produce a TestResult"
    );
}
