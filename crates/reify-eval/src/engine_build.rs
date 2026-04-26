// Split from lib.rs (task 2032) — build methods.

use std::collections::HashMap;

use reify_compiler::CompiledModule;
use reify_types::{
    CompiledFunction, Diagnostic, DiagnosticLabel, ExportFormat, GeometryHandleId, GeometryKernel,
    Mesh, SourceSpan, ValueMap,
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
            let had_realization_ops = module
                .templates
                .iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

            for template in &module.templates {
                // `named_steps` is scoped per-template so that two structures
                // that each declare `let body = …` cannot clobber each other's
                // name → handle entries.  Cross-template GeomRef::Sub references
                // are intentionally not supported.
                let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
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
                        realization.span,
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
            let had_realization_ops = module
                .templates
                .iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

            for template in &module.templates {
                // `named_steps` is scoped per-template so that two structures
                // that each declare `let body = …` cannot clobber each other's
                // name → handle entries.  Cross-template GeomRef::Sub references
                // are intentionally not supported.
                let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
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
                        realization.span,
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

    /// Returns the tessellation tolerance to use for `module`, in SI metres.
    ///
    /// Threads the module-level `#precision` pragma value (stored on
    /// `CompiledModule::default_tolerance` by `apply_module_pragmas`) through
    /// to the kernel. Falls back to [`Self::DEFAULT_TESSELLATION_TOLERANCE`]
    /// when the pragma is absent or was malformed.
    fn effective_tessellation_tolerance(module: &CompiledModule) -> f64 {
        module
            .default_tolerance
            .unwrap_or(Self::DEFAULT_TESSELLATION_TOLERANCE)
    }

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

        for template in &module.templates {
            // `named_steps` is scoped per-template so that two structures
            // that each declare `let body = …` cannot clobber each other's
            // name → handle entries.  Cross-template GeomRef::Sub references
            // are intentionally not supported.
            let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
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
                    realization.span,
                );

                // Tessellate this realization's final handle (if any new handles were produced)
                if step_handles.len() > handle_start {
                    let last_handle = step_handles[step_handles.len() - 1];
                    match kernel
                        .tessellate(last_handle, Self::effective_tessellation_tolerance(module))
                    {
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
    ///
    /// **Duplicate `realization_name` within a template:** last-write-wins —
    /// a later realization with the same name shadows the earlier one in
    /// `named_steps`.  Pinned by
    /// `execute_realization_ops_duplicate_name_shadows_previous`.
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
        realization_span: SourceSpan,
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
                        diagnostics.push(
                            Diagnostic::error(format!("geometry error: {}", e)).with_label(
                                DiagnosticLabel::new(realization_span, "in this realization"),
                            ),
                        );
                        break;
                    }
                },
                Err(err) => {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "failed to compile geometry operation: {}",
                            err
                        ))
                        .with_label(DiagnosticLabel::new(
                            realization_span,
                            "in this realization",
                        )),
                    );
                    step_handles.push(GeometryHandleId::INVALID);
                    had_failure = true;
                }
            }
        }
        // Discard intermediate handles from partially-failed realizations
        let rolled_back = had_failure || step_handles.len().saturating_sub(handle_start) < operations.len();
        if rolled_back {
            step_handles.truncate(handle_start);
        } else if let Some(name) = realization_name {
            // Record name → final handle only after a fully successful realization.
            // Insertion happens AFTER the rollback check so failed realizations
            // never leave a stale entry that would let later realizations resolve
            // a name whose geometry was never successfully produced.
            //
            // Use `step_handles[handle_start..]` rather than `step_handles.last()` so
            // that an empty-ops realization (operations.len() == 0) contributes nothing
            // to named_steps instead of incorrectly inheriting the final handle of
            // the previous realization.
            if let Some(&last) = step_handles[handle_start..].last() {
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
            SourceSpan::new(0, 0),
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
            SourceSpan::new(0, 0),
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
            SourceSpan::new(0, 0),
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
            SourceSpan::new(0, 0),
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
            SourceSpan::new(0, 0),
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
            SourceSpan::new(0, 0),
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
            SourceSpan::new(0, 0),
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

    /// Pins the last-write-wins (shadowing) semantics for `named_steps` when
    /// two sibling realizations share the same `realization_name`.  Reify's
    /// source syntax permits two sibling `let body = …` geometry bindings
    /// inside a structure with no compile error (`CompilationScope::register`
    /// uses plain `HashMap::insert` without a duplicate-name check).  When
    /// that happens, `execute_realization_ops` must overwrite the earlier
    /// entry so that `named_steps["body"]` resolves to the most-recent
    /// successful binding.  A regression flipping `HashMap::insert` to
    /// `entry().or_insert(…)` (first-write-wins) must fail this test.
    #[test]
    fn execute_realization_ops_duplicate_name_shadows_previous() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let box_ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];
        let cyl_ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Cylinder,
            args: vec![
                ("radius".into(), mm_lit(5.0)),
                ("height".into(), mm_lit(20.0)),
            ],
        }];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        // First binding: let body = box(…)
        Engine::execute_realization_ops(
            &mut kernel,
            &box_ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            Some("body"),
            SourceSpan::new(0, 0),
        );
        // Snapshot via the contract-visible map entry, not by positional index,
        // so the snapshot stays correct if internal handle-slot layout changes.
        let h1 = named_steps["body"];

        // Second binding: let body = cylinder(…) — same name, different primitive
        Engine::execute_realization_ops(
            &mut kernel,
            &cyl_ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            Some("body"),
            SourceSpan::new(0, 0),
        );
        let h2 = named_steps["body"];

        // The kernel must have issued distinct handles so the test is non-trivial
        assert_ne!(
            h1,
            h2,
            "MockGeometryKernel must return distinct handles for distinct ops"
        );

        // Last-write-wins: named_steps["body"] must equal h2 (the cylinder binding)
        assert_eq!(
            named_steps.get("body").copied(),
            Some(h2),
            "shadowing contract: the second `let body` binding must overwrite \
             the first — named_steps[\"body\"] must be the handle from the \
             most-recent successful realization"
        );

        // Explicit anti-assertion: a first-write-wins regression must fail here
        assert_ne!(
            named_steps.get("body").copied(),
            Some(h1),
            "first-write-wins regression guard: named_steps[\"body\"] must NOT \
             resolve to the first binding's handle after the second binding has \
             shadowed it"
        );

        assert!(
            diagnostics.is_empty(),
            "no errors expected for two valid realizations"
        );
    }

    /// Pins the rollback-vs-shadowing interaction: when a named realization
    /// fails (compile error → rollback path), the function must NOT overwrite
    /// a prior successful binding for the same name in `named_steps`.  This
    /// covers the intersection between the shadowing semantics tested above and
    /// the rollback invariant tested in
    /// `execute_realization_ops_rollback_does_not_leak_into_named_steps`.
    ///
    /// If the guard inside `execute_realization_ops` (the `else if` branch that
    /// only inserts into `named_steps` after a fully successful realization)
    /// were removed, a failed second binding would silently clear or overwrite
    /// the first successful one, causing later `GeomRef::Sub("body")` lookups
    /// to fail or resolve to invalid geometry.
    #[test]
    fn execute_realization_ops_failed_shadow_does_not_overwrite_previous() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let box_ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];
        // A realization that will fail to compile: OOB step reference forces the
        // compile-error path → had_failure = true → rollback.
        let fail_ops = vec![CompiledGeometryOp::Boolean {
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

        // First binding: let body = box(…) — succeeds, populates named_steps.
        Engine::execute_realization_ops(
            &mut kernel,
            &box_ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            Some("body"),
            SourceSpan::new(0, 0),
        );
        let h1 = named_steps["body"];
        assert!(diagnostics.is_empty(), "first realization must succeed cleanly");

        // Second binding: let body = <invalid> — fails (rollback path).
        Engine::execute_realization_ops(
            &mut kernel,
            &fail_ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            Some("body"),
            SourceSpan::new(0, 0),
        );

        // The failed shadow must NOT have overwritten the successful binding.
        assert_eq!(
            named_steps.get("body").copied(),
            Some(h1),
            "rollback guard: a failed shadow must not overwrite the previous \
             successful binding — named_steps[\"body\"] must still resolve to h1"
        );

        // The second call must have emitted a diagnostic (compile failure).
        assert!(
            !diagnostics.is_empty(),
            "expected a diagnostic from the failed second realization"
        );
    }

    // ── span-label threading tests ─────────────────────────────────────────────

    /// Pins that the compile-failure Error diagnostic emitted by
    /// `execute_realization_ops` carries a `DiagnosticLabel` whose span
    /// equals the supplied `realization_span`.
    ///
    /// Uses an OOB `GeomRef::Step(99)` to force the compile-failure path
    /// (same trigger as `execute_realization_ops_compile_failure_diagnostic_includes_specific_reason`).
    /// Passes a distinct non-zero span `SourceSpan::new(100, 150)` so the
    /// assertion cannot collide with a sentinel value.
    ///
    /// This test fails to compile until step-6 adds the `realization_span:
    /// SourceSpan` parameter to `execute_realization_ops`.
    #[test]
    fn execute_realization_ops_compile_failure_diagnostic_has_realization_span_label() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{Severity, SourceSpan};

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
        let realization_span = SourceSpan::new(100, 150);

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
            realization_span,
        );

        // Find the compile-failure Error diagnostic.
        let compile_err_diag = diagnostics
            .iter()
            .find(|d| {
                d.message.contains("failed to compile geometry operation")
                    && matches!(d.severity, Severity::Error)
            })
            .expect("expected an Error diagnostic with 'failed to compile geometry operation'");

        assert_eq!(
            compile_err_diag.labels.len(),
            1,
            "compile-failure diagnostic should carry exactly 1 DiagnosticLabel, \
             got {}: {:?}",
            compile_err_diag.labels.len(),
            compile_err_diag.labels
        );
        assert_eq!(
            compile_err_diag.labels[0].span,
            realization_span,
            "compile-failure label span should equal the supplied realization_span \
             {:?}, got {:?}",
            realization_span,
            compile_err_diag.labels[0].span
        );
    }

    /// Pins that the kernel-error Error diagnostic emitted by
    /// `execute_realization_ops` carries a `DiagnosticLabel` whose span
    /// equals the supplied `realization_span`.
    ///
    /// Uses `FailingMockGeometryKernel` (ops compile but kernel.execute returns Err)
    /// so we exercise the kernel-error path.  Passes a distinct non-zero span
    /// `SourceSpan::new(200, 250)`.
    ///
    /// After step-6, this test FAILS because step-6 only attaches the label to
    /// the compile-failure path.  Step-8 will attach it to the kernel-error path
    /// and make this test pass.
    #[test]
    fn execute_realization_ops_kernel_error_diagnostic_has_realization_span_label() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_test_support::mocks::FailingMockGeometryKernel;
        use reify_types::{CompiledExpr, Severity, SourceSpan, Type};

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
        let realization_span = SourceSpan::new(200, 250);

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
            realization_span,
        );

        // Find the kernel-error Error diagnostic.
        let kernel_err_diag = diagnostics
            .iter()
            .find(|d| {
                d.message.contains("geometry error")
                    && matches!(d.severity, Severity::Error)
            })
            .expect("expected an Error diagnostic with 'geometry error'");

        assert_eq!(
            kernel_err_diag.labels.len(),
            1,
            "kernel-error diagnostic should carry exactly 1 DiagnosticLabel, \
             got {}: {:?}",
            kernel_err_diag.labels.len(),
            kernel_err_diag.labels
        );
        assert_eq!(
            kernel_err_diag.labels[0].span,
            realization_span,
            "kernel-error label span should equal the supplied realization_span \
             {:?}, got {:?}",
            realization_span,
            kernel_err_diag.labels[0].span
        );
    }

    // ── effective_tessellation_tolerance unit tests ──────────────────────────

    /// When `module.default_tolerance` is `Some(v)`, the helper returns `v`
    /// (in SI metres) verbatim — the module-level `#precision` pragma value
    /// overrides the engine's hardcoded default.
    #[test]
    fn effective_tessellation_tolerance_uses_module_default_when_set() {
        use reify_test_support::builders::CompiledModuleBuilder;
        use reify_types::ModulePath;

        let mut module = CompiledModuleBuilder::new(ModulePath::single("t")).build();
        module.default_tolerance = Some(0.005);

        assert_eq!(
            Engine::effective_tessellation_tolerance(&module),
            0.005,
            "effective_tessellation_tolerance must return module.default_tolerance \
             when it is Some(_)"
        );
    }

    /// When `module.default_tolerance` is `None`, the helper falls back to
    /// `Engine::DEFAULT_TESSELLATION_TOLERANCE` — preserving v0.1 behaviour
    /// for modules without a `#precision` pragma.
    #[test]
    fn effective_tessellation_tolerance_falls_back_to_default_when_none() {
        use reify_test_support::builders::CompiledModuleBuilder;
        use reify_types::ModulePath;

        let module = CompiledModuleBuilder::new(ModulePath::single("t")).build();
        assert!(
            module.default_tolerance.is_none(),
            "fresh module from CompiledModuleBuilder should have default_tolerance == None"
        );

        assert_eq!(
            Engine::effective_tessellation_tolerance(&module),
            Engine::DEFAULT_TESSELLATION_TOLERANCE,
            "effective_tessellation_tolerance must fall back to \
             Engine::DEFAULT_TESSELLATION_TOLERANCE when default_tolerance is None"
        );
    }

    // ── End-to-end #precision threading: field → kernel.tessellate ───────────
    //
    // The unit tests above pin `effective_tessellation_tolerance` in isolation,
    // but a regression that decoupled `default_tolerance` from the actual
    // `kernel.tessellate(...)` call site (e.g. someone reverting that line back
    // to the hardcoded constant) would slip through. The two tests below close
    // that gap by driving `tessellate_realizations` with a recording stub kernel
    // that captures every `tolerance` argument.

    /// Recording stub kernel: produces successful handles + a minimal mesh,
    /// and captures every `tolerance` argument passed to `tessellate` into a
    /// shared Vec the test can read back after the engine takes ownership.
    struct RecordingTessellationKernel {
        next_id: u64,
        recorded_tolerances: std::sync::Arc<std::sync::Mutex<Vec<f64>>>,
    }

    impl reify_types::GeometryKernel for RecordingTessellationKernel {
        fn execute(
            &mut self,
            _op: &reify_types::GeometryOp,
        ) -> Result<reify_types::GeometryHandle, reify_types::GeometryError> {
            let id = reify_types::GeometryHandleId(self.next_id);
            self.next_id += 1;
            Ok(reify_types::GeometryHandle {
                id,
                repr: reify_types::ReprKind::Solid,
            })
        }

        fn query(
            &self,
            _query: &reify_types::GeometryQuery,
        ) -> Result<reify_types::Value, reify_types::QueryError> {
            Err(reify_types::QueryError::QueryFailed(
                "RecordingTessellationKernel::query not used in this test".into(),
            ))
        }

        fn export(
            &self,
            _handle: reify_types::GeometryHandleId,
            _format: reify_types::ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_types::ExportError> {
            Ok(())
        }

        fn tessellate(
            &self,
            _handle: reify_types::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_types::Mesh, reify_types::TessError> {
            self.recorded_tolerances.lock().unwrap().push(tolerance);
            // Minimal valid mesh (one triangle), shape-compatible with
            // MockGeometryKernel::tessellate.
            Ok(reify_types::Mesh {
                vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                indices: vec![0, 1, 2],
                normals: Some(vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
            })
        }
    }

    /// Build a CompiledModule with one Box-primitive realization, suitable for
    /// driving `tessellate_realizations`. Uses the same builder pattern as the
    /// fixture in `geometry_error_handling.rs::module_with_box_realization`.
    fn module_with_one_box_realization() -> reify_compiler::CompiledModule {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder, mm};
        use reify_types::{CompiledExpr, ModulePath, Type};

        let e = "TestShape";
        let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());

        let box_op = CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(80.0)),
                ("height".into(), mm_lit(100.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        };

        let template = TopologyTemplateBuilder::new(e)
            .param(e, "width", Type::length(), Some(mm_lit(80.0)))
            .param(e, "height", Type::length(), Some(mm_lit(100.0)))
            .param(e, "depth", Type::length(), Some(mm_lit(5.0)))
            .realization(e, 0, vec![box_op])
            .build();

        CompiledModuleBuilder::new(ModulePath::single("test_precision_threading"))
            .template(template)
            .build()
    }

    /// End-to-end: when `module.default_tolerance == Some(0.005)`, the value
    /// passed to `kernel.tessellate(...)` must be exactly `0.005`. Pins the
    /// `kernel.tessellate(last_handle, Self::effective_tessellation_tolerance(module))`
    /// call site against a regression that re-introduces the hardcoded
    /// `Self::DEFAULT_TESSELLATION_TOLERANCE`.
    #[test]
    fn tessellate_realizations_threads_module_default_tolerance_into_kernel() {
        use reify_test_support::MockConstraintChecker;
        use std::sync::{Arc, Mutex};

        let mut module = module_with_one_box_realization();
        module.default_tolerance = Some(0.005);

        let recorded: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
        let kernel = RecordingTessellationKernel {
            next_id: 1,
            recorded_tolerances: Arc::clone(&recorded),
        };
        let checker = MockConstraintChecker::new();
        let mut engine = crate::Engine::new(Box::new(checker), Some(Box::new(kernel)));

        let _ = engine.tessellate_realizations(&module);

        let tolerances = recorded.lock().unwrap().clone();
        assert_eq!(
            tolerances.len(),
            1,
            "expected exactly 1 tessellate call (one realization), got {}: {:?}",
            tolerances.len(),
            tolerances
        );
        assert_eq!(
            tolerances[0], 0.005,
            "kernel.tessellate must receive module.default_tolerance verbatim, got {}",
            tolerances[0]
        );
    }

    /// End-to-end fallback: when `module.default_tolerance == None`, the value
    /// passed to `kernel.tessellate(...)` must be exactly
    /// `Engine::DEFAULT_TESSELLATION_TOLERANCE`. Pins the same call site for
    /// the no-pragma path.
    #[test]
    fn tessellate_realizations_falls_back_to_default_tolerance_in_kernel() {
        use reify_test_support::MockConstraintChecker;
        use std::sync::{Arc, Mutex};

        let module = module_with_one_box_realization();
        assert!(
            module.default_tolerance.is_none(),
            "fixture must start with default_tolerance == None"
        );

        let recorded: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
        let kernel = RecordingTessellationKernel {
            next_id: 1,
            recorded_tolerances: Arc::clone(&recorded),
        };
        let checker = MockConstraintChecker::new();
        let mut engine = crate::Engine::new(Box::new(checker), Some(Box::new(kernel)));

        let _ = engine.tessellate_realizations(&module);

        let tolerances = recorded.lock().unwrap().clone();
        assert_eq!(
            tolerances.len(),
            1,
            "expected exactly 1 tessellate call (one realization), got {}: {:?}",
            tolerances.len(),
            tolerances
        );
        assert_eq!(
            tolerances[0],
            Engine::DEFAULT_TESSELLATION_TOLERANCE,
            "kernel.tessellate must receive Engine::DEFAULT_TESSELLATION_TOLERANCE \
             when default_tolerance is None, got {}",
            tolerances[0]
        );
    }
}
