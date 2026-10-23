//! Static, pre-eval capability probes over a compiled module that pick a command's
//! engine route.

/// Returns `true` if the compiled module contains geometry — i.e. any template
/// has a realization with at least one geometry operation, OR any value cell
/// is typed `reify_core::Type::Geometry`.
///
/// Two compile-time signals are OR'd (no kernel required):
///
/// * **(a) Realization with ops** — any template has a realization with at
///   least one geometry operation. This is the exact signal used by
///   `engine_build.rs`'s `had_realization_ops` gate internally.
///
/// * **(b) `Type::Geometry` value cell** — any template has a value cell
///   typed [`reify_core::Type::Geometry`]. This clause is intentionally
///   conservative/defensive: a module with only (b) true (e.g. a structure
///   that exposes a `Solid`-typed parameter without a realization op) is
///   still routed through `with_registered_kernel + build()`. In that
///   sub-case `build()` will skip the geometry pipeline (no ops → no
///   handles) and geometry-query cells stay `undef`, but the routing is
///   harmless: the kernel block is a no-op without ops and the broader gate
///   future-proofs detection for geometry-forwarding structures.
///
/// Both signals are present for `examples/spec-shape-physical.ri` (the
/// `box(...)` realization op + the `geometry : Solid` cell) and absent for
/// all existing non-geometry eval fixtures.
///
/// Used by `cmd_eval` to decide whether to route through the kernel-backed
/// `Engine::with_registered_kernel + build()` path (so that geometry-query
/// value cells such as `mass`/`centroid` are resolved by
/// `run_post_processes`/`post_process_geometry_queries`) or to stay on the
/// existing lightweight `Engine::new(None) + eval()` path for plain numeric
/// modules.
pub(crate) fn module_has_geometry(module: &reify_compiler::CompiledModule) -> bool {
    module.templates.iter().any(|t| {
        t.realizations.iter().any(|r| !r.operations.is_empty())
            || t.value_cells
                .iter()
                .any(|vc| vc.cell_type == reify_core::Type::Geometry)
    })
}

/// Returns `true` when any template in the module carries at least one
/// `RepresentationWithin(subject, bound)` constraint.
///
/// Used by `cmd_check` to decide whether to route through the kernel-backed
/// `set_capture_repr_tol(true)` → `tessellate_realizations` → `check` path
/// (so that `dispatch_constraints` can evaluate the assertion against the
/// populated `achieved_repr_tol` map) or to stay on the existing lightweight
/// `Engine::new(None)+check()` path for modules with no such assertion.
///
/// # Delegation (task #6170)
///
/// The walk itself lives in
/// [`reify_eval::tolerance_combine::module_declares_representation_within`],
/// which is defined as `!compute_representation_bounds(module).is_empty()` — so
/// this predicate and the C-BOUND bound table are the same traversal rather than
/// two that must be kept in sync. It reaches the canonical recognition gate (UFC
/// name + arity + arg0 ValueRef:StructureRef + arg1 Literal Scalar LENGTH
/// finite≥0) that the engine's dispatch interception also uses.
///
/// This CLI-local name is retained (rather than inlining the call) so
/// `cmd_check`'s routing reads unchanged.
///
/// # Not the export-refusal gate
///
/// Both export-refusal sites — `cmd_build`'s `-o` arm in cmd/build.rs, and
/// `Engine::build_outputs_with_result` in the engine — need the bound TABLE
/// rather than a boolean, because the refusal names the unenforced bound. They
/// therefore call
/// [`reify_eval::tolerance_combine::unenforced_representation_bound_diagnostic`]
/// instead of this predicate; its `None` case is exactly this function returning
/// `false`, so all three sites still share one traversal.
///
/// Non-assertion modules: this function returns `false` and `cmd_check` keeps
/// the existing path verbatim (C2 — byte-identical behavior for all existing
/// `reify check` inputs).
pub(crate) fn module_has_representation_within(module: &reify_compiler::CompiledModule) -> bool {
    reify_eval::tolerance_combine::module_declares_representation_within(module)
}

/// Returns `true` when `module` contains at least one template whose
/// [`reify_compiler::TopologyTemplate::trait_bounds`] includes `"DFMRule"`.
///
/// This gate keys on the `: DFMRule` trait declaration as a deliberate static
/// *proxy* for the engine's duck-typed DFM rule recognition.  The engine's
/// `measure_dfm_rules` (engine_constraints.rs) discovers DFM rules entirely by
/// duck-typing on field shape via `dfm_rule_spec` — it requires a
/// `severity : DFMSeverity` enum field, an `applies_to` struct instance carrying
/// an overhang/draft angle scalar, and a `subject` field — and does **not**
/// reference `trait_bounds` or `satisfies_trait_bound`.  The two predicates are
/// intentionally different: this gate is a static pre-eval check on declarations
/// (no evaluated values are available at routing time), whereas the engine's
/// recognition fires at measurement time on evaluated field shapes.
///
/// **Accepted limitation:** a structure that duck-types as a DFM rule (matching
/// `dfm_rule_spec`) but omits the `: DFMRule` declaration would make this gate
/// return `false`, silently keeping the lightweight `Engine::new(None)`+`check()`
/// path, never calling `build()`, and therefore leaving `realization_handles`
/// unpopulated — `measure_dfm_rules` would emit nothing.  By convention all DFM
/// rule structures in the stdlib and user code declare `: DFMRule` explicitly, so
/// this case is out of scope for the routing gate.
///
/// `DFMRule` is a terminal stdlib trait (process.ri: `trait DFMRule {}`; no
/// refinements, no subtraits), so a direct name-equality match (`b == "DFMRule"`)
/// is exact.  A `: DFMRule` conformer is always compiled to a top-level
/// `module.templates` entry regardless of instantiation site, so scanning
/// templates catches both of `measure_dfm_rules`' discovery sources (A:
/// top-level templates; B: sub-component instances).
///
/// When `true`, `cmd_check` routes through the kernel-backed
/// `build(ExportFormat::Step)`-before-`check` path so that
/// `realization_handles` is populated with live B-rep handles — `measure_dfm_rules`
/// reads `self.realization_handles` to set each rule's `subject_handle`, and
/// skips any rule where `subject_handle.is_none()` (the handle is only present
/// after `build()`).  The C1 no-kernel no-op (the `default_kernel_name` None
/// guard in `measure_dfm_rules` fires when OCCT is absent, emitting nothing)
/// and C2 byte-identical behavior for non-DFM modules are both preserved.
pub(crate) fn module_has_dfm_rule(module: &reify_compiler::CompiledModule) -> bool {
    module
        .templates
        .iter()
        .any(|t| t.trait_bounds.iter().any(|b| b == "DFMRule"))
}

/// Returns `true` when `module` has both a `DFMRule` conformer AND at least
/// one template that declares a `min_feature_size : Length` value cell.
///
/// This is a STATIC pre-eval proxy for the engine's runtime `dfm_thickness_spec`
/// (engine_constraints.rs, which requires `applies_to.min_feature_size` to be
/// of type `Length` at eval time).  `min_feature_size : Length` is declared on
/// the `Subtracting`, `Adding`, and `Parting` process traits (process.ri:55,
/// 69, 98); a conformer's template carries it as a `ValueCellDecl` with
/// `cell_type == Type::Scalar { dimension: DimensionVector::LENGTH }` (ty.rs:81).
///
/// `Forming` (draft-only: `draft_angle : Angle`, no `min_feature_size`) returns
/// `false` → `ensure_openvdb_kernel` is NOT called → alloc-cost optimization
/// preserved (the voxelization path is never needed for draft/overhang checks).
///
/// Accepted limitation: over-acquires OpenVDB in a contrived module that
/// combines a `min_feature_size` process conformer with a `DFMRule` applying
/// to a DIFFERENT process that is Forming-only.  Cost-only (wasted allocation),
/// never a wrong diagnostic; never under-acquires for realistic single-process
/// fixtures.  Mirrors the accepted-limitation note on `module_has_dfm_rule`.
///
/// `cmd_check` calls this BEFORE `engine.ensure_openvdb_kernel()` so that
/// non-thickness modules (Forming/overhang/draft) and non-DFM modules keep
/// the single-pick OCCT engine (engine_admin.rs:754-760 alloc-cost contract).
pub(crate) fn module_has_thickness_dfm_rule(module: &reify_compiler::CompiledModule) -> bool {
    module_has_dfm_rule(module)
        && module.templates.iter().any(|t| {
            t.value_cells.iter().any(|vc| {
                vc.id.member == "min_feature_size"
                    && matches!(
                        &vc.cell_type,
                        reify_core::Type::Scalar { dimension }
                            if *dimension == reify_core::DimensionVector::LENGTH
                    )
            })
        })
}

/// Returns `true` when `module` contains at least one realization operation
/// that is `CompiledGeometryOp::Isosurface` (the `isosurface(...)` builtin,
/// which lowers to the runtime-IR `Operation::Surface` — engine_build.rs:1961).
///
/// This is a STATIC pre-eval proxy mirroring [`module_has_thickness_dfm_rule`]'s
/// routing-gate shape: `cmd_build` calls this BEFORE
/// `engine.ensure_openvdb_kernel()` so that non-isosurface modules keep the
/// single-pick OCCT engine (alloc-cost contract, engine_admin.rs) and stay
/// byte-identical (C2). Isosurface modules need OpenVDB registered because
/// the operand's Mesh→Voxel voxelize stage and the terminal Voxel→Mesh
/// marching-cubes stage (task γ/5001) both dispatch to the openvdb kernel.
///
/// Note: `CompiledGeometryOp::Surface { kind: SurfaceKind, .. }` (free-form
/// `nurbs_surface` construction) is a DISTINCT variant from `Isosurface` —
/// this predicate matches only the latter.
pub(crate) fn module_has_isosurface(module: &reify_compiler::CompiledModule) -> bool {
    module.templates.iter().any(|t| {
        t.realizations.iter().any(|r| {
            r.operations
                .iter()
                .any(|op| matches!(op, reify_compiler::CompiledGeometryOp::Isosurface { .. }))
        })
    })
}

/// Returns `true` when `module` carries a *geometric* `Conforms` instance — one
/// whose compiled [`reify_compiler::CompiledConstraint::arg_bindings`] include an
/// explicit `actual` binding (η/4480).
///
/// This is the CLI counterpart of the engine's own `has_geometric_conforms`
/// fast-path inside `Engine::measure_gdt_conformance`: both key on the presence
/// of an `"actual"` arg-binding on a template (or guarded-group) constraint.
/// `Conforms`'s predicate body never references `actual`, so the binding captured
/// at instantiation is the only static trace of geometric intent — a *scalar*
/// `Conforms` (whose `actual` fell to its `nominal()` default) is NOT detected,
/// so `cmd_check` keeps its scalar verdict byte-identical (B4). The two gates are
/// deliberately the same predicate so the routing decision cannot drift from the
/// pass's own no-op check.
///
/// When `true`, `cmd_check` routes through the kernel-backed
/// `build(ExportFormat::Step)`-before-`check` path so that `realization_handles`
/// is populated with live B-rep handles for the pass — a `MaxDeviation` query is
/// `BRepOnly`, and only `build()` (not `tessellate_realizations`) populates that
/// map. When `false`, the existing RepresentationWithin and lightweight paths are
/// kept verbatim (C2).
pub(crate) fn module_has_geometric_conforms(module: &reify_compiler::CompiledModule) -> bool {
    module.templates.iter().any(|t| {
        let top = t.constraints.iter();
        let guarded = t
            .guarded_groups
            .iter()
            .flat_map(|g| g.constraints.iter().chain(g.else_constraints.iter()));
        top.chain(guarded)
            .any(|c| c.arg_bindings.iter().any(|(n, _)| n == "actual"))
    })
}

#[cfg(test)]
mod eval_geometry_gate_tests {
    use super::module_has_geometry;

    /// RED until `module_has_geometry` is implemented (step-2).
    ///
    /// Compiles two sources with the stdlib:
    /// 1. A geometry-bearing `Bracket : Physical` module (has a `box(...)` realization
    ///    op and a `geometry : Solid` value cell) — expects `true`.
    /// 2. A plain numeric module with no realization ops and no `Geometry`-typed
    ///    cells — expects `false`.
    ///
    /// No OCCT required: the predicate is compile-time only.
    #[test]
    fn module_has_geometry_detects_geometry_vs_plain() {
        // Geometry-bearing: Bracket : Physical has `param geometry : Solid = box(...)`
        // (a realization with operations) and a `geometry : Solid` value cell.
        let geometry_source = r#"
structure def Bracket : Physical {
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = Steel_AISI_1045()
}
"#;
        let compiled_geo = reify_test_support::parse_and_compile_with_stdlib(geometry_source);
        assert!(
            module_has_geometry(&compiled_geo),
            "Bracket : Physical should be detected as a geometry module"
        );

        // Plain numeric: no realization, no Geometry-typed cells.
        let plain_source = r#"
structure def Plain {
    param x : Real = 1.0
    let y = x + 2.0
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile_with_stdlib(plain_source);
        assert!(
            !module_has_geometry(&compiled_plain),
            "Plain numeric module should NOT be detected as a geometry module"
        );

        // Third case: Type::Geometry cell only — no realization operations.
        // This exercises clause (b) of module_has_geometry independently of
        // clause (a). The Bracket test above triggers both clauses simultaneously;
        // this case ensures a regression that breaks the cell_type check while
        // leaving the realization check intact would still fail.
        //
        // Constructed directly via the builder API (no stdlib compile needed)
        // so we can precisely control which fields are set.
        let geo_cell_only =
            reify_test_support::CompiledModuleBuilder::new(reify_core::ModulePath::new(vec![
                "test".to_string(),
            ]))
            .template(
                reify_test_support::TopologyTemplateBuilder::new("GeoCell")
                    .param("GeoCell", "shape", reify_core::Type::Geometry, None)
                    .build(),
            )
            .build();
        assert!(
            module_has_geometry(&geo_cell_only),
            "Module with a Type::Geometry value cell (no realization ops) should be \
             detected as geometry (clause (b) of module_has_geometry)"
        );
    }
}

#[cfg(test)]
mod representation_within_gate_tests {
    use super::module_has_representation_within;

    /// Non-OCCT routing gate test: `module_has_representation_within` must
    /// correctly detect a `RepresentationWithin` constraint in real compiled
    /// IR, and must return `false` for a plain module without one.
    ///
    /// This test is always-running (no OCCT guard) so that a regression in
    /// template-level recognition (e.g. if the compiler changes the IR shape
    /// for resolved stdlib calls) fails CI independently of OCCT availability.
    /// Without this test, the OCCT-gated CLI test would silently pass even if
    /// routing is broken: in stub mode `cmd_check` exits 0 regardless of
    /// whether it took the kernel-backed path or the lightweight path.
    ///
    /// Uses `parse_and_compile` (no stdlib) because `mm` is a built-in length
    /// unit, mirroring the INTERCEPTION_SOURCE fixture used by the engine-level
    /// interception tests in `representation_within_assertion.rs`.
    #[test]
    fn module_has_representation_within_detects_assertion_vs_plain() {
        // Assertion module: Checker carries a `RepresentationWithin(subject, 1mm)`
        // template constraint — must be detected (returns `true`) so that
        // `cmd_check` routes through the kernel-backed
        // `set_capture_repr_tol(true)` → `tessellate_realizations` → `check`
        // path.
        let assertion_source = r#"
structure MyGeom {
    param x : Real = 1.0
}

structure Checker {
    param subject : MyGeom
    param w : Real = 5.0
    constraint RepresentationWithin(subject, 1mm)
    constraint w > 0.0
}
"#;
        let compiled_assertion = reify_test_support::parse_and_compile(assertion_source);
        assert!(
            module_has_representation_within(&compiled_assertion),
            "module with a RepresentationWithin template constraint should be \
             detected (routing gate must return true)"
        );

        // Plain module: no RepresentationWithin constraints anywhere — must NOT
        // be detected (returns `false`) so that `cmd_check` keeps the existing
        // lightweight `Engine::new(None)+check()` path (C2).
        let plain_source = r#"
structure Plain {
    param x : Real = 1.0
    constraint x > 0.0
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile(plain_source);
        assert!(
            !module_has_representation_within(&compiled_plain),
            "module without RepresentationWithin constraints must NOT be detected \
             (routing gate must return false — C2 path preserved)"
        );
    }
}

#[cfg(test)]
mod geometric_conforms_gate_tests {
    use super::module_has_geometric_conforms;

    /// Non-OCCT routing gate test: `module_has_geometric_conforms` must detect a
    /// *geometric* `Conforms` instance (one carrying an explicit `actual`
    /// binding) in real compiled IR, and must return `false` for a *scalar*
    /// `Conforms` (no `actual`) and for a plain module with no `Conforms` at all.
    ///
    /// This is the CLI counterpart of the engine's own `has_geometric_conforms`
    /// fast-path gate (`Engine::measure_gdt_conformance`): both key on the
    /// presence of an `"actual"` arg-binding on a template (or guarded-group)
    /// constraint. `Conforms`'s predicate body never references `actual`, so the
    /// arg-binding — captured at instantiation on `CompiledConstraint` — is the
    /// only static trace of geometric intent.
    ///
    /// Always-running (no OCCT guard) so a regression in template-level
    /// recognition fails CI independently of OCCT availability: in stub mode
    /// `cmd_check` exits 0 regardless of which path it took, so the OCCT-gated
    /// CLI test alone could not catch broken routing.
    ///
    /// Uses `parse_and_compile_with_stdlib` because `Conforms`, `Flatness`, and
    /// `Geometry` are stdlib-prelude entities, mirroring the engine GD&T
    /// conformance fixtures.
    #[test]
    fn module_has_geometric_conforms_detects_explicit_actual_vs_scalar_and_plain() {
        // Geometric module: a `Conforms` instance with an EXPLICIT `actual`
        // binding — must be detected (returns `true`) so that `cmd_check` routes
        // through the kernel-backed build-before-check path that populates live
        // B-rep handles for the η `measure_gdt_conformance` pass.
        let geometric_source = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    param act : Geometry = box(1mm, 1mm, 1mm)
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: act)
}
"#;
        let compiled_geometric = reify_test_support::parse_and_compile_with_stdlib(geometric_source);
        assert!(
            module_has_geometric_conforms(&compiled_geometric),
            "module with a Conforms instance binding an explicit `actual` should be \
             detected (routing gate must return true)"
        );

        // Scalar module: a `Conforms` instance with NO `actual` (falls to its
        // `nominal()` default) — must NOT be detected (returns `false`) so that
        // `cmd_check` keeps the lightweight path and the scalar verdict stays
        // byte-identical (B4).
        let scalar_source = r#"
structure def Probe {
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: box(1mm, 1mm, 1mm))
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm)
}
"#;
        let compiled_scalar = reify_test_support::parse_and_compile_with_stdlib(scalar_source);
        assert!(
            !module_has_geometric_conforms(&compiled_scalar),
            "module with a scalar-only Conforms (no explicit `actual`) must NOT be \
             detected (routing gate must return false — B4 scalar path preserved)"
        );

        // Plain module: no `Conforms` constraints anywhere — must NOT be detected.
        let plain_source = r#"
structure def Plain {
    param x : Length = 1mm
    constraint x > 0mm
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile_with_stdlib(plain_source);
        assert!(
            !module_has_geometric_conforms(&compiled_plain),
            "module without any Conforms constraints must NOT be detected \
             (routing gate must return false)"
        );
    }
}

#[cfg(test)]
mod dfm_rule_gate_tests {
    use super::module_has_dfm_rule;

    /// Non-OCCT routing gate test: `module_has_dfm_rule` must detect a
    /// `DFMRule` conformer template in real compiled IR, and must return
    /// `false` for a plain module without one.
    ///
    /// Always-running (no OCCT guard) so a regression in template-level
    /// recognition fails CI independently of OCCT availability: in stub mode
    /// `cmd_check` exits 0 regardless of which path it took, so the
    /// OCCT-gated CLI test alone could not catch broken routing.
    ///
    /// Uses `parse_and_compile_with_stdlib` because `DFMRule`, `DFMSeverity`,
    /// `Adding`, and `Process` are stdlib-prelude entities (std.process).
    #[test]
    fn module_has_dfm_rule_detects_conformer_vs_plain() {
        // DFM module: a `structure def OverhangRule : DFMRule {...}` conformer
        // — must be detected (returns `true`) so that `cmd_check` routes
        // through the kernel-backed build(Step)-before-check path that populates
        // `realization_handles` for `measure_dfm_rules`.
        let dfm_source = r#"
import std.process

structure def FDM : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 5USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 0deg
}

structure def OverhangRule : DFMRule {
    param rule_name  : String      = "overhang-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = FDM()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;
        let compiled_dfm = reify_test_support::parse_and_compile_with_stdlib(dfm_source);
        assert!(
            module_has_dfm_rule(&compiled_dfm),
            "module with a DFMRule conformer (OverhangRule : DFMRule) should be \
             detected (routing gate must return true)"
        );

        // Plain module: no DFMRule conformer anywhere — must NOT be detected
        // (returns `false`) so that `cmd_check` keeps the existing lightweight
        // `Engine::new(None)+check()` path (C2).
        let plain_source = r#"
structure def Plain {
    param x : Length = 1mm
    constraint x > 0mm
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile_with_stdlib(plain_source);
        assert!(
            !module_has_dfm_rule(&compiled_plain),
            "module without any DFMRule conformer must NOT be detected \
             (routing gate must return false — C2 path preserved)"
        );
    }
}

#[cfg(test)]
mod thickness_dfm_gate_tests {
    use super::module_has_thickness_dfm_rule;

    /// Cfg-independent routing gate test: `module_has_thickness_dfm_rule` must
    /// return `true` for a module with a Subtracting (or Adding) conformer
    /// carrying `min_feature_size : Length` plus a `DFMRule` conformer, and
    /// `false` for a Forming-based draft-only DFMRule module (no `min_feature_size`)
    /// and for a plain non-DFM module.
    ///
    /// This is the static-gate half of the "doesn't allocate by default"
    /// regression pin: the gate prevents `ensure_openvdb_kernel` from being
    /// called on Forming (draft-only) or plain modules, preserving the
    /// single-pick OCCT engine's alloc-cost contract.
    ///
    /// Always-running (no has_openvdb / OCCT guard): the gate inspects only
    /// compiled IR — it does not perform geometry operations.
    #[test]
    fn module_has_thickness_dfm_rule_detects_thickness_vs_draft_only_vs_plain() {
        // (a) TRUE — Subtracting conformer carrying `min_feature_size : Length`
        // plus a `DFMRule` conformer: the gate must return `true` so that
        // `cmd_check` calls `ensure_openvdb_kernel()`.
        let subtracting_dfm_source = r#"
import std.process

structure def Milling : Subtracting {
    param duration          : Time   = 30min
    param cost              : Money  = 10USD
    param tool_access       : Solid  = box(200mm, 200mm, 200mm)
    param min_feature_size  : Length = 2mm
    param achievable_finish : Length = 0.01mm
}

structure def ThicknessRule : DFMRule {
    param rule_name  : String      = "thickness-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = Milling()
    param subject    : Solid       = box(10mm, 10mm, 1mm)
}
"#;
        let compiled_subtracting =
            reify_test_support::parse_and_compile_with_stdlib(subtracting_dfm_source);
        assert!(
            module_has_thickness_dfm_rule(&compiled_subtracting),
            "Subtracting+DFMRule module with min_feature_size : Length must return \
             true (thickness-DFM gate — OpenVDB acquisition required)"
        );

        // (b) FALSE — Forming conformer (draft-only: has `draft_angle : Angle`
        // but NO `min_feature_size : Length`) plus a `DFMRule` conformer: the
        // gate must return `false` so that `ensure_openvdb_kernel` is NOT called
        // (alloc-cost optimization preserved; overhang/draft arm never needs
        // the voxelization path).
        let forming_dfm_source = r#"
import std.process

structure def Stamping : Forming {
    param duration       : Time   = 10min
    param cost           : Money  = 3USD
    param min_bend_radius : Length = 2mm
    param max_draw_depth : Length = 50mm
    param draft_angle    : Angle  = 3deg
}

structure def DraftRule : DFMRule {
    param rule_name  : String      = "draft-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = Stamping()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;
        let compiled_forming =
            reify_test_support::parse_and_compile_with_stdlib(forming_dfm_source);
        assert!(
            !module_has_thickness_dfm_rule(&compiled_forming),
            "Forming+DFMRule module (draft-only, no min_feature_size) must return \
             false (alloc-cost optimization: OpenVDB must NOT be acquired for draft/overhang)"
        );

        // (c) FALSE — plain module with no DFMRule at all: must return false
        // so the existing `Engine::new(None)+check()` lightweight path is
        // preserved (C2).
        let plain_source = r#"
structure def Plain {
    param x : Length = 1mm
    constraint x > 0mm
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile_with_stdlib(plain_source);
        assert!(
            !module_has_thickness_dfm_rule(&compiled_plain),
            "plain module (no DFMRule, no min_feature_size) must return false (C2 path)"
        );

        // (d) FALSE — Subtracting conformer WITH `min_feature_size : Length`
        // but NO `DFMRule` conformer anywhere: the gate must return `false`
        // because the `module_has_dfm_rule` conjunct is load-bearing.
        //
        // This is the configuration most likely to regress if the conjunction
        // order in `module_has_thickness_dfm_rule` is refactored (e.g. the
        // min_feature_size check is mistakenly left as the only conjunct,
        // dropping the `module_has_dfm_rule` AND). Without this pin, a
        // Subtracting process module with no DFMRule would incorrectly trigger
        // `ensure_openvdb_kernel`, breaking the single-pick OCCT alloc-cost
        // contract for non-DFM files.
        let subtracting_no_dfm_source = r#"
import std.process

structure def Milling : Subtracting {
    param duration          : Time   = 30min
    param cost              : Money  = 10USD
    param tool_access       : Solid  = box(200mm, 200mm, 200mm)
    param min_feature_size  : Length = 2mm
    param achievable_finish : Length = 0.01mm
}
"#;
        let compiled_subtracting_no_dfm =
            reify_test_support::parse_and_compile_with_stdlib(subtracting_no_dfm_source);
        assert!(
            !module_has_thickness_dfm_rule(&compiled_subtracting_no_dfm),
            "Subtracting conformer with min_feature_size but NO DFMRule must return \
             false — the DFMRule conjunct in module_has_thickness_dfm_rule is \
             load-bearing and cannot be dropped by a refactor"
        );
    }
}

#[cfg(test)]
mod isosurface_gate_tests {
    use super::module_has_isosurface;

    /// Cfg-independent routing gate test (mirrors
    /// `module_has_thickness_dfm_rule_detects_thickness_vs_draft_only_vs_plain`):
    /// `module_has_isosurface` must return `true` for a module with an
    /// `isosurface(...)` realization, and `false` for a module using the
    /// distinct `nurbs_surface(...)` builtin (a different
    /// `CompiledGeometryOp::Surface` variant — free-form NURBS construction,
    /// not marching-cubes surfacing) and for a plain module with neither.
    ///
    /// Always-running (no has_openvdb / OCCT guard): the gate inspects only
    /// compiled IR — it does not perform geometry operations.
    #[test]
    fn module_has_isosurface_detects_isosurface_vs_nurbs_surface_vs_plain() {
        // (a) TRUE — `isosurface(solid)` realization: the gate must return
        // `true` so that `cmd_build` calls `ensure_openvdb_kernel()`.
        let isosurface_source = r#"
structure IsoWire {
    param size: Length = 20mm
    let solid = box(size, size, size)
    let shell = isosurface(solid)
}
"#;
        let compiled_isosurface =
            reify_test_support::parse_and_compile_with_stdlib(isosurface_source);
        assert!(
            module_has_isosurface(&compiled_isosurface),
            "module with an isosurface(...) realization must be detected \
             (routing gate must return true)"
        );

        // (b) FALSE — `nurbs_surface(...)` lowers to `CompiledGeometryOp::Surface
        // { kind: SurfaceKind::Nurbs, .. }`, a DISTINCT variant from
        // `Isosurface`: the gate must NOT match it, so a NURBS-only module
        // keeps the single-pick OCCT engine (OpenVDB is never acquired).
        let nurbs_surface_source = r#"
structure def NurbsOnly {
    let p = nurbs_surface(
        [[point3(0mm,0mm,0mm),point3(0mm,10mm,0mm)],[point3(10mm,0mm,0mm),point3(10mm,10mm,5mm)]],
        [[1.0,1.0],[1.0,1.0]],
        [0,0,1,1],
        [0,0,1,1],
        1,
        1
    )
}
"#;
        let compiled_nurbs_surface =
            reify_test_support::parse_and_compile_with_stdlib(nurbs_surface_source);
        assert!(
            !module_has_isosurface(&compiled_nurbs_surface),
            "module with only a nurbs_surface(...) (CompiledGeometryOp::Surface, \
             distinct from Isosurface) must NOT be detected by this gate"
        );

        // (c) FALSE — plain module with no Surface-family op at all.
        let plain_source = r#"
structure def Plain {
    param x : Length = 1mm
    constraint x > 0mm
}
"#;
        let compiled_plain = reify_test_support::parse_and_compile_with_stdlib(plain_source);
        assert!(
            !module_has_isosurface(&compiled_plain),
            "plain module (no isosurface, no nurbs_surface) must return false \
             (C2 path preserved — OpenVDB never acquired for non-surfacing builds)"
        );
    }
}
