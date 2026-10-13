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

// ── Triangles: N helper unit tests (task δ/5002 amend: reviewer_comprehensive
// test-coverage + consistency findings) ─────────────────────────────────────
//
// `stl_triangle_count` and `threemf_triangle_count` are exercised directly,
// including a same-mesh cross-writer parity check driven through the REAL
// `reify_ir::write_stl_binary` / `write_3mf` producers — not two hand-built
// byte buffers each independently pre-loaded with the same literal count,
// which would only prove each reader parses back what it was handed. A full
// dual-format CLI/engine integration test still belongs in
// `cli_build_voxel_to_mesh.rs` / `voxel_to_mesh_e2e.rs`, both outside this
// amendment's locked scope (`main.rs` and the `.ri` fixture only).
#[cfg(test)]
mod triangle_count_tests {
    use super::{stl_triangle_count, threemf_triangle_count};
    use reify_ir::{Mesh, ThreeMfOptions, write_3mf, write_stl_binary};

    /// A well-formed binary-STL byte buffer with `count` baked into bytes
    /// 80..84 (little-endian `u32`), per the layout `stl_triangle_count`
    /// parses. The per-triangle payload is irrelevant to the function under
    /// test, so it is omitted entirely — only the 84-byte header is built.
    fn stl_bytes_with_count(count: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 84];
        bytes[80..84].copy_from_slice(&count.to_le_bytes());
        bytes
    }

    /// A minimal valid `Mesh` with `triangle_count` triangles, all reusing
    /// the same 3 vertices. Neither `write_stl_binary` nor `write_3mf`
    /// validate geometry (manifoldness, winding, area) — only buffer-length
    /// and index-bounds — so a degenerate repeated triangle is sufficient to
    /// drive both real writers.
    ///
    /// Each triangle's index triple is a distinct cyclic rotation of `0,1,2`
    /// (`0,1,2` / `1,2,0` / `2,0,1`, repeating), rather than every triangle
    /// sharing the literal triple `[0,1,2]`. The three vertices themselves
    /// still repeat (still degenerate/zero-area — irrelevant to the byte-count
    /// this test drives), but distinct index triples mean no two triangles are
    /// byte-identical records, so a hypothetical future dedup pass in either
    /// writer would visibly change the reported count instead of silently
    /// collapsing both writers in lockstep — keeping the parity assertion
    /// robust to writer-internals changes.
    fn repeated_triangle_mesh(triangle_count: usize) -> Mesh {
        let mut indices = Vec::with_capacity(triangle_count * 3);
        for i in 0..triangle_count {
            let r = i % 3;
            indices.push(r as u32);
            indices.push(((r + 1) % 3) as u32);
            indices.push(((r + 2) % 3) as u32);
        }
        Mesh { vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], indices, normals: None }
    }

    /// Normal case: reads the count straight out of the header field.
    #[test]
    fn stl_triangle_count_reads_header_field() {
        assert_eq!(stl_triangle_count(&stl_bytes_with_count(7)), 7);
    }

    /// Defensive case: a buffer shorter than the 84-byte header (e.g. an
    /// empty/degenerate export) must report `0`, not panic —
    /// `data.get(80..84)` returns `None` rather than indexing out of bounds.
    #[test]
    fn stl_triangle_count_defensive_on_short_buffer() {
        assert_eq!(stl_triangle_count(&[]), 0);
        assert_eq!(stl_triangle_count(&[0u8; 10]), 0);
    }

    /// Normal case: counts `<triangle ` elements in a real `write_3mf` output.
    #[test]
    fn threemf_triangle_count_reads_real_archive() {
        let mesh = repeated_triangle_mesh(7);
        let mut buf = Vec::new();
        write_3mf(&mesh, ThreeMfOptions::default(), &mut buf).expect("write_3mf should succeed");
        assert_eq!(threemf_triangle_count(&buf), 7);
    }

    /// An empty buffer (no `<triangle ` element at all) reports `0`, not a
    /// panic.
    #[test]
    fn threemf_triangle_count_empty_is_zero() {
        assert_eq!(threemf_triangle_count(&[]), 0);
    }

    /// Cross-writer parity (reviewer_comprehensive correctness_consistency
    /// finding): `stl_triangle_count` and `threemf_triangle_count` must
    /// report the same count when reading the REAL bytes
    /// `reify_ir::write_stl_binary` / `reify_ir::write_3mf` emit for the SAME
    /// mesh. Deriving both from one shared `Mesh` via the actual production
    /// writers — rather than two independently hand-built buffers — is what
    /// makes this a genuine dual-source check instead of a tautology.
    #[test]
    fn triangle_count_derivations_agree_for_same_mesh_via_real_writers() {
        let triangle_count = 11;
        let mesh = repeated_triangle_mesh(triangle_count);

        let mut stl_bytes = Vec::new();
        write_stl_binary(&mesh, &mut stl_bytes).expect("write_stl_binary should succeed");

        let mut mf_bytes = Vec::new();
        write_3mf(&mesh, ThreeMfOptions::default(), &mut mf_bytes)
            .expect("write_3mf should succeed");

        let stl = stl_triangle_count(&stl_bytes);
        let mf = threemf_triangle_count(&mf_bytes);
        assert_eq!(stl, triangle_count);
        assert_eq!(mf, triangle_count);
        assert_eq!(
            stl, mf,
            "stl_triangle_count and threemf_triangle_count must report the \
             same count when reading the real write_stl_binary/write_3mf \
             output for the same mesh"
        );
    }
}
