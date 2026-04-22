// Split from lib.rs (task 2032) — build methods.

use std::collections::HashMap;

use reify_compiler::CompiledModule;
use reify_types::{
    CompiledFunction, Diagnostic, ExportFormat, GeometryHandleId, GeometryKernel, Mesh, ValueMap,
};

use crate::geometry_ops::compile_geometry_op;
use crate::{BuildResult, Engine, TessellateResult};

impl Engine {
    /// Build geometry from the current snapshot values, without re-calling eval().
    ///
    /// Returns `None` if no snapshot exists. Otherwise: checks constraints from
    /// snapshot (same as check_snapshot), then executes geometry operations from
    /// module realizations using the geometry kernel. This is the incremental
    /// companion to build(): after edit_param() updates values, call
    /// build_snapshot() to get updated geometry without a cold restart.
    pub fn build_snapshot(
        &mut self,
        module: &CompiledModule,
        format: ExportFormat,
    ) -> Option<BuildResult> {
        let state = self.eval_state.as_ref()?;

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        // Check constraints (guard-aware)
        let (constraint_results, mut diagnostics) =
            self.check_constraints_against_templates(module, &values, Some(&state.snapshot.values));

        // Execute geometry operations
        let geometry_output = if let Some(ref mut kernel) = self.geometry_kernel {
            let mut step_handles: Vec<GeometryHandleId> = Vec::new();
            let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
            let had_realization_ops = module
                .templates
                .iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

            for template in &module.templates {
                for realization in &template.realizations {
                    Engine::execute_realization_ops(
                        kernel.as_mut(),
                        &realization.operations,
                        &values,
                        &self.functions,
                        &self.meta_map,
                        &mut step_handles,
                        &mut diagnostics,
                        &mut named_steps,
                        realization.name.as_deref(),
                    );
                }
            }

            if step_handles.is_empty() {
                // Only emit the summary diagnostic when ops were actually declared
                // but all failed; when no ops were declared there is simply no geometry.
                if had_realization_ops {
                    diagnostics.push(Diagnostic::error(
                        "all geometry operations failed; no geometry output produced",
                    ));
                }
                None
            } else {
                let export_handle = *step_handles.last().unwrap();
                let mut output = Vec::new();
                match kernel.export(export_handle, format, &mut output) {
                    Ok(()) => Some(output),
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!("export error: {}", e)));
                        None
                    }
                }
            }
        } else {
            None
        };

        Some(BuildResult {
            values,
            constraint_results,
            geometry_output,
            diagnostics,
            resolved_params: HashMap::new(),
        })
    }

    /// Full build: evaluate, check constraints, produce geometry.
    pub fn build(&mut self, module: &CompiledModule, format: ExportFormat) -> BuildResult {
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;

        let geometry_output = if let Some(ref mut kernel) = self.geometry_kernel {
            // Execute geometry operations from realizations
            let mut step_handles: Vec<GeometryHandleId> = Vec::new();
            let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
            let had_realization_ops = module
                .templates
                .iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

            for template in &module.templates {
                for realization in &template.realizations {
                    Engine::execute_realization_ops(
                        kernel.as_mut(),
                        &realization.operations,
                        &check_result.values,
                        &self.functions,
                        &self.meta_map,
                        &mut step_handles,
                        &mut diagnostics,
                        &mut named_steps,
                        realization.name.as_deref(),
                    );
                }
            }

            if step_handles.is_empty() {
                // No geometry handles available — nothing to export.
                // Only emit the summary diagnostic when ops were actually declared
                // but all failed; when no ops were declared there is simply no geometry.
                if had_realization_ops {
                    diagnostics.push(Diagnostic::error(
                        "all geometry operations failed; no geometry output produced",
                    ));
                }
                None
            } else {
                // Safety: step_handles is non-empty (guarded by the is_empty() check above),
                // so last() is always Some and unwrap() cannot panic.
                let export_handle = *step_handles.last().unwrap();
                let mut output = Vec::new();
                match kernel.export(export_handle, format, &mut output) {
                    Ok(()) => Some(output),
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!("export error: {}", e)));
                        None
                    }
                }
            }
        } else {
            None
        };

        BuildResult {
            values: check_result.values,
            constraint_results: check_result.constraint_results,
            geometry_output,
            diagnostics,
            resolved_params: check_result.resolved_params,
        }
    }

    /// Tessellate all realizations in the module for GUI mesh rendering.
    ///
    /// Evaluates the module via [`check()`], then executes geometry operations
    /// per realization (same loop as [`build()`]) and tessellates each
    /// realization's final shape. Returns one `(entity_path, Mesh)` pair per
    /// realization that produced geometry.
    ///
    /// When no geometry kernel is configured, returns empty meshes with no
    /// error diagnostics (matching the pattern in [`build()`]).
    pub fn tessellate_realizations(&mut self, module: &CompiledModule) -> TessellateResult {
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernel,
            module,
            &check_result.values,
            &self.functions,
            &mut diagnostics,
            &self.meta_map,
        );

        TessellateResult {
            values: check_result.values,
            constraint_results: check_result.constraint_results,
            meshes,
            diagnostics,
            resolved_params: check_result.resolved_params,
        }
    }

    /// Default tessellation tolerance in SI meters (0.1mm).
    const DEFAULT_TESSELLATION_TOLERANCE: f64 = 0.0001;

    /// Shared helper: execute geometry operations and tessellate each realization.
    ///
    /// Used by both `tessellate_realizations()` and `tessellate_snapshot()`.
    fn tessellate_from_values(
        geometry_kernel: &mut Option<Box<dyn GeometryKernel>>,
        module: &CompiledModule,
        values: &ValueMap,
        functions: &[CompiledFunction],
        diagnostics: &mut Vec<Diagnostic>,
        meta_map: &HashMap<String, HashMap<String, String>>,
    ) -> Vec<(String, Mesh)> {
        let mut meshes = Vec::new();

        let kernel = match geometry_kernel.as_mut() {
            Some(k) => k,
            None => return meshes,
        };

        let mut step_handles: Vec<GeometryHandleId> = Vec::new();
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        for template in &module.templates {
            for realization in &template.realizations {
                let handle_start = step_handles.len();
                Engine::execute_realization_ops(
                    kernel.as_mut(),
                    &realization.operations,
                    values,
                    functions,
                    meta_map,
                    &mut step_handles,
                    diagnostics,
                    &mut named_steps,
                    realization.name.as_deref(),
                );

                // Tessellate this realization's final handle (if any new handles were produced)
                if step_handles.len() > handle_start {
                    let last_handle = step_handles[step_handles.len() - 1];
                    match kernel.tessellate(last_handle, Self::DEFAULT_TESSELLATION_TOLERANCE) {
                        Ok(mesh) => {
                            meshes.push((realization.id.to_string(), mesh));
                        }
                        Err(e) => {
                            diagnostics
                                .push(Diagnostic::error(format!("tessellation error: {}", e)));
                        }
                    }
                }
            }
        }

        meshes
    }

    /// Execute the per-realization geometry operation loop and perform rollback
    /// on partial failure.
    ///
    /// Captures `handle_start = step_handles.len()` on entry.  For each op in
    /// `operations`, evaluates it via `compile_geometry_op` and dispatches to
    /// the kernel:
    ///
    /// - `Ok(geom_op)` — dispatches to the kernel; on success pushes
    ///   `handle.id` to `step_handles`; on kernel error emits a geometry-error
    ///   diagnostic and breaks the loop.  Kernel errors break immediately: a
    ///   geometry engine failure is often unrecoverable (e.g. corrupt state),
    ///   and subsequent ops that depend on the failed handle would fail too.
    /// - `Err(reason)` — pushes `GeometryHandleId::INVALID` sentinel, emits a
    ///   compile-error diagnostic, sets `had_failure = true`, and continues.
    ///   Compile errors are cheaper to continue past because the sentinel lets
    ///   independent ops proceed.
    ///
    /// After the op loop, if `had_failure` or fewer handles were produced than
    /// there are `operations`, truncates `step_handles` to `handle_start` (discards
    /// all partial handles from this realization).
    #[allow(clippy::too_many_arguments)]
    fn execute_realization_ops(
        kernel: &mut dyn GeometryKernel,
        operations: &[reify_compiler::CompiledGeometryOp],
        values: &ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        step_handles: &mut Vec<GeometryHandleId>,
        diagnostics: &mut Vec<Diagnostic>,
        named_steps: &mut HashMap<String, GeometryHandleId>,
        realization_name: Option<&str>,
    ) {
        let handle_start = step_handles.len();
        let mut had_failure = false;
        for op in operations {
            let geom_op = compile_geometry_op(
                op,
                values,
                &step_handles[handle_start..],
                functions,
                meta_map,
                named_steps,
                diagnostics,
            );
            match geom_op {
                Ok(geom_op) => match kernel.execute(&geom_op) {
                    Ok(handle) => {
                        step_handles.push(handle.id);
                    }
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!("geometry error: {}", e)));
                        break;
                    }
                },
                Err(err) => {
                    diagnostics.push(Diagnostic::error(format!(
                        "failed to compile geometry operation: {}",
                        err
                    )));
                    step_handles.push(GeometryHandleId::INVALID);
                    had_failure = true;
                }
            }
        }
        // Discard intermediate handles from partially-failed realizations
        let rolled_back = had_failure || step_handles.len() - handle_start < operations.len();
        if rolled_back {
            step_handles.truncate(handle_start);
        } else if let Some(name) = realization_name {
            // Record name → final handle only after a fully successful realization.
            // Insertion happens AFTER the rollback check so failed realizations
            // never leave a stale entry that would let later realizations resolve
            // a name whose geometry was never successfully produced.
            if let Some(&last) = step_handles.last() {
                named_steps.insert(name.to_string(), last);
            }
        }
    }

    /// Tessellate realizations from the current snapshot values, without
    /// re-calling eval().
    ///
    /// Returns `None` if no snapshot exists (no prior `eval()` call).
    /// Otherwise: checks constraints from snapshot, then executes geometry
    /// operations and tessellates each realization. This is the incremental
    /// companion to `tessellate_realizations()`: after `edit_param()` updates
    /// values, call `tessellate_snapshot()` to get updated meshes without a
    /// cold restart.
    pub fn tessellate_snapshot(&mut self, module: &CompiledModule) -> Option<TessellateResult> {
        let state = self.eval_state.as_ref()?;

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        // Check constraints (guard-aware)
        let (constraint_results, mut diagnostics) =
            self.check_constraints_against_templates(module, &values, Some(&state.snapshot.values));

        // Execute geometry and tessellate
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernel,
            module,
            &values,
            &self.functions,
            &mut diagnostics,
            &self.meta_map,
        );

        Some(TessellateResult {
            values,
            constraint_results,
            meshes,
            diagnostics,
            resolved_params: HashMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── execute_realization_ops unit tests ────────────────────────────────────

    /// Happy path: all operations compile and execute successfully.
    /// Appends exactly one handle and emits no diagnostics.
    #[test]
    fn execute_realization_ops_happy_path_appends_handle() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            None,
        );

        assert_eq!(step_handles.len(), 1, "expected one handle appended");
        assert!(diagnostics.is_empty(), "expected no diagnostics");
    }

    /// Compile failure: a Boolean op with out-of-bounds step references causes
    /// `compile_geometry_op` to return `None`. Truncates `step_handles` back to
    /// `handle_start` and emits 1 compile-error diagnostic.
    #[test]
    fn execute_realization_ops_compile_failure_truncates_handles() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        use reify_test_support::mocks::MockGeometryKernel;

        // Step(99) is out-of-bounds when step_handles is empty → compile_geometry_op returns None
        let ops = vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99),
            right: GeomRef::Step(99),
        }];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        // Pre-seed with a sentinel so we can assert truncation went back to exactly
        // this pre-call length, distinguishing "INVALID pushed then truncated" from
        // "INVALID never pushed at all".
        let pre_existing = GeometryHandleId(0xCAFE);
        let mut step_handles: Vec<GeometryHandleId> = vec![pre_existing];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            None,
        );

        assert_eq!(
            step_handles.len(),
            1,
            "step_handles should be truncated back to pre-call length of 1; \
             the INVALID sentinel must not remain"
        );
        assert_eq!(
            step_handles[0], pre_existing,
            "the pre-existing handle must be preserved unchanged"
        );
        let compile_failures = diagnostics
            .iter()
            .filter(|d| d.message.contains("failed to compile geometry operation"))
            .count();
        assert_eq!(
            compile_failures, 1,
            "expected exactly 1 compile-error diagnostic, got {}: {:?}",
            compile_failures, diagnostics
        );
    }

    /// Kernel error: ops compile successfully but `kernel.execute()` returns `Err`.
    /// Truncates `step_handles` to `handle_start` and emits exactly 1 geometry-error
    /// diagnostic.
    #[test]
    fn execute_realization_ops_kernel_error_truncates_handles() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_test_support::mocks::FailingMockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernel = FailingMockGeometryKernel;
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            None,
        );

        assert!(
            step_handles.is_empty(),
            "handles should be truncated back to handle_start (0)"
        );
        let geometry_errors = diagnostics
            .iter()
            .filter(|d| d.message.contains("geometry error"))
            .count();
        assert_eq!(
            geometry_errors, 1,
            "expected exactly 1 geometry-error diagnostic, got {}: {:?}",
            geometry_errors, diagnostics
        );
    }

    /// Multi-op rollback: a realization where the first op succeeds (real handle
    /// pushed) and a later op fails via compile error. Verifies that the real
    /// handle from the first op is discarded — `step_handles` is truncated back
    /// to its pre-call length, leaving only the handles that were there before
    /// `execute_realization_ops` was called.
    #[test]
    fn execute_realization_ops_partial_success_then_failure_discards_earlier_handles() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Two-op realization:
        //   op 0 — Box primitive: compiles and executes OK (real handle pushed)
        //   op 1 — Boolean union of Step(99) and Step(99): Step(99) is OOB
        //          (step_handles[handle_start..] will only have 1 entry after op 0)
        //          → compile_geometry_op returns None → rollback triggered
        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Step(99),
                right: GeomRef::Step(99),
            },
        ];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        // Pre-seed step_handles with a sentinel to verify truncation goes back
        // to exactly this pre-call length, not to zero.
        let pre_existing = GeometryHandleId(0xBEEF);
        let mut step_handles: Vec<GeometryHandleId> = vec![pre_existing];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            None,
        );

        // The real handle produced by op 0 must have been discarded.
        // Only the pre-existing handle should remain.
        assert_eq!(
            step_handles.len(),
            1,
            "step_handles should be truncated back to the pre-call length of 1; \
             the real handle from op 0 must be gone"
        );
        assert_eq!(
            step_handles[0], pre_existing,
            "the pre-existing handle must be preserved unchanged"
        );
        // Exactly one compile-error diagnostic from the failing op 1
        let compile_failures = diagnostics
            .iter()
            .filter(|d| d.message.contains("failed to compile geometry operation"))
            .count();
        assert_eq!(
            compile_failures, 1,
            "expected exactly 1 compile-error diagnostic, got {}: {:?}",
            compile_failures, diagnostics
        );
    }

    /// Richer error propagation: the compile-failure Error diagnostic must include
    /// the specific reason from `compile_geometry_op`'s `Err(reason)`, not just the
    /// generic prefix.  Uses a Boolean op whose GeomRef::Step(99) is out-of-bounds
    /// so the reason string contains "unresolvable" / "Step" / "99".
    ///
    /// This test drives step-4: it fails until `execute_realization_ops` appends
    /// the `err` string to the diagnostic message.
    #[test]
    fn execute_realization_ops_compile_failure_diagnostic_includes_specific_reason() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        use reify_test_support::mocks::MockGeometryKernel;

        // Step(99) is out-of-bounds when step_handles is empty →
        // compile_geometry_op returns Err("unresolvable GeomRef::Step(99) …")
        let ops = vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99),
            right: GeomRef::Step(99),
        }];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            None,
        );

        // The Error diagnostic must contain the standard prefix (preserves
        // existing integration-test substring checks) AND the specific reason.
        let compile_err_diag = diagnostics
            .iter()
            .find(|d| {
                d.message.contains("failed to compile geometry operation")
                    && matches!(d.severity, reify_types::Severity::Error)
            })
            .expect("expected an Error diagnostic with 'failed to compile geometry operation'");

        assert!(
            compile_err_diag.message.contains("unresolvable")
                || compile_err_diag.message.contains("Step")
                || compile_err_diag.message.contains("99"),
            "Error diagnostic should include the specific reason (unresolvable / Step / 99), \
             got: {:?}",
            compile_err_diag.message
        );
    }

    // ── named_steps plumbing tests (step-7) ───────────────────────────────────

    /// Happy-path naming: a successful named realization populates `named_steps`
    /// with the kernel-returned handle after execution completes.
    ///
    /// Fails to compile until step-8 adds `named_steps: &mut HashMap<String,
    /// GeometryHandleId>` and `realization_name: Option<&str>` to
    /// `execute_realization_ops`.
    #[test]
    fn execute_realization_ops_named_realization_populates_named_steps() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            Some("body"),
        );

        assert!(diagnostics.is_empty(), "expected no diagnostics");
        assert_eq!(step_handles.len(), 1, "expected one handle appended");
        let body_handle = named_steps.get("body").copied();
        assert!(
            body_handle.is_some(),
            "named_steps should contain 'body' after successful named realization"
        );
        assert_eq!(
            body_handle.unwrap(),
            step_handles[0],
            "named_steps['body'] should equal the handle returned by the kernel"
        );
    }

    /// Rollback-must-not-leak: a named realization that fails (Boolean op with
    /// out-of-bounds GeomRef::Step triggers compile failure + rollback) must NOT
    /// leave any entry in `named_steps` — stale entries would let later
    /// realizations resolve a name that never actually produced valid geometry.
    ///
    /// Fails to compile until step-8 adds the `named_steps` parameter.
    #[test]
    fn execute_realization_ops_rollback_does_not_leak_into_named_steps() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        use reify_test_support::mocks::MockGeometryKernel;

        // A realization named "bad" whose only op is an OOB Boolean → compile
        // failure → rollback path; named_steps must not contain "bad" afterwards.
        let ops = vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99),
            right: GeomRef::Step(99),
        }];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            Some("bad"),
        );

        assert!(
            !named_steps.contains_key("bad"),
            "named_steps must NOT contain 'bad' after rollback; stale entries \
             would let later realizations resolve a name whose geometry was never \
             successfully produced"
        );
        // Verify rollback did happen (existing invariant)
        assert!(step_handles.is_empty(), "handles should be truncated on failure");
    }
}
