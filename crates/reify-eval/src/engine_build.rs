// Split from lib.rs (task 2032) — build methods.

use std::collections::HashMap;
use std::time::Instant;

use reify_compiler::CompiledModule;
use reify_types::{
    AttributeHistory, CompiledFunction, Diagnostic, DiagnosticLabel, ErrorRef, ExportFormat,
    FeatureId, FeatureTag, FeatureTagTable, Freshness, GeometryHandleId, GeometryKernel,
    GeometryOp, LoftOpHistoryRecords, Mesh, RealizationNodeId, SourceSpan, SweepOpHistoryRecords,
    TopologyAttributeTable, ValueMap, VersionId,
};

use crate::cache::{CacheStore, CachedResult, FAILED_REALIZATION_STUB_HANDLE, NodeCache, NodeId};
use crate::deps::DependencyTrace;
use crate::geometry_ops::compile_geometry_op;
use crate::journal::{EvalEvent, EventJournal, EventKind};
use crate::primitive_attribute_seed::seed_primitive_attributes_for_handle;
use crate::topology_attribute_propagation::{
    populate_extrude_attributes, populate_loft_attributes, populate_revolve_attributes,
    populate_sweep_attributes,
};
use crate::{BuildResult, Engine, TessellateResult};

/// Per-op kind for `populate_single_parent_sweep_op` — the three single-
/// parent sweep variants (extrude, revolve, sweep) that share the
/// `SweepOpHistoryRecords` shape but emit different per-op
/// `Role` / `Cap`-flavor combos through their dedicated propagation
/// helper. Loft is *not* included here because it is multi-parent and
/// uses its own `LoftOpHistoryRecords` + `populate_loft_op` helper.
#[derive(Debug, Clone, Copy)]
enum SingleParentSweepKind {
    Extrude,
    Revolve,
    Sweep,
}

/// Dispatch on `attribute_history` to populate `topology_attribute_table`
/// for sweep-style ops (extrude / revolve, currently). Called by
/// `Engine::execute_realization_ops` immediately after the existing
/// primitive-attribute seeding step.
///
/// For `AttributeHistory::None` this is a zero-cost no-op (no kernel
/// `extract_*` calls), so non-overriding kernels and non-attributable ops
/// pay nothing. For `Extrude(history)` / `Revolve(history)` it extracts
/// the profile and result face/edge handles in canonical TopExp order
/// and forwards to the appropriate per-op helper.
///
/// Failures (kernel `extract_*` errors, helper out-of-range index errors)
/// are returned to the caller, which surfaces them as `Diagnostic::warning`
/// and continues. Per task-2574 design, attribute population is auxiliary
/// metadata — a failure here must NOT regress the realization to Failed.
fn populate_attribute_history(
    table: &mut TopologyAttributeTable,
    kernel: &mut dyn GeometryKernel,
    feature_id: &FeatureId,
    geom_op: &GeometryOp,
    result_handle: GeometryHandleId,
    attribute_history: &AttributeHistory,
) -> Result<(), reify_types::QueryError> {
    match attribute_history {
        AttributeHistory::None => Ok(()),
        AttributeHistory::Extrude(history) => {
            let profile_handle = match geom_op {
                GeometryOp::Extrude { profile, .. } => *profile,
                _ => {
                    return Err(reify_types::QueryError::QueryFailed(format!(
                        "AttributeHistory::Extrude returned for non-Extrude GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_single_parent_sweep_op(
                table,
                kernel,
                feature_id,
                profile_handle,
                result_handle,
                history,
                SingleParentSweepKind::Extrude,
            )
        }
        AttributeHistory::Revolve(history) => {
            let profile_handle = match geom_op {
                GeometryOp::Revolve { profile, .. } => *profile,
                _ => {
                    return Err(reify_types::QueryError::QueryFailed(format!(
                        "AttributeHistory::Revolve returned for non-Revolve GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_single_parent_sweep_op(
                table,
                kernel,
                feature_id,
                profile_handle,
                result_handle,
                history,
                SingleParentSweepKind::Revolve,
            )
        }
        AttributeHistory::Sweep(history) => {
            // GeometryOp::Sweep is single-parent like Extrude/Revolve: the
            // profile is the operand whose sub-shapes propagate into the
            // result; the path/spine is not itself a parent.
            let profile_handle = match geom_op {
                GeometryOp::Sweep { profile, .. } => *profile,
                _ => {
                    return Err(reify_types::QueryError::QueryFailed(format!(
                        "AttributeHistory::Sweep returned for non-Sweep GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_single_parent_sweep_op(
                table,
                kernel,
                feature_id,
                profile_handle,
                result_handle,
                history,
                SingleParentSweepKind::Sweep,
            )
        }
        AttributeHistory::Loft(history) => {
            // GeometryOp::Loft is multi-parent: each profile section is a
            // parent; `parent_index` in `face_generated` denotes the
            // section index in `[0, profiles.len())`.
            let profiles = match geom_op {
                GeometryOp::Loft { profiles } => profiles,
                _ => {
                    return Err(reify_types::QueryError::QueryFailed(format!(
                        "AttributeHistory::Loft returned for non-Loft GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_loft_op(
                table,
                kernel,
                feature_id,
                profiles,
                result_handle,
                history,
            )
        }
    }
}

/// Shared helper for the three single-parent sweep variants (extrude,
/// revolve, sweep). Extracts the profile and result face/edge slices
/// from `kernel`, then dispatches to the appropriate per-op propagation
/// helper based on `kind`. Centralised so the extract sequence +
/// error-propagation shape stays uniform across the variants.
fn populate_single_parent_sweep_op(
    table: &mut TopologyAttributeTable,
    kernel: &mut dyn GeometryKernel,
    feature_id: &FeatureId,
    profile_handle: GeometryHandleId,
    result_handle: GeometryHandleId,
    history: &SweepOpHistoryRecords,
    kind: SingleParentSweepKind,
) -> Result<(), reify_types::QueryError> {
    let profile_faces = kernel.extract_faces(profile_handle)?;
    let profile_edges = kernel.extract_edges(profile_handle)?;
    let result_faces = kernel.extract_faces(result_handle)?;
    let result_edges = kernel.extract_edges(result_handle)?;
    match kind {
        SingleParentSweepKind::Extrude => populate_extrude_attributes(
            table,
            feature_id,
            &profile_faces,
            &profile_edges,
            &result_faces,
            &result_edges,
            history,
        ),
        SingleParentSweepKind::Revolve => populate_revolve_attributes(
            table,
            feature_id,
            &profile_faces,
            &profile_edges,
            &result_faces,
            &result_edges,
            history,
        ),
        SingleParentSweepKind::Sweep => populate_sweep_attributes(
            table,
            feature_id,
            &profile_faces,
            &profile_edges,
            &result_faces,
            &result_edges,
            history,
        ),
    }
}

/// Multi-parent variant of `populate_single_parent_sweep_op` for
/// `GeometryOp::Loft`. Walks the `profiles` handle list, calls
/// `kernel.extract_faces` / `extract_edges` once per section to build
/// the per-section profile face/edge slice families, extracts the
/// result face/edge slices, and dispatches to
/// `populate_loft_attributes`. Failure semantics preserved (Diagnostic::
/// warning at the call site, no Failed regression per task-2574).
///
/// Duplicate handles in `profile_handles` (legal but unusual — a loft
/// referencing the same section twice) re-extract on each iteration
/// rather than memoising; loft profile counts are typically small (2–8)
/// so the per-call cost is negligible, and a memo would add a HashMap
/// allocation that is unwarranted for the common path. If real models
/// surface heavy duplicate-handle lofts a future task can introduce a
/// `HashMap<GeometryHandleId, Vec<GeometryHandleId>>` cache here.
///
/// The two extractions whose results are currently dropped inside
/// `populate_loft_attributes` (`extract_faces(profile_handle)` per section,
/// `extract_edges(result_handle)` once) are still performed eagerly because:
///   (a) loft profiles are typically wires (≈ 0 faces extracted), so
///       per-section `extract_faces` is near-free;
///   (b) result-edge extraction is a single call;
///   (c) calling `extract_faces` once per section keeps
///       `section_faces.len() == section_edges.len()`, which is the
///       two-way equality pinned by the lockstep `debug_assert_eq!` at the
///       top of `populate_loft_attributes` (see `topology_attribute_propagation.rs`);
///       the additional equality `== profile_handles.len()` is enforced
///       structurally by the single push-per-iteration loop above (one
///       `section_faces.push(...)` and one `section_edges.push(...)` per
///       `profile_handle`).  Skipping `extract_faces` and passing `&[]`
///       would still violate the assertion (because `section_edges` would
///       still be populated per-section).
fn populate_loft_op(
    table: &mut TopologyAttributeTable,
    kernel: &mut dyn GeometryKernel,
    feature_id: &FeatureId,
    profile_handles: &[GeometryHandleId],
    result_handle: GeometryHandleId,
    history: &LoftOpHistoryRecords,
) -> Result<(), reify_types::QueryError> {
    let mut section_faces: Vec<Vec<GeometryHandleId>> = Vec::with_capacity(profile_handles.len());
    let mut section_edges: Vec<Vec<GeometryHandleId>> = Vec::with_capacity(profile_handles.len());
    for &profile_handle in profile_handles {
        section_faces.push(kernel.extract_faces(profile_handle)?);
        section_edges.push(kernel.extract_edges(profile_handle)?);
    }
    let result_faces = kernel.extract_faces(result_handle)?;
    let result_edges = kernel.extract_edges(result_handle)?;
    populate_loft_attributes(
        table,
        feature_id,
        &section_faces,
        &section_edges,
        &result_faces,
        &result_edges,
        history,
    )
}

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

        // Execute geometry operations. Use the snapshot's eval-round id rather
        // than `self.next_version_id`: build_snapshot is keyed off `state.snapshot.values`,
        // so Failed events must carry that snapshot's version, not the un-used
        // next round that `next_version_id` points at after prior eval/edit calls.
        let version_id = self.current_eval_version();
        let geometry_output = if let Some(ref mut kernel) = self.geometry_kernel {
            let mut step_handles: Vec<GeometryHandleId> = Vec::new();
            let had_realization_ops = module
                .templates
                .iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

            self.feature_tag_table = FeatureTagTable::default();
            self.topology_attribute_table = TopologyAttributeTable::default();
            for template in &module.templates {
                // `named_steps` is scoped per-template so that two structures
                // that each declare `let body = …` cannot clobber each other's
                // name → handle entries.  Cross-template GeomRef::Sub references
                // are intentionally not supported.
                let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
                for realization in &template.realizations {
                    let mut kernel_error: Option<ErrorRef> = None;
                    Engine::execute_realization_ops(
                        kernel.as_mut(),
                        &realization.operations,
                        &realization.feature_tags,
                        &values,
                        &self.functions,
                        &self.meta_map,
                        &mut step_handles,
                        &mut diagnostics,
                        &mut named_steps,
                        &mut self.feature_tag_table,
                        &mut self.topology_attribute_table,
                        &realization.id,
                        realization.name.as_deref(),
                        realization.span,
                        &mut kernel_error,
                    );
                    // Arch §9.1 lines 868–877: kernel error on a realization →
                    // mark realization NodeId as Failed { error } and emit one
                    // EventKind::Failed event. The Diagnostic::error("geometry
                    // error: …") inside `execute_realization_ops` is preserved.
                    if let Some(error) = kernel_error {
                        Engine::mark_realization_failed(
                            &mut self.cache,
                            &mut self.journal,
                            &realization.id,
                            error,
                            version_id,
                        );
                    }
                }
                // Task 2320: see `Engine::post_process_conformance_queries`
                // docstring for the full contract. Mirrored in `build` and
                // `tessellate_from_values` — keep all four call sites in
                // sync (follow-up: the broader build/build_snapshot
                // realization-loop duplication is tracked separately).
                Engine::post_process_conformance_queries(
                    template,
                    &named_steps,
                    &mut values,
                    kernel.as_ref(),
                    &mut diagnostics,
                );
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
        // Task 2320: `values` is moved out of `check_result` here so the
        // per-template post-process can patch conformance-query results
        // (`is_watertight` / `is_manifold` / `is_orientable`) into the map
        // before it is moved into the returned `BuildResult` below.
        let mut values = check_result.values;

        // Use the eval round that produced `values`. `check()` already
        // called `eval()` which bumped `next_version_id` past
        // `snapshot.version`, so reading `self.next_version_id` here
        // would tag Failed events one round ahead of the values that
        // caused the kernel failure.
        let version_id = self.current_eval_version();
        let geometry_output = if let Some(ref mut kernel) = self.geometry_kernel {
            // Execute geometry operations from realizations
            let mut step_handles: Vec<GeometryHandleId> = Vec::new();
            let had_realization_ops = module
                .templates
                .iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

            self.feature_tag_table = FeatureTagTable::default();
            self.topology_attribute_table = TopologyAttributeTable::default();
            for template in &module.templates {
                // `named_steps` is scoped per-template so that two structures
                // that each declare `let body = …` cannot clobber each other's
                // name → handle entries.  Cross-template GeomRef::Sub references
                // are intentionally not supported.
                let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
                for realization in &template.realizations {
                    let mut kernel_error: Option<ErrorRef> = None;
                    Engine::execute_realization_ops(
                        kernel.as_mut(),
                        &realization.operations,
                        &realization.feature_tags,
                        &values,
                        &self.functions,
                        &self.meta_map,
                        &mut step_handles,
                        &mut diagnostics,
                        &mut named_steps,
                        &mut self.feature_tag_table,
                        &mut self.topology_attribute_table,
                        &realization.id,
                        realization.name.as_deref(),
                        realization.span,
                        &mut kernel_error,
                    );
                    // Arch §9.1 lines 868–877: kernel error on a realization →
                    // mark realization NodeId as Failed { error } and emit one
                    // EventKind::Failed event. The Diagnostic::error("geometry
                    // error: …") inside `execute_realization_ops` is preserved.
                    if let Some(error) = kernel_error {
                        Engine::mark_realization_failed(
                            &mut self.cache,
                            &mut self.journal,
                            &realization.id,
                            error,
                            version_id,
                        );
                    }
                }
                // Task 2320: see `Engine::post_process_conformance_queries`
                // docstring for the full contract. Mirrored in
                // `build_snapshot` and `tessellate_from_values` — keep all
                // four call sites in sync (follow-up: the broader
                // build/build_snapshot realization-loop duplication is
                // tracked separately).
                Engine::post_process_conformance_queries(
                    template,
                    &named_steps,
                    &mut values,
                    kernel.as_ref(),
                    &mut diagnostics,
                );
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
            values,
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
        // Task 2320 amendment: `values` is moved into a local mutable binding
        // here so `tessellate_from_values` can patch conformance-query results
        // (`is_watertight` / `is_manifold` / `is_orientable`) into the map
        // before it is moved into the returned `TessellateResult` below.
        // Keeps `TessellateResult.values` semantically aligned with
        // `BuildResult.values` — a reader of either map sees the same
        // kernel-resolved Bool answers (when a kernel is configured).
        let mut values = check_result.values;
        self.feature_tag_table = FeatureTagTable::default();
        self.topology_attribute_table = TopologyAttributeTable::default();
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernel,
            module,
            &mut values,
            &self.functions,
            &mut diagnostics,
            &self.meta_map,
            &mut self.feature_tag_table,
            &mut self.topology_attribute_table,
        );

        TessellateResult {
            values,
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
    ///
    /// `values` is mutable so that conformance-query helpers
    /// (`is_watertight` / `is_manifold` / `is_orientable`) — whose
    /// kernel-aware dispatch lives outside the pure-value `eval_expr` path —
    /// can be patched into the per-template `value_cells`. Reads of `values`
    /// inside `execute_realization_ops` happen *before* the post-process
    /// runs, so the patch is observable only on the final `TessellateResult`
    /// surface — matching the build-pipeline semantics.
    #[allow(clippy::too_many_arguments)]
    fn tessellate_from_values(
        geometry_kernel: &mut Option<Box<dyn GeometryKernel>>,
        module: &CompiledModule,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        diagnostics: &mut Vec<Diagnostic>,
        meta_map: &HashMap<String, HashMap<String, String>>,
        feature_tag_table: &mut FeatureTagTable,
        topology_attribute_table: &mut TopologyAttributeTable,
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
                // Tessellate paths do not propagate kernel errors into
                // `Freshness::Failed` today (arch §9.1 wires that on the
                // build path only — see `Engine::build` / `Engine::build_snapshot`).
                // Pass `&mut None` so `execute_realization_ops` collects the
                // diagnostic but no caller acts on the kernel error here.
                let mut kernel_error: Option<ErrorRef> = None;
                Engine::execute_realization_ops(
                    kernel.as_mut(),
                    &realization.operations,
                    &realization.feature_tags,
                    values,
                    functions,
                    meta_map,
                    &mut step_handles,
                    diagnostics,
                    &mut named_steps,
                    feature_tag_table,
                    topology_attribute_table,
                    &realization.id,
                    realization.name.as_deref(),
                    realization.span,
                    &mut kernel_error,
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
            // Task 2320 amendment: mirrors the `build` / `build_snapshot`
            // wire-up so `TessellateResult.values` exposes the same
            // kernel-resolved `Bool` for conformance-query cells as
            // `BuildResult.values`. See
            // `Engine::post_process_conformance_queries` docstring.
            Engine::post_process_conformance_queries(
                template,
                &named_steps,
                values,
                kernel.as_ref(),
                diagnostics,
            );
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
    ///
    /// **`kernel_error_out`** (arch §9.1 lines 868–877): when
    /// `kernel.execute(...)` returns `Err(...)`, the helper additionally writes
    /// `Some(ErrorRef::new("geometry error: …"))` to `*kernel_error_out` so the
    /// caller can mark the realization NodeId as `Freshness::Failed { error }`
    /// in the eval cache and emit a single `EventKind::Failed` event.  When
    /// the loop completes without a kernel error (success or compile-only
    /// failure), `*kernel_error_out` is left untouched (typically `None`).  The
    /// caller is responsible for the cache + journal writes because the
    /// realization NodeId, cache, and journal are not threaded into this
    /// helper — see `Engine::mark_realization_failed` for the wire site.
    #[allow(clippy::too_many_arguments)]
    fn execute_realization_ops(
        kernel: &mut dyn GeometryKernel,
        operations: &[reify_compiler::CompiledGeometryOp],
        feature_tags: &[FeatureTag],
        values: &ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        step_handles: &mut Vec<GeometryHandleId>,
        diagnostics: &mut Vec<Diagnostic>,
        named_steps: &mut HashMap<String, GeometryHandleId>,
        feature_tag_table: &mut FeatureTagTable,
        topology_attribute_table: &mut TopologyAttributeTable,
        realization_id: &RealizationNodeId,
        realization_name: Option<&str>,
        realization_span: SourceSpan,
        kernel_error_out: &mut Option<ErrorRef>,
    ) {
        let handle_start = step_handles.len();
        let mut had_failure = false;
        for (op_idx, op) in operations.iter().enumerate() {
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
                Ok(geom_op) => match kernel.execute_with_history(&geom_op) {
                    Ok((handle, attribute_history)) => {
                        // Record the parallel-array feature tag for this handle.
                        if let Some(&tag) = feature_tags.get(op_idx) {
                            feature_tag_table.record(handle.id, tag);
                        }
                        // v0.2 persistent-naming-v2 (PRD task 6, #2574): seed
                        // per-face/per-edge `TopologyAttribute` records for
                        // primitive constructors (Box / Cylinder / Sphere).
                        // Non-primitive variants are no-ops at zero kernel
                        // cost — `seed_primitive_attributes_for_handle` skips
                        // the extract_* calls entirely for them. A seeding
                        // failure (e.g. extract_faces / FaceNormal query
                        // error) emits a Warning diagnostic and continues:
                        // attribute seeding is auxiliary metadata, not
                        // primary geometry, so it must not regress the
                        // realization to Failed when only the metadata path
                        // breaks. Per-task design decision recorded in
                        // .task/plan.json.
                        let feature_id = FeatureId::from(realization_id);
                        if let Err(e) = seed_primitive_attributes_for_handle(
                            topology_attribute_table,
                            kernel,
                            handle.id,
                            &feature_id,
                            &geom_op,
                        ) {
                            diagnostics.push(Diagnostic::warning(format!(
                                "topology-attribute seeding failed for {realization_id} op {op_idx}: {e}"
                            )));
                        }
                        // v0.2 persistent-naming-v2 (PRD task 5a, #2573): per-op
                        // attribute population for sweep ops (extrude / revolve).
                        // Mirrors the seeding warning idiom above — a failure
                        // here is auxiliary-metadata-only and must not regress
                        // the realization to Failed. Non-attributable ops
                        // return `AttributeHistory::None` from the default
                        // `GeometryKernel::execute_with_history` impl, so this
                        // match is a no-op for them.
                        if let Err(e) = populate_attribute_history(
                            topology_attribute_table,
                            kernel,
                            &feature_id,
                            &geom_op,
                            handle.id,
                            &attribute_history,
                        ) {
                            diagnostics.push(Diagnostic::warning(format!(
                                "topology-attribute attribute history population failed for {realization_id} op {op_idx}: {e}"
                            )));
                        }
                        step_handles.push(handle.id);
                    }
                    Err(e) => {
                        let err_msg = format!("geometry error: {}", e);
                        diagnostics.push(Diagnostic::error(err_msg.clone()).with_label(
                            DiagnosticLabel::new(realization_span, "in this realization"),
                        ));
                        // Arch §9.1 lines 868–877: surface the kernel error to the
                        // caller so the realization NodeId can be marked Failed in
                        // the eval cache and a single EventKind::Failed event emitted.
                        // First-error-wins inside a single realization: if a later
                        // call into this helper somehow triggers another kernel error
                        // (it won't — we `break` immediately), the first one is kept.
                        if kernel_error_out.is_none() {
                            *kernel_error_out = Some(ErrorRef::new(err_msg));
                        }
                        break;
                    }
                },
                Err(err) => {
                    diagnostics.push(
                        Diagnostic::error(format!("failed to compile geometry operation: {}", err))
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
        let rolled_back =
            had_failure || step_handles.len().saturating_sub(handle_start) < operations.len();
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

    /// Returns the `VersionId` of the current eval round — the id stamped into
    /// `eval_state.snapshot` by the most recent `eval()` or `edit_param()` call.
    ///
    /// Both `build` and `build_snapshot` must tag kernel-error `Failed` events
    /// with this version (not `self.next_version_id`, which already points at
    /// the *next*, un-used round after `eval()` bumped the counter). Centralising
    /// the read here means a future call site cannot accidentally use the wrong
    /// counter.
    ///
    /// Panics if `eval_state` is not yet populated.
    fn current_eval_version(&self) -> VersionId {
        self.eval_state
            .as_ref()
            .expect("eval_state must be populated before reading current_eval_version")
            .snapshot
            .version
    }

    /// Mark a realization NodeId as `Freshness::Failed { error }` in the eval
    /// cache and emit a single `EventKind::Failed` event in the journal.
    ///
    /// Implements arch §9.1 lines 868–877 (kernel.execute(...) Err → mark
    /// realization Failed + emit one error event). Called from `build` and
    /// `build_snapshot` after `execute_realization_ops` surfaced a kernel
    /// error via the `kernel_error_out` parameter.
    ///
    /// Behavior:
    /// - If a cache entry already exists under `NodeId::Realization(rid)`:
    ///   uses [`CacheStore::mark_failed`] to flip `freshness` in place,
    ///   preserving the prior `result` and `dependency_trace`.
    /// - If no entry exists yet (cold-start build before any successful
    ///   handle was produced for this realization): inserts a stub entry
    ///   with `CachedResult::GeometryHandle(FAILED_REALIZATION_STUB_HANDLE)`
    ///   and `Freshness::Failed { error }` directly. The stub const
    ///   ([`FAILED_REALIZATION_STUB_HANDLE`] in `cache.rs`) is `u64::MAX - 1`
    ///   — explicitly **not** `0` (which is plausibly a real handle in
    ///   counters that start at zero) and not `GeometryHandleId::INVALID`
    ///   (`u64::MAX`) because `GeometryHandleId::content_hash` debug-asserts
    ///   on INVALID and `NodeCache::new` always hashes its result.
    ///   Consumers MUST gate on `Freshness::Failed` before reading the
    ///   handle — this stub is defence-in-depth, not an escape hatch.
    /// - Records exactly one `EventKind::Failed { error }` event scoped to
    ///   `NodeId::Realization(rid)`. The pre-existing
    ///   `Diagnostic::error("geometry error: …")` from
    ///   `execute_realization_ops` is left unchanged on `BuildResult.diagnostics`.
    ///
    /// Pinned by
    /// `tests/failed_propagation.rs::kernel_execute_error_marks_realization_failed_and_emits_one_error_event`.
    fn mark_realization_failed(
        cache: &mut CacheStore,
        journal: &mut EventJournal,
        rid: &RealizationNodeId,
        error: ErrorRef,
        version: VersionId,
    ) {
        let r_node = NodeId::Realization(rid.clone());
        // Try the in-place mutation first; if no entry exists, create a stub.
        if !cache.mark_failed(&r_node, error.clone()) {
            cache.put(
                r_node.clone(),
                NodeCache::new(
                    CachedResult::GeometryHandle(FAILED_REALIZATION_STUB_HANDLE),
                    Freshness::Failed {
                        error: error.clone(),
                    },
                    DependencyTrace::default(),
                    version,
                ),
            );
        }
        journal.record(EvalEvent {
            timestamp: Instant::now(),
            node_id: r_node,
            kind: EventKind::Failed { error },
            version,
            payload: None,
        });
    }

    /// Post-process value cells for a template after `execute_realization_ops`
    /// has populated `named_steps`.
    ///
    /// For each `ValueCellDecl` in `template.value_cells` whose `default_expr`
    /// is a recognised conformance-query helper (`is_watertight`,
    /// `is_manifold`, `is_orientable`), this writes the kernel-resolved
    /// `Value::Bool(_)` answer (or the user-assertion override) into
    /// `values`, overwriting the `Value::Undef` left behind by the pure
    /// `eval_expr` path. Cells whose `default_expr` is `None` or whose
    /// dispatch returns `None` (literal arg, unresolvable cell-member name,
    /// non-helper function call) are left untouched — see
    /// [`crate::geometry_ops::try_eval_conformance_query`]'s `None`-return
    /// contract.
    ///
    /// Called once per template from `build` / `build_snapshot` and
    /// `tessellate_realizations` / `tessellate_snapshot` after each path's
    /// per-realization loop has populated `named_steps`. Tessellation
    /// itself does not consume value cells, but the surfaced
    /// `TessellateResult.values` map *is* read by callers (e.g. GUI
    /// overlays that show query-helper results next to a mesh), so the
    /// post-process must run on those paths too — without it, the
    /// tessellate surface would expose `Value::Undef` for these cells
    /// while the build surface exposes the kernel-resolved Bool.
    ///
    /// Pinned by `tests/conformance_runtime.rs::*` (task 2320 step-11)
    /// and the tessellate-path coverage in
    /// `tessellate_realizations_post_processes_conformance_queries`
    /// (task 2320 amendment).
    fn post_process_conformance_queries(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, GeometryHandleId>,
        values: &mut ValueMap,
        kernel: &dyn GeometryKernel,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for cell in &template.value_cells {
            let default_expr = match &cell.default_expr {
                Some(e) => e,
                None => continue,
            };
            if let Some(value) = crate::geometry_ops::try_eval_conformance_query(
                default_expr,
                &template.trait_bounds,
                named_steps,
                kernel,
                diagnostics,
            ) {
                values.insert(cell.id.clone(), value);
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

        // Execute geometry and tessellate. `values` is passed `&mut` so the
        // post-process inside `tessellate_from_values` can patch
        // conformance-query results (`is_watertight` / `is_manifold` /
        // `is_orientable`) before they're surfaced via `TessellateResult`
        // (task 2320 amendment).
        self.feature_tag_table = FeatureTagTable::default();
        self.topology_attribute_table = TopologyAttributeTable::default();
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernel,
            module,
            &mut values,
            &self.functions,
            &mut diagnostics,
            &self.meta_map,
            &mut self.feature_tag_table,
            &mut self.topology_attribute_table,
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

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
        );

        assert_eq!(step_handles.len(), 1, "expected one handle appended");
        // Filter to error-severity only: the v0.2 topology-attribute seeder
        // (#2574) emits a Diagnostic::warning when extract_faces / extract_edges
        // fail (e.g. on a mock kernel without an extraction fixture). The
        // happy-path contract is "no Error diagnostics"; auxiliary-metadata
        // warnings are expected noise on mock kernels.
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "expected no error diagnostics, got: {:?}",
            errors
        );
        // Pin the expected warning count so unrelated warning regressions still
        // fail the test instead of being silently absorbed by the
        // error-severity filter above. Per primitive op that succeeds at the
        // kernel level, the seeder makes exactly one warn-and-continue
        // attempt (extract_faces fails first on this mock kernel because
        // no topology fixture is configured). One Box op → 1 seeder warning.
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Warning))
            .collect();
        assert_eq!(
            warnings.len(),
            1,
            "expected exactly 1 warning (seeder extract_faces failure on mock kernel), \
             got {}: {:?}",
            warnings.len(),
            warnings
        );
        assert!(
            warnings[0]
                .message
                .contains("topology-attribute seeding failed"),
            "the single warning must be the seeder's auxiliary-metadata failure, got: {:?}",
            warnings[0].message
        );
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

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
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

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
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

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
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

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
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

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            Some("body"),
            SourceSpan::new(0, 0),
            &mut None,
        );

        // Filter to error-severity only: see comment in the happy-path test.
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "expected no error diagnostics, got: {:?}",
            errors
        );
        // Pin the expected warning count (one seeder extract-failure per
        // successful primitive op). See the happy-path test for the rationale.
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Warning))
            .collect();
        assert_eq!(
            warnings.len(),
            1,
            "expected exactly 1 warning (seeder extract_faces failure on mock kernel), \
             got {}: {:?}",
            warnings.len(),
            warnings
        );
        assert!(
            warnings[0]
                .message
                .contains("topology-attribute seeding failed"),
            "the single warning must be the seeder's auxiliary-metadata failure, got: {:?}",
            warnings[0].message
        );
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

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            Some("bad"),
            SourceSpan::new(0, 0),
            &mut None,
        );

        assert!(
            !named_steps.contains_key("bad"),
            "named_steps must NOT contain 'bad' after rollback; stale entries \
             would let later realizations resolve a name whose geometry was never \
             successfully produced"
        );
        // Verify rollback did happen (existing invariant)
        assert!(
            step_handles.is_empty(),
            "handles should be truncated on failure"
        );
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
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &box_ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            Some("body"),
            SourceSpan::new(0, 0),
            &mut None,
        );
        // Snapshot via the contract-visible map entry, not by positional index,
        // so the snapshot stays correct if internal handle-slot layout changes.
        let h1 = named_steps["body"];

        // Second binding: let body = cylinder(…) — same name, different primitive
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &cyl_ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            Some("body"),
            SourceSpan::new(0, 0),
            &mut None,
        );
        let h2 = named_steps["body"];

        // The kernel must have issued distinct handles so the test is non-trivial
        assert_ne!(
            h1, h2,
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

        // Filter to error-severity only: the v0.2 topology-attribute seeder
        // (#2574) emits a Diagnostic::warning when extract_faces / extract_edges
        // fail (e.g. on a mock kernel without an extraction fixture). The
        // happy-path contract is "no Error diagnostics"; auxiliary-metadata
        // warnings are expected noise on mock kernels.
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "no errors expected for two valid realizations, got: {:?}",
            errors
        );
        // Pin the expected warning count: this test runs two successful
        // primitive ops (Box, then Cylinder) through the same `diagnostics`
        // Vec, so one seeder warning per op accumulates → 2 total.
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Warning))
            .collect();
        assert_eq!(
            warnings.len(),
            2,
            "expected exactly 2 warnings (one seeder failure per successful primitive op), \
             got {}: {:?}",
            warnings.len(),
            warnings
        );
        assert!(
            warnings
                .iter()
                .all(|w| w.message.contains("topology-attribute seeding failed")),
            "every warning must be a seeder auxiliary-metadata failure, got: {:?}",
            warnings
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
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &box_ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            Some("body"),
            SourceSpan::new(0, 0),
            &mut None,
        );
        let h1 = named_steps["body"];
        // Filter to error-severity only: see comment in the happy-path test.
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "first realization must succeed cleanly, got: {:?}",
            errors
        );
        // Pin the expected warning count (one seeder failure for the
        // successful Box op). See the happy-path test for the rationale.
        let warnings_after_first: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Warning))
            .collect();
        assert_eq!(
            warnings_after_first.len(),
            1,
            "first realization should emit exactly 1 seeder warning, \
             got {}: {:?}",
            warnings_after_first.len(),
            warnings_after_first
        );

        // Second binding: let body = <invalid> — fails (rollback path).
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &fail_ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            Some("body"),
            SourceSpan::new(0, 0),
            &mut None,
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
        // Pin the warning count after the second call: the second op fails
        // before reaching `kernel.execute`, so the seeder is never invoked
        // and no NEW warning lands on top of the one from the first call.
        let warnings_after_second: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Warning))
            .collect();
        assert_eq!(
            warnings_after_second.len(),
            1,
            "after the failing second realization the warning count must remain \
             at 1 (only the first realization's seeder warning); the failing op \
             never reaches the seeder. Got {}: {:?}",
            warnings_after_second.len(),
            warnings_after_second
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

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            None,
            realization_span,
            &mut None,
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
            compile_err_diag.labels[0].span, realization_span,
            "compile-failure label span should equal the supplied realization_span \
             {:?}, got {:?}",
            realization_span, compile_err_diag.labels[0].span
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

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
            &mut named_steps,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &test_realization_id,
            None,
            realization_span,
            &mut None,
        );

        // Find the kernel-error Error diagnostic.
        let kernel_err_diag = diagnostics
            .iter()
            .find(|d| d.message.contains("geometry error") && matches!(d.severity, Severity::Error))
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
            kernel_err_diag.labels[0].span, realization_span,
            "kernel-error label span should equal the supplied realization_span \
             {:?}, got {:?}",
            realization_span, kernel_err_diag.labels[0].span
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

    /// Recording stub kernel: delegates the full `GeometryKernel` surface to a
    /// `MockGeometryKernel` and only intercepts `tessellate` to capture every
    /// `tolerance` argument into a shared Vec the test can read back after the
    /// engine takes ownership. Delegating (rather than reimplementing the
    /// trait) keeps this stub consistent with how the rest of this file's
    /// tests construct kernels and avoids drift if `MockGeometryKernel` gains
    /// new behaviour.
    struct RecordingTessellationKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        recorded_tolerances: std::sync::Arc<std::sync::Mutex<Vec<f64>>>,
    }

    impl reify_types::GeometryKernel for RecordingTessellationKernel {
        fn execute(
            &mut self,
            op: &reify_types::GeometryOp,
        ) -> Result<reify_types::GeometryHandle, reify_types::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            query: &reify_types::GeometryQuery,
        ) -> Result<reify_types::Value, reify_types::QueryError> {
            self.inner.query(query)
        }

        fn export(
            &self,
            handle: reify_types::GeometryHandleId,
            format: reify_types::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_types::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_types::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_types::Mesh, reify_types::TessError> {
            self.recorded_tolerances.lock().unwrap().push(tolerance);
            self.inner.tessellate(handle, tolerance)
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
            inner: reify_test_support::mocks::MockGeometryKernel::new(),
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
            inner: reify_test_support::mocks::MockGeometryKernel::new(),
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
