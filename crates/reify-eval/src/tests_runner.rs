use reify_compiler::{CompiledModule, TopologyTemplate};
use reify_types::{ConstraintChecker, Diagnostic, Satisfaction};

use crate::{ConstraintCheckEntry, Engine};

/// Overall status of a single `@test` entity run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Pass,
    Fail,
    Indeterminate,
}

/// Compute the overall test status from per-constraint satisfaction entries.
///
/// - Empty → `Pass` (vacuously satisfied).
/// - Any `Violated` → `Fail` (violations dominate).
/// - Else any `Indeterminate` → `Indeterminate`.
/// - Else → `Pass`.
fn compute_status(results: &[ConstraintCheckEntry]) -> TestStatus {
    let mut has_indeterminate = false;
    for entry in results {
        match entry.satisfaction {
            Satisfaction::Violated => return TestStatus::Fail,
            Satisfaction::Indeterminate => has_indeterminate = true,
            Satisfaction::Satisfied => {}
        }
    }
    if has_indeterminate {
        TestStatus::Indeterminate
    } else {
        TestStatus::Pass
    }
}

/// Build an isolated `CompiledModule` for running a single `@test` template.
///
/// - `target` is included in the result's `templates`.
/// - Other `@test` templates are excluded (one test cannot affect another).
/// - Non-test templates are kept so `sub`-component lookups by name still
///   resolve, but their constraints and objectives are stripped so only the
///   target test's constraints fire during `Engine::check`.
/// - Shared infrastructure (functions, fields, types, units, traits, enum
///   defs, constraint defs, imports, pragmas, module path) is preserved.
fn build_isolated_module(module: &CompiledModule, target: &TopologyTemplate) -> CompiledModule {
    let mut isolated = module.clone();
    isolated.templates = module
        .templates
        .iter()
        .filter(|t| !t.is_test || t.name == target.name)
        .map(|t| {
            let mut t = t.clone();
            if t.name != target.name {
                t.constraints.clear();
                for group in &mut t.guarded_groups {
                    group.constraints.clear();
                    group.else_constraints.clear();
                }
                t.objective = None;
            }
            t
        })
        .collect();
    isolated
}

/// Result of running a single `@test` entity.
#[derive(Debug, Clone)]
pub struct TestResult {
    /// Name of the test template (from `TopologyTemplate::name`).
    pub name: String,
    /// Overall test status: Pass/Fail/Indeterminate.
    pub status: TestStatus,
    /// Diagnostics emitted by constraint checking during the test run.
    pub diagnostics: Vec<Diagnostic>,
    /// Per-constraint satisfaction entries from the test template.
    pub constraint_results: Vec<ConstraintCheckEntry>,
}

/// Run all `@test`-annotated structure/occurrence templates in `module`,
/// returning one `TestResult` per test.
///
/// Each test is evaluated in an isolated `Engine` instance (no state leaks
/// between tests) against a `CompiledModule` that contains the target test
/// template plus non-test templates (with their constraints stripped so only
/// the test's own constraints fire). `make_checker` is called once per test
/// to produce a fresh `Box<dyn ConstraintChecker>`; stateless checkers like
/// `SimpleConstraintChecker` can return `Box::new(SimpleConstraintChecker)`.
pub fn run_tests<F>(module: &CompiledModule, mut make_checker: F) -> Vec<TestResult>
where
    F: FnMut() -> Box<dyn ConstraintChecker>,
{
    let mut results = Vec::new();
    for test_template in module.test_templates() {
        let isolated = build_isolated_module(module, test_template);
        let mut engine = Engine::new(make_checker(), None);
        let check_result = engine.check(&isolated);
        results.push(TestResult {
            name: test_template.name.clone(),
            status: compute_status(&check_result.constraint_results),
            diagnostics: check_result.diagnostics,
            constraint_results: check_result.constraint_results,
        });
    }
    results
}

#[cfg(test)]
mod tests {
    fn entry(sat: reify_types::Satisfaction) -> crate::ConstraintCheckEntry {
        use reify_types::ConstraintNodeId;
        crate::ConstraintCheckEntry {
            id: ConstraintNodeId::new("E", 0),
            label: None,
            satisfaction: sat,
        }
    }

    #[test]
    fn compute_status_empty_returns_pass() {
        use super::compute_status;
        assert_eq!(compute_status(&[]), super::TestStatus::Pass);
    }

    #[test]
    fn compute_status_all_satisfied_returns_pass() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![entry(Satisfaction::Satisfied), entry(Satisfaction::Satisfied)];
        assert_eq!(compute_status(&entries), super::TestStatus::Pass);
    }

    #[test]
    fn compute_status_any_violated_returns_fail() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![entry(Satisfaction::Satisfied), entry(Satisfaction::Violated)];
        assert_eq!(compute_status(&entries), super::TestStatus::Fail);
    }

    #[test]
    fn compute_status_only_indeterminate_returns_indeterminate() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![entry(Satisfaction::Indeterminate)];
        assert_eq!(compute_status(&entries), super::TestStatus::Indeterminate);
    }

    #[test]
    fn compute_status_mix_satisfied_indeterminate_returns_indeterminate() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![entry(Satisfaction::Satisfied), entry(Satisfaction::Indeterminate)];
        assert_eq!(compute_status(&entries), super::TestStatus::Indeterminate);
    }

    #[test]
    fn compute_status_violated_dominates_indeterminate_returns_fail() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![entry(Satisfaction::Indeterminate), entry(Satisfaction::Violated)];
        assert_eq!(compute_status(&entries), super::TestStatus::Fail);
    }

    #[test]
    fn compute_status_violated_dominates_satisfied_and_indeterminate_returns_fail() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![
            entry(Satisfaction::Satisfied),
            entry(Satisfaction::Indeterminate),
            entry(Satisfaction::Violated),
        ];
        assert_eq!(compute_status(&entries), super::TestStatus::Fail);
    }

    fn parse_and_compile_inline(source: &str) -> reify_compiler::CompiledModule {
        use reify_types::{ModulePath, Severity};
        let parsed = reify_syntax::parse(source, ModulePath::single("test_inline"));
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
    fn build_isolated_module_keeps_only_target_test_and_nontest() {
        use super::build_isolated_module;
        let source = "@test structure TestA { param x : Real\n constraint x > 0 }\nstructure def B { param y : Real\n constraint y > 0 }\n@test structure TestC { param z : Real }";
        let module = parse_and_compile_inline(source);
        let test_templates = module.test_templates();
        let target = test_templates.iter().find(|t| t.name == "TestA").expect("TestA not found");
        let isolated = build_isolated_module(&module, target);
        let names: Vec<&str> = isolated.templates.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), 2, "expected TestA + B, got {:?}", names);
        assert!(names.contains(&"TestA"), "TestA missing from {:?}", names);
        assert!(names.contains(&"B"), "B missing from {:?}", names);
        assert!(!names.contains(&"TestC"), "TestC should be excluded from {:?}", names);
    }

    #[test]
    fn build_isolated_module_strips_nontest_constraints() {
        use super::build_isolated_module;
        let source = "@test structure TestA { param x : Real\n constraint x > 0 }\nstructure def B { param y : Real\n constraint y > 0\n constraint y < 100 }";
        let module = parse_and_compile_inline(source);
        let test_templates = module.test_templates();
        let target = test_templates.iter().find(|t| t.name == "TestA").expect("TestA not found");
        let isolated = build_isolated_module(&module, target);
        let b = isolated.templates.iter().find(|t| t.name == "B").expect("B not found");
        assert!(b.constraints.is_empty(), "B.constraints should be stripped, got {} constraints", b.constraints.len());
        for group in &b.guarded_groups {
            assert!(group.constraints.is_empty(), "B guarded group constraints should be stripped");
            assert!(group.else_constraints.is_empty(), "B guarded group else_constraints should be stripped");
        }
        assert!(b.objective.is_none(), "B.objective should be stripped");
    }

    #[test]
    fn build_isolated_module_preserves_target_test_constraints() {
        use super::build_isolated_module;
        let source = "@test structure TestA { param x : Real\n constraint x > 0 }\nstructure def B { param y : Real\n constraint y > 0 }";
        let module = parse_and_compile_inline(source);
        let test_templates = module.test_templates();
        let target = test_templates.iter().find(|t| t.name == "TestA").expect("TestA not found");
        let isolated = build_isolated_module(&module, target);
        let testa = isolated.templates.iter().find(|t| t.name == "TestA").expect("TestA not in isolated");
        assert!(!testa.constraints.is_empty(), "TestA constraints should be preserved");
    }

    #[test]
    fn build_isolated_module_preserves_shared_infrastructure() {
        use super::build_isolated_module;
        // Rich source that populates every shared-infrastructure collection so
        // equality assertions cannot trivially pass as 0==0.
        let source = r#"
fn double(x: Real) -> Real { x * 2 }

enum Quality { Standard, Premium }

trait Measurable {
    param size : Real
}

type Alias = Real

field def temp : Point3 -> Scalar { source = analytical { |p| p } }

constraint def Positive {
    param v : Real
    v > 0
}

@test structure TestA {
    param x : Real
    constraint Positive(x)
}
"#;
        let module = parse_and_compile_inline(source);
        let test_templates = module.test_templates();
        let target = test_templates.iter().find(|t| t.name == "TestA").expect("TestA not found");
        let isolated = build_isolated_module(&module, target);
        assert_eq!(isolated.constraint_defs.len(), module.constraint_defs.len(),
            "constraint_defs must be preserved");
        assert!(module.functions.len() > 0, "functions must be non-empty in source module");
        assert_eq!(isolated.functions.len(), module.functions.len(),
            "functions must be preserved");
        assert!(module.fields.len() > 0, "fields must be non-empty in source module");
        assert_eq!(isolated.fields.len(), module.fields.len(),
            "fields must be preserved");
        assert!(module.type_aliases.len() > 0, "type_aliases must be non-empty in source module");
        assert_eq!(isolated.type_aliases.len(), module.type_aliases.len(),
            "type_aliases must be preserved");
        assert!(module.enum_defs.len() > 0, "enum_defs must be non-empty in source module");
        assert_eq!(isolated.enum_defs.len(), module.enum_defs.len(),
            "enum_defs must be preserved");
        assert!(module.trait_defs.len() > 0, "trait_defs must be non-empty in source module");
        assert_eq!(isolated.trait_defs.len(), module.trait_defs.len(),
            "trait_defs must be preserved");
    }

    #[test]
    fn test_result_constructs_with_required_fields() {
        use super::{TestResult, TestStatus};
        let tr = TestResult {
            name: "TestFoo".to_string(),
            status: TestStatus::Pass,
            diagnostics: Vec::new(),
            constraint_results: Vec::new(),
        };
        assert_eq!(tr.name, "TestFoo");
        assert_eq!(tr.status, TestStatus::Pass);
        assert!(tr.diagnostics.is_empty());
        assert!(tr.constraint_results.is_empty());
    }
}
