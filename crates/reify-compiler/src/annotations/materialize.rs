//! Materialization-time annotation argument compilation.
//!
//! Implements the compiler-side helper that extracts annotation arg slots
//! declared with `eval_time = AtMaterialization` from a `TopologyTemplate`'s
//! annotations, compiles each `reify_ast::Expr` arg into a `reify_ir::CompiledExpr`,
//! and returns one [`MaterializationAnnotationArg`] per matched slot.
//!
//! The public surface is:
//! - [`MaterializationArgType`] — expected value kind for type-checking at eval time.
//! - [`MaterializationAnnotationArg`] — one compiled slot ready for the eval driver.
//! - [`compile_materialization_annotation_args`] — the main entry point.
//!
//! See annotation-args PRD §4 (Phase 2, LEAF) and design decision 2 in plan.json.

use crate::scope::CompilationScope;
use crate::types::TopologyTemplate;
use crate::{compile_expr, CompiledFunction};
use reify_ir::{AnnotationArgValue, CompiledExpr, EnumDef};

use super::schema::{self, ArgType, EvalTime};

// ─── Public types ────────────────────────────────────────────────────────────

/// Expected value kind for a materialization-time annotation argument.
///
/// Mirrors [`schema::ArgType`] but is `pub` so the `reify-eval` driver can
/// import it without depending on `reify-compiler`'s private schema module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializationArgType {
    /// The arg must evaluate to a `Value::String`.
    String,
    /// The arg must evaluate to a `Value::Int`.
    Int,
    /// The arg must evaluate to a `Value::Real`.
    Real,
    /// The arg must evaluate to a `Value::Bool`.
    Bool,
    /// The arg must evaluate to a dimensioned scalar (Length).
    Length,
    /// Any value kind is accepted.
    Any,
}

/// A compiled `AtMaterialization` annotation argument, ready for the eval driver.
///
/// Produced by [`compile_materialization_annotation_args`] for every annotation
/// arg slot that has `eval_time = AtMaterialization` and whose current value is
/// an `AnnotationArgValue::Expr`.
#[derive(Debug, Clone)]
pub struct MaterializationAnnotationArg {
    /// Annotation name (e.g. `"test_eval"`).
    pub annotation: String,
    /// Arg name from the schema (e.g. `"value"`, keyed by `positional_index 0`).
    pub arg_name: String,
    /// Compiled expression to evaluate at materialization time.
    pub expr: CompiledExpr,
    /// Expected value kind; checked after evaluation to detect type mismatches.
    pub expected: MaterializationArgType,
}

// ─── Main entry point ────────────────────────────────────────────────────────

/// Collect all `AtMaterialization` annotation arg slots from `template`'s
/// annotations and compile each `Expr` arg into a `CompiledExpr`.
///
/// For each annotation on `template`:
/// 1. Look up the annotation schema via [`schema::lookup_schema`].
/// 2. For each [`schema::ArgSchema`] whose `eval_time` is
///    [`EvalTime::AtMaterialization`], take the annotation's positional arg at
///    `positional_index`.
/// 3. If the arg value is [`AnnotationArgValue::Expr`], compile the expression
///    using a minimal `CompilationScope::new(&template.name)` (no param bindings
///    — deferred to task ι).
/// 4. Record the result as a [`MaterializationAnnotationArg`].
///
/// Expression compile diagnostics are routed to a **throwaway sink** so
/// unresolved-ident compile errors surface at materialization as runtime
/// `AnnotationEvalFailed`, not as compile errors (PRD §2 Q-AA-6).
///
/// Returns an empty `Vec` when there are no `AtMaterialization` arg slots,
/// allowing the call site to fast-path structures with no materialization-time
/// annotations.
pub fn compile_materialization_annotation_args(
    template: &TopologyTemplate,
    enum_defs: &[EnumDef],
    functions: &[CompiledFunction],
) -> Vec<MaterializationAnnotationArg> {
    let mut result = Vec::new();

    for ann in &template.annotations {
        let Some(schema) = schema::lookup_schema(&ann.name) else {
            continue;
        };
        for arg_schema in schema.args {
            if arg_schema.eval_time != EvalTime::AtMaterialization {
                continue;
            }
            // Take the positional arg at this index (if present).
            let Some(ann_arg) = ann.args.get(arg_schema.positional_index) else {
                continue;
            };
            // Only `Expr`-valued args reach the materialization driver;
            // literals/idents/strings are handled at validate time.
            let AnnotationArgValue::Expr(ref ast_expr) = ann_arg.value else {
                continue;
            };
            // Compile with a minimal scope (no param bindings — deferred to task ι).
            // Throwaway diagnostics: unresolved-ident errors surface as Undef at
            // materialization time, triggering AnnotationEvalFailed there.
            let scope = CompilationScope::new(&template.name);
            let mut throwaway: Vec<reify_core::Diagnostic> = Vec::new();
            let compiled = compile_expr(ast_expr, &scope, enum_defs, functions, &mut throwaway);

            result.push(MaterializationAnnotationArg {
                annotation: ann.name.clone(),
                arg_name: arg_schema.name.to_string(),
                expr: compiled,
                expected: arg_type_to_materialization(arg_schema.ty),
            });
        }
    }

    result
}

// ─── Internal helpers ────────────────────────────────────────────────────────

fn arg_type_to_materialization(ty: ArgType) -> MaterializationArgType {
    match ty {
        ArgType::String => MaterializationArgType::String,
        ArgType::Int => MaterializationArgType::Int,
        ArgType::Real => MaterializationArgType::Real,
        ArgType::Bool => MaterializationArgType::Bool,
        ArgType::Length => MaterializationArgType::Length,
        ArgType::Any => MaterializationArgType::Any,
    }
}
