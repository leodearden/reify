use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_eval::run_tests;
use reify_ir::Satisfaction;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;

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
    let compiled =
        parse_and_compile("@test structure TestA { param x : Length = 5mm\n constraint x > 0mm }");
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
    let compiled =
        parse_and_compile("@test structure TestA { param x : Length = -3mm\n constraint x > 0mm }");
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
    let compiled =
        parse_and_compile("@test structure TestA { param x : Length = 1mm\n constraint x > 0mm }");
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
    let by_name: std::collections::HashMap<&str, reify_eval::TestStatus> = results
        .iter()
        .map(|r| (r.name.as_str(), r.status))
        .collect();
    assert_eq!(by_name.get("TestPass"), Some(&reify_eval::TestStatus::Pass));
    assert_eq!(by_name.get("TestFail"), Some(&reify_eval::TestStatus::Fail));
    assert!(
        !by_name.contains_key("Prod"),
        "non-test template must not produce a TestResult"
    );
}

// Step 15: non-test template constraint failures must NOT affect test results
#[test]
fn run_tests_ignores_nontest_template_constraint_failures() {
    // Broken has a violated constraint (y = -10mm, y > 0mm), TestA has a passing constraint.
    // TestA's result should be Pass regardless of Broken's violation.
    let source = "@test structure TestA { param x : Length = 5mm\n constraint x > 0mm }\nstructure Broken { param y : Length = -10mm\n constraint y > 0mm }";
    let compiled = parse_and_compile(source);
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "TestA");
    assert_eq!(
        results[0].status,
        reify_eval::TestStatus::Pass,
        "Broken's violated constraint must NOT pollute TestA's result"
    );
    for entry in &results[0].constraint_results {
        assert_eq!(
            entry.id.entity, "TestA",
            "all constraint results must be from TestA, not Broken"
        );
    }
}

// Step 16: engine state isolation between tests (fail then pass)
#[test]
fn run_tests_isolates_engine_state_between_tests() {
    let source = "@test structure TestFail { param x : Length = -1mm\n constraint x > 0mm }\n@test structure TestPass { param y : Length = 5mm\n constraint y > 0mm }";
    let compiled = parse_and_compile(source);
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(results.len(), 2);
    let by_name: std::collections::HashMap<&str, reify_eval::TestStatus> = results
        .iter()
        .map(|r| (r.name.as_str(), r.status))
        .collect();
    assert_eq!(by_name.get("TestFail"), Some(&reify_eval::TestStatus::Fail));
    assert_eq!(
        by_name.get("TestPass"),
        Some(&reify_eval::TestStatus::Pass),
        "TestPass must be Pass even after TestFail ran — engines must be isolated"
    );
}

// Step 17: diagnostics propagation — violated constraint produces non-empty diagnostics
#[test]
fn run_tests_propagates_violation_diagnostics() {
    let source = "constraint def Positive { param v : Length\n v > 0mm }\n@test structure TestA { param x : Length = -1mm\n constraint Positive(v: x) }";
    let compiled = parse_and_compile(source);
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "TestA");
    assert_eq!(results[0].status, reify_eval::TestStatus::Fail);
    assert!(
        results[0]
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Violated),
        "expected at least one Violated constraint result, got: {:?}",
        results[0].constraint_results
    );
    assert!(
        results[0]
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error
                && d.message.contains("Positive#0[0]")
                && d.message.contains("violated")
                && d.message.contains("constraint")),
        "expected at least one Error diagnostic with message containing 'Positive[0]', 'violated', and 'constraint', got: {:?}",
        results[0].diagnostics
    );
}

// Step 18: sub-component reference to non-test template as fixture
#[test]
fn run_tests_supports_subcomponent_references_to_nontest_templates() {
    let source = "structure Widget { param size : Length = 10mm }\n@test structure TestWidgetFits {\n  sub w = Widget()\n  constraint self.w.size > 0mm\n}";
    let compiled = parse_and_compile(source);
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "TestWidgetFits");
    assert_eq!(
        results[0].status,
        reify_eval::TestStatus::Pass,
        "sub-component reference should elaborate w.size = 10mm and the test should pass"
    );
}

// Step 1: @test with constraint def reference — all satisfied → Pass
#[test]
fn run_tests_with_constraint_def_reference_satisfied_returns_pass() {
    let source = "constraint def Positive { param v : Length\n v > 0mm }\n@test structure TestPos { param x : Length = 5mm\n constraint Positive(v: x) }";
    let compiled = parse_and_compile(source);
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(results.len(), 1, "expected 1 test result");
    assert_eq!(results[0].name, "TestPos");
    assert_eq!(
        results[0].status,
        reify_eval::TestStatus::Pass,
        "Positive(v: 5mm) should be satisfied"
    );
    assert!(
        !results[0].constraint_results.is_empty(),
        "expected at least one constraint result"
    );
    for entry in &results[0].constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "expected all Satisfied, got {:?}",
            entry.satisfaction
        );
    }
}

// Step 2: @test with constraint def reference — violated → Fail
#[test]
fn run_tests_with_constraint_def_reference_violated_returns_fail() {
    let source = "constraint def Positive { param v : Length\n v > 0mm }\n@test structure TestNeg { param x : Length = -1mm\n constraint Positive(v: x) }";
    let compiled = parse_and_compile(source);
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "TestNeg");
    assert_eq!(
        results[0].status,
        reify_eval::TestStatus::Fail,
        "Positive(v: -1mm) should be violated"
    );
    assert!(
        results[0]
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Violated),
        "expected at least one Violated entry"
    );
}

// Step 3: @test with auto param — actual Indeterminate (no mock, real SimpleConstraintChecker)
#[test]
fn run_tests_with_auto_param_returns_indeterminate() {
    let compiled = parse_and_compile(
        "@test structure TestAuto { param x : Scalar = auto\n constraint x > 0 }",
    );
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "TestAuto");
    assert_eq!(
        results[0].status,
        reify_eval::TestStatus::Indeterminate,
        "auto param should produce Indeterminate via real SimpleConstraintChecker"
    );
}

// Step 4: @test with multiple sub-structure fixtures — both pass
#[test]
fn run_tests_with_multiple_sub_structures_returns_pass() {
    let source = "structure def Widget { param size : Length = 10mm }\nstructure def Gadget { param weight : Scalar = 2 }\n@test structure TestAssembly {\n  sub w = Widget()\n  sub g = Gadget()\n  constraint self.w.size > 0mm\n  constraint self.g.weight > 0\n}";
    let compiled = parse_and_compile(source);
    let results = run_tests(&compiled, || Box::new(SimpleConstraintChecker));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "TestAssembly");
    assert_eq!(
        results[0].status,
        reify_eval::TestStatus::Pass,
        "Widget.size=10mm and Gadget.weight=2 should both satisfy their constraints"
    );
    assert!(
        results[0].constraint_results.len() >= 2,
        "expected at least 2 constraint results (one per sub-structure constraint), got {}",
        results[0].constraint_results.len()
    );
}
