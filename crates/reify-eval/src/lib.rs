use reify_types::{ConstraintChecker, Diagnostic, GeometryKernel, Satisfaction, ValueMap};

/// The engine facade — main entry point for evaluation.
pub struct Engine {
    #[allow(dead_code)]
    constraint_checker: Box<dyn ConstraintChecker>,
    #[allow(dead_code)]
    geometry_kernel: Option<Box<dyn GeometryKernel>>,
}

/// Result of evaluating a compiled module.
#[derive(Debug)]
pub struct EvalResult {
    pub values: ValueMap,
    pub diagnostics: Vec<Diagnostic>,
}

/// Result of checking constraints.
#[derive(Debug)]
pub struct CheckResult {
    pub values: ValueMap,
    pub constraint_results: Vec<ConstraintCheckEntry>,
    pub diagnostics: Vec<Diagnostic>,
}

/// A single constraint's check result.
#[derive(Debug)]
pub struct ConstraintCheckEntry {
    pub id: reify_types::ConstraintNodeId,
    pub label: Option<String>,
    pub satisfaction: Satisfaction,
}

/// Result of a full build (eval + geometry).
#[derive(Debug)]
pub struct BuildResult {
    pub values: ValueMap,
    pub constraint_results: Vec<ConstraintCheckEntry>,
    pub geometry_output: Option<Vec<u8>>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Engine {
    pub fn new(
        constraint_checker: Box<dyn ConstraintChecker>,
        geometry_kernel: Option<Box<dyn GeometryKernel>>,
    ) -> Self {
        Self {
            constraint_checker,
            geometry_kernel,
        }
    }

    /// Evaluate a compiled module, returning computed values.
    pub fn eval(
        &mut self,
        _module: &reify_compiler::CompiledModule,
    ) -> EvalResult {
        todo!("reify-eval: evaluation not yet implemented")
    }

    /// Evaluate and check constraints.
    pub fn check(
        &mut self,
        _module: &reify_compiler::CompiledModule,
    ) -> CheckResult {
        todo!("reify-eval: check not yet implemented")
    }

    /// Full build: evaluate, check constraints, produce geometry.
    pub fn build(
        &mut self,
        _module: &reify_compiler::CompiledModule,
        _format: reify_types::ExportFormat,
    ) -> BuildResult {
        todo!("reify-eval: build not yet implemented")
    }
}
