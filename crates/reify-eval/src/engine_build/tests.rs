    use super::*;

    /// step-05 (RED): `resolve_artifact_path` resolves an `Output` occurrence's
    /// raw `path` field against the design-file directory, an optional
    /// `--out-dir` override, or verbatim when already absolute.
    ///
    /// This is the pure core of the B7 design-relative-path rule
    /// (`docs/prds/v0_6/io-export-import-completion.md` §7.3): a relative
    /// occurrence path joins onto `out_dir_override.unwrap_or(design_dir)` — so
    /// the override is a CI escape hatch that beats the design dir — while an
    /// absolute path ignores both bases. Encapsulating the rule here makes
    /// `build_outputs`'s `ExportArtifact.path` fully resolved and unit-testable
    /// without spawning the CLI binary.
    #[test]
    fn resolve_artifact_path_handles_relative_override_and_absolute() {
        use std::path::{Path, PathBuf};

        // Relative path + design dir, no override → joins onto the design dir.
        assert_eq!(
            resolve_artifact_path("o.stl", Path::new("/d"), None),
            PathBuf::from("/d/o.stl"),
        );

        // Relative path + override → the override wins over the design dir.
        assert_eq!(
            resolve_artifact_path("o.stl", Path::new("/d"), Some(Path::new("/ci"))),
            PathBuf::from("/ci/o.stl"),
        );

        // Absolute path → verbatim, ignoring both bases.
        assert_eq!(
            resolve_artifact_path("/abs/x.stl", Path::new("/d"), Some(Path::new("/ci"))),
            PathBuf::from("/abs/x.stl"),
        );
    }

    // ── build_outputs occurrence-driven export (io-export δ steps 7–14) ───────

    /// Recording kernel for the io-export δ driver tests: delegates the full
    /// `GeometryKernel` surface to a `MockGeometryKernel`, and additionally
    /// captures (a) every handle `execute` produced — so a test can identify the
    /// realized geometry handle (e.g. the `part` box) the occurrence's `subject`
    /// must resolve to — and (b) every `export(handle, format)` call's
    /// `(handle, format)` pair. `export` still delegates to the inner mock (which
    /// writes `MOCK_EXPORT_DATA`), so `ExportArtifact.bytes` is non-empty.
    /// Capturing the export format proves the DSL `Output` occurrence — not a
    /// hardcoded CLI flag — drove the serializer.
    /// Per-call `(handle, format, step_schema, color, include_colors)` log
    /// captured by [`ExportRecordingKernel`]'s `export_with_options`. Factored
    /// into a `type` alias to satisfy `clippy::type_complexity`.
    type ExportedOptionsLog = std::sync::Arc<
        std::sync::Mutex<
            Vec<(
                reify_ir::GeometryHandleId,
                reify_ir::ExportFormat,
                reify_ir::StepSchema,
                Option<reify_ir::Rgb8>,
                bool,
            )>,
        >,
    >;

    struct ExportRecordingKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        executed: std::sync::Arc<std::sync::Mutex<Vec<reify_ir::GeometryHandleId>>>,
        exported: std::sync::Arc<
            std::sync::Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>,
        >,
        /// Per-call `(handle, format, step_schema, color, include_colors)` recorded by
        /// `export_with_options` — proves the DSL `version` and body color reached
        /// the kernel as a [`reify_ir::StepSchema`] and [`reify_ir::Rgb8`].
        exported_options: ExportedOptionsLog,
        /// Warnings `export_with_options` returns. The live OCCT AP242 fallback
        /// can't be triggered in-build (this build supports AP242DIS), so the
        /// `W_STEP_AP242_FALLBACK` diagnostic wiring is exercised by injecting
        /// [`reify_ir::ExportWarning::StepAp242Fallback`] here. Default empty.
        warnings_to_return: Vec<reify_ir::ExportWarning>,
    }

    impl ExportRecordingKernel {
        /// Construct a recording kernel sharing the caller's `executed` and
        /// `exported` capture buffers, with a fresh empty `exported_options`
        /// log and no injected warnings. New fields acquire their defaults
        /// here, so adding one no longer ripples across every call site.
        ///
        /// Read the per-call `(handle, format, step_schema)` log back via
        /// [`recorded_options`](Self::recorded_options); inject fallback
        /// warnings via [`with_warnings`](Self::with_warnings).
        fn new(
            executed: std::sync::Arc<std::sync::Mutex<Vec<reify_ir::GeometryHandleId>>>,
            exported: std::sync::Arc<
                std::sync::Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>,
            >,
        ) -> Self {
            Self {
                inner: reify_test_support::mocks::MockGeometryKernel::new(),
                executed,
                exported,
                exported_options: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                warnings_to_return: Vec::new(),
            }
        }

        /// A clone of the shared `exported_options` handle — the per-call
        /// `(handle, format, step_schema, color, include_colors)` records
        /// captured by `export_with_options`. Grab it before the kernel is
        /// moved into the `Engine`.
        fn recorded_options(&self) -> ExportedOptionsLog {
            std::sync::Arc::clone(&self.exported_options)
        }

        /// Builder: seed the warnings `export_with_options` returns. The live
        /// OCCT AP242 fallback can't be triggered in-build (this build supports
        /// AP242DIS), so the `W_STEP_AP242_FALLBACK` diagnostic wiring is
        /// exercised by injecting [`reify_ir::ExportWarning::StepAp242Fallback`].
        fn with_warnings(mut self, warnings: Vec<reify_ir::ExportWarning>) -> Self {
            self.warnings_to_return = warnings;
            self
        }
    }

    impl reify_ir::GeometryKernel for ExportRecordingKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            let result = self.inner.execute(op);
            if let Ok(handle) = &result {
                self.executed.lock().unwrap().push(handle.id);
            }
            result
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.exported.lock().unwrap().push((handle, format));
            self.inner.export(handle, format, writer)
        }

        fn export_with_options(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            options: &reify_ir::ExportOptions,
            writer: &mut dyn std::io::Write,
        ) -> Result<Vec<reify_ir::ExportWarning>, reify_ir::ExportError> {
            // Record the schema, color, and include_colors the driver threaded from
            // the DSL occurrence, then delegate to `export` (which records (handle,
            // format) for prior tests and writes bytes via the inner mock). Return
            // the configured warnings so warning-diagnostic wiring can be exercised
            // without a live kernel.
            self.exported_options
                .lock()
                .unwrap()
                .push((handle, format, options.step_schema, options.color, options.include_colors));
            self.export(handle, format, writer)?;
            Ok(self.warnings_to_return.clone())
        }

        fn tessellate(
            &self,
            handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.inner.tessellate(handle, tolerance)
        }

        fn make_compound(
            &mut self,
            handles: &[reify_ir::GeometryHandleId],
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.make_compound(handles)
        }
    }

    /// step-07 (RED): `build_outputs` drives a single `STLOutput` occurrence to
    /// exactly one `ExportArtifact` whose `format` (STL) and `path` ("o.stl",
    /// resolved design-relative) come from the DSL, and whose exported handle is
    /// the realized `part` box (the occurrence's `subject`).
    ///
    /// Asserting the single export's `format == Stl` proves the DSL occurrence —
    /// not a hardcoded flag — chose the serializer (B5); asserting its handle is
    /// one the kernel realized proves the `subject: part` arg resolved to live
    /// geometry.
    ///
    /// RED until step-08 adds `Engine::build_outputs`: the method does not yet
    /// exist, so this test fails to compile.
    #[test]
    fn build_outputs_drives_single_stl_output() {
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::{Path, PathBuf};
        use std::sync::{Arc, Mutex};

        let module = parse_and_compile_with_stdlib(
            r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub o = STLOutput(subject: part, resolution: 0.2mm, path: "o.stl")
}"#,
        );

        let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
            Arc::new(Mutex::new(Vec::new()));
        let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported));
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);

        assert_eq!(
            artifacts.len(),
            1,
            "exactly one ExportArtifact for the single STLOutput occurrence, got {}",
            artifacts.len()
        );
        let art = &artifacts[0];
        assert_eq!(
            art.format,
            reify_ir::ExportFormat::Stl,
            "the DSL STLOutput occurrence must drive ExportFormat::Stl"
        );
        assert_eq!(
            art.path,
            PathBuf::from("/tmp/d/o.stl"),
            "a relative occurrence path joins onto the design dir (B7)"
        );
        assert!(
            !art.bytes.is_empty(),
            "the kernel export() must have written bytes into the artifact"
        );

        let exported = exported.lock().unwrap().clone();
        assert_eq!(
            exported.len(),
            1,
            "exactly one export() call for the single occurrence, got {}",
            exported.len()
        );
        assert_eq!(
            exported[0].1,
            reify_ir::ExportFormat::Stl,
            "the recorded export() format must be Stl (DSL-driven, not flag-driven)"
        );
        let executed = executed.lock().unwrap().clone();
        assert!(
            executed.contains(&exported[0].0),
            "the exported handle {:?} must be a realized kernel handle (the resolved \
             `subject: part`); realized handles were {:?}",
            exported[0].0,
            executed
        );
    }

    /// step-09 (ε / task 4288): the `build_outputs` driver threads each
    /// STEPOutput occurrence's STEP schema — read off its `version` field by
    /// `extract_output_export_spec` — into the kernel via `export_with_options`,
    /// proving the DSL `version`, not a hardcoded default, reaches the
    /// serializer.
    ///
    /// `version: STEPVersion.AP203` → the recording kernel observes exactly one
    /// `export_with_options` call whose recorded `step_schema == Ap203`; a
    /// STEPOutput with no `version` field defaults to `Ap214` (the DSL default
    /// `version : STEPVersion = STEPVersion.AP214`).
    #[test]
    fn build_outputs_threads_step_version_into_export_options() {
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::Path;
        use std::sync::{Arc, Mutex};

        // Run build_outputs on `src` and return the per-call `step_schema`s the
        // kernel recorded via `export_with_options`, in call order.
        let run = |src: &str| -> Vec<reify_ir::StepSchema> {
            let module = parse_and_compile_with_stdlib(src);
            let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
                Arc::new(Mutex::new(Vec::new()));
            let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
                Arc::new(Mutex::new(Vec::new()));
            let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported));
            let exported_options = kernel.recorded_options();
            let mut engine = crate::Engine::new(
                Box::new(MockConstraintChecker::new()),
                Some(Box::new(kernel)),
            );
            engine.build_outputs(&module, Path::new("/tmp/d"), None);
            let recorded = exported_options.lock().unwrap().clone();
            recorded.into_iter().map(|(_, _, schema, _, _)| schema).collect()
        };

        // version: STEPVersion.AP203 → exactly one export_with_options call, Ap203.
        let ap203 = run(r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub s = STEPOutput(subject: part, version: STEPVersion.AP203, path: "p.step")
}"#);
        assert_eq!(
            ap203,
            vec![reify_ir::StepSchema::Ap203],
            "the DSL `version: STEPVersion.AP203` must thread Ap203 into export_with_options"
        );

        // No `version` field → DSL default Ap214.
        let default = run(r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub d = STEPOutput(subject: part, path: "def.step")
}"#);
        assert_eq!(
            default,
            vec![reify_ir::StepSchema::Ap214],
            "a STEPOutput with no `version` defaults to Ap214 (the DSL default)"
        );
    }

    /// δ step-7 / step-8 (task #4763): build_outputs threads a Physical body's resolved
    /// material color into `export_with_options` via ExportOptions.color (B7), and
    /// surfaces the ThreeMfNoMaterials warning as a W_3MF_NO_MATERIALS diagnostic when
    /// color is absent and include_colors is true (B8).
    ///
    /// (B7) Physical body with Material appearance Color(named:"#8899AA") + ThreeMFOutput →
    ///   recorded ExportOptions.color == Some(Rgb8{0x88,0x99,0xAA}), include_colors == true,
    ///   no W_3MF_NO_MATERIALS diagnostic.
    ///
    /// (B8) Raw box (no material body) + ThreeMFOutput(include_colors:true), kernel injected
    ///   with ThreeMfNoMaterials warning → recorded color == None, artifact diagnostic
    ///   contains "W_3MF_NO_MATERIALS".
    ///
    /// RED until step-8: build_outputs passes ..ExportOptions::default() (color None,
    /// include_colors false), not the body's resolved color or DSL include flags.
    // Deferred: the declarative `ThreeMFOutput(subject: self.body.geometry)` subject is a
    // cross-sub geometry-param access that compiles to the V0.1 no-op `CrossSubGeometryRef`
    // bypass (reify-compiler/src/expr.rs:726), so `build_outputs` cannot resolve it to a live
    // handle and emits 0 exports. Pre-existing substrate gap (CrossSubGeometryRef predates
    // this task), not a color-egress defect — the imperative `-o *.3mf` color path (the B7
    // user signal) lands here and is covered by build_imperative_threemf_threads_body_color +
    // cli_build_3mf::build_colored_box_to_3mf_writes_basematerials.
    #[test]
    #[ignore = "blocked on #4875 — declarative ThreeMFOutput cross-sub geometry subject (self.body.geometry) is a V0.1 CrossSubGeometryRef no-op pending GHR substrate; imperative color egress lands in #4763"]
    fn build_outputs_threads_body_color_into_export_options() {
        use reify_core::Severity;
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::Path;
        use std::sync::{Arc, Mutex};

        // --- B7: colored body → color threaded into ExportOptions ---
        {
            let module = parse_and_compile_with_stdlib(
                // r##"..."## avoids "#8899AA" containing `"#` which closes r#"..."#.
                r##"structure def ColoredBox : Physical {
    param geometry : Solid = box(10mm, 20mm, 5mm)
    param material : Material = Material(
        name: "painted",
        density: 7850kg/m^3,
        youngs_modulus: 200GPa,
        appearance: Appearance(color: Color(named: "#8899AA"))
    )
}
structure def D {
    sub body : ColoredBox
    sub out = ThreeMFOutput(subject: self.body.geometry, path: "o.3mf")
}"##,
            );
            let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
                Arc::new(Mutex::new(Vec::new()));
            let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
                Arc::new(Mutex::new(Vec::new()));
            let kernel =
                ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported));
            let exported_options = kernel.recorded_options();
            let mut engine = crate::Engine::new(
                Box::new(MockConstraintChecker::new()),
                Some(Box::new(kernel)),
            );
            let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);
            let recorded = exported_options.lock().unwrap().clone();

            // Exactly one export_with_options call.
            assert_eq!(
                recorded.len(),
                1,
                "B7: exactly one ThreeMFOutput occurrence must yield one export_with_options call"
            );
            let (_, fmt, _, color, include_colors) = &recorded[0];
            assert_eq!(
                *fmt,
                reify_ir::ExportFormat::ThreeMF,
                "B7: format must be ThreeMF"
            );
            assert_eq!(
                *color,
                Some(reify_ir::Rgb8 { r: 0x88, g: 0x99, b: 0xAA }),
                "B7: the body's resolved #8899AA color must reach ExportOptions.color"
            );
            assert!(
                *include_colors,
                "B7: ThreeMFOutput default include_colors=true must thread into ExportOptions"
            );

            // No W_3MF_NO_MATERIALS diagnostic when color is present.
            let w3mf_diags: Vec<_> = artifacts
                .iter()
                .flat_map(|a| &a.diagnostics)
                .filter(|d| d.message.contains("W_3MF_NO_MATERIALS"))
                .collect();
            assert!(
                w3mf_diags.is_empty(),
                "B7: no W_3MF_NO_MATERIALS diagnostic expected when color is Some; got {:?}",
                w3mf_diags
            );
        }

        // --- B8: no material → color None, injected ThreeMfNoMaterials → diagnostic ---
        {
            let module = parse_and_compile_with_stdlib(
                r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub out = ThreeMFOutput(subject: part, include_colors: true, path: "o.3mf")
}"#,
            );
            let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
                Arc::new(Mutex::new(Vec::new()));
            let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
                Arc::new(Mutex::new(Vec::new()));
            // Inject the ThreeMfNoMaterials warning the real kernel would emit for color=None+include_colors=true.
            let kernel =
                ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported))
                    .with_warnings(vec![reify_ir::ExportWarning::ThreeMfNoMaterials]);
            let exported_options = kernel.recorded_options();
            let mut engine = crate::Engine::new(
                Box::new(MockConstraintChecker::new()),
                Some(Box::new(kernel)),
            );
            let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);
            let recorded = exported_options.lock().unwrap().clone();

            assert_eq!(
                recorded.len(),
                1,
                "B8: exactly one export_with_options call expected"
            );
            let (_, _, _, color, _) = &recorded[0];
            assert_eq!(
                *color,
                None,
                "B8: a raw box with no Physical body has color == None"
            );

            // The injected ThreeMfNoMaterials must appear as a W_3MF_NO_MATERIALS diagnostic.
            let w3mf_diag_count = artifacts
                .iter()
                .flat_map(|a| &a.diagnostics)
                .filter(|d| {
                    d.message.contains("W_3MF_NO_MATERIALS") && d.severity == Severity::Warning
                })
                .count();
            assert_eq!(
                w3mf_diag_count, 1,
                "B8: exactly one W_3MF_NO_MATERIALS warning diagnostic expected from injected ThreeMfNoMaterials"
            );
        }
    }

    /// δ step-9 (task #4763): the imperative `engine.build()` Phase-B export walk
    /// uses `export_with_options()` (not `export()`) and threads the Physical body's
    /// resolved material color into `ExportOptions.color`.
    ///
    /// Module: `Assembly { sub part : ColoredBox }` where `ColoredBox : Physical`
    /// carries `Material(appearance: Appearance(color: Color(named: "#8899AA")))`.
    ///
    /// Assert: exactly one `export_with_options` call is recorded, with
    /// `ExportOptions.color == Some(Rgb8{0x88, 0x99, 0xAA})`.
    ///
    /// RED until step-10: `build_with_geometry_output` Phase-B calls
    /// `default_kernel.export(...)` (no options), so `exported_options` stays empty.
    #[test]
    fn build_imperative_threemf_threads_body_color() {
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::sync::{Arc, Mutex};

        let module = parse_and_compile_with_stdlib(
            // Same fixture shape as box_3mf_colored.ri: Assembly wrapper so
            // ValueCellId("Assembly","part") = StructureInstance{geometry,material}
            // appears in values and resolve_export_body_color can match it.
            r##"structure def ColoredBox : Physical {
    param geometry : Solid = box(10mm, 20mm, 5mm)
    param material : Material = Material(
        name: "painted",
        density: 7850kg/m^3,
        youngs_modulus: 200GPa,
        appearance: Appearance(color: Color(named: "#8899AA"))
    )
}
structure Assembly {
    sub part : ColoredBox
}"##,
        );

        let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
            Arc::new(Mutex::new(Vec::new()));
        let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported));
        let exported_options = kernel.recorded_options();

        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        // Imperative build: engine.build() uses the Phase-B export walk.
        let _result = engine.build(&module, reify_ir::ExportFormat::ThreeMF);

        let recorded = exported_options.lock().unwrap().clone();

        // Phase-B must use export_with_options (not export), so exactly one
        // entry in exported_options for the single product body.
        assert_eq!(
            recorded.len(),
            1,
            "imperative build must yield exactly one export_with_options call \
             for the single Assembly.part product body; got {:?}",
            recorded.len()
        );
        let (_, fmt, _, color, _) = &recorded[0];
        assert_eq!(
            *fmt,
            reify_ir::ExportFormat::ThreeMF,
            "format must be ThreeMF"
        );
        assert_eq!(
            *color,
            Some(reify_ir::Rgb8 { r: 0x88, g: 0x99, b: 0xAA }),
            "the body's resolved #8899AA color must reach ExportOptions.color on the imperative path"
        );
    }

    /// step-11 (ε / task 4288): when the kernel reports an AP242→AP214
    /// fallback (`ExportWarning::StepAp242Fallback`), the driver surfaces it as
    /// exactly one warning-severity diagnostic carrying the
    /// `W_STEP_AP242_FALLBACK` code and naming the occurrence — *without*
    /// dropping the successfully written bytes (a fallback is honest
    /// degradation, not a failure). The live OCCT AP242 fallback cannot be
    /// triggered in this build (it supports AP242DIS), so the warning is
    /// injected via the recording kernel's `warnings_to_return`.
    #[test]
    fn build_outputs_surfaces_ap242_fallback_warning() {
        use reify_core::Severity;
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::Path;
        use std::sync::{Arc, Mutex};

        let module = parse_and_compile_with_stdlib(
            r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub s = STEPOutput(subject: part, version: STEPVersion.AP242, path: "x.step")
}"#,
        );

        let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
            Arc::new(Mutex::new(Vec::new()));
        let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
            Arc::new(Mutex::new(Vec::new()));
        // Inject the AP242→AP214 fallback the in-build OCCT can't produce.
        let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported))
            .with_warnings(vec![reify_ir::ExportWarning::StepAp242Fallback]);
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);

        assert_eq!(
            artifacts.len(),
            1,
            "exactly one ExportArtifact for the single STEPOutput occurrence, got {}",
            artifacts.len()
        );
        let art = &artifacts[0];
        assert!(
            !art.bytes.is_empty(),
            "a fallback is a WARNING, not a failure: the written bytes must survive"
        );

        let fallback_diags: Vec<&reify_core::Diagnostic> = art
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("W_STEP_AP242_FALLBACK"))
            .collect();
        assert_eq!(
            fallback_diags.len(),
            1,
            "exactly one W_STEP_AP242_FALLBACK diagnostic for the injected fallback, got {}",
            fallback_diags.len()
        );
        assert_eq!(
            fallback_diags[0].severity,
            Severity::Warning,
            "the AP242 fallback must be warning-severity (honest degradation, not an error)"
        );
        assert!(
            fallback_diags[0].message.contains("D.s"),
            "the diagnostic must name the occurrence (`D.s`); message was: {}",
            fallback_diags[0].message
        );
    }

    /// step-09 (RED): `build_outputs` emits one [`crate::ExportArtifact`] per
    /// recognized `Output` occurrence, in declaration order (B6).
    ///
    /// Two occurrences on the same solid — `sub o = STLOutput(...)` then
    /// `sub s = STEPOutput(...)` — must yield exactly two artifacts in source
    /// order: `[{Stl, "/tmp/d/o.stl"}, {Step, "/tmp/d/o2.step"}]`, and the
    /// recording kernel must observe the two `export()` calls as `[Stl, Step]`
    /// in that same order. The `STEPOutput` occurrence's `format` default
    /// (`OutputFormat.STEP`) must route to `ExportFormat::Step`, proving the
    /// per-occurrence DSL format — not a single shared flag — drives each file.
    ///
    /// RED until step-10: the step-08 happy path breaks after the FIRST
    /// recognized occurrence, so it emits a single STL artifact and this test's
    /// `artifacts.len() == 2` (and the `[Stl, Step]` export order) fail.
    #[test]
    fn build_outputs_emits_one_artifact_per_occurrence_in_declaration_order() {
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::{Path, PathBuf};
        use std::sync::{Arc, Mutex};

        let module = parse_and_compile_with_stdlib(
            r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub o = STLOutput(subject: part, path: "o.stl")
    sub s = STEPOutput(subject: part, path: "o2.step")
}"#,
        );

        let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
            Arc::new(Mutex::new(Vec::new()));
        let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported));
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);

        assert_eq!(
            artifacts.len(),
            2,
            "one artifact per Output occurrence (STLOutput + STEPOutput), got {}",
            artifacts.len()
        );
        // Declaration order: STLOutput first, STEPOutput second.
        assert_eq!(artifacts[0].format, reify_ir::ExportFormat::Stl);
        assert_eq!(artifacts[0].path, PathBuf::from("/tmp/d/o.stl"));
        assert_eq!(
            artifacts[1].format,
            reify_ir::ExportFormat::Step,
            "the STEPOutput occurrence's format default (STEP) must route to Step"
        );
        assert_eq!(artifacts[1].path, PathBuf::from("/tmp/d/o2.step"));

        let exported = exported.lock().unwrap().clone();
        let formats: Vec<reify_ir::ExportFormat> = exported.iter().map(|(_, f)| *f).collect();
        assert_eq!(
            formats,
            vec![reify_ir::ExportFormat::Stl, reify_ir::ExportFormat::Step],
            "the recording kernel must observe per-occurrence exports [Stl, Step] \
             in declaration order, got {:?}",
            formats
        );
        let executed = executed.lock().unwrap().clone();
        for (handle, _) in &exported {
            assert!(
                executed.contains(handle),
                "each exported handle {:?} must be a realized `subject: part` \
                 handle; realized handles were {:?}",
                handle,
                executed
            );
        }
    }

    /// step-11 (RED): `build_outputs` RECOGNIZES a `DisplayOutput` occurrence as
    /// a conforming `Output` but DEFERS its file emission (the viewport drive is
    /// a sibling PRD), surfacing an info-severity [`crate::I_DISPLAY_OUTPUT_DEFERRED`]
    /// diagnostic instead of a file — while an `Input` occurrence (`STEPInput`)
    /// is EXCLUDED entirely (it conforms to `Input`, not `Output`).
    ///
    /// The module mixes all three: one `STLOutput` (a file), one `DisplayOutput`
    /// (recognize-but-defer), one `STEPInput` (not an Output at all). The driver
    /// must therefore produce exactly ONE file artifact (the STLOutput, with
    /// non-empty bytes), surface exactly ONE `I_DISPLAY_OUTPUT_DEFERRED` info
    /// diagnostic for the DisplayOutput, and emit NEITHER artifact NOR diagnostic
    /// for the STEPInput. The recording kernel must observe exactly ONE
    /// `export()` call (the STLOutput) — proving DisplayOutput/STEPInput drove no
    /// serialization.
    ///
    /// RED until step-12: the step-8/10 happy path `continue`s silently on a
    /// `DisplayDeferred` target, so no `I_DISPLAY_OUTPUT_DEFERRED` diagnostic is
    /// surfaced and this test's diagnostic assertion fails.
    #[test]
    fn build_outputs_defers_display_output_and_excludes_input() {
        use reify_core::Severity;
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::{Path, PathBuf};
        use std::sync::{Arc, Mutex};

        let module = parse_and_compile_with_stdlib(
            r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub o = STLOutput(subject: part, path: "o.stl")
    sub d = DisplayOutput(subject: part)
    sub i = STEPInput(source: "in.step")
}"#,
        );

        let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
            Arc::new(Mutex::new(Vec::new()));
        let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported));
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);

        // Exactly one FILE artifact (non-empty bytes): the STLOutput. The
        // DisplayOutput is recognized-but-deferred (a zero-byte skipped entry,
        // never a written file); STEPInput contributes no entry at all.
        let files: Vec<&crate::ExportArtifact> =
            artifacts.iter().filter(|a| !a.bytes.is_empty()).collect();
        assert_eq!(
            files.len(),
            1,
            "exactly one FILE artifact (the STLOutput); DisplayOutput defers and \
             STEPInput is excluded, got files {:?}",
            files.iter().map(|a| &a.path).collect::<Vec<_>>()
        );
        assert_eq!(files[0].format, reify_ir::ExportFormat::Stl);
        assert_eq!(files[0].path, PathBuf::from("/tmp/d/o.stl"));

        // Exactly one info-severity I_DISPLAY_OUTPUT_DEFERRED diagnostic, for the
        // DisplayOutput. "Result diagnostics" = every artifact's diagnostics.
        let display_diags: Vec<&reify_core::Diagnostic> = artifacts
            .iter()
            .flat_map(|a| &a.diagnostics)
            .filter(|d| d.message.contains(crate::I_DISPLAY_OUTPUT_DEFERRED))
            .collect();
        assert_eq!(
            display_diags.len(),
            1,
            "exactly one I_DISPLAY_OUTPUT_DEFERRED diagnostic for the single \
             DisplayOutput occurrence, got {}",
            display_diags.len()
        );
        assert_eq!(
            display_diags[0].severity,
            Severity::Info,
            "the DisplayOutput-deferred diagnostic must be info-severity (not an \
             error that would fail the build)"
        );

        // STEPInput (an `Input`, not an `Output`) produces NO diagnostic of any
        // kind — it is filtered out by the conforms_to_output gate before any
        // spec read.
        let input_diags = artifacts
            .iter()
            .flat_map(|a| &a.diagnostics)
            .filter(|d| d.message.contains("STEPInput") || d.message.contains(".i"))
            .count();
        assert_eq!(
            input_diags, 0,
            "STEPInput is not an Output: it must produce neither artifact nor diagnostic"
        );

        // The kernel serialized exactly once — the STLOutput. DisplayOutput and
        // STEPInput drove no export() call.
        let exported = exported.lock().unwrap().clone();
        assert_eq!(
            exported.len(),
            1,
            "only the STLOutput exports; DisplayOutput defers and STEPInput is \
             excluded, got {} export() calls",
            exported.len()
        );
        assert_eq!(exported[0].1, reify_ir::ExportFormat::Stl);
    }

    /// step-13 helper: a kernel whose FIRST `export()` call fails with a
    /// [`reify_ir::ExportError`] and whose subsequent calls succeed (delegated
    /// to the inner mock). With `build_outputs`'s Phase-B product export
    /// disabled, the only `export()` calls are the per-occurrence ones, so call
    /// #1 is the first `Output` occurrence and call #2 the second — letting a
    /// test drive "first occurrence fails, second succeeds".
    struct FailFirstExportKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        export_calls: std::sync::Mutex<usize>,
    }

    impl reify_ir::GeometryKernel for FailFirstExportKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            let mut n = self.export_calls.lock().unwrap();
            *n += 1;
            if *n == 1 {
                return Err(reify_ir::ExportError::FormatError(
                    "injected failure (first export)".to_string(),
                ));
            }
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.inner.tessellate(handle, tolerance)
        }

        fn make_compound(
            &mut self,
            handles: &[reify_ir::GeometryHandleId],
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.make_compound(handles)
        }
    }

    /// step-13 (RED): a per-occurrence export failure must be ISOLATED — it
    /// emits an error diagnostic and the loop CONTINUES, so a later valid
    /// `Output` occurrence still serializes its file. One bad Output never
    /// aborts the others (PRD §4.3/§7.3 per-artifact failure isolation).
    ///
    /// Two `STLOutput`s on the same solid; the kernel fails the FIRST `export()`
    /// (occurrence `o`) and succeeds the second (occurrence `s`). The driver
    /// must NOT panic/abort: it surfaces an error-severity diagnostic naming the
    /// failed occurrence's path (`o.stl`) AND still produces a written artifact
    /// (non-empty bytes) for the valid `s` (`o2.stl`).
    ///
    /// RED until step-14: the step-8 happy path `continue`s SILENTLY on an
    /// export `Err` (no diagnostic), so the error-diagnostic assertion fails.
    #[test]
    fn build_outputs_isolates_per_occurrence_export_failure() {
        use reify_core::Severity;
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::{Path, PathBuf};

        let module = parse_and_compile_with_stdlib(
            r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub o = STLOutput(subject: part, path: "o.stl")
    sub s = STLOutput(subject: part, path: "o2.stl")
}"#,
        );

        let kernel = FailFirstExportKernel {
            inner: reify_test_support::mocks::MockGeometryKernel::new(),
            export_calls: std::sync::Mutex::new(0),
        };
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        // Must not panic even though the first occurrence's export errors.
        let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);

        // The failed occurrence (`o`) carries an error-severity diagnostic that
        // names its path, so the failure is attributable and not silent.
        let error_diags: Vec<&reify_core::Diagnostic> = artifacts
            .iter()
            .flat_map(|a| &a.diagnostics)
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(
            error_diags.len(),
            1,
            "the failed occurrence must surface exactly one error diagnostic, got {}",
            error_diags.len()
        );
        assert!(
            error_diags[0].message.contains("o.stl"),
            "the error diagnostic must name the failed occurrence's path (o.stl); got {:?}",
            error_diags[0].message
        );

        // Isolation: the valid SECOND occurrence (`s`) still produced a written
        // file with bytes despite the first occurrence failing.
        let written: Vec<&crate::ExportArtifact> =
            artifacts.iter().filter(|a| !a.bytes.is_empty()).collect();
        assert_eq!(
            written.len(),
            1,
            "the valid second occurrence still serializes a file despite the \
             first failing, got {} written artifacts",
            written.len()
        );
        assert_eq!(
            written[0].path,
            PathBuf::from("/tmp/d/o2.stl"),
            "the surviving artifact must be the second (valid) occurrence o2.stl"
        );
    }

    /// step-09 (RED): `seed_cross_sub_named_steps` must thread [`KernelHandle`]
    /// (not bare [`GeometryHandleId`]) through `named_steps` /
    /// `module_named_steps`.
    ///
    /// Exercises the no-args seeding path: a parent template with
    /// `sub a = Inner()` copies the child template's completed `Inner.body`
    /// snapshot entry into the parent's `named_steps` under the compound key
    /// `a.body`. The seeded value is a [`KernelHandle`] carrying the producing
    /// kernel's [`KernelId`] (Manifold here) alongside the kernel-local
    /// [`GeometryHandleId`]; the no-args path copies it verbatim, so `a.body`
    /// must resolve to exactly that [`KernelHandle`] — `.id` equal to the seeded
    /// handle id and `.kernel` equal to the seeding kernel's [`KernelId`].
    ///
    /// RED on the pre-migration signature: `module_named_steps` / `named_steps`
    /// are typed `…GeometryHandleId`, so passing `…KernelHandle` maps fails to
    /// type-check until step-10 flips the value type.
    #[test]
    fn seed_cross_sub_named_steps_threads_kernel_handle_on_no_args_path() {
        use reify_ir::{GeometryHandleId, KernelHandle, KernelId};
        use reify_test_support::builders::TopologyTemplateBuilder;

        // Parent template: `sub a = Inner()` — no args, non-collection.
        let template = TopologyTemplateBuilder::new("Parent")
            .sub_component("a", "Inner", Vec::new())
            .build();

        // Child snapshot: `Inner.body` was produced by the Manifold kernel as
        // GeometryHandleId(5), recorded as a KernelHandle.
        let seeded = KernelHandle {
            kernel: KernelId::Manifold,
            id: GeometryHandleId(5),
        };
        let mut inner_snapshot: HashMap<String, KernelHandle> = HashMap::new();
        inner_snapshot.insert("body".to_string(), seeded);
        let mut module_named_steps: HashMap<String, HashMap<String, KernelHandle>> = HashMap::new();
        module_named_steps.insert("Inner".to_string(), inner_snapshot);

        // The no-args path reads only `template.sub_components` +
        // `module_named_steps`; the kernel/value/function/template inputs are
        // unused on this path, so empty instances suffice.
        let mut named_steps: HashMap<String, KernelHandle> = HashMap::new();
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = Vec::new();
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let templates: Vec<TopologyTemplate> = Vec::new();

        seed_cross_sub_named_steps(
            &template,
            &module_named_steps,
            &mut named_steps,
            &mut kernels,
            "default",
            &values,
            &functions,
            &meta_map,
            &mut diagnostics,
            &templates,
        );

        let got = named_steps
            .get("a.body")
            .copied()
            .expect("no-args seeding must insert compound key `a.body`");
        assert_eq!(
            got, seeded,
            "named_steps value type must be KernelHandle, copied verbatim from the child snapshot"
        );
        assert_eq!(
            got.id,
            GeometryHandleId(5),
            ".id must equal the seeded GeometryHandleId"
        );
        assert_eq!(
            got.kernel,
            KernelId::Manifold,
            ".kernel must equal the seeding kernel's KernelId"
        );
        assert!(diagnostics.is_empty(), "no-args path emits no diagnostics");
    }

    /// `arg_contains_cross_sub_geometry_ref` must detect a `CrossSubGeometryRef`
    /// at the top level *and* nested inside a larger operator node, and must not
    /// false-positive on ref-free args. The nested case is the task-3616
    /// regression: the old top-level-only `matches!` guard let a
    /// `CrossSubGeometryRef` nested in a transform-chain arg
    /// (`translate(rotate(self.inner.body, …), …)`) reach `eval_expr`'s
    /// `unreachable!()` — pinned end-to-end by
    /// `cross_sub_geometry_anti_cascade_no_spurious_errors_in_translate_chain`.
    #[test]
    fn arg_contains_cross_sub_geometry_ref_walks_nested_refs() {
        use reify_core::Type;
        use reify_core::identity::ValueCellId;
        use reify_ir::{BinOp, CompiledExpr};

        // Top-level cross-sub ref → detected.
        let xref = CompiledExpr::cross_sub_geometry_ref(
            ValueCellId::new("Parent.sub", "body"),
            Type::Geometry,
        );
        assert!(arg_contains_cross_sub_geometry_ref(&xref));

        // Cross-sub ref nested inside an operator node → detected (the case the
        // old top-level `matches!` missed).
        let scalar = CompiledExpr::value_ref(ValueCellId::new("E", "width"), Type::Bool);
        let nested = CompiledExpr::binop(BinOp::Gt, xref.clone(), scalar, Type::Bool);
        assert!(arg_contains_cross_sub_geometry_ref(&nested));

        // Ref-free arg → not skipped.
        let plain = CompiledExpr::binop(
            BinOp::Gt,
            CompiledExpr::value_ref(ValueCellId::new("E", "a"), Type::Bool),
            CompiledExpr::value_ref(ValueCellId::new("E", "b"), Type::Bool),
            Type::Bool,
        );
        assert!(!arg_contains_cross_sub_geometry_ref(&plain));
    }

    // ── shared test helpers (task ε / 3436, step-8) ───────────────────────────

    /// Build a [`CapabilityDescriptor`] that supports every [`Operation`]
    /// variant against [`ReprKind::BRep`]. Used by the
    /// `execute_realization_ops_*` unit tests below to construct a synthetic
    /// dispatch registry that routes every supported op to a single
    /// kernel-by-name (`"default"`) — preserving the v0.2 single-kernel
    /// behaviour while exercising the per-op dispatch routing seam wired in
    /// step-8.
    ///
    /// Tests that exercise the "no kernel for op" path (`dispatch` returns
    /// `None`) construct their own minimal descriptor inline instead.
    fn dispatch_test_descriptor_all_brep() -> CapabilityDescriptor {
        CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (Operation::PrimitiveCylinder, ReprKind::BRep),
                (Operation::PrimitiveSphere, ReprKind::BRep),
                (Operation::PrimitiveTube, ReprKind::BRep),
                (Operation::PrimitiveCone, ReprKind::BRep),
                (Operation::PrimitiveWedge, ReprKind::BRep),
                (Operation::BooleanUnion, ReprKind::BRep),
                (Operation::BooleanDifference, ReprKind::BRep),
                (Operation::BooleanIntersection, ReprKind::BRep),
                (Operation::ModifyFillet, ReprKind::BRep),
                (Operation::ModifyChamfer, ReprKind::BRep),
                (Operation::ModifyShell, ReprKind::BRep),
                (Operation::ModifyDraft, ReprKind::BRep),
                (Operation::ModifyThicken, ReprKind::BRep),
                (Operation::ModifyOffsetCurve, ReprKind::BRep),
                (Operation::ModifyZoneSlab, ReprKind::BRep),
                (Operation::ModifyOffsetSolid, ReprKind::BRep),
                (Operation::TransformTranslate, ReprKind::BRep),
                (Operation::TransformRotate, ReprKind::BRep),
                (Operation::TransformScale, ReprKind::BRep),
                (Operation::TransformRotateAround, ReprKind::BRep),
                (Operation::TransformApplyTransform, ReprKind::BRep),
                (Operation::TransformAffineApply, ReprKind::BRep),
                (Operation::PatternLinear, ReprKind::BRep),
                (Operation::PatternCircular, ReprKind::BRep),
                (Operation::PatternMirror, ReprKind::BRep),
                (Operation::PatternLinear2D, ReprKind::BRep),
                (Operation::PatternArbitrary, ReprKind::BRep),
                (Operation::SweepLoft, ReprKind::BRep),
                (Operation::SweepExtrude, ReprKind::BRep),
                (Operation::SweepRevolve, ReprKind::BRep),
                (Operation::SweepSweep, ReprKind::BRep),
                (Operation::SweepExtrudeSymmetric, ReprKind::BRep),
                (Operation::SweepExtrudeInfinite, ReprKind::BRep),
                (Operation::SweepSweepGuided, ReprKind::BRep),
                (Operation::SweepLoftGuided, ReprKind::BRep),
                (Operation::SweepPipe, ReprKind::BRep),
                (Operation::CurveLineSegment, ReprKind::BRep),
                (Operation::CurveArc, ReprKind::BRep),
                (Operation::CurveHelix, ReprKind::BRep),
                (Operation::CurveInterpCurve, ReprKind::BRep),
                (Operation::CurveBezierCurve, ReprKind::BRep),
                (Operation::CurveNurbsCurve, ReprKind::BRep),
            ],
        }
    }

    /// Wrap a single boxed [`GeometryKernel`] into a multi-handle kernel map
    /// keyed by `"default"`. Returns the map ready to pass as
    /// `&mut kernels` to [`Engine::execute_realization_ops`]. Mirrors what
    /// `with_prelude`/`new` do for the production builders (synthetic default
    /// name) while keeping per-test setup terse.
    fn dispatch_test_kernels(
        kernel: Box<dyn GeometryKernel>,
    ) -> BTreeMap<String, Box<dyn GeometryKernel>> {
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert("default".to_string(), kernel);
        kernels
    }

    /// Build the "single-default" borrowed registry view used by most
    /// `execute_realization_ops_*` unit tests. The descriptor must outlive the
    /// returned map because the `&CapabilityDescriptor` value borrows from it;
    /// callers typically use the pattern
    /// `let desc = dispatch_test_descriptor_all_brep(); let registry =
    /// dispatch_test_single_default_registry(&desc);`.
    fn dispatch_test_single_default_registry(
        descriptor: &CapabilityDescriptor,
    ) -> BTreeMap<String, &CapabilityDescriptor> {
        let mut r: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        r.insert("default".to_string(), descriptor);
        r
    }

    /// Per-test mutable state for the `execute_realization_ops_*` unit tests
    /// (amendment to task ε / 3436 — addresses reviewer suggestion #1).
    ///
    /// Owns the bag of `&mut`-borrowed scratch storage that
    /// [`Engine::execute_realization_ops`] writes into — step handles,
    /// diagnostics, named-steps map, attribute tables, kernel-error channel,
    /// realization cache, dispatch counter, and the produced-repr out-param.
    /// Constructed via [`Default::default`] and inspected via public fields
    /// after [`Self::run`] returns.
    ///
    /// Tests with pre-seeded `step_handles` (the rollback-truncation tests)
    /// push directly into `state.step_handles` before the call. Tests that
    /// drive multiple sequential realizations against the same state (the
    /// `_shadows_previous` / `_failed_shadow_…` tests) call
    /// [`Self::reset_attribute_tables`] between calls, mirroring the per-build
    /// reset in production.
    ///
    /// A future signature change to `Engine::execute_realization_ops` updates
    /// [`Self::run`] alone instead of every per-test call site.
    struct DispatchTestState {
        step_handles: Vec<KernelHandle>,
        diagnostics: Vec<Diagnostic>,
        named_steps: HashMap<String, KernelHandle>,
        // Task 5033 Gap #2 Gap A: by-name repr sibling of `named_steps`. See
        // `RealizationOutputs::named_step_reprs` doc for why.
        named_step_reprs: HashMap<String, ReprKind>,
        topology_attribute_table: TopologyAttributeTable,
        swept_kind_table: SweptKindTable,
        kernel_error_out: Option<ErrorRef>,
        realization_cache: RealizationCache<KernelHandle>,
        dispatch_count: usize,
        // Task ε (4741): per-realization sibling of `dispatch_count`; threaded
        // into `execute_realization_ops` by `run` / `run_demand`. Not asserted
        // by these unit tests (they pin the aggregate), but required to satisfy
        // the new `execute_realization_ops` signature.
        dispatch_count_by_realization: HashMap<RealizationNodeId, usize>,
        produced_repr_out: Option<ReprKind>,
    }

    // Hand-written `Default` instead of `#[derive(Default)]`: the inner
    // `RealizationCache<KernelHandle>` does not satisfy the derive bound
    // (`V: Default`) — `KernelHandle` pairs a `KernelId` with a `NewType(u64)`
    // and has no `Default` impl — but `RealizationCache::new()` constructs an empty cache
    // without that bound. Mirrors how production code initialises the field
    // (engine_admin.rs `Engine::with_prelude_and_kernels`).
    impl Default for DispatchTestState {
        fn default() -> Self {
            Self {
                step_handles: Vec::new(),
                diagnostics: Vec::new(),
                named_steps: HashMap::new(),
                named_step_reprs: HashMap::new(),
                topology_attribute_table: TopologyAttributeTable::default(),
                swept_kind_table: SweptKindTable::default(),
                kernel_error_out: None,
                realization_cache: RealizationCache::new(),
                dispatch_count: 0,
                dispatch_count_by_realization: HashMap::new(),
                produced_repr_out: None,
            }
        }
    }

    impl DispatchTestState {
        /// Reset the two per-realization attribute tables (mirrors the
        /// per-build reset in production at `build` / `build_snapshot` /
        /// `tessellate_*`). Called by the shadow tests between sequential
        /// realizations so the second call sees the same clean-table state the
        /// first did.
        fn reset_attribute_tables(&mut self) {
            self.topology_attribute_table = TopologyAttributeTable::default();
            self.swept_kind_table = SweptKindTable::default();
        }

        /// Drive [`Engine::execute_realization_ops`] against this state with
        /// the canonical unit-test boilerplate — empty `ValueMap` /
        /// `functions` / `meta_map`, the canonical `TestEntity` realization
        /// id, and `demanded_tol = None` (the cache short-circuit is exercised
        /// from the integration tests in `tests/multi_handle_engine_dispatch.rs`,
        /// not from this unit-test surface).
        ///
        /// A future signature change to `execute_realization_ops` updates
        /// this method alone instead of every per-test call site (~14
        /// mechanical edits).
        #[allow(clippy::too_many_arguments)]
        fn run(
            &mut self,
            kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
            registry: &BTreeMap<String, &CapabilityDescriptor>,
            default_kernel: &str,
            ops: &[reify_compiler::CompiledGeometryOp],
            realization_name: Option<&str>,
            realization_span: SourceSpan,
            // Task #3443: pragma preference forwarded to `execute_realization_ops`.
            // Existing pragma-agnostic tests pass `None`; the S3 pragma steering
            // test supplies `Some("occt")`.
            prefer_kernel: Option<&str>,
        ) {
            let values = ValueMap::new();
            let functions: Vec<CompiledFunction> = vec![];
            let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
            let test_realization_id = RealizationNodeId::new("TestEntity", 0);
            Engine::execute_realization_ops(
                RealizationOpsInput::new(
                    kernels,
                    registry,
                    default_kernel,
                    ops,
                    &values,
                    &functions,
                    &meta_map,
                    &mut self.diagnostics,
                    &test_realization_id,
                    realization_span,
                    &mut self.kernel_error_out,
                    &mut self.realization_cache,
                    &mut self.dispatch_count,
                    &mut self.dispatch_count_by_realization,
                )
                .with_realization_name(realization_name)
                // Task 4050 step-8: the existing single-kernel unit tests want
                // the v0.2 BRep demand (the `RealizationOpsInput::new` default);
                // the cross-kernel tests use `run_demand`.
                .with_prefer_kernel(prefer_kernel)
                // Test helpers operate on a single realization; it is always terminal.
                .with_is_terminal_realization(true)
                // Amendment (reviewer_comprehensive robustness @ engine_build.rs:230):
                // `RealizationOpsInput::new`'s `long_chain_threshold` now defaults to
                // a cheap constant, not an env read (see that field's doc). This is a
                // general-purpose helper shared by every test in this module, so keep
                // it explicitly env-sensitive rather than silently inheriting the
                // constant default — preserves the pre-refactor contract for any test
                // that sets `REIFY_LONG_CHAIN_THRESHOLD_MS`.
                .with_long_chain_threshold(crate::dispatcher::long_chain_threshold_from_env()),
                RealizationOutputs::new(
                    &mut self.step_handles,
                    &mut self.named_steps,
                    &mut self.named_step_reprs,
                    &mut self.topology_attribute_table,
                    &mut self.swept_kind_table,
                    &mut self.produced_repr_out,
                ),
            );
        }

        /// Like [`Self::run`] but threads a caller-controlled `demanded_repr`,
        /// `demanded_tol`, `realization_id`, and `realization_name` so the
        /// conversion-executor / cache-unpin tests (task 4050 steps 7/9/11/13)
        /// can drive a `Mesh` demand, name a realization for caching, and reuse
        /// `self`'s shared `realization_cache` / `dispatch_count` across
        /// sequential calls. `run` hard-codes `demanded_tol = None` /
        /// `demanded_repr = BRep` / `TestEntity`, which the v0.2 single-kernel
        /// tests want; the cross-kernel tests need all four under their own
        /// control.
        #[allow(clippy::too_many_arguments)]
        fn run_demand(
            &mut self,
            kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
            registry: &BTreeMap<String, &CapabilityDescriptor>,
            default_kernel: &str,
            ops: &[reify_compiler::CompiledGeometryOp],
            realization_id: &RealizationNodeId,
            realization_name: Option<&str>,
            realization_span: SourceSpan,
            demanded_repr: ReprKind,
            demanded_tol: Option<f64>,
            // Task #3443: pragma preference forwarded to `execute_realization_ops`.
            // Existing pragma-agnostic tests pass `None`.
            prefer_kernel: Option<&str>,
        ) {
            let values = ValueMap::new();
            let functions: Vec<CompiledFunction> = vec![];
            let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
            Engine::execute_realization_ops(
                RealizationOpsInput::new(
                    kernels,
                    registry,
                    default_kernel,
                    ops,
                    &values,
                    &functions,
                    &meta_map,
                    &mut self.diagnostics,
                    realization_id,
                    realization_span,
                    &mut self.kernel_error_out,
                    &mut self.realization_cache,
                    &mut self.dispatch_count,
                    &mut self.dispatch_count_by_realization,
                )
                .with_realization_name(realization_name)
                .with_demanded_tol(demanded_tol)
                .with_demanded_repr(demanded_repr)
                .with_prefer_kernel(prefer_kernel)
                // Test helpers operate on a single realization; it is always terminal.
                .with_is_terminal_realization(true)
                // Amendment (reviewer_comprehensive robustness @ engine_build.rs:230):
                // `RealizationOpsInput::new`'s `long_chain_threshold` now defaults to
                // a cheap constant, not an env read (see that field's doc). This is a
                // general-purpose helper shared by every test in this module, so keep
                // it explicitly env-sensitive rather than silently inheriting the
                // constant default — preserves the pre-refactor contract for any test
                // that sets `REIFY_LONG_CHAIN_THRESHOLD_MS`.
                .with_long_chain_threshold(crate::dispatcher::long_chain_threshold_from_env()),
                RealizationOutputs::new(
                    &mut self.step_handles,
                    &mut self.named_steps,
                    &mut self.named_step_reprs,
                    &mut self.topology_attribute_table,
                    &mut self.swept_kind_table,
                    &mut self.produced_repr_out,
                ),
            );
        }
    }

    // ── RealizationOpsInput builder unit tests (task 5054 ζ) ──────────────────

    /// `RealizationOpsInput` is the input-side twin of `RealizationOutputs`
    /// (task 3119): `new()` takes the 14 CORE borrows that have no meaningful
    /// default, and the 8 ORTHOGONAL fields — the historical signature-churn
    /// axis (survey §H2) — default and are overridable via chainable
    /// `with_*` setters. This is the one genuinely new testable surface the
    /// input-twin introduces; the ~600-line executor body itself stays
    /// guarded by the pre-existing `execute_realization_ops_*` unit tests
    /// below (this task makes zero behavior changes to that body).
    #[test]
    fn realization_ops_input_builder_defaults_and_overrides() {
        // ── (1) `new()` defaults every ORTHOGONAL field to its documented value ──
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        let registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        let ops: Vec<CompiledGeometryOp> = vec![];
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let id = RealizationNodeId::new("TestEntity", 0);
        let mut diags: Vec<Diagnostic> = vec![];
        let mut kerr: Option<ErrorRef> = None;
        let mut cache = RealizationCache::new();
        let mut dc = 0usize;
        let mut dcbr: HashMap<RealizationNodeId, usize> = HashMap::new();

        let input = RealizationOpsInput::new(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut diags,
            &id,
            SourceSpan::new(0, 0),
            &mut kerr,
            &mut cache,
            &mut dc,
            &mut dcbr,
        );
        assert!(
            input.realization_name.is_none(),
            "realization_name must default to None"
        );
        assert!(input.demanded_tol.is_none(), "demanded_tol must default to None");
        assert_eq!(
            input.demanded_repr,
            ReprKind::BRep,
            "demanded_repr must default to BRep"
        );
        assert!(
            !input.demanded_boundary,
            "demanded_boundary must default to false"
        );
        assert!(input.prefer_kernel.is_none(), "prefer_kernel must default to None");
        assert!(
            !input.is_terminal_realization,
            "is_terminal_realization must default to false (conservative: no cache probe)"
        );
        assert_eq!(
            input.long_chain_threshold,
            Duration::from_millis(crate::dispatcher::LONG_CHAIN_DEFAULT_THRESHOLD_MS),
            "long_chain_threshold must default to the cheap PRD-default constant \
             (not an env read — every production call site overrides it with a \
             once-per-entry-resolved value anyway)"
        );

        // ── (2) `with_*` setters roundtrip an override for each ORTHOGONAL field ──
        let mut kernels2: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        let registry2: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        let ops2: Vec<CompiledGeometryOp> = vec![];
        let values2 = ValueMap::new();
        let functions2: Vec<CompiledFunction> = vec![];
        let meta_map2: HashMap<String, HashMap<String, String>> = HashMap::new();
        let id2 = RealizationNodeId::new("TestEntity", 0);
        let mut diags2: Vec<Diagnostic> = vec![];
        let mut kerr2: Option<ErrorRef> = None;
        let mut cache2 = RealizationCache::new();
        let mut dc2 = 0usize;
        let mut dcbr2: HashMap<RealizationNodeId, usize> = HashMap::new();

        let overridden = RealizationOpsInput::new(
            &mut kernels2,
            &registry2,
            "occt",
            &ops2,
            &values2,
            &functions2,
            &meta_map2,
            &mut diags2,
            &id2,
            SourceSpan::new(0, 0),
            &mut kerr2,
            &mut cache2,
            &mut dc2,
            &mut dcbr2,
        )
        .with_demanded_repr(ReprKind::Voxel)
        .with_demanded_tol(Some(0.01))
        .with_demanded_boundary(true)
        .with_prefer_kernel(Some("occt"))
        .with_is_terminal_realization(true)
        .with_long_chain_threshold(Duration::ZERO)
        .with_realization_name(Some("R"));

        assert_eq!(overridden.demanded_repr, ReprKind::Voxel);
        assert_eq!(overridden.demanded_tol, Some(0.01));
        assert!(overridden.demanded_boundary);
        assert_eq!(overridden.prefer_kernel, Some("occt"));
        assert!(overridden.is_terminal_realization);
        assert_eq!(overridden.long_chain_threshold, Duration::ZERO);
        assert_eq!(overridden.realization_name, Some("R"));
    }

    // ── execute_realization_ops unit tests ────────────────────────────────────

    /// Happy path: all operations compile and execute successfully.
    /// Appends exactly one handle and emits no diagnostics.
    #[test]
    fn execute_realization_ops_happy_path_appends_handle() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        assert_eq!(state.step_handles.len(), 1, "expected one handle appended");
        // Filter to error-severity only: the v0.2 topology-attribute seeder
        // (#2574) emits a Diagnostic::warning when extract_faces / extract_edges
        // fail (e.g. on a mock kernel without an extraction fixture). The
        // happy-path contract is "no Error diagnostics"; auxiliary-metadata
        // warnings are expected noise on mock kernels.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
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
        let warnings: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Warning))
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

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        // Pre-seed with a sentinel so we can assert truncation went back to exactly
        // this pre-call length, distinguishing "INVALID pushed then truncated" from
        // "INVALID never pushed at all".
        let pre_existing = KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(0xCAFE),
        };
        let mut state = DispatchTestState::default();
        state.step_handles.push(pre_existing);
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        assert_eq!(
            state.step_handles.len(),
            1,
            "step_handles should be truncated back to pre-call length of 1; \
             the INVALID sentinel must not remain"
        );
        assert_eq!(
            state.step_handles[0], pre_existing,
            "the pre-existing handle must be preserved unchanged"
        );
        let compile_failures = state
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("failed to compile geometry operation"))
            .count();
        assert_eq!(
            compile_failures, 1,
            "expected exactly 1 compile-error diagnostic, got {}: {:?}",
            compile_failures, state.diagnostics
        );
    }

    /// Kernel error: ops compile successfully but `kernel.execute()` returns `Err`.
    /// Truncates `step_handles` to `handle_start` and emits exactly 1 geometry-error
    /// diagnostic.
    #[test]
    fn execute_realization_ops_kernel_error_truncates_handles() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::FailingMockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernels = dispatch_test_kernels(Box::new(FailingMockGeometryKernel));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        assert!(
            state.step_handles.is_empty(),
            "handles should be truncated back to handle_start (0)"
        );
        let geometry_errors = state
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("geometry error"))
            .count();
        assert_eq!(
            geometry_errors, 1,
            "expected exactly 1 geometry-error diagnostic, got {}: {:?}",
            geometry_errors, state.diagnostics
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
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::MockGeometryKernel;

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

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        // Pre-seed step_handles with a sentinel to verify truncation goes back
        // to exactly this pre-call length, not to zero.
        let pre_existing = KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(0xBEEF),
        };
        let mut state = DispatchTestState::default();
        state.step_handles.push(pre_existing);
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        // The real handle produced by op 0 must have been discarded.
        // Only the pre-existing handle should remain.
        assert_eq!(
            state.step_handles.len(),
            1,
            "step_handles should be truncated back to the pre-call length of 1; \
             the real handle from op 0 must be gone"
        );
        assert_eq!(
            state.step_handles[0], pre_existing,
            "the pre-existing handle must be preserved unchanged"
        );
        // Exactly one compile-error diagnostic from the failing op 1
        let compile_failures = state
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("failed to compile geometry operation"))
            .count();
        assert_eq!(
            compile_failures, 1,
            "expected exactly 1 compile-error diagnostic, got {}: {:?}",
            compile_failures, state.diagnostics
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

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        // The Error diagnostic must contain the standard prefix (preserves
        // existing integration-test substring checks) AND the specific reason.
        let compile_err_diag = state
            .diagnostics
            .iter()
            .find(|d| {
                d.message.contains("failed to compile geometry operation")
                    && matches!(d.severity, reify_core::Severity::Error)
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
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            Some("body"),
            SourceSpan::new(0, 0),
            None,
        );

        // Filter to error-severity only: see comment in the happy-path test.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "expected no error diagnostics, got: {:?}",
            errors
        );
        // Pin the expected warning count (one seeder extract-failure per
        // successful primitive op). See the happy-path test for the rationale.
        let warnings: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Warning))
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
        assert_eq!(state.step_handles.len(), 1, "expected one handle appended");
        let body_handle = state.named_steps.get("body").copied();
        assert!(
            body_handle.is_some(),
            "named_steps should contain 'body' after successful named realization"
        );
        assert_eq!(
            body_handle.unwrap(),
            state.step_handles[0],
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

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            Some("bad"),
            SourceSpan::new(0, 0),
            None,
        );

        assert!(
            !state.named_steps.contains_key("bad"),
            "named_steps must NOT contain 'bad' after rollback; stale entries \
             would let later realizations resolve a name whose geometry was never \
             successfully produced"
        );
        // Verify rollback did happen (existing invariant)
        assert!(
            state.step_handles.is_empty(),
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
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::MockGeometryKernel;

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

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();

        // First binding: let body = box(…)
        state.run(
            &mut kernels,
            &registry,
            "default",
            &box_ops,
            Some("body"),
            SourceSpan::new(0, 0),
            None,
        );
        // Snapshot via the contract-visible map entry, not by positional index,
        // so the snapshot stays correct if internal handle-slot layout changes.
        let h1 = state.named_steps["body"];

        // Second binding: let body = cylinder(…) — same name, different primitive.
        // Reset the attribute tables between calls to mirror the per-build
        // reset in production (each realization sees clean attribute state).
        state.reset_attribute_tables();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &cyl_ops,
            Some("body"),
            SourceSpan::new(0, 0),
            None,
        );
        let h2 = state.named_steps["body"];

        // The kernel must have issued distinct handles so the test is non-trivial
        assert_ne!(
            h1, h2,
            "MockGeometryKernel must return distinct handles for distinct ops"
        );

        // Last-write-wins: named_steps["body"] must equal h2 (the cylinder binding)
        assert_eq!(
            state.named_steps.get("body").copied(),
            Some(h2),
            "shadowing contract: the second `let body` binding must overwrite \
             the first — named_steps[\"body\"] must be the handle from the \
             most-recent successful realization"
        );

        // Explicit anti-assertion: a first-write-wins regression must fail here
        assert_ne!(
            state.named_steps.get("body").copied(),
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
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "no errors expected for two valid realizations, got: {:?}",
            errors
        );
        // Pin the expected warning count: this test runs two successful
        // primitive ops (Box, then Cylinder) through the same `diagnostics`
        // Vec, so one seeder warning per op accumulates → 2 total.
        let warnings: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Warning))
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
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::MockGeometryKernel;

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

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();

        // First binding: let body = box(…) — succeeds, populates named_steps.
        state.run(
            &mut kernels,
            &registry,
            "default",
            &box_ops,
            Some("body"),
            SourceSpan::new(0, 0),
            None,
        );
        let h1 = state.named_steps["body"];
        // Filter to error-severity only: see comment in the happy-path test.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "first realization must succeed cleanly, got: {:?}",
            errors
        );
        // Pin the expected warning count (one seeder failure for the
        // successful Box op). See the happy-path test for the rationale.
        let warnings_after_first: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Warning))
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
        // Reset attribute tables between realizations.
        state.reset_attribute_tables();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &fail_ops,
            Some("body"),
            SourceSpan::new(0, 0),
            None,
        );

        // The failed shadow must NOT have overwritten the successful binding.
        assert_eq!(
            state.named_steps.get("body").copied(),
            Some(h1),
            "rollback guard: a failed shadow must not overwrite the previous \
             successful binding — named_steps[\"body\"] must still resolve to h1"
        );

        // The second call must have emitted a diagnostic (compile failure).
        assert!(
            !state.diagnostics.is_empty(),
            "expected a diagnostic from the failed second realization"
        );
        // Pin the warning count after the second call: the second op fails
        // before reaching `kernel.execute`, so the seeder is never invoked
        // and no NEW warning lands on top of the one from the first call.
        let warnings_after_second: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Warning))
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
        use reify_core::{Severity, SourceSpan};
        use reify_test_support::mocks::MockGeometryKernel;

        // Step(99) is out-of-bounds when step_handles is empty →
        // compile_geometry_op returns Err("unresolvable GeomRef::Step(99) …")
        let ops = vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99),
            right: GeomRef::Step(99),
        }];

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let realization_span = SourceSpan::new(100, 150);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            realization_span,
            None,
        );

        // Find the compile-failure Error diagnostic.
        let compile_err_diag = state
            .diagnostics
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
        use reify_core::{Severity, SourceSpan, Type};
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::FailingMockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernels = dispatch_test_kernels(Box::new(FailingMockGeometryKernel));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let realization_span = SourceSpan::new(200, 250);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            realization_span,
            None,
        );

        // Find the kernel-error Error diagnostic.
        let kernel_err_diag = state
            .diagnostics
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

    // ── per-op dispatch routing tests (step-7 #3436) ──────────────────────────
    //
    // These tests drive the multi-handle reshape of `execute_realization_ops`
    // landing in step-8: instead of a single `&mut dyn GeometryKernel`, the
    // helper takes a `&mut BTreeMap<String, Box<dyn GeometryKernel>>` keyed on
    // kernel name, a borrowed `&BTreeMap<String, &CapabilityDescriptor>`
    // dispatch registry, and a `&str` default-kernel name. For each op the
    // helper calls `dispatcher::dispatch(registry, op, BRep, {BRep})`, routes
    // the op to `kernels[plan.kernel]` (falling back to the default name when
    // the plan's kernel is absent from the map), or emits a `NoKernelChain`
    // diagnostic + sets `kernel_error_out` when dispatch returns `None`.

    /// Recording kernel: delegates the full `GeometryKernel` surface to a
    /// `MockGeometryKernel` and additionally pushes its own `name` onto a
    /// shared `Arc<Mutex<Vec<String>>>` on every `execute` /
    /// `execute_with_history` call. Lets the routing tests assert *which*
    /// kernel in the map received the op call — proof that per-op dispatch
    /// indexed into the named entry rather than the default.
    struct NamedRecordingKernel {
        name: String,
        inner: reify_test_support::mocks::MockGeometryKernel,
        log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl reify_ir::GeometryKernel for NamedRecordingKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.log.lock().unwrap().push(self.name.clone());
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.inner.tessellate(handle, tolerance)
        }
    }

    /// Two BRep kernels — `"aaa"` (lex-min) and `"default"` — both supporting
    /// `(PrimitiveBox, BRep)`. `dispatch(registry, PrimitiveBox, BRep, {BRep})`
    /// must pick `"aaa"` by lex-min tie-break (BTreeMap iteration order). The
    /// recording kernel under `"aaa"` captures the `execute` call, proving the
    /// op was routed to the dispatcher-named kernel — NOT the default.
    ///
    /// RED before step-8: `execute_realization_ops` still has the
    /// single-kernel `&mut dyn GeometryKernel` first parameter, so this test
    /// fails to compile until step-8 reshapes the signature to take
    /// `&mut BTreeMap<String, Box<dyn GeometryKernel>>` +
    /// `&BTreeMap<String, &CapabilityDescriptor>` + `&str` default name.
    #[test]
    fn execute_realization_ops_routes_to_dispatcher_picked_kernel() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "aaa".to_string(),
            Box::new(NamedRecordingKernel {
                name: "aaa".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );
        kernels.insert(
            "default".to_string(),
            Box::new(NamedRecordingKernel {
                name: "default".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );

        let desc_a = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let desc_d = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("aaa".to_string(), &desc_a);
        registry.insert("default".to_string(), &desc_d);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["aaa".to_string()],
            "the op must be routed to the dispatcher-picked kernel (lex-min = \"aaa\"), \
             not the default — got call log {:?}",
            calls
        );
        assert_eq!(
            state.step_handles.len(),
            1,
            "expected one handle pushed from the dispatched kernel"
        );
    }

    /// Behavior-preserved: with only the default kernel in the map (and a
    /// registry naming it for the op), `execute_realization_ops` must run the
    /// op on the default kernel — exactly the v0.2 single-kernel path.
    ///
    /// RED before step-8: same signature change as
    /// `execute_realization_ops_routes_to_dispatcher_picked_kernel` above.
    #[test]
    fn execute_realization_ops_routes_to_default_when_only_default_registered() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "default".to_string(),
            Box::new(NamedRecordingKernel {
                name: "default".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );

        let desc_d = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("default".to_string(), &desc_d);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["default".to_string()],
            "single-kernel-in-map: op must run on the default kernel; got log {:?}",
            calls,
        );
        assert_eq!(state.step_handles.len(), 1, "expected one handle pushed");
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "behavior-preserved single-default path must not emit error diagnostics; got {:?}",
            errors,
        );
    }

    /// Task 5001 (γ): pins [`reify_test_support::mocks::MockGeometryKernel`]'s
    /// new `realize_mesh_from_voxel` / `surface_options_content_hash`
    /// overrides — the mock counterpart of the real `OpenVdbKernel` overrides
    /// added in step-4, giving the mock a well-behaved options-carrying
    /// Voxel→Mesh source for any future conversion-executor test that needs
    /// one.
    ///
    /// Driven directly against the mock (not through
    /// `execute_realization_ops`): a same-call `available_for_op == {Voxel}`
    /// with `demanded_repr == Mesh` is architecturally unreachable there —
    /// `demanded_repr` is a single parameter shared by every op in one call
    /// (engine_build.rs `execute_realization_ops` loop), and `dispatch`'s BFS
    /// (dispatcher.rs) only returns `Some(plan)` when a popped state's repr
    /// exactly equals that one `demanded` value — so a preceding op can only
    /// ever produce `demanded_repr` itself or `ReprKind::BRep` (the `.or_else`
    /// fallback target), never a third repr for a later op to consume. See
    /// `.task/plan.json` step-5 for the full proof; the primary
    /// user-observable signal for the seam is
    /// `openvdb_realize_mesh_from_voxel_and_options_hash_thread_correctly`
    /// (dispatcher_integration.rs), which hand-seeds `{Voxel}` via a direct
    /// `dispatcher::dispatch` call and so does not share this constraint.
    #[test]
    fn mock_geometry_kernel_realize_mesh_from_voxel_and_options_hash() {
        use reify_test_support::mocks::MockGeometryKernel;

        let k = MockGeometryKernel::new();

        let mesh = GeometryKernel::realize_mesh_from_voxel(&k, GeometryHandleId(1), 0.0, false)
            .expect("MockGeometryKernel::realize_mesh_from_voxel must return Ok");
        assert!(
            !mesh.vertices.is_empty(),
            "MockGeometryKernel::realize_mesh_from_voxel must return a non-empty mesh"
        );

        let hash_a = GeometryKernel::surface_options_content_hash(&k, 0.0, false);
        let hash_b = GeometryKernel::surface_options_content_hash(&k, 0.5, false);
        assert_ne!(
            hash_a, hash_b,
            "two distinct iso_level values must hash to distinct \
             surface_options_content_hash keys"
        );
        assert_ne!(
            hash_a,
            crate::realization_cache::NO_OPTIONS,
            "surface_options_content_hash must never alias the NO_OPTIONS sentinel"
        );
        assert_ne!(
            hash_b,
            crate::realization_cache::NO_OPTIONS,
            "surface_options_content_hash must never alias the NO_OPTIONS sentinel"
        );
    }

    // ── cross-kernel conversion executor tests (task 4050 step-7) ─────────────
    //
    // These drive the multi-stage conversion executor + the Mesh→BRep dispatch
    // fallback landing in step-8. RED before step-8: `run_demand` calls
    // `Engine::execute_realization_ops` with the not-yet-existing `demanded_repr`
    // parameter, so the whole `mod tests` build fails to compile until step-8
    // grows that parameter, wires the `dispatch(.., demanded_repr, ..).or_else(
    // BRep)` fallback, and replaces the `Some(_) =>` deferred-error arm with the
    // tessellate→ingest cross-kernel handoff.

    /// occt-like counting kernel: `execute` / `query` / `export` delegate to an
    /// inner [`MockGeometryKernel`] (so `PrimitiveBox` → BRep solid handles),
    /// and `tessellate` bumps a shared counter before returning a trivial
    /// single-triangle [`Mesh`] — the BRep→Mesh source projection the conversion
    /// executor drives for each prior-stage input handle.
    struct CountingTessellateKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        tessellate_count: std::sync::Arc<std::sync::Mutex<usize>>,
    }

    impl reify_ir::GeometryKernel for CountingTessellateKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            *self.tessellate_count.lock().unwrap() += 1;
            Ok(reify_ir::Mesh {
                vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                indices: vec![0, 1, 2],
                normals: None,
            })
        }
    }

    /// manifold-like counting kernel: `ingest_mesh` bumps a shared counter and
    /// returns a fresh handle (the BRep→Mesh target projection), and `execute`
    /// bumps a shared counter (the final cross-kernel `BooleanUnion` op runs
    /// here). `query` / `export` / `tessellate` delegate to an inner
    /// [`MockGeometryKernel`]; only the union is ever routed here in the
    /// fixtures, so the `execute` counter is the `BooleanUnion`-on-Manifold
    /// count.
    struct CountingManifoldKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        ingest_count: std::sync::Arc<std::sync::Mutex<usize>>,
        execute_count: std::sync::Arc<std::sync::Mutex<usize>>,
        next_ingest_id: u64,
    }

    impl reify_ir::GeometryKernel for CountingManifoldKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            *self.execute_count.lock().unwrap() += 1;
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.inner.tessellate(handle, tolerance)
        }

        fn ingest_mesh(
            &mut self,
            _mesh: &reify_ir::Mesh,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            *self.ingest_count.lock().unwrap() += 1;
            let id = reify_ir::GeometryHandleId(self.next_ingest_id);
            self.next_ingest_id += 1;
            Ok(reify_ir::GeometryHandle { id, repr: None })
        }
    }

    /// openvdb-like counting kernel: `ingest_mesh` bumps a shared counter and
    /// returns a fresh handle (the Mesh→Voxel target projection via voxelising ingest),
    /// `execute` bumps a shared counter (the final cross-kernel `BooleanUnion` op runs
    /// here), and `tessellate` returns `Err(TessError::TessellationFailed)` mirroring the
    /// real OpenVDB stub — the real kernel cannot tessellate Voxel handles back to Mesh.
    /// `query` / `export` delegate to an inner [`MockGeometryKernel`].
    struct CountingVoxelizerKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        ingest_count: std::sync::Arc<std::sync::Mutex<usize>>,
        execute_count: std::sync::Arc<std::sync::Mutex<usize>>,
        next_ingest_id: u64,
    }

    impl reify_ir::GeometryKernel for CountingVoxelizerKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            *self.execute_count.lock().unwrap() += 1;
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            // Mirrors the real OpenVDB stub: Voxel handles cannot be tessellated
            // back to Mesh via this kernel — the executor must NOT call this.
            Err(reify_ir::TessError::TessellationFailed(
                "openvdb stub: tessellate not supported".into(),
            ))
        }

        fn ingest_mesh(
            &mut self,
            _mesh: &reify_ir::Mesh,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            *self.ingest_count.lock().unwrap() += 1;
            let id = reify_ir::GeometryHandleId(self.next_ingest_id);
            self.next_ingest_id += 1;
            Ok(reify_ir::GeometryHandle { id, repr: None })
        }
    }

    /// step-7(A) CONVERSION PATH (RED). With `demanded_repr = Mesh`, the
    /// dispatcher routes the terminal `BooleanUnion` to the Mesh-capable
    /// `"manifold"` kernel, preceded by a single BRep→Mesh conversion stage
    /// carried by `"occt"`. The executor must, for each of the union's two BRep
    /// input handles, `occt.tessellate` → Mesh then `manifold.ingest_mesh` →
    /// handle, substitute the converted handles, and run the union on
    /// `"manifold"`. Asserts the per-kernel call counts (2 / 2 / 1), the
    /// terminal `KernelId::Manifold` handle, and `produced_repr == Mesh`.
    #[test]
    fn execute_realization_ops_conversion_path_tessellates_and_ingests_cross_kernel() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryKernel, KernelId, Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Shared call counters, read back after the call via the Arc clones.
        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let union_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );
        kernels.insert(
            "manifold".to_string(),
            Box::new(CountingManifoldKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: std::sync::Arc::clone(&ingest_count),
                execute_count: std::sync::Arc::clone(&union_count),
                next_ingest_id: 1000,
            }),
        );

        // occt: (PrimitiveBox, BRep) + (Convert{BRep}, Mesh); manifold:
        // (BooleanUnion, Mesh). For demanded = Mesh / available = {BRep} the
        // dispatcher yields plan { kernel: "manifold", conversions:
        // [(Occt, BRep, Mesh)] } for the union.
        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("manifold".to_string(), &desc_manifold);

        // Two BRep primitives + one BooleanUnion consuming them.
        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("Cross", 0);
        let mut state = DispatchTestState::default();
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            None,
            None,
        );

        // The cross-kernel handoff must succeed: no error diagnostics, no
        // kernel_error_out.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "cross-kernel conversion must not emit error diagnostics, got: {:?}",
            errors
        );
        assert!(
            state.kernel_error_out.is_none(),
            "cross-kernel conversion must leave kernel_error_out None, got {:?}",
            state.kernel_error_out
        );

        // (a) occt.tessellate fires once per BooleanUnion input handle = 2.
        assert_eq!(
            *tess_count.lock().unwrap(),
            2,
            "occt.tessellate must be called once per union input handle (2)"
        );
        // (b) manifold.ingest_mesh fires once per converted input = 2.
        assert_eq!(
            *ingest_count.lock().unwrap(),
            2,
            "manifold.ingest_mesh must be called once per converted input (2)"
        );
        // (c) manifold runs the final BooleanUnion exactly once.
        assert_eq!(
            *union_count.lock().unwrap(),
            1,
            "manifold must run the final BooleanUnion exactly once"
        );

        // The terminal pushed handle is a Manifold handle (plan.kernel).
        let terminal = state
            .step_handles
            .last()
            .expect("a terminal handle must be pushed on success");
        assert_eq!(
            terminal.kernel,
            KernelId::Manifold,
            "terminal handle must be tagged KernelId::Manifold, got {:?}",
            terminal.kernel
        );

        // produced_repr surfaced as Mesh (plan_output_repr of the union on
        // manifold).
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::Mesh),
            "produced_repr_out must be Mesh for the cross-kernel realization"
        );
    }

    /// step-3 TWO-STAGE BRep→Voxel EXECUTOR (RED). With `demanded_repr = Voxel`
    /// and kernels `"occt"` (CountingTessellateKernel) + `"openvdb"`
    /// (CountingVoxelizerKernel), the two-stage chain
    /// `[(occt,BRep,Mesh),(openvdb,Mesh,Voxel)]` must run EXACTLY ONCE per
    /// op-input parent: `occt.tessellate` × 2 → Mesh, `openvdb.ingest_mesh`
    /// × 2 → Voxel handle; then the union runs on `"openvdb"` once.
    ///
    /// RED: the current per-stage executor re-processes stage-2
    /// `(openvdb,Mesh,Voxel)` by calling `openvdb.tessellate(brep_pid)` →
    /// `TessError::TessellationFailed` → conversion-error diagnostic, so the
    /// no-error / ingest==2 / terminal assertions fail.
    #[test]
    fn execute_realization_ops_conversion_path_two_stage_brep_to_voxel() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryKernel, KernelId, Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Shared call counters, read back after the call via the Arc clones.
        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let union_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );
        kernels.insert(
            "openvdb".to_string(),
            Box::new(CountingVoxelizerKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: std::sync::Arc::clone(&ingest_count),
                execute_count: std::sync::Arc::clone(&union_count),
                next_ingest_id: 2000,
            }),
        );

        // occt: (PrimitiveBox, BRep) + (Convert{BRep}, Mesh);
        // openvdb: (BooleanUnion, Voxel) + (Convert{Mesh}, Voxel).
        // For demanded = Voxel / available = {BRep} the dispatcher yields plan:
        // { kernel: "openvdb", conversions: [(Occt,BRep,Mesh),(OpenVdb,Mesh,Voxel)] }.
        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_openvdb = CapabilityDescriptor {
            supports: vec![
                (Operation::BooleanUnion, ReprKind::Voxel),
                (
                    Operation::Convert {
                        from: ReprKind::Mesh,
                    },
                    ReprKind::Voxel,
                ),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("openvdb".to_string(), &desc_openvdb);

        // Two BRep primitives + one BooleanUnion consuming them.
        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("MyDesign", 0);
        let mut state = DispatchTestState::default();
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Voxel,
            None,
            None,
        );

        // The two-stage conversion must succeed: no error diagnostics, no
        // kernel_error_out.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "two-stage BRep→Voxel conversion must not emit error diagnostics, got: {:?}",
            errors
        );
        assert!(
            state.kernel_error_out.is_none(),
            "two-stage BRep→Voxel conversion must leave kernel_error_out None, got {:?}",
            state.kernel_error_out
        );

        // (a) occt.tessellate fires once per BooleanUnion input handle = 2.
        assert_eq!(
            *tess_count.lock().unwrap(),
            2,
            "occt.tessellate must be called once per union input handle (2)"
        );
        // (b) openvdb.ingest_mesh fires once per converted input = 2.
        assert_eq!(
            *ingest_count.lock().unwrap(),
            2,
            "openvdb.ingest_mesh must be called once per converted input (2)"
        );
        // (c) openvdb runs the final BooleanUnion exactly once.
        assert_eq!(
            *union_count.lock().unwrap(),
            1,
            "openvdb must run the final BooleanUnion exactly once"
        );

        // The terminal pushed handle is an OpenVdb handle (plan.kernel).
        let terminal = state
            .step_handles
            .last()
            .expect("a terminal handle must be pushed on success");
        assert_eq!(
            terminal.kernel,
            KernelId::OpenVdb,
            "terminal handle must be tagged KernelId::OpenVdb, got {:?}",
            terminal.kernel
        );

        // produced_repr surfaced as Voxel (plan_output_repr of the union on openvdb).
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::Voxel),
            "produced_repr_out must be Voxel for the two-stage BRep→Voxel realization"
        );
    }

    /// Amendment (suggestion 4): NEGATIVE — unsupported conversion crossing
    /// degrades gracefully (no kernel work performed).
    ///
    /// Exercises the Phase-1 validation gate: when the dispatcher produces a
    /// chain containing a crossing that `v03_conversion_projection` classifies
    /// as `None` (e.g. a direct `BRep→Voxel` stage, which is not one of the
    /// two supported crossings), the executor must emit exactly one
    /// `Error`-severity diagnostic and must perform zero kernel work —
    /// `ingest_mesh` and `execute` (for the final op) must never be called.
    ///
    /// Scenario: "occt" registers `(Convert{from:BRep}, Voxel)` — a
    /// single-step BRep→Voxel crossing.  The dispatcher BFS finds a plan
    /// `{kernel:"openvdb", conversions:[(Occt,BRep,Voxel)]}`.  Phase 1 calls
    /// `v03_conversion_projection(BRep, Voxel)` → `None` → `conversion_error`.
    #[test]
    fn execute_realization_ops_conversion_path_unsupported_crossing_degrades_gracefully() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let union_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        // "occt": produces BRep primitives and claims a direct BRep→Voxel
        // Convert edge (unsupported crossing in v0.3-β).
        kernels.insert("occt".to_string(), Box::new(MockGeometryKernel::new()));
        // "openvdb": counts ingest_mesh + execute calls so the test can assert
        // they never fire on the error path.
        kernels.insert(
            "openvdb".to_string(),
            Box::new(CountingVoxelizerKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: std::sync::Arc::clone(&ingest_count),
                execute_count: std::sync::Arc::clone(&union_count),
                next_ingest_id: 4000,
            }),
        );

        // occt: (PrimitiveBox, BRep) + (Convert{BRep}, Voxel) — the direct
        // BRep→Voxel crossing is not one of the two β-supported shapes.
        // openvdb: (BooleanUnion, Voxel) only — no Convert capability.
        //
        // Dispatcher BFS for demanded=Voxel / available={BRep}:
        //   pop(BRep): expand via occt's (Convert{BRep},Voxel) → (Voxel,[occt,BRep,Voxel])
        //   pop(Voxel): openvdb supports (BooleanUnion,Voxel) → plan found.
        // Phase 1: v03_conversion_projection(BRep,Voxel) = None → error.
        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Voxel,
                ),
            ],
        };
        let desc_openvdb = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Voxel)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("openvdb".to_string(), &desc_openvdb);

        // Two BRep primitives + one BooleanUnion consuming them.
        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("BadConv", 0);
        let mut state = DispatchTestState::default();
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("BadConv"),
            SourceSpan::new(0, 0),
            ReprKind::Voxel,
            None,
            None,
        );

        // Must emit at least one Error diagnostic (the unsupported crossing).
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            !errors.is_empty(),
            "an unsupported BRep→Voxel crossing must emit an Error diagnostic, \
             got no errors (diagnostics: {:?})",
            state.diagnostics,
        );
        // Pin that the error originated from the Phase-1 classification gate
        // (v03_conversion_projection(BRep,Voxel) = None), not from a None
        // dispatch plan or some other unrelated code path.  The gate message
        // always contains "not executable in v0.3-β"; if the error comes from
        // elsewhere (e.g. dispatch returns None / NoKernelChain path) the test
        // would still pass the non-empty check above, but for the wrong reason.
        assert!(
            errors
                .iter()
                .any(|d| d.message.contains("not executable in v0.3-\u{03b2}")),
            "the Error diagnostic must originate from the Phase-1 classification \
             gate (message must contain 'not executable in v0.3-β'); \
             got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>(),
        );

        // No kernel work must have been performed after the Phase-1 error.
        assert_eq!(
            *ingest_count.lock().unwrap(),
            0,
            "ingest_mesh must not be called when the conversion stage is unsupported"
        );
        assert_eq!(
            *union_count.lock().unwrap(),
            0,
            "BooleanUnion must not run when the conversion stage is unsupported"
        );
    }

    /// GR-034 (task #3445): A 3-stage conversion chain (BRep→Mesh via occt,
    /// Mesh→Sdf via fidget, Sdf→Voxel via openvdb) emits exactly one
    /// `Severity::Warning` diagnostic with `code = LongChainRealization`
    /// naming each kernel stage. The chain rolls back at Phase-1 validation
    /// (the Mesh→Sdf crossing is unsupported in v0.3-β), but the diagnostic
    /// is emitted AFTER the per-op loop, independent of rollback, when
    /// `long_chain_threshold = Duration::ZERO` is threaded directly.
    ///
    /// RED: compile error — `long_chain_threshold` parameter does not yet
    /// exist on `execute_realization_ops`.
    #[test]
    fn execute_realization_ops_emits_single_long_chain_warning_naming_stages() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::{DiagnosticCode, Severity, Type};
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;
        use std::time::Duration;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert("occt".to_string(), Box::new(MockGeometryKernel::new()));
        kernels.insert("fidget".to_string(), Box::new(MockGeometryKernel::new()));
        kernels.insert("openvdb".to_string(), Box::new(MockGeometryKernel::new()));

        // 3-stage BFS chain: BRep→Mesh (occt) → Mesh→Sdf (fidget) → Sdf→Voxel
        // (openvdb). For demanded=Voxel / available={BRep} the dispatcher yields:
        // { kernel:"openvdb", conversions:
        //   [(Occt,BRep,Mesh),(Fidget,Mesh,Sdf),(OpenVdb,Sdf,Voxel)] }
        // — 3 conversions, which trips `is_long_chain_realization` when
        // `elapsed > Duration::ZERO` (threshold threaded as ZERO).
        //
        // NOTE (test coupling): `Duration::ZERO` is the threshold floor, chosen to
        // isolate the STAGE-COUNT gate (`conversions.len() > 2`). The elapsed half
        // of the gate (`elapsed > Duration::ZERO`) is satisfied by the nanosecond-
        // resolution monotonic `Instant` after executing 3 ops on any supported
        // Linux host. The elapsed gate is exercised independently by
        // `execute_realization_ops_high_threshold_suppresses_long_chain_warning`,
        // which passes `Duration::from_secs(3600)` to suppress a fast 3-stage chain.
        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_fidget = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert {
                    from: ReprKind::Mesh,
                },
                ReprKind::Sdf,
            )],
        };
        let desc_openvdb = CapabilityDescriptor {
            supports: vec![
                (
                    Operation::Convert {
                        from: ReprKind::Sdf,
                    },
                    ReprKind::Voxel,
                ),
                (Operation::BooleanUnion, ReprKind::Voxel),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("fidget".to_string(), &desc_fidget);
        registry.insert("openvdb".to_string(), &desc_openvdb);

        // Two BRep primitives + one BooleanUnion consuming them.
        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let mut state = DispatchTestState::default();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let realization_id = RealizationNodeId::new("LongChain", 0);
        Engine::execute_realization_ops(
            RealizationOpsInput::new(
                &mut kernels,
                &registry,
                "occt",
                &ops,
                &values,
                &functions,
                &meta_map,
                &mut state.diagnostics,
                &realization_id,
                SourceSpan::new(0, 0),
                &mut state.kernel_error_out,
                &mut state.realization_cache,
                &mut state.dispatch_count,
                &mut state.dispatch_count_by_realization,
            )
            .with_realization_name(Some("LongChain"))
            .with_demanded_repr(ReprKind::Voxel)
            .with_is_terminal_realization(true)
            .with_long_chain_threshold(Duration::ZERO), // GR-034 / #3445
            RealizationOutputs::new(
                &mut state.step_handles,
                &mut state.named_steps,
                &mut state.named_step_reprs,
                &mut state.topology_attribute_table,
                &mut state.swept_kind_table,
                &mut state.produced_repr_out,
            ),
        );

        // Exactly one LongChainRealization Warning must be emitted.
        let long_chain_diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::LongChainRealization))
            .collect();
        assert_eq!(
            long_chain_diags.len(),
            1,
            "exactly one LongChainRealization diagnostic expected; \
             got: {:?}",
            state.diagnostics,
        );
        let diag = long_chain_diags[0];
        assert_eq!(
            diag.severity,
            Severity::Warning,
            "LongChainRealization must be Severity::Warning",
        );
        // The diagnostic message must name each kernel stage in the 3-stage chain.
        for kernel_name in ["occt", "fidget", "openvdb"] {
            assert!(
                diag.message.contains(kernel_name),
                "LongChainRealization message must name '{kernel_name}'; \
                 got: {:?}",
                diag.message,
            );
        }
    }

    /// GR-034 (task #3445): A fully-executable 2-stage chain (BRep→Mesh via
    /// occt, then Mesh→Voxel via openvdb, demanded=Voxel) must emit ZERO
    /// `LongChainRealization` diagnostics: the gate `conversions.len() > 2`
    /// is false for a 2-stage plan, so `is_long_chain_realization` returns
    /// false and `long_chain_diagnostic` returns None even at threshold=ZERO.
    ///
    /// This guards against a naive (ungated) emission that would nag all
    /// 2-stage chains and confirms that a successful short-chain realization
    /// is not nagged.
    #[test]
    fn execute_realization_ops_two_stage_chain_emits_no_long_chain_warning() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::{DiagnosticCode, Type};
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        // "occt" uses MockGeometryKernel — it has a working tessellate() (returns a
        // minimal valid Mesh) so the BRep→Mesh conversion stage succeeds.
        kernels.insert("occt".to_string(), Box::new(MockGeometryKernel::new()));
        // "openvdb" needs to implement ingest_mesh() for the Mesh→Voxel stage.
        // MockGeometryKernel's default ingest_mesh() returns Err, so we use the
        // existing CountingVoxelizerKernel test helper which properly implements it.
        let ingest_count = Arc::new(Mutex::new(0usize));
        let execute_count = Arc::new(Mutex::new(0usize));
        kernels.insert(
            "openvdb".to_string(),
            Box::new(CountingVoxelizerKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: Arc::clone(&ingest_count),
                execute_count: Arc::clone(&execute_count),
                next_ingest_id: 2000,
            }),
        );

        // 2-stage chain: BRep→Mesh (occt) → Mesh→Voxel (openvdb).
        // For demanded=Voxel / available={BRep} the dispatcher yields:
        // { kernel:"openvdb", conversions:[(Occt,BRep,Mesh),(OpenVdb,Mesh,Voxel)] }
        // — exactly 2 conversions, so `conversions.len() > 2` is FALSE and
        // `long_chain_diagnostic` returns None even at threshold=ZERO.
        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_openvdb = CapabilityDescriptor {
            supports: vec![
                (
                    Operation::Convert {
                        from: ReprKind::Mesh,
                    },
                    ReprKind::Voxel,
                ),
                (Operation::BooleanUnion, ReprKind::Voxel),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("openvdb".to_string(), &desc_openvdb);

        // Two BRep primitives + one BooleanUnion consuming them.
        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let mut state = DispatchTestState::default();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let realization_id = RealizationNodeId::new("TwoStage", 0);
        Engine::execute_realization_ops(
            RealizationOpsInput::new(
                &mut kernels,
                &registry,
                "occt",
                &ops,
                &values,
                &functions,
                &meta_map,
                &mut state.diagnostics,
                &realization_id,
                SourceSpan::new(0, 0),
                &mut state.kernel_error_out,
                &mut state.realization_cache,
                &mut state.dispatch_count,
                &mut state.dispatch_count_by_realization,
            )
            .with_realization_name(Some("TwoStage"))
            .with_demanded_repr(ReprKind::Voxel)
            .with_is_terminal_realization(true)
            // long_chain_threshold=ZERO → only the stage gate matters.
            .with_long_chain_threshold(Duration::ZERO),
            RealizationOutputs::new(
                &mut state.step_handles,
                &mut state.named_steps,
                &mut state.named_step_reprs,
                &mut state.topology_attribute_table,
                &mut state.swept_kind_table,
                &mut state.produced_repr_out,
            ),
        );

        // ZERO LongChainRealization diagnostics — the 2-stage gate `> 2` is false.
        let long_chain_diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::LongChainRealization))
            .collect();
        assert_eq!(
            long_chain_diags.len(),
            0,
            "a 2-stage chain must NOT emit LongChainRealization (gate: conversions.len() > 2); \
             got: {:?}",
            long_chain_diags,
        );
        // The 2-stage chain (BRep→Mesh→Voxel) is fully executable; no errors expected.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "the 2-stage BRep→Mesh→Voxel chain must succeed with no errors; \
             got: {:?}",
            errors,
        );
    }

    /// GR-034 (task #3445): A 3-stage chain with a high `long_chain_threshold`
    /// (`Duration::from_secs(3600)`) emits ZERO `LongChainRealization`
    /// diagnostics, confirming that the elapsed gate (not just the stage-count
    /// gate) is honored end-to-end at the wiring level. The stage count (3)
    /// passes the `> 2` gate, but the real elapsed (sub-ms) is far below the
    /// 1-hour threshold, so `is_long_chain_realization` returns false and the
    /// warning is suppressed. Verifies that production callers passing
    /// `long_chain_threshold_from_env()` (default 5s) will NOT spuriously warn
    /// on a fast 3-stage chain.
    #[test]
    fn execute_realization_ops_high_threshold_suppresses_long_chain_warning() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::{DiagnosticCode, Type};
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;
        use std::time::Duration;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Same 3-stage registry as the first wiring test (occt→fidget→openvdb).
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert("occt".to_string(), Box::new(MockGeometryKernel::new()));
        kernels.insert("fidget".to_string(), Box::new(MockGeometryKernel::new()));
        kernels.insert("openvdb".to_string(), Box::new(MockGeometryKernel::new()));

        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_fidget = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert {
                    from: ReprKind::Mesh,
                },
                ReprKind::Sdf,
            )],
        };
        let desc_openvdb = CapabilityDescriptor {
            supports: vec![
                (
                    Operation::Convert {
                        from: ReprKind::Sdf,
                    },
                    ReprKind::Voxel,
                ),
                (Operation::BooleanUnion, ReprKind::Voxel),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("fidget".to_string(), &desc_fidget);
        registry.insert("openvdb".to_string(), &desc_openvdb);

        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let mut state = DispatchTestState::default();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let realization_id = RealizationNodeId::new("HighThreshold", 0);
        Engine::execute_realization_ops(
            RealizationOpsInput::new(
                &mut kernels,
                &registry,
                "occt",
                &ops,
                &values,
                &functions,
                &meta_map,
                &mut state.diagnostics,
                &realization_id,
                SourceSpan::new(0, 0),
                &mut state.kernel_error_out,
                &mut state.realization_cache,
                &mut state.dispatch_count,
                &mut state.dispatch_count_by_realization,
            )
            .with_realization_name(Some("HighThreshold"))
            .with_demanded_repr(ReprKind::Voxel)
            .with_is_terminal_realization(true)
            // long_chain_threshold: far above any real elapsed.
            .with_long_chain_threshold(Duration::from_secs(3600)),
            RealizationOutputs::new(
                &mut state.step_handles,
                &mut state.named_steps,
                &mut state.named_step_reprs,
                &mut state.topology_attribute_table,
                &mut state.swept_kind_table,
                &mut state.produced_repr_out,
            ),
        );

        // ZERO LongChainRealization diagnostics — the elapsed gate suppresses it
        // (real elapsed << 1h threshold), confirming the threshold parameter is
        // honored end-to-end and not short-circuited by the stage-count gate alone.
        let long_chain_diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::LongChainRealization))
            .collect();
        assert_eq!(
            long_chain_diags.len(),
            0,
            "a 3-stage chain with threshold=1h must NOT emit LongChainRealization \
             (elapsed is far below threshold); got: {:?}",
            long_chain_diags,
        );
    }

    /// step-7(B) FALLBACK CONTROL (RED) — pins design_decision 3. With
    /// `demanded_repr = Mesh` but a registry that has NO Mesh-capable kernel for
    /// the op (occt supports only `(PrimitiveBox, BRep)`), a lone PrimitiveBox
    /// realization must NOT error: the Mesh dispatch returns `None`, the
    /// executor falls back to a BRep dispatch, and the op runs on occt producing
    /// a BRep handle. Without the fallback this would hit the strict
    /// no-kernel-chain error arm and regress every Stl/Obj primitive export.
    #[test]
    fn execute_realization_ops_mesh_demand_falls_back_to_brep_when_no_mesh_kernel() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );

        // No Mesh-capable kernel for the op: a Mesh demand can't be satisfied and
        // must fall back to BRep rather than error.
        let desc_occt = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let realization_id = RealizationNodeId::new("Lone", 0);
        let mut state = DispatchTestState::default();
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Lone"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            None,
            None,
        );

        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "the Mesh→BRep fallback must not emit error diagnostics, got: {:?}",
            errors
        );
        assert!(
            state.kernel_error_out.is_none(),
            "the Mesh→BRep fallback must not set kernel_error_out, got {:?}",
            state.kernel_error_out
        );
        assert_eq!(
            state.step_handles.len(),
            1,
            "the fallback must produce exactly one BRep handle from occt"
        );
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::BRep),
            "the fallback realization's produced_repr must be BRep"
        );
        assert_eq!(
            *tess_count.lock().unwrap(),
            0,
            "the fallback path must not tessellate (no conversion stage runs)"
        );
    }

    /// step-9 (RED): a NAMED Mesh-demanding conversion realization must cache
    /// its terminal handle at `(entity, Mesh, tol)` so a second identical build
    /// hits the cache short-circuit — `dispatch_count == 0`, the cached Manifold
    /// terminal handle returned, `produced_repr == Mesh`, and the occt/manifold
    /// call counters UNCHANGED (the whole realization short-circuits).
    ///
    /// RED before step-10: `cache_repr` is pinned to `ReprKind::BRep`, so the
    /// post-loop INSERT keys the genuinely-Mesh terminal at the BRep slot and
    /// the cache-hit short-circuit (which also keys on the pinned BRep) reports
    /// `produced_repr == BRep` for the second build instead of `Mesh`.
    #[test]
    fn execute_realization_ops_mesh_realization_caches_and_hits_at_mesh_key() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryKernel, KernelId, Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let union_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );
        kernels.insert(
            "manifold".to_string(),
            Box::new(CountingManifoldKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: std::sync::Arc::clone(&ingest_count),
                execute_count: std::sync::Arc::clone(&union_count),
                next_ingest_id: 1000,
            }),
        );

        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("manifold".to_string(), &desc_manifold);

        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("Cross", 0);
        let tol = 0.001;
        let mut state = DispatchTestState::default();

        // ── First build: cold cache, full cross-kernel conversion. ──────────
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        assert!(
            state.dispatch_count > 0,
            "first (cold-cache) build must dispatch, got {}",
            state.dispatch_count
        );
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::Mesh),
            "first build produced_repr"
        );
        let terminal_1 = *state
            .step_handles
            .last()
            .expect("first build must push a terminal handle");
        assert_eq!(terminal_1.kernel, KernelId::Manifold);
        let tess_after_1 = *tess_count.lock().unwrap();
        let ingest_after_1 = *ingest_count.lock().unwrap();
        let union_after_1 = *union_count.lock().unwrap();
        assert_eq!((tess_after_1, ingest_after_1, union_after_1), (2, 2, 1));

        // ── Reset the per-build instrumentation the way production does. ─────
        state.dispatch_count = 0;
        state.produced_repr_out = None;
        state.reset_attribute_tables();

        // ── Second build: identical inputs, SAME cache → full short-circuit. ─
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        assert_eq!(
            state.dispatch_count, 0,
            "second build must hit the cache short-circuit (no dispatch), got {}",
            state.dispatch_count
        );
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::Mesh),
            "second build must report the Mesh terminal repr from the cache, not BRep"
        );
        let terminal_2 = *state
            .step_handles
            .last()
            .expect("second build must push the cached terminal handle");
        assert_eq!(
            terminal_2, terminal_1,
            "second build must return the cached Manifold terminal handle"
        );
        assert_eq!(
            *tess_count.lock().unwrap(),
            tess_after_1,
            "tessellate must be untouched on the cache hit"
        );
        assert_eq!(
            *ingest_count.lock().unwrap(),
            ingest_after_1,
            "ingest_mesh must be untouched on the cache hit"
        );
        assert_eq!(
            *union_count.lock().unwrap(),
            union_after_1,
            "the boolean union must be untouched on the cache hit"
        );
    }

    /// step-9 control: a NAMED BRep-demanding realization still caches + hits at
    /// `(entity, BRep, tol)` and reports `produced_repr == BRep`. This is the
    /// backward-compat guard — it passes both before and after the step-10
    /// `cache_repr` unpin, so it pins that the BRep path is unaffected.
    #[test]
    fn execute_realization_ops_brep_realization_caches_and_hits_at_brep_key() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );

        let desc_occt = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let realization_id = RealizationNodeId::new("Solid", 0);
        let tol = 0.001;
        let mut state = DispatchTestState::default();

        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Solid"),
            SourceSpan::new(0, 0),
            ReprKind::BRep,
            Some(tol),
            None,
        );
        assert!(state.dispatch_count > 0, "first build must dispatch");
        assert_eq!(state.produced_repr_out, Some(ReprKind::BRep));
        let terminal_1 = *state.step_handles.last().expect("a terminal handle");

        state.dispatch_count = 0;
        state.produced_repr_out = None;
        state.reset_attribute_tables();

        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Solid"),
            SourceSpan::new(0, 0),
            ReprKind::BRep,
            Some(tol),
            None,
        );
        assert_eq!(
            state.dispatch_count, 0,
            "second BRep build must hit the cache short-circuit"
        );
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::BRep),
            "second BRep build must report BRep from the cache"
        );
        assert_eq!(
            *state.step_handles.last().expect("a terminal handle"),
            terminal_1,
            "second BRep build must return the cached terminal handle"
        );
    }

    /// Amendment (reviewer_comprehensive #1, perf regression): a NAMED
    /// Mesh-demanding realization whose registry has NO Mesh-capable terminal
    /// kernel falls back to a BRep dispatch (design_decision 3), RESOLVES to
    /// BRep, and caches its terminal at `(entity, BRep, tol)`. A second
    /// identical Mesh-demanding build must STILL hit that cache — via the BRep
    /// fallback probe — so `dispatch_count == 0`, the cached BRep terminal
    /// handle is returned, `produced_repr == BRep`, and `tessellate` stays at 0.
    ///
    /// This pins the fix for the regression where the cache_repr unpin keyed the
    /// lookup at Mesh while the fell-back terminal was stored at BRep: without
    /// the BRep fallback probe the second build's Mesh lookup would miss the
    /// BRep entry and recompute the whole realization on every rebuild — the
    /// dominant occt-only Stl/Obj production export path.
    #[test]
    fn execute_realization_ops_mesh_demand_resolved_brep_hits_cache_via_brep_fallback() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );

        // No Mesh-capable kernel for the op: a Mesh demand resolves only via the
        // BRep fallback (design_decision 3) — the occt-only production config.
        let desc_occt = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let realization_id = RealizationNodeId::new("FellBack", 0);
        let tol = 0.001;
        let mut state = DispatchTestState::default();

        // ── First (cold) build: Mesh demand falls back to BRep, caches at BRep. ─
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("FellBack"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        assert!(state.dispatch_count > 0, "first build must dispatch");
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::BRep),
            "a Mesh demand with no Mesh kernel must resolve BRep (fallback)"
        );
        let terminal_1 = *state.step_handles.last().expect("a terminal handle");
        assert_eq!(
            *tess_count.lock().unwrap(),
            0,
            "the fallback path must not tessellate"
        );

        // ── Reset the per-build instrumentation the way production does. ────────
        state.dispatch_count = 0;
        state.produced_repr_out = None;
        state.reset_attribute_tables();

        // ── Second build: SAME Mesh demand → BRep fallback probe must HIT. ──────
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("FellBack"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        assert_eq!(
            state.dispatch_count, 0,
            "second Mesh build must hit the cache via the BRep fallback probe, got {}",
            state.dispatch_count
        );
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::BRep),
            "the fallback cache-hit must report the resolved BRep repr, not Mesh"
        );
        assert_eq!(
            *state.step_handles.last().expect("a terminal handle"),
            terminal_1,
            "the fallback cache-hit must return the cached BRep terminal handle"
        );
        assert_eq!(
            *tess_count.lock().unwrap(),
            0,
            "the cache hit must not tessellate"
        );
    }

    /// step-11 (RED): intermediate caching + cross-realization reuse. After one
    /// successful Mesh-demanding conversion realization (the step-7(A) fixture),
    /// each BRep→Mesh intermediate produced by the conversion executor must be
    /// present in the [`RealizationCache`] at `(intermediate_entity, Mesh,
    /// per_stage_tol, NO_OPTIONS)`, where `intermediate_entity` is the
    /// per-input cache-key entity (`"{entity}#conv-step{idx}"` — the input's
    /// local step index makes it distinct-per-input AND stable across identical
    /// rebuilds) and `per_stage_tol = per_stage_tolerance_for_plan(&plan, tol)`
    /// for the single BRep→Mesh stage (`tol × 0.8`).
    ///
    /// A SECOND realization with the same entity + ops + tol but ANONYMOUS (no
    /// name, so the whole-realization terminal cache short-circuit cannot fire —
    /// it is gated on `realization_name.is_some()`) must reach the conversion
    /// executor again and REUSE both cached intermediates: occt.tessellate and
    /// manifold.ingest_mesh stay at the first realization's counts (2 / 2).
    ///
    /// RED before step-12: the conversion executor neither inserts intermediates
    /// into the cache nor consults it before tessellating, so the presence
    /// lookups miss (first assertion fails) and the anonymous second realization
    /// re-tessellates + re-ingests (counts climb to 4 / 4).
    #[test]
    fn execute_realization_ops_conversion_intermediates_cache_and_reuse() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryHandleId, GeometryKernel, KernelId,
            Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let union_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );
        kernels.insert(
            "manifold".to_string(),
            Box::new(CountingManifoldKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: std::sync::Arc::clone(&ingest_count),
                execute_count: std::sync::Arc::clone(&union_count),
                next_ingest_id: 1000,
            }),
        );

        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("manifold".to_string(), &desc_manifold);

        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("Cross", 0);
        let tol = 0.001;
        let mut state = DispatchTestState::default();

        // ── Realization 1: named, cold cache, full cross-kernel conversion. ──
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "realization 1 errors: {:?}", errors);
        let tess_after_1 = *tess_count.lock().unwrap();
        let ingest_after_1 = *ingest_count.lock().unwrap();
        assert_eq!(
            (tess_after_1, ingest_after_1, *union_count.lock().unwrap()),
            (2, 2, 1),
            "realization 1 must tessellate 2 inputs, ingest 2, run 1 union"
        );

        // The per-stage tolerance the executor used for the single BRep→Mesh
        // stage (one conversion ⇒ `tol × 0.8`).
        let per_stage_tol = per_stage_tolerance_for_plan(
            &DispatchPlan {
                kernel: "manifold".to_string(),
                conversions: vec![(KernelId::Occt, ReprKind::BRep, ReprKind::Mesh)],
            },
            tol,
        );

        // Both intermediates are cached at `("Cross#conv-step{0,1}", Mesh,
        // per_stage_tol, NO_OPTIONS)` — 2 distinct keys (the conversion source
        // provenance is the input's local step index), each holding a genuinely-
        // Mesh Manifold handle (the `manifold.ingest_mesh` result: ids 1000 /
        // 1001 in tessellate order). The key format MUST match the executor's
        // `conversion_intermediate_entity_id` (step-12).
        let cached_0 = state.realization_cache.lookup(
            "Cross#conv-step0",
            ReprKind::Mesh,
            per_stage_tol,
            NO_OPTIONS,
        );
        assert!(
            cached_0.is_some(),
            "intermediate for input step 0 must be cached at (Cross#conv-step0, Mesh, per_stage_tol)"
        );
        let cached_0 = *cached_0.unwrap();
        assert_eq!(
            cached_0.kernel,
            KernelId::Manifold,
            "intermediate handle must be tagged Manifold (target kernel)"
        );
        assert_eq!(cached_0.id, GeometryHandleId(1000));

        let cached_1 = state.realization_cache.lookup(
            "Cross#conv-step1",
            ReprKind::Mesh,
            per_stage_tol,
            NO_OPTIONS,
        );
        assert!(
            cached_1.is_some(),
            "intermediate for input step 1 must be cached at (Cross#conv-step1, Mesh, per_stage_tol)"
        );
        let cached_1 = *cached_1.unwrap();
        assert_eq!(cached_1.kernel, KernelId::Manifold);
        assert_eq!(cached_1.id, GeometryHandleId(1001));

        // ── Realization 2: same entity + ops + tol, ANONYMOUS (no name) so the
        //    whole-realization terminal short-circuit does NOT fire — the
        //    conversion executor runs again and must REUSE both cached
        //    intermediates rather than re-tessellate/re-ingest. ──
        state.dispatch_count = 0;
        state.produced_repr_out = None;
        state.reset_attribute_tables();
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            None,
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "realization 2 errors: {:?}", errors);
        assert_eq!(
            *tess_count.lock().unwrap(),
            tess_after_1,
            "the anonymous re-realization must REUSE cached intermediates — no extra tessellate"
        );
        assert_eq!(
            *ingest_count.lock().unwrap(),
            ingest_after_1,
            "the anonymous re-realization must REUSE cached intermediates — no extra ingest_mesh"
        );
    }

    /// manifold-like mock whose `ingest_mesh` SUCCEEDS (counting + fresh ids, so
    /// the conversion executor produces and caches intermediates) but whose
    /// `execute` (the final `BooleanUnion`) FAILS — driving the realization into
    /// the rollback branch AFTER at least one intermediate was inserted. Used by
    /// step-13 to pin atomic intermediate-cache rollback.
    struct FailingUnionManifoldKernel {
        ingest_count: std::sync::Arc<std::sync::Mutex<usize>>,
        next_ingest_id: u64,
    }

    impl reify_ir::GeometryKernel for FailingUnionManifoldKernel {
        fn execute(
            &mut self,
            _op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            Err(reify_ir::GeometryError::OperationFailed(
                "simulated union failure".into(),
            ))
        }

        fn query(
            &self,
            _q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            Err(reify_ir::QueryError::QueryFailed(
                "should not reach: execute always fails".into(),
            ))
        }

        fn export(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _format: reify_ir::ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            Err(reify_ir::ExportError::FormatError(
                "should not reach: execute always fails".into(),
            ))
        }

        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            Err(reify_ir::TessError::TessellationFailed(
                "should not reach: execute always fails".into(),
            ))
        }

        fn ingest_mesh(
            &mut self,
            _mesh: &reify_ir::Mesh,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            *self.ingest_count.lock().unwrap() += 1;
            let id = reify_ir::GeometryHandleId(self.next_ingest_id);
            self.next_ingest_id += 1;
            Ok(reify_ir::GeometryHandle { id, repr: None })
        }
    }

    /// step-13 (RED): atomic intermediate-cache rollback. A Mesh-demanding
    /// conversion realization whose final `BooleanUnion` execute FAILS (after
    /// both BRep→Mesh intermediates were tessellated, ingested, and cached) must
    /// roll back ATOMICALLY: (i) `step_handles` truncated back to `handle_start`
    /// (no terminal handle leaked), and (ii) every intermediate cache entry the
    /// realization inserted is REMOVED, so a later lookup misses rather than
    /// returning a handle from a realization that never completed.
    ///
    /// RED before step-14: step-12 inserts the intermediates but the
    /// `rolled_back` branch does not yet remove them, so the post-failure
    /// lookups still HIT.
    #[test]
    fn execute_realization_ops_failed_conversion_rolls_back_intermediate_cache() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryHandleId, GeometryKernel, KernelId,
            Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );
        kernels.insert(
            "manifold".to_string(),
            Box::new(FailingUnionManifoldKernel {
                ingest_count: std::sync::Arc::clone(&ingest_count),
                next_ingest_id: 1000,
            }),
        );

        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("manifold".to_string(), &desc_manifold);

        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("Cross", 0);
        let tol = 0.001;
        let mut state = DispatchTestState::default();
        // Pre-seed a sentinel so we can assert truncation went back to exactly
        // the pre-call length (handle_start = 1), not merely "emptied".
        let sentinel = KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(0xCAFE),
        };
        state.step_handles.push(sentinel);

        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );

        // The realization must have FAILED at the union execute, AFTER both
        // intermediates were produced (proving the rollback is non-vacuous).
        assert!(
            state.kernel_error_out.is_some(),
            "the failing union must surface a kernel error"
        );
        assert_eq!(
            *tess_count.lock().unwrap(),
            2,
            "both inputs must have been tessellated before the union failed"
        );
        assert_eq!(
            *ingest_count.lock().unwrap(),
            2,
            "both intermediates must have been ingested (and cached) before the union failed"
        );

        // (i) step_handles truncated back to handle_start — only the sentinel
        //     survives; no occt primitive handles and no terminal handle leaked.
        assert_eq!(
            state.step_handles.len(),
            1,
            "step_handles must truncate back to the pre-call length of 1"
        );
        assert_eq!(
            state.step_handles[0], sentinel,
            "the pre-existing sentinel must be preserved unchanged"
        );

        // (ii) the intermediate cache entries inserted during the failed
        //      realization must be GONE.
        let per_stage_tol = per_stage_tolerance_for_plan(
            &DispatchPlan {
                kernel: "manifold".to_string(),
                conversions: vec![(KernelId::Occt, ReprKind::BRep, ReprKind::Mesh)],
            },
            tol,
        );
        assert!(
            state
                .realization_cache
                .lookup(
                    "Cross#conv-step0",
                    ReprKind::Mesh,
                    per_stage_tol,
                    NO_OPTIONS
                )
                .is_none(),
            "intermediate step-0 must be rolled out of the cache on realization failure"
        );
        assert!(
            state
                .realization_cache
                .lookup(
                    "Cross#conv-step1",
                    ReprKind::Mesh,
                    per_stage_tol,
                    NO_OPTIONS
                )
                .is_none(),
            "intermediate step-1 must be rolled out of the cache on realization failure"
        );
    }

    /// When the registry claims no kernel for the op (dispatch returns
    /// `None`), `execute_realization_ops` must emit a
    /// `DiagnosticCode::NoKernelChain` error diagnostic, set
    /// `kernel_error_out` so the caller can mark the realization Failed, and
    /// truncate `step_handles` back to its pre-call length.
    ///
    /// RED before step-8: routing + dispatch + NoKernelChain wiring all land
    /// in step-8.
    #[test]
    fn execute_realization_ops_emits_no_kernel_chain_diagnostic_when_dispatch_returns_none() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{DiagnosticCode, Type};
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "default".to_string(),
            Box::new(MockGeometryKernel::new()) as Box<dyn reify_ir::GeometryKernel>,
        );

        // Registry deliberately does NOT support PrimitiveBox/BRep: every
        // descriptor in the map only supports BooleanUnion/Mesh, so
        // `dispatch(registry, PrimitiveBox, BRep, {BRep})` returns `None`.
        let desc_d = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("default".to_string(), &desc_d);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        // A NoKernelChain error diagnostic must be emitted.
        let no_chain: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::NoKernelChain))
            .collect();
        assert_eq!(
            no_chain.len(),
            1,
            "expected exactly one NoKernelChain diagnostic when the registry has no \
             kernel for the op; got {} diagnostics total: {:?}",
            no_chain.len(),
            state.diagnostics
        );
        assert!(
            matches!(no_chain[0].severity, reify_core::Severity::Error),
            "NoKernelChain must be an Error-severity diagnostic; got {:?}",
            no_chain[0].severity,
        );

        // Realization must surface as Failed via the caller-write kernel_error_out
        // out-param (the same channel `mark_realization_failed` consumes for
        // kernel errors today).
        assert!(
            state.kernel_error_out.is_some(),
            "unroutable op must set kernel_error_out so the caller can mark the \
             realization NodeId as Failed; got None"
        );

        // step_handles must be truncated to its pre-call length: no real handle
        // was produced.
        assert!(
            state.step_handles.is_empty(),
            "unroutable op must leave step_handles truncated to handle_start; got {:?}",
            state.step_handles,
        );
    }

    /// Step-13 (task ε / 3436) RED: the backward-compat None-fallback arm of
    /// [`Engine::execute_realization_ops`] must capture a synthetic
    /// `ReprKind::BRep` into the `produced_repr_out` channel for the executor-
    /// write invariant (step-10) to remain TOTAL across BOTH construction
    /// paths — `Engine::new(_, Some(kernel))` (which wraps the caller-supplied
    /// kernel under the synthetic [`Engine::DEFAULT_KERNEL_NAME`] sentinel and
    /// leaves the inventory registry deliberately out of sync with the kernels
    /// map) AND `with_registered_kernels` (which loads one kernel per
    /// inventory registration so dispatch always finds coverage).
    ///
    /// Pins the production gap the reviewer identified on
    /// `tests/multi_handle_engine_dispatch.rs::executor_writes_produced_repr_brep_on_build_snapshot`:
    /// that integration test passes incidentally when the local build has
    /// `cfg(has_occt)` (OCCT in the registry → dispatch returns
    /// `Some(plan{kernel:"occt"})` → 0-conversion arm falls back to the
    /// DEFAULT_KERNEL_NAME-keyed mock → `last_plan` is `Some` → post-loop
    /// `plan_output_repr` reads OCCT's `(PrimitiveBox, BRep)` support → writes
    /// `BRep`), but FAILS in stub-mode builds where the registry is empty and
    /// the None-fallback arm leaves `last_plan = None`, so the post-loop guard
    /// `if let (Some(plan), Some(op)) = (last_plan.as_ref(), last_operation)`
    /// short-circuits and `produced_repr_out` is never written.
    ///
    /// **Pre-corruption idiom**: this unit test pre-seeds `produced_repr_out =
    /// Some(ReprKind::Mesh)` before calling `execute_realization_ops`, exactly
    /// like the integration test pre-corrupts the snapshot graph node to
    /// `ReprKind::Mesh` before calling `build_snapshot()`. `Mesh` is the
    /// baseline-impossible value in v0.3-ε (the BRep baseline produces only
    /// BRep handles), so any later read of `BRep` here can only come from a
    /// step-14 fallback-arm write of `Some(ReprKind::BRep)`. A naïve
    /// `produced_repr_out == Some(BRep)` assertion against the construction-
    /// time `None` default would pass with or without the step-14 fix.
    ///
    /// **Why this fixture isolates the gap from OCCT availability**: the
    /// registry constructed below has NO `(PrimitiveBox, BRep)` support
    /// regardless of build profile — it carries only `(BooleanUnion, Mesh)`,
    /// a coverage that cannot satisfy the BRep-baseline query triple. The
    /// `assert!(dispatch(...).is_none())` sanity check below pins this
    /// invariant directly so a future registry change that accidentally
    /// covers `(PrimitiveBox, BRep)` would surface here rather than masking
    /// the fallback-arm exercise.
    ///
    /// RED before step-14: `last_produced_repr` does not yet exist, so the
    /// post-loop write key still reads `last_plan` — which is `None` in the
    /// fallback arm — and assertion (iii) below fires.
    #[test]
    fn execute_realization_ops_writes_produced_repr_brep_in_none_fallback_backward_compat() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{DiagnosticCode, Type};
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // (a) Registry that does NOT cover `(PrimitiveBox, BRep)`. The lone
        //     descriptor's `supports` list is `[(BooleanUnion, Mesh)]` — a
        //     valid `CapabilityDescriptor` (non-empty `supports`) that cannot
        //     answer the BRep-baseline dispatcher query for a Box op.
        let desc_none_match = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert(Engine::DEFAULT_KERNEL_NAME.to_string(), &desc_none_match);

        // (i) Sanity check: dispatch returns `None` for `(PrimitiveBox, BRep,
        //     {BRep})` against this registry, confirming the test reaches the
        //     None arm of the per-op match in `execute_realization_ops`.
        let available_set: std::collections::HashSet<ReprKind> = {
            let mut s = std::collections::HashSet::new();
            s.insert(ReprKind::BRep);
            s
        };
        assert!(
            dispatch(
                &registry,
                Operation::PrimitiveBox,
                ReprKind::BRep,
                &available_set,
                None,
            )
            .is_none(),
            "test invariant: synthetic registry must yield dispatch() == None for \
             (PrimitiveBox, BRep, {{BRep}}) so the executor reaches the backward-compat \
             fallback arm. If this fires, the registry was accidentally given coverage \
             for (PrimitiveBox, BRep)"
        );

        // (b) Single recording mock kernel keyed under
        //     `Engine::DEFAULT_KERNEL_NAME` — the synthetic sentinel that
        //     `Engine::new(_, Some(kernel))` / `with_prelude` wrap the caller-
        //     supplied kernel under (engine_admin.rs:197).
        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            Engine::DEFAULT_KERNEL_NAME.to_string(),
            Box::new(NamedRecordingKernel {
                name: Engine::DEFAULT_KERNEL_NAME.to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        // (d) Pre-corrupt `produced_repr_out` to `Some(ReprKind::Mesh)` — a
        //     baseline-impossible value. Any later read of `BRep` can only
        //     come from the step-14 fallback-arm write of
        //     `Some(ReprKind::BRep)`; the construction-time `None` default
        //     would also let the assertion fail loudly if step-14 instead
        //     left the channel untouched. Mirrors the pre-corruption idiom in
        //     `tests/multi_handle_engine_dispatch.rs::executor_writes_produced_repr_brep_on_build_snapshot`.
        //
        //     Constructed via struct-update from `Default::default()` rather
        //     than a post-`default()` field reassignment to avoid the clippy
        //     `field_reassign_with_default` lint — the only field overridden
        //     from default is `produced_repr_out`, so the struct-init form
        //     stays readable.
        let mut state = DispatchTestState {
            produced_repr_out: Some(ReprKind::Mesh),
            ..DispatchTestState::default()
        };

        // (c) `default_kernel_name = Engine::DEFAULT_KERNEL_NAME` — the
        //     sentinel comparison `default_kernel_name ==
        //     Engine::DEFAULT_KERNEL_NAME` inside the None arm gates the
        //     fallback vs strict-mode behaviour (engine_build.rs:2379).
        state.run(
            &mut kernels,
            &registry,
            Engine::DEFAULT_KERNEL_NAME,
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        // (ii) The recording mock kernel must have captured the call, proving
        //      the fallback arm executed the op on the synthetic default
        //      (rather than emitting NoKernelChain and breaking out of the
        //      loop without executing).
        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![Engine::DEFAULT_KERNEL_NAME.to_string()],
            "fallback arm must execute the op on the kernel registered under \
             Engine::DEFAULT_KERNEL_NAME; got call log {:?}",
            calls
        );
        assert_eq!(
            state.step_handles.len(),
            1,
            "expected one handle pushed from the fallback-routed default kernel"
        );

        // No NoKernelChain diagnostic must be emitted: the sentinel-gated
        // fallback arm is the backward-compat success path, NOT the strict-
        // mode missing-coverage error path the `no_kernel_chain` test pins.
        let no_chain: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::NoKernelChain))
            .collect();
        assert!(
            no_chain.is_empty(),
            "backward-compat None-fallback arm (default_kernel_name == \
             Engine::DEFAULT_KERNEL_NAME && kernels.contains_key(default_kernel_name)) \
             must NOT emit a NoKernelChain diagnostic — that diagnostic belongs to the \
             strict-mode arm only; got {:?}",
            no_chain
        );
        // Realization must NOT be marked Failed: kernel_error_out stays None
        // on the fallback success path (no error to surface to the caller).
        assert!(
            state.kernel_error_out.is_none(),
            "backward-compat fallback success must leave kernel_error_out untouched; \
             got {:?}",
            state.kernel_error_out
        );

        // (iii) The produced_repr_out channel must now carry
        //       `Some(ReprKind::BRep)` — overwriting the pre-corrupted
        //       `Some(ReprKind::Mesh)`. RED before step-14: the post-loop
        //       write guard `if let (Some(plan), Some(op)) =
        //       (last_plan.as_ref(), last_operation)` short-circuits because
        //       the fallback arm never sets `last_plan`, so the pre-corrupted
        //       Mesh value survives and this assertion fires. Step-14
        //       introduces a parallel `last_produced_repr` channel that the
        //       fallback arm sets to `Some(BRep)` (the v0.2 single-kernel
        //       invariant) and rewrites the post-loop write to consult it.
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::BRep),
            "executor must write produced_repr = BRep through the None-fallback \
             backward-compat arm so the executor-write invariant (step-10) remains \
             TOTAL across both construction paths; got {:?}. If this fires after \
             step-14 lands, check that `last_produced_repr` is set in the None arm \
             (default_kernel_name == Engine::DEFAULT_KERNEL_NAME && \
             kernels.contains_key(default_kernel_name)) and that the post-loop write \
             consults it.",
            state.produced_repr_out
        );
    }

    // ── pragma-steering seam tests (task #3443, step S3) ─────────────────────

    /// Pragma-steering at the execute_realization_ops seam: when
    /// `prefer_kernel=Some("occt")` is supplied, the op routes to "occt" even
    /// though lex-min would pick "manifold" (m < o). A sibling call with
    /// `prefer_kernel=None` confirms lex-min routing to "manifold".
    ///
    /// Registry: `{"manifold", "occt"}` both supporting `(BooleanUnion, BRep)`.
    /// Available = `{BRep}` (direct dispatch). Kernels are `NamedRecordingKernel`
    /// instances so the test can read back which kernel's `execute()` fired.
    ///
    /// RED until S4 adds `prefer_kernel: Option<&str>` to `DispatchTestState::run`
    /// and threads it through `execute_realization_ops`.
    #[test]
    fn execute_realization_ops_pragma_steers_to_preferred_kernel() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Both kernels support (PrimitiveBox, BRep) and (BooleanUnion, BRep) so
        // both primitives AND the union can route to either kernel.  Lex-min
        // picks "manifold" (m < o) for every op; prefer_kernel=Some("occt")
        // must override the terminal union.
        let desc = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (Operation::BooleanUnion, ReprKind::BRep),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("manifold".to_string(), &desc);
        registry.insert("occt".to_string(), &desc);

        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "manifold".to_string(),
            Box::new(NamedRecordingKernel {
                name: "manifold".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );
        kernels.insert(
            "occt".to_string(),
            Box::new(NamedRecordingKernel {
                name: "occt".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );

        // One PrimitiveBox followed by a BooleanUnion of step 0 with itself.
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(0),
            },
        ];

        // ── No pragma: lex-min "manifold" must be picked for every op. ──────
        let mut state_none = DispatchTestState::default();
        state_none.run(
            &mut kernels,
            &registry,
            "manifold",
            &ops,
            None,
            SourceSpan::new(0, 0),
            // RED: this 8th argument does not exist until S4 adds prefer_kernel
            // to DispatchTestState::run.
            None,
        );
        let calls_none = log.lock().unwrap().clone();
        assert!(
            calls_none
                .iter()
                .all(|k| k == "manifold"),
            "no pragma: every op must route to lex-min 'manifold'; got: {calls_none:?}",
        );

        // Reset log and re-use kernels for the pragma run.
        log.lock().unwrap().clear();

        // ── pragma "occt": union must be routed to "occt". ──────────────────
        let mut state_occt = DispatchTestState::default();
        state_occt.run(
            &mut kernels,
            &registry,
            "manifold",
            &ops,
            None,
            SourceSpan::new(0, 0),
            Some("occt"),
        );
        let calls_occt = log.lock().unwrap().clone();
        // Both kernels support ALL ops (PrimitiveBox + BooleanUnion), so
        // prefer_kernel=Some("occt") steers EVERY op — including the primitive —
        // to "occt". The comment "primitives can be on either" was inaccurate:
        // with this descriptor, pragma steering applies per-op unconditionally.
        assert!(
            calls_occt.iter().all(|k| k == "occt"),
            "prefer_kernel=Some(\"occt\"): every op must route to 'occt' \
             (pragma steers all ops when both kernels support all ops); \
             calls: {calls_occt:?}",
        );
    }

    // ── pragma-unsatisfiable diagnostic seam tests (task #3443, step S5) ───────

    /// `execute_realization_ops` must emit a `Severity::Warning` diagnostic with
    /// code `KernelPragmaUnsatisfiable` when `prefer_kernel` names a kernel that
    /// is absent from the registry (or present but not supporting the demanded
    /// `(op, demanded)` pair), and must STILL route the op via lex-min fallback
    /// (no `kernel_error_out`, one handle produced).
    ///
    /// Two scenarios:
    ///
    /// - **Unsatisfiable** (`prefer_kernel=Some("occt")`, "occt" absent): one
    ///   `KernelPragmaUnsatisfiable` warning; op routed to lex-min "manifold";
    ///   `kernel_error_out` is `None`; `step_handles.len() == 1`.
    /// - **Satisfiable** (`prefer_kernel=Some("manifold")`, "manifold" present
    ///   and supporting): zero `KernelPragmaUnsatisfiable` diagnostics.
    ///
    /// RED until S6 wires `kernel_pragma_unsatisfiable_diagnostic` into the
    /// per-op dispatch site in `execute_realization_ops`.
    #[test]
    fn execute_realization_ops_emits_kernel_pragma_unsatisfiable_and_falls_through() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::{DiagnosticCode, Severity, Type};
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Registry: only "manifold" supports (PrimitiveBox, BRep) and
        // (BooleanUnion, BRep). "occt" is deliberately absent — so
        // prefer_kernel=Some("occt") is unsatisfiable.
        let desc = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (Operation::BooleanUnion, ReprKind::BRep),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("manifold".to_string(), &desc);

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "manifold".to_string(),
            Box::new(MockGeometryKernel::new()) as Box<dyn reify_ir::GeometryKernel>,
        );

        // Two ops: one PrimitiveBox followed by a BooleanUnion.
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(0),
            },
        ];

        // ── Unsatisfiable pragma: "occt" is absent from the registry. ────────
        let mut state_unsat = DispatchTestState::default();
        state_unsat.run(
            &mut kernels,
            &registry,
            "manifold",
            &ops,
            None,
            SourceSpan::new(0, 0),
            Some("occt"),
        );

        // (i) Exactly one KernelPragmaUnsatisfiable Warning must be emitted.
        // RED: execute_realization_ops does not yet call
        // kernel_pragma_unsatisfiable_diagnostic (that wiring is S6's job).
        let unsat_diags: Vec<_> = state_unsat
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::KernelPragmaUnsatisfiable))
            .collect();
        assert_eq!(
            unsat_diags.len(),
            1,
            "unsatisfiable pragma must emit exactly ONE KernelPragmaUnsatisfiable \
             warning; got {} (all diagnostics: {:?})",
            unsat_diags.len(),
            state_unsat.diagnostics,
        );
        assert!(
            matches!(unsat_diags[0].severity, Severity::Warning),
            "KernelPragmaUnsatisfiable must be Warning-severity; got {:?}",
            unsat_diags[0].severity,
        );

        // (ii) Op STILL routes via lex-min "manifold" fall-through — no error.
        assert!(
            state_unsat.kernel_error_out.is_none(),
            "unsatisfiable pragma must fall through (lex-min routes the op); \
             kernel_error_out should remain None, got {:?}",
            state_unsat.kernel_error_out,
        );
        assert_eq!(
            state_unsat.step_handles.len(),
            ops.len(),
            "unsatisfiable pragma: all ops must produce handles via lex-min; \
             expected {}, got {:?}",
            ops.len(),
            state_unsat.step_handles,
        );

        // ── Satisfiable pragma: "manifold" is present and supports the ops. ──
        let mut state_sat = DispatchTestState::default();
        state_sat.run(
            &mut kernels,
            &registry,
            "manifold",
            &ops,
            None,
            SourceSpan::new(0, 0),
            Some("manifold"),
        );

        // NO KernelPragmaUnsatisfiable diagnostic when the pragma is satisfiable.
        let sat_unsat_diags: Vec<_> = state_sat
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::KernelPragmaUnsatisfiable))
            .collect();
        assert!(
            sat_unsat_diags.is_empty(),
            "satisfiable pragma must NOT emit KernelPragmaUnsatisfiable; \
             got {:?}",
            sat_unsat_diags,
        );
    }

    /// BRep-fallback path does NOT forward `prefer_kernel`: when
    /// `demanded_repr=Mesh` and `prefer_kernel=Some("occt")` where both
    /// kernels support `(op, BRep)` but NEITHER supports `(op, Mesh)`, the
    /// op must route via the BRep fallback to lex-min `"manifold"` (NOT
    /// `"occt"`), and exactly one `KernelPragmaUnsatisfiable` warning must be
    /// emitted.
    ///
    /// This pins the intentional design that the BRep-fallback dispatch (the
    /// `.or_else(|| dispatch(…, BRep, …, None))` path) does NOT forward
    /// `prefer_kernel`. Without this test a future refactor could silently
    /// pass `prefer_kernel` to the fallback, routing to `"occt"` at BRep even
    /// when the user's `#kernel(occt)` intent was for the primary demanded
    /// repr — exactly the behaviour the inline comment at the fallback site
    /// warns against.
    #[test]
    fn execute_realization_ops_brep_fallback_uses_lexmin_not_pragma_kernel() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{DiagnosticCode, Severity, Type};
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Registry: "manifold" and "occt" both support (PrimitiveBox, BRep)
        // but NEITHER supports (PrimitiveBox, Mesh). demanded_repr=Mesh means
        // the primary dispatch returns None (no Mesh path) and the BRep
        // fallback fires with prefer_kernel=None, so lex-min "manifold"
        // (m < o) wins over the pragma-preferred "occt".
        let brep_desc = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("manifold".to_string(), &brep_desc);
        registry.insert("occt".to_string(), &brep_desc);

        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "manifold".to_string(),
            Box::new(NamedRecordingKernel {
                name: "manifold".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );
        kernels.insert(
            "occt".to_string(),
            Box::new(NamedRecordingKernel {
                name: "occt".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let realization_id = RealizationNodeId::new("FallbackTest", 0);
        let mut state = DispatchTestState::default();
        state.run_demand(
            &mut kernels,
            &registry,
            "manifold",
            &ops,
            &realization_id,
            Some("FallbackTest"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            None,
            Some("occt"),
        );

        // (i) Exactly one KernelPragmaUnsatisfiable Warning: the dispatch
        // resolved "manifold" (BRep fallback lex-min) != "occt" (prefer_kernel).
        let unsat_diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::KernelPragmaUnsatisfiable))
            .collect();
        assert_eq!(
            unsat_diags.len(),
            1,
            "BRep-fallback with unsatisfied Mesh pragma must emit exactly ONE \
             KernelPragmaUnsatisfiable warning; got {} (all diagnostics: {:?})",
            unsat_diags.len(),
            state.diagnostics,
        );
        assert!(
            matches!(unsat_diags[0].severity, Severity::Warning),
            "KernelPragmaUnsatisfiable must be Warning-severity; got {:?}",
            unsat_diags[0].severity,
        );

        // (ii) Op must route via the BRep fallback to lex-min "manifold",
        // NOT to pragma-preferred "occt". The BRep-fallback dispatch passes
        // prefer_kernel=None so the pragma does not sneak onto the fallback
        // path and pick occt-at-BRep when the user's intent was occt-at-Mesh.
        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls.len(),
            1,
            "one PrimitiveBox op must produce exactly one execute() call; got: {calls:?}"
        );
        assert_eq!(
            calls[0].as_str(),
            "manifold",
            "BRep fallback must route to lex-min 'manifold', not pragma 'occt'; \
             call log: {calls:?}"
        );

        // (iii) The realization must still succeed — no error, one handle.
        assert!(
            state.kernel_error_out.is_none(),
            "BRep-fallback routing must succeed; kernel_error_out should remain None, \
             got {:?}",
            state.kernel_error_out
        );
        assert_eq!(
            state.step_handles.len(),
            1,
            "BRep-fallback routing must produce exactly one handle; got {:?}",
            state.step_handles
        );
    }

    // ── pragma mixed-satisfiability seam test (task #3443, amendment) ─────────

    /// Mixed-satisfiability: intermediate op unsatisfiable by pragma kernel,
    /// terminal op satisfiable.
    ///
    /// When `prefer_kernel=Some("occt")` and the registry has:
    /// - `"manifold"` supporting `(PrimitiveBox, BRep)` AND `(BooleanUnion, BRep)`
    /// - `"occt"` supporting `(BooleanUnion, BRep)` ONLY (NOT `PrimitiveBox`)
    ///
    /// a two-op realization `[PrimitiveBox, BooleanUnion]` must:
    ///
    /// - Route `PrimitiveBox` to lex-min `"manifold"` (pragma unsatisfiable),
    ///   emitting exactly ONE `KernelPragmaUnsatisfiable` warning whose message
    ///   references `PrimitiveBox` (the first unsatisfiable op).
    /// - Route `BooleanUnion` to `"occt"` (pragma satisfiable → preferred).
    /// - Produce 2 handles with `kernel_error_out == None` (realization succeeds).
    ///
    /// This pins the dedup semantics (`pragma_warn_emitted`): the warning fires on
    /// the FIRST unsatisfiable op and is suppressed for all subsequent ops,
    /// regardless of whether they are themselves satisfiable. A regression that
    /// changes which op the warning is attributed to, or skips it entirely, would
    /// be caught here.
    #[test]
    fn execute_realization_ops_pragma_mixed_satisfiability_warns_on_first_unsatisfiable_op() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::{DiagnosticCode, Severity, Type};
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // "manifold" supports both (PrimitiveBox, BRep) and (BooleanUnion, BRep).
        // "occt" supports ONLY (BooleanUnion, BRep) — NOT (PrimitiveBox, BRep).
        // With prefer_kernel=Some("occt"), the PrimitiveBox op is unsatisfiable
        // (occt cannot serve it) while the BooleanUnion op IS satisfiable (occt can).
        let manifold_desc = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (Operation::BooleanUnion, ReprKind::BRep),
            ],
        };
        let occt_desc = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("manifold".to_string(), &manifold_desc);
        registry.insert("occt".to_string(), &occt_desc);

        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "manifold".to_string(),
            Box::new(NamedRecordingKernel {
                name: "manifold".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );
        kernels.insert(
            "occt".to_string(),
            Box::new(NamedRecordingKernel {
                name: "occt".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );

        // Two ops: PrimitiveBox (occt cannot serve) → BooleanUnion (occt CAN serve).
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(0),
            },
        ];

        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "manifold",
            &ops,
            None,
            SourceSpan::new(0, 0),
            Some("occt"),
        );

        // (i) Exactly ONE KernelPragmaUnsatisfiable Warning, keyed on PrimitiveBox
        //     (the first unsatisfiable op). The dedup gate suppresses a second
        //     warning for BooleanUnion even though it routed to "occt" (satisfiable).
        let unsat_diags: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::KernelPragmaUnsatisfiable))
            .collect();
        assert_eq!(
            unsat_diags.len(),
            1,
            "mixed-satisfiability: exactly ONE KernelPragmaUnsatisfiable warning \
             (the intermediate PrimitiveBox op); got {} (all diagnostics: {:?})",
            unsat_diags.len(),
            state.diagnostics,
        );
        assert!(
            matches!(unsat_diags[0].severity, Severity::Warning),
            "KernelPragmaUnsatisfiable must be Warning-severity; got {:?}",
            unsat_diags[0].severity,
        );
        // The warning message names the op that could not be served by the pragma kernel.
        assert!(
            unsat_diags[0].message.contains("PrimitiveBox"),
            "KernelPragmaUnsatisfiable message must reference 'PrimitiveBox' \
             (the unsatisfiable intermediate op); got: {:?}",
            unsat_diags[0].message,
        );

        // (ii) Routing: PrimitiveBox → "manifold" (lex-min), BooleanUnion → "occt" (pragma).
        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls.len(),
            2,
            "mixed-satisfiability: expected 2 recorded kernel calls; got: {calls:?}",
        );
        assert_eq!(
            calls[0], "manifold",
            "PrimitiveBox (pragma unsatisfiable) must route to lex-min 'manifold'; \
             got: {:?}",
            calls[0],
        );
        assert_eq!(
            calls[1], "occt",
            "BooleanUnion (pragma satisfiable) must route to preferred 'occt'; \
             got: {:?}",
            calls[1],
        );

        // (iii) Realization succeeds: all ops produced handles, no kernel error.
        assert!(
            state.kernel_error_out.is_none(),
            "mixed-satisfiability: realization must succeed (fall-through continues); \
             kernel_error_out should be None, got {:?}",
            state.kernel_error_out,
        );
        assert_eq!(
            state.step_handles.len(),
            ops.len(),
            "mixed-satisfiability: all ops must produce handles; expected {}, got {:?}",
            ops.len(),
            state.step_handles,
        );
    }

    // ── effective_tessellation_tolerance unit tests ──────────────────────────

    /// When `module.default_tolerance` is `Some(v)`, the helper returns `v`
    /// (in SI metres) verbatim — the module-level `#precision` pragma value
    /// overrides the engine's hardcoded default.
    #[test]
    fn effective_tessellation_tolerance_uses_module_default_when_set() {
        use reify_core::ModulePath;
        use reify_test_support::builders::CompiledModuleBuilder;

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
        use reify_core::ModulePath;
        use reify_test_support::builders::CompiledModuleBuilder;

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

    impl reify_ir::GeometryKernel for RecordingTessellationKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            query: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(query)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.recorded_tolerances.lock().unwrap().push(tolerance);
            self.inner.tessellate(handle, tolerance)
        }
    }

    /// Build a CompiledModule with one Box-primitive realization, suitable for
    /// driving `tessellate_realizations`. Uses the same builder pattern as the
    /// fixture in `geometry_error_handling.rs::module_with_box_realization`.
    fn module_with_one_box_realization() -> reify_compiler::CompiledModule {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{ModulePath, Type};
        use reify_ir::CompiledExpr;
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder, mm};

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

    // ── parent_handles_for_op unit tests ─────────────────────────────────────

    /// Pins the per-variant-family parent extraction semantics of
    /// `parent_handles_for_op`. All variant families are covered in a single
    /// table; the `label` field doubles as the assertion failure message and
    /// as documentation for each exclusion rationale (path/spine, guide,
    /// plane — the three reference-geometry exclusion contracts).
    ///
    /// Rust's exhaustive `match` in `parent_handles_for_op` catches any new
    /// `GeometryOp` variant at compile time, so one representative per arm
    /// family is enough to guard against misclassification.
    #[test]
    fn parent_handles_for_op_returns_expected_handles_per_variant_family() {
        use reify_ir::Value;
        use reify_ir::geometry::GeometryOpDiscriminants;
        use strum::IntoEnumIterator;

        struct Case {
            op: GeometryOp,
            expected: Vec<GeometryHandleId>,
            label: &'static str,
        }

        let cases: Vec<Case> = vec![
            // ── Primitives ────────────────────────────────────────────────────
            Case {
                op: GeometryOp::Box {
                    width: Value::Real(0.01),
                    height: Value::Real(0.02),
                    depth: Value::Real(0.005),
                },
                expected: vec![],
                label: "Box → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::Cylinder {
                    radius: Value::Real(0.005),
                    height: Value::Real(0.02),
                },
                expected: vec![],
                label: "Cylinder → empty (primitive, no parents)",
            },
            // ── Curve constructors ────────────────────────────────────────────
            Case {
                op: GeometryOp::LineSegment {
                    x1: 0.0,
                    y1: 0.0,
                    z1: 0.0,
                    x2: 1.0,
                    y2: 0.0,
                    z2: 0.0,
                },
                expected: vec![],
                label: "LineSegment → empty (curve constructor, no parents)",
            },
            // ── Pipe ──────────────────────────────────────────────────────────
            Case {
                op: GeometryOp::Pipe {
                    path: GeometryHandleId(30),
                    radius: Value::Real(0.005),
                },
                expected: vec![],
                label: "Pipe → empty (kernel-internal circle profile, no user-facing parent)",
            },
            // ── Boolean ops ───────────────────────────────────────────────────
            Case {
                op: GeometryOp::Union {
                    left: GeometryHandleId(1),
                    right: GeometryHandleId(2),
                },
                expected: vec![GeometryHandleId(1), GeometryHandleId(2)],
                label: "Union → [left, right] in left-then-right order",
            },
            Case {
                op: GeometryOp::Difference {
                    left: GeometryHandleId(3),
                    right: GeometryHandleId(4),
                },
                expected: vec![GeometryHandleId(3), GeometryHandleId(4)],
                label: "Difference → [left, right]",
            },
            Case {
                op: GeometryOp::Intersection {
                    left: GeometryHandleId(5),
                    right: GeometryHandleId(6),
                },
                expected: vec![GeometryHandleId(5), GeometryHandleId(6)],
                label: "Intersection → [left, right]",
            },
            // ── Single-target shape-mods ──────────────────────────────────────
            Case {
                op: GeometryOp::Fillet {
                    target: GeometryHandleId(7),
                    edges: vec![],
                    radius: Value::Real(0.001),
                },
                expected: vec![GeometryHandleId(7)],
                label: "Fillet → [target]",
            },
            Case {
                op: GeometryOp::Chamfer {
                    target: GeometryHandleId(82),
                    edges: vec![],
                    distance: Value::Real(0.001),
                },
                expected: vec![GeometryHandleId(82)],
                label: "Chamfer → [target]",
            },
            Case {
                op: GeometryOp::Translate {
                    target: GeometryHandleId(80),
                    dx: 0.01,
                    dy: 0.0,
                    dz: 0.0,
                },
                expected: vec![GeometryHandleId(80)],
                label: "Translate → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::LinearPattern {
                    target: GeometryHandleId(81),
                    direction: [1.0, 0.0, 0.0],
                    count: 3,
                    spacing: Value::Real(0.01),
                },
                expected: vec![GeometryHandleId(81)],
                label: "LinearPattern → [target] (single-target pattern)",
            },
            Case {
                op: GeometryOp::Thicken {
                    target: GeometryHandleId(83),
                    offset: Value::Real(0.002),
                },
                expected: vec![GeometryHandleId(83)],
                label: "Thicken → [target]",
            },
            Case {
                op: GeometryOp::OffsetSolid {
                    target: GeometryHandleId(85),
                    distance: Value::Real(0.002),
                },
                expected: vec![GeometryHandleId(85)],
                label: "OffsetSolid → [target]",
            },
            Case {
                op: GeometryOp::Shell {
                    target: GeometryHandleId(84),
                    thickness: Value::Real(0.002),
                    faces_to_remove: vec![0],
                    open_face_handles: vec![],
                },
                expected: vec![GeometryHandleId(84)],
                label: "Shell → [target]",
            },
            Case {
                op: GeometryOp::ZoneSlab {
                    target: GeometryHandleId(90),
                    width: Value::Real(0.002),
                },
                expected: vec![GeometryHandleId(90)],
                label: "ZoneSlab → [target]",
            },
            Case {
                op: GeometryOp::Draft {
                    target: GeometryHandleId(70),
                    faces: vec![],
                    angle: Value::Real(0.1),
                    plane: GeometryHandleId(71),
                },
                expected: vec![GeometryHandleId(70)],
                // Draft's `plane` is a reference geometry / constraint, not a
                // parent whose sub-shapes propagate — analogous to SweepGuided's
                // guide.
                label: "Draft → [target] only; plane excluded (reference constraint, not a parent)",
            },
            // ── Single-profile sweep ops (path / spine excluded) ──────────────
            Case {
                op: GeometryOp::Extrude {
                    profile: GeometryHandleId(85),
                    distance: Value::Real(0.01),
                },
                expected: vec![GeometryHandleId(85)],
                label: "Extrude → [profile] (single-profile sweep)",
            },
            Case {
                op: GeometryOp::ExtrudeSymmetric {
                    profile: GeometryHandleId(50),
                    distance: Value::Real(0.01),
                },
                expected: vec![GeometryHandleId(50)],
                label: "ExtrudeSymmetric → [profile]",
            },
            Case {
                op: GeometryOp::ExtrudeInfinite {
                    profile: GeometryHandleId(50),
                    axis: [0.0, 0.0, 1.0],
                    both: false,
                },
                expected: vec![GeometryHandleId(50)],
                label: "ExtrudeInfinite → [profile]",
            },
            Case {
                op: GeometryOp::Revolve {
                    profile: GeometryHandleId(60),
                    axis_origin: [0.0, 0.0, 0.0],
                    axis_dir: [0.0, 0.0, 1.0],
                    angle_rad: std::f64::consts::PI,
                },
                expected: vec![GeometryHandleId(60)],
                label: "Revolve → [profile] (axis fields are scalars, not parent handles)",
            },
            Case {
                op: GeometryOp::Sweep {
                    profile: GeometryHandleId(20),
                    path: GeometryHandleId(21),
                },
                expected: vec![GeometryHandleId(20)],
                // Path/spine is a route, not a parent whose sub-shapes propagate
                // into the result — mirrors populate_attribute_history semantics
                // (engine_build.rs:103-114).
                label: "Sweep → [profile] only; path excluded (spine is not a parent)",
            },
            Case {
                op: GeometryOp::SweepGuided {
                    profile: GeometryHandleId(40),
                    path: GeometryHandleId(41),
                    guide: GeometryHandleId(42),
                },
                expected: vec![GeometryHandleId(40)],
                label: "SweepGuided → [profile] only; both path and guide excluded (guide is an auxiliary constraint wire, not a parent)",
            },
            // ── Multi-profile loft ops (guides excluded) ───────────────────────
            Case {
                op: GeometryOp::Loft {
                    profiles: vec![
                        GeometryHandleId(10),
                        GeometryHandleId(11),
                        GeometryHandleId(12),
                    ],
                },
                expected: vec![
                    GeometryHandleId(10),
                    GeometryHandleId(11),
                    GeometryHandleId(12),
                ],
                label: "Loft → all profiles in input order (multi-profile, ordering preserved)",
            },
            Case {
                op: GeometryOp::LoftGuided {
                    profiles: vec![
                        GeometryHandleId(20),
                        GeometryHandleId(21),
                        GeometryHandleId(22),
                    ],
                    guides: vec![GeometryHandleId(30), GeometryHandleId(31)],
                },
                expected: vec![
                    GeometryHandleId(20),
                    GeometryHandleId(21),
                    GeometryHandleId(22),
                ],
                // Most error-prone exclusion: a regression that appended guides to
                // the parent list would be silently missed without this case.
                label: "LoftGuided → profiles only; guides excluded (constraints, not parents)",
            },
            // ── Remaining primitives (task 4671 step-3: full 47-variant coverage) ─
            Case {
                op: GeometryOp::Sphere { radius: Value::Real(0.005) },
                expected: vec![],
                label: "Sphere → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::Tube {
                    outer_r: Value::Real(0.01),
                    inner_r: Value::Real(0.005),
                    height: Value::Real(0.02),
                },
                expected: vec![],
                label: "Tube → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::Cone {
                    bottom_radius: Value::Real(0.01),
                    top_radius: Value::Real(0.005),
                    height: Value::Real(0.02),
                },
                expected: vec![],
                label: "Cone → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::Wedge {
                    width: Value::Real(0.020),
                    depth: Value::Real(0.010),
                    height: Value::Real(0.015),
                    top_width: Value::Real(0.005),
                },
                expected: vec![],
                label: "Wedge → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::Torus {
                    major_radius: Value::Real(0.02),
                    minor_radius: Value::Real(0.005),
                },
                expected: vec![],
                label: "Torus → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::HalfSpace {
                    px: Value::Real(0.0),
                    py: Value::Real(0.0),
                    pz: Value::Real(0.0),
                    nx: Value::Real(0.0),
                    ny: Value::Real(0.0),
                    nz: Value::Real(1.0),
                },
                expected: vec![],
                label: "HalfSpace → empty (primitive, no parents)",
            },
            // ── Remaining curve constructors ──────────────────────────────────
            Case {
                op: GeometryOp::Arc {
                    center: [0.0, 0.0, 0.0],
                    radius: 0.01,
                    start_angle: 0.0,
                    end_angle: 1.57,
                    axis: [0.0, 0.0, 1.0],
                },
                expected: vec![],
                label: "Arc → empty (curve constructor, no parents)",
            },
            Case {
                op: GeometryOp::Helix {
                    radius: 0.01,
                    pitch: 0.005,
                    height: 0.05,
                },
                expected: vec![],
                label: "Helix → empty (curve constructor, no parents)",
            },
            Case {
                op: GeometryOp::InterpCurve {
                    points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                },
                expected: vec![],
                label: "InterpCurve → empty (curve constructor, no parents)",
            },
            Case {
                op: GeometryOp::BezierCurve {
                    control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                },
                expected: vec![],
                label: "BezierCurve → empty (curve constructor, no parents)",
            },
            Case {
                op: GeometryOp::NurbsCurve {
                    control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                    weights: vec![1.0, 1.0],
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    degree: 1,
                },
                expected: vec![],
                label: "NurbsCurve → empty (curve constructor, no parents)",
            },
            // ── Surface constructors ───────────────────────────────────────────
            Case {
                op: GeometryOp::NurbsSurface {
                    control_points: vec![
                        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                        vec![[0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
                    ],
                    weights: vec![vec![1.0, 1.0], vec![1.0, 1.0]],
                    u_knots: vec![0.0, 0.0, 1.0, 1.0],
                    v_knots: vec![0.0, 0.0, 1.0, 1.0],
                    u_degree: 1,
                    v_degree: 1,
                },
                expected: vec![],
                label: "NurbsSurface → empty (surface constructor, no parents)",
            },
            // ── Profile face producers ─────────────────────────────────────────
            Case {
                op: GeometryOp::RectangleProfile {
                    width: Value::Real(0.02),
                    height: Value::Real(0.01),
                },
                expected: vec![],
                label: "RectangleProfile → empty (profile producer, no parents)",
            },
            Case {
                op: GeometryOp::CircleProfile { radius: Value::Real(0.008) },
                expected: vec![],
                label: "CircleProfile → empty (profile producer, no parents)",
            },
            Case {
                op: GeometryOp::PolygonProfile {
                    points: vec![[0.0, 0.0], [0.01, 0.0], [0.01, 0.01], [0.0, 0.01]],
                },
                expected: vec![],
                label: "PolygonProfile → empty (profile producer, no parents)",
            },
            Case {
                op: GeometryOp::EllipseProfile {
                    semi_major: Value::Real(0.010),
                    semi_minor: Value::Real(0.005),
                },
                expected: vec![],
                label: "EllipseProfile → empty (profile producer, no parents)",
            },
            // ── Remaining single-target shape-mods ────────────────────────────
            Case {
                op: GeometryOp::ChamferAsymmetric {
                    target: GeometryHandleId(91),
                    edges: vec![],
                    d1: Value::Real(0.001),
                    d2: Value::Real(0.002),
                },
                expected: vec![GeometryHandleId(91)],
                label: "ChamferAsymmetric → [target]",
            },
            Case {
                op: GeometryOp::Rotate {
                    target: GeometryHandleId(92),
                    axis: [0.0, 0.0, 1.0],
                    angle_rad: 0.5,
                },
                expected: vec![GeometryHandleId(92)],
                label: "Rotate → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::Scale {
                    target: GeometryHandleId(93),
                    factor: 2.0,
                },
                expected: vec![GeometryHandleId(93)],
                label: "Scale → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::ScaleNonUniform {
                    target: GeometryHandleId(103),
                    sx: 2.0,
                    sy: 1.0,
                    sz: 0.5,
                },
                expected: vec![GeometryHandleId(103)],
                label: "ScaleNonUniform → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::RotateAround {
                    target: GeometryHandleId(94),
                    point: [0.0, 0.0, 0.0],
                    axis: [0.0, 0.0, 1.0],
                    angle_rad: 0.5,
                },
                expected: vec![GeometryHandleId(94)],
                label: "RotateAround → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::ApplyTransform {
                    target: GeometryHandleId(95),
                    rotation: [1.0, 0.0, 0.0, 0.0],
                    translation: [0.0, 0.0, 0.0],
                },
                expected: vec![GeometryHandleId(95)],
                label: "ApplyTransform → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::AffineApply {
                    target: GeometryHandleId(102),
                    linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                    translation: [0.0, 0.0, 0.0],
                },
                expected: vec![GeometryHandleId(102)],
                label: "AffineApply → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::CircularPattern {
                    target: GeometryHandleId(96),
                    axis_origin: [0.0, 0.0, 0.0],
                    axis_dir: [0.0, 0.0, 1.0],
                    count: 4,
                    angle: Value::Real(1.57),
                },
                expected: vec![GeometryHandleId(96)],
                label: "CircularPattern → [target] (single-target pattern)",
            },
            Case {
                op: GeometryOp::Mirror {
                    target: GeometryHandleId(97),
                    plane_origin: [0.0, 0.0, 0.0],
                    plane_normal: [1.0, 0.0, 0.0],
                },
                expected: vec![GeometryHandleId(97)],
                label: "Mirror → [target] (single-target pattern)",
            },
            Case {
                op: GeometryOp::LinearPattern2D {
                    target: GeometryHandleId(98),
                    direction1: [1.0, 0.0, 0.0],
                    count1: 3,
                    spacing1: Value::Real(0.01),
                    direction2: [0.0, 1.0, 0.0],
                    count2: 3,
                    spacing2: Value::Real(0.01),
                },
                expected: vec![GeometryHandleId(98)],
                label: "LinearPattern2D → [target] (single-target pattern)",
            },
            Case {
                op: GeometryOp::ArbitraryPattern {
                    target: GeometryHandleId(99),
                    transforms: vec![([1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0])],
                },
                expected: vec![GeometryHandleId(99)],
                label: "ArbitraryPattern → [target] (single-target pattern)",
            },
            Case {
                op: GeometryOp::OffsetCurve {
                    target: GeometryHandleId(100),
                    distance: Value::Real(0.002),
                    reference: None,
                    direction: None,
                },
                expected: vec![GeometryHandleId(100)],
                label: "OffsetCurve → [target]; reference is a constraint surface, not a parent",
            },
            // ── Surface (isosurface / marching-cubes, task 4999) ───────────────
            Case {
                op: GeometryOp::Surface {
                    grid: GeometryHandleId(104),
                    iso_level: 0.0,
                    adaptive: false,
                },
                expected: vec![GeometryHandleId(104)],
                label: "Surface → [grid] (voxel grid parent for isosurface)",
            },
        ];

        for case in &cases {
            assert_eq!(
                parent_handles_for_op(&case.op).as_slice(),
                case.expected.as_slice(),
                "parent_handles_for_op mismatch: {}",
                case.label,
            );
        }

        // Coverage-completeness assertion: every non-Split GeometryOpDiscriminants
        // must appear exactly once in the cases table (DD-3 model — adding a variant
        // forces a RED test-time failure before it reaches unreachable!() in production).
        let seen: HashSet<GeometryOpDiscriminants> =
            cases.iter().map(|c| GeometryOpDiscriminants::from(&c.op)).collect();
        let all_non_split: HashSet<GeometryOpDiscriminants> = GeometryOpDiscriminants::iter()
            .filter(|d| *d != GeometryOpDiscriminants::Split)
            .collect();
        assert_eq!(
            seen,
            all_non_split,
            "parent_handles_for_op coverage gap — missing discriminants: {:?}",
            all_non_split.difference(&seen).collect::<Vec<_>>()
        );
    }

    // ── substitute_op_parents unit tests ─────────────────────────────────────

    /// Characterizes the per-variant-family parent-handle substitution semantics
    /// of `substitute_op_parents`. For every non-Split variant (47 total):
    /// builds an op with known handle ids, applies `substitute_op_parents` with
    /// a mapping that remaps those ids, and asserts that only the PARENT fields
    /// are rewritten — non-parent fields (Pipe.path, Sweep.path, SweepGuided.path
    /// + .guide, Draft.plane, OffsetCurve.reference, LoftGuided.guides) are
    ///   deliberately placed in the map but must NOT be rewritten. Handles absent
    ///   from the map are left as-is (tested via Union left absent from map).
    ///
    /// All expected values are hardcoded independently of the L1 table, so
    /// full 47-variant coverage gives full validation of the table's
    /// `parent_role` column for this function.
    ///
    /// Stays GREEN against the current per-variant fn; the coverage-completeness
    /// assertion turns RED if a new variant is added and not covered here.
    #[test]
    fn substitute_op_parents_rewrites_parents_per_variant_family() {
        use std::collections::HashMap;
        use reify_ir::Value;
        use reify_ir::geometry::GeometryOpDiscriminants;
        use strum::IntoEnumIterator;

        let h = GeometryHandleId;
        let mut seen: HashSet<GeometryOpDiscriminants> = HashSet::new();

        fn make_map(
            pairs: &[(u64, u64)],
        ) -> HashMap<GeometryHandleId, GeometryHandleId> {
            pairs.iter().map(|&(s, d)| (GeometryHandleId(s), GeometryHandleId(d))).collect()
        }

        // ── None-role: primitives — scalar fields only, nothing to substitute ─
        let no_handles = make_map(&[(999, 9999)]); // map with irrelevant entries

        let mut op = GeometryOp::Box { width: Value::Real(1.0), height: Value::Real(1.0), depth: Value::Real(1.0) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles); // must not panic

        let mut op = GeometryOp::Cylinder { radius: Value::Real(0.005), height: Value::Real(0.02) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Sphere { radius: Value::Real(0.005) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Tube { outer_r: Value::Real(0.01), inner_r: Value::Real(0.005), height: Value::Real(0.02) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Cone { bottom_radius: Value::Real(0.01), top_radius: Value::Real(0.0), height: Value::Real(0.02) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Wedge { width: Value::Real(0.02), depth: Value::Real(0.01), height: Value::Real(0.015), top_width: Value::Real(0.005) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Torus { major_radius: Value::Real(0.02), minor_radius: Value::Real(0.005) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::HalfSpace { px: Value::Real(0.0), py: Value::Real(0.0), pz: Value::Real(0.0), nx: Value::Real(0.0), ny: Value::Real(0.0), nz: Value::Real(1.0) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles); // HalfSpace has no parent handles (primitive)

        // ── None-role: curve constructors ─────────────────────────────────────

        let mut op = GeometryOp::LineSegment { x1: 0.0, y1: 0.0, z1: 0.0, x2: 1.0, y2: 0.0, z2: 0.0 };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Arc { center: [0.0; 3], radius: 0.01, start_angle: 0.0, end_angle: 1.57, axis: [0.0, 0.0, 1.0] };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Helix { radius: 0.01, pitch: 0.005, height: 0.05 };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::InterpCurve { points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]] };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::BezierCurve { control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]] };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::NurbsCurve { control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], weights: vec![1.0, 1.0], knots: vec![0.0, 0.0, 1.0, 1.0], degree: 1 };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        // ── None-role: surface constructors ──────────────────────────────────

        let mut op = GeometryOp::NurbsSurface {
            control_points: vec![
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                vec![[0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
            ],
            weights: vec![vec![1.0, 1.0], vec![1.0, 1.0]],
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_degree: 1,
            v_degree: 1,
        };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        // ── None-role: profile face producers ────────────────────────────────

        let mut op = GeometryOp::RectangleProfile { width: Value::Real(0.02), height: Value::Real(0.01) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::CircleProfile { radius: Value::Real(0.008) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::PolygonProfile { points: vec![[0.0, 0.0], [0.01, 0.0], [0.01, 0.01]] };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::EllipseProfile { semi_major: Value::Real(0.01), semi_minor: Value::Real(0.005) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        // ── None-role: Pipe — path IS in the map but must NOT be remapped ────
        {
            let mut op = GeometryOp::Pipe { path: h(30), radius: Value::Real(0.005) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(30, 300)]));
            match &op {
                GeometryOp::Pipe { path, .. } => assert_eq!(
                    *path, h(30),
                    "Pipe.path must NOT be substituted (kernel-internal profile, not a user-facing parent)"
                ),
                _ => panic!("op must still be Pipe"),
            }
        }

        // ── Pair: both left and right are parents ─────────────────────────────
        {
            let mut op = GeometryOp::Union { left: h(1), right: h(2) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(1, 101), (2, 102)]));
            match &op {
                GeometryOp::Union { left, right } => {
                    assert_eq!(*left, h(101), "Union.left must be remapped");
                    assert_eq!(*right, h(102), "Union.right must be remapped");
                }
                _ => panic!("op must still be Union"),
            }

            // Absent-from-map: right is NOT in the map, must stay as-is
            let mut op = GeometryOp::Union { left: h(3), right: h(4) };
            substitute_op_parents(&mut op, &make_map(&[(3, 103)])); // 4 absent
            match &op {
                GeometryOp::Union { left, right } => {
                    assert_eq!(*left, h(103), "Union.left must be remapped");
                    assert_eq!(*right, h(4), "Union.right absent from map must stay as-is");
                }
                _ => panic!("op must still be Union"),
            }
        }
        {
            let mut op = GeometryOp::Difference { left: h(1), right: h(2) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(1, 101), (2, 102)]));
            match &op {
                GeometryOp::Difference { left, right } => {
                    assert_eq!(*left, h(101), "Difference.left remapped");
                    assert_eq!(*right, h(102), "Difference.right remapped");
                }
                _ => panic!("op must still be Difference"),
            }
        }
        {
            let mut op = GeometryOp::Intersection { left: h(1), right: h(2) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(1, 101), (2, 102)]));
            match &op {
                GeometryOp::Intersection { left, right } => {
                    assert_eq!(*left, h(101), "Intersection.left remapped");
                    assert_eq!(*right, h(102), "Intersection.right remapped");
                }
                _ => panic!("op must still be Intersection"),
            }
        }

        // ── SingleTarget: target is the sole parent ──────────────────────────
        macro_rules! check_single_target {
            ($op:expr, $target_id:expr, $new_id:expr, $label:literal) => {{
                let disc = GeometryOpDiscriminants::from(&$op);
                seen.insert(disc);
                let mut op = $op;
                substitute_op_parents(&mut op, &make_map(&[($target_id, $new_id)]));
                assert_eq!(
                    parent_handles_for_op(&op).as_slice(),
                    &[GeometryHandleId($new_id)],
                    "SingleTarget {}: target must be remapped",
                    $label
                );
            }};
        }

        check_single_target!(
            GeometryOp::Fillet { target: h(10), edges: vec![], radius: Value::Real(0.001) },
            10, 110, "Fillet"
        );
        check_single_target!(
            GeometryOp::Chamfer { target: h(10), edges: vec![], distance: Value::Real(0.001) },
            10, 110, "Chamfer"
        );
        check_single_target!(
            GeometryOp::ChamferAsymmetric { target: h(10), edges: vec![], d1: Value::Real(0.001), d2: Value::Real(0.002) },
            10, 110, "ChamferAsymmetric"
        );
        check_single_target!(
            GeometryOp::Translate { target: h(10), dx: 0.0, dy: 0.0, dz: 0.01 },
            10, 110, "Translate"
        );
        check_single_target!(
            GeometryOp::Rotate { target: h(10), axis: [0.0, 0.0, 1.0], angle_rad: 0.5 },
            10, 110, "Rotate"
        );
        check_single_target!(
            GeometryOp::Scale { target: h(10), factor: 2.0 },
            10, 110, "Scale"
        );
        check_single_target!(
            GeometryOp::ScaleNonUniform { target: h(10), sx: 2.0, sy: 1.0, sz: 0.5 },
            10, 110, "ScaleNonUniform"
        );
        check_single_target!(
            GeometryOp::RotateAround { target: h(10), point: [0.0; 3], axis: [0.0, 0.0, 1.0], angle_rad: 0.5 },
            10, 110, "RotateAround"
        );
        check_single_target!(
            GeometryOp::ApplyTransform { target: h(10), rotation: [1.0, 0.0, 0.0, 0.0], translation: [0.0; 3] },
            10, 110, "ApplyTransform"
        );
        check_single_target!(
            GeometryOp::AffineApply { target: h(10), linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], translation: [0.0; 3] },
            10, 110, "AffineApply"
        );
        check_single_target!(
            GeometryOp::LinearPattern { target: h(10), direction: [1.0, 0.0, 0.0], count: 3, spacing: Value::Real(0.01) },
            10, 110, "LinearPattern"
        );
        check_single_target!(
            GeometryOp::CircularPattern { target: h(10), axis_origin: [0.0; 3], axis_dir: [0.0, 0.0, 1.0], count: 4, angle: Value::Real(1.57) },
            10, 110, "CircularPattern"
        );
        check_single_target!(
            GeometryOp::Mirror { target: h(10), plane_origin: [0.0; 3], plane_normal: [1.0, 0.0, 0.0] },
            10, 110, "Mirror"
        );
        check_single_target!(
            GeometryOp::LinearPattern2D { target: h(10), direction1: [1.0, 0.0, 0.0], count1: 3, spacing1: Value::Real(0.01), direction2: [0.0, 1.0, 0.0], count2: 3, spacing2: Value::Real(0.01) },
            10, 110, "LinearPattern2D"
        );
        check_single_target!(
            GeometryOp::ArbitraryPattern { target: h(10), transforms: vec![([1.0, 0.0, 0.0, 0.0], [0.0; 3])] },
            10, 110, "ArbitraryPattern"
        );
        check_single_target!(
            GeometryOp::Thicken { target: h(10), offset: Value::Real(0.002) },
            10, 110, "Thicken"
        );
        check_single_target!(
            GeometryOp::OffsetSolid { target: h(10), distance: Value::Real(0.002) },
            10, 110, "OffsetSolid"
        );
        check_single_target!(
            GeometryOp::Shell { target: h(10), thickness: Value::Real(0.002), faces_to_remove: vec![0], open_face_handles: vec![] },
            10, 110, "Shell"
        );
        check_single_target!(
            GeometryOp::ZoneSlab { target: h(10), width: Value::Real(0.002) },
            10, 110, "ZoneSlab"
        );

        // Draft.plane is a constraint, not a parent — must NOT be remapped
        {
            let mut op = GeometryOp::Draft { target: h(10), faces: vec![], angle: Value::Real(0.1), plane: h(20) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (20, 220)]));
            match &op {
                GeometryOp::Draft { target, plane, .. } => {
                    assert_eq!(*target, h(110), "Draft.target must be remapped");
                    assert_eq!(*plane, h(20), "Draft.plane must NOT be remapped (reference constraint)");
                }
                _ => panic!("op must still be Draft"),
            }
        }
        // OffsetCurve.reference is a constraint surface, not a parent
        {
            let mut op = GeometryOp::OffsetCurve { target: h(10), distance: Value::Real(0.002), reference: Some(h(20)), direction: None };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (20, 220)]));
            match &op {
                GeometryOp::OffsetCurve { target, reference, .. } => {
                    assert_eq!(*target, h(110), "OffsetCurve.target must be remapped");
                    assert_eq!(*reference, Some(h(20)), "OffsetCurve.reference must NOT be remapped (constraint surface)");
                }
                _ => panic!("op must still be OffsetCurve"),
            }
        }
        // Surface (isosurface / marching-cubes, task 4999): grid is the sole parent
        {
            let mut op = GeometryOp::Surface { grid: h(10), iso_level: 0.0, adaptive: false };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110)]));
            match &op {
                GeometryOp::Surface { grid, .. } => assert_eq!(*grid, h(110), "Surface.grid must be remapped"),
                _ => panic!("op must still be Surface"),
            }
        }

        // ── SingleProfile: profile only; path/guide excluded ─────────────────
        {
            let mut op = GeometryOp::Extrude { profile: h(10), distance: Value::Real(0.01) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110)]));
            match &op {
                GeometryOp::Extrude { profile, .. } => assert_eq!(*profile, h(110), "Extrude.profile remapped"),
                _ => panic!("op must still be Extrude"),
            }
        }
        {
            let mut op = GeometryOp::ExtrudeSymmetric { profile: h(10), distance: Value::Real(0.01) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110)]));
            match &op {
                GeometryOp::ExtrudeSymmetric { profile, .. } => assert_eq!(*profile, h(110), "ExtrudeSymmetric.profile remapped"),
                _ => panic!("op must still be ExtrudeSymmetric"),
            }
        }
        {
            let mut op = GeometryOp::ExtrudeInfinite { profile: h(10), axis: [0.0, 0.0, 1.0], both: false };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110)]));
            match &op {
                GeometryOp::ExtrudeInfinite { profile, .. } => assert_eq!(*profile, h(110), "ExtrudeInfinite.profile remapped"),
                _ => panic!("op must still be ExtrudeInfinite"),
            }
        }
        {
            let mut op = GeometryOp::Revolve { profile: h(10), axis_origin: [0.0; 3], axis_dir: [0.0, 0.0, 1.0], angle_rad: 1.0 };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110)]));
            match &op {
                GeometryOp::Revolve { profile, .. } => assert_eq!(*profile, h(110), "Revolve.profile remapped"),
                _ => panic!("op must still be Revolve"),
            }
        }
        // Sweep.path is a route, not a parent — must NOT be remapped
        {
            let mut op = GeometryOp::Sweep { profile: h(10), path: h(20) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (20, 220)]));
            match &op {
                GeometryOp::Sweep { profile, path } => {
                    assert_eq!(*profile, h(110), "Sweep.profile must be remapped");
                    assert_eq!(*path, h(20), "Sweep.path must NOT be remapped (spine is not a parent)");
                }
                _ => panic!("op must still be Sweep"),
            }
        }
        // SweepGuided.path and .guide are both excluded
        {
            let mut op = GeometryOp::SweepGuided { profile: h(10), path: h(20), guide: h(30) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (20, 220), (30, 330)]));
            match &op {
                GeometryOp::SweepGuided { profile, path, guide } => {
                    assert_eq!(*profile, h(110), "SweepGuided.profile must be remapped");
                    assert_eq!(*path, h(20), "SweepGuided.path must NOT be remapped");
                    assert_eq!(*guide, h(30), "SweepGuided.guide must NOT be remapped (auxiliary constraint)");
                }
                _ => panic!("op must still be SweepGuided"),
            }
        }

        // ── VariadicProfiles: every profile remapped; guides excluded ─────────
        {
            let mut op = GeometryOp::Loft { profiles: vec![h(10), h(11), h(12)] };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (11, 111), (12, 112)]));
            match &op {
                GeometryOp::Loft { profiles } => assert_eq!(
                    profiles.as_slice(),
                    &[h(110), h(111), h(112)],
                    "Loft: all profiles must be remapped"
                ),
                _ => panic!("op must still be Loft"),
            }
        }
        // LoftGuided.guides must NOT be remapped
        {
            let mut op = GeometryOp::LoftGuided { profiles: vec![h(10), h(11)], guides: vec![h(30), h(31)] };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (11, 111), (30, 330), (31, 331)]));
            match &op {
                GeometryOp::LoftGuided { profiles, guides } => {
                    assert_eq!(profiles.as_slice(), &[h(110), h(111)], "LoftGuided: profiles must be remapped");
                    assert_eq!(guides.as_slice(), &[h(30), h(31)], "LoftGuided: guides must NOT be remapped");
                }
                _ => panic!("op must still be LoftGuided"),
            }
        }

        // Coverage-completeness assertion: every non-Split GeometryOpDiscriminants
        // must appear in the cases above (DD-3 model).
        let all_non_split: HashSet<GeometryOpDiscriminants> = GeometryOpDiscriminants::iter()
            .filter(|d| *d != GeometryOpDiscriminants::Split)
            .collect();
        assert_eq!(
            seen,
            all_non_split,
            "substitute_op_parents coverage gap — missing discriminants: {:?}",
            all_non_split.difference(&seen).collect::<Vec<_>>()
        );
    }

    // ── compute_demanded_tols unit tests ─────────────────────────────────────

    /// Pins the new return type of `compute_demanded_tols`:
    /// `Vec<Vec<Option<f64>>>` indexed `[template_idx][realization_idx]`
    /// rather than `HashMap<(String, String), Option<f64>>`.
    ///
    /// Two sub-scenarios:
    ///
    /// (a) **Shape + all-None**: module with two templates — template `A`
    ///     (1 realization, entity "EntityA") and template `B` (2 realizations,
    ///     entities "EntityB_0" / "EntityB_1"), no tolerance contributors →
    ///     outer length == 2, inner lengths [1, 2], all cells `None`.
    ///
    /// (b) **Positive-path / positional alignment**: same module, but
    ///     `active_tolerance_scope` is seeded so EntityA → `Some(1e-5)` and
    ///     EntityB_0 → `Some(2e-5)`, while EntityB_1 is left unset.
    ///     Asserts that `result[0][0] == Some(1e-5)`,
    ///     `result[1][0] == Some(2e-5)`, and `result[1][1] == None` —
    ///     pinning correct positional alignment plus that an
    ///     `active_tolerance_scope` entry surfaces through the chain as
    ///     `Some(_)`.  Note: `demanded_tolerance_for_output` already
    ///     incorporates `active_tolerance_for` internally (the purpose_bound
    ///     path in `combine_demanded_tolerance`), so the seeded scope entry
    ///     surfaces via that function directly — no `.or_else` fallback is
    ///     required or present in the production code.
    #[test]
    fn compute_demanded_tols_returns_positionally_indexed_vec_of_vec() {
        use reify_core::ModulePath;
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        let checker = MockConstraintChecker::new();
        // `mut` required for the positive-path sub-scenario where we seed
        // `active_tolerance_scope` directly (crate-internal field).
        let mut engine = crate::Engine::new(Box::new(checker), None);

        let template_a = TopologyTemplateBuilder::new("EntityA")
            .realization("EntityA", 0, vec![])
            .build();
        // Use distinct entity refs for B's two realizations so we can set one
        // scope entry and leave the other unset, pinning positional alignment.
        let template_b = TopologyTemplateBuilder::new("EntityB")
            .realization("EntityB_0", 0, vec![])
            .realization("EntityB_1", 1, vec![])
            .build();
        let module = CompiledModuleBuilder::new(ModulePath::single("test_demanded_tols"))
            .template(template_a)
            .template(template_b)
            .build();

        // ── (a) shape + all-None ─────────────────────────────────────────────
        let result: Vec<Vec<Option<f64>>> = engine.compute_demanded_tols(&module);

        assert_eq!(
            result.len(),
            2,
            "outer Vec must have one entry per template"
        );
        assert_eq!(result[0].len(), 1, "template A has 1 realization");
        assert_eq!(result[1].len(), 2, "template B has 2 realizations");
        assert!(
            result[0][0].is_none(),
            "no tolerance contributor → None for template A realization 0"
        );
        assert!(
            result[1][0].is_none(),
            "no tolerance contributor → None for template B realization 0"
        );
        assert!(
            result[1][1].is_none(),
            "no tolerance contributor → None for template B realization 1"
        );

        // ── (b) positive-path: active-tolerance contributor surfaces, positional alignment ──
        //
        // Seed `active_tolerance_scope` (crate-private field, directly
        // accessible from `mod tests` within the same crate) so that
        // `active_tolerance_for("EntityA")` and `active_tolerance_for("EntityB_0")`
        // return `Some`.  `demanded_tolerance_for_output` incorporates
        // `active_tolerance_for` as its purpose_bound path inside
        // `combine_demanded_tolerance`, so the seeded scope entries surface
        // as `Some(_)` through that function directly.  This test pins
        // (i) that the entry surfaces as `Some(_)` via the production path,
        // and (ii) correct positional alignment.
        engine
            .active_tolerance_scope
            .insert("EntityA".to_string(), 1e-5_f64);
        engine
            .active_tolerance_scope
            .insert("EntityB_0".to_string(), 2e-5_f64);
        // "EntityB_1" is intentionally left unset → result[1][1] stays None.

        let positive: Vec<Vec<Option<f64>>> = engine.compute_demanded_tols(&module);

        assert_eq!(
            positive[0][0],
            Some(1e-5),
            "EntityA scope → Some(1e-5) at [template_idx=0][r_idx=0]; \
             priority chain must surface it rather than return None"
        );
        assert_eq!(
            positive[1][0],
            Some(2e-5),
            "EntityB_0 scope → Some(2e-5) at [template_idx=1][r_idx=0]; \
             positional alignment: first realization must map to inner index 0"
        );
        assert!(
            positive[1][1].is_none(),
            "EntityB_1 unset → None at [template_idx=1][r_idx=1]; \
             positional alignment: second realization must map to inner index 1"
        );
    }

    // ── geometry_op_to_operation unit tests ──────────────────────────────────

    /// Pins the `GeometryOp` → `Operation` total mapping (task ε / 3436,
    /// PRD §8 step-3/4). Each entry constructs a representative `GeometryOp`
    /// (argument values are immaterial — the mapping is purely on variant
    /// kind, mirroring `parent_handles_for_op`'s table) and asserts the
    /// dispatcher-classifier output.
    ///
    /// Coverage spans every variant family — Primitives, Curves, Pipe,
    /// Booleans, single-target Modify/Transform/Pattern, single-profile
    /// Sweep, multi-profile Loft. Rust's exhaustive `match` inside
    /// `geometry_op_to_operation` makes a new `GeometryOp` variant fail to
    /// compile at the helper site, so the helper itself guards against
    /// missing arms — this test pins the chosen `Operation` per arm.
    ///
    /// RED before step-4 impl: `geometry_op_to_operation` does not exist yet.
    #[test]
    fn geometry_op_to_operation_maps_every_variant_family() {
        use reify_ir::{Operation, Value};
        use reify_ir::geometry::GeometryOpDiscriminants;
        use strum::IntoEnumIterator;

        let h = |id| GeometryHandleId(id);
        let r = |v| Value::Real(v);

        struct Case {
            op: GeometryOp,
            expected: Operation,
            label: &'static str,
        }

        let cases: Vec<Case> = vec![
            // Primitives
            Case {
                op: GeometryOp::Box {
                    width: r(0.01),
                    height: r(0.01),
                    depth: r(0.01),
                },
                expected: Operation::PrimitiveBox,
                label: "Box → PrimitiveBox",
            },
            Case {
                op: GeometryOp::Cylinder {
                    radius: r(0.005),
                    height: r(0.02),
                },
                expected: Operation::PrimitiveCylinder,
                label: "Cylinder → PrimitiveCylinder",
            },
            Case {
                op: GeometryOp::Sphere { radius: r(0.005) },
                expected: Operation::PrimitiveSphere,
                label: "Sphere → PrimitiveSphere",
            },
            Case {
                op: GeometryOp::Tube {
                    outer_r: r(0.01),
                    inner_r: r(0.005),
                    height: r(0.02),
                },
                expected: Operation::PrimitiveTube,
                label: "Tube → PrimitiveTube",
            },
            Case {
                op: GeometryOp::Cone {
                    bottom_radius: r(0.01),
                    top_radius: r(0.005),
                    height: r(0.02),
                },
                expected: Operation::PrimitiveCone,
                label: "Cone → PrimitiveCone",
            },
            Case {
                op: GeometryOp::Wedge {
                    width: r(0.020),
                    depth: r(0.010),
                    height: r(0.015),
                    top_width: r(0.005),
                },
                expected: Operation::PrimitiveWedge,
                label: "Wedge → PrimitiveWedge",
            },
            Case {
                op: GeometryOp::Torus {
                    major_radius: r(0.02),
                    minor_radius: r(0.005),
                },
                expected: Operation::PrimitiveTorus,
                label: "Torus → PrimitiveTorus",
            },
            Case {
                op: GeometryOp::HalfSpace {
                    px: r(0.0),
                    py: r(0.0),
                    pz: r(0.0),
                    nx: r(0.0),
                    ny: r(0.0),
                    nz: r(1.0),
                },
                expected: Operation::PrimitiveHalfSpace,
                label: "HalfSpace → PrimitiveHalfSpace",
            },
            // Booleans
            Case {
                op: GeometryOp::Union {
                    left: h(1),
                    right: h(2),
                },
                expected: Operation::BooleanUnion,
                label: "Union → BooleanUnion",
            },
            Case {
                op: GeometryOp::Difference {
                    left: h(1),
                    right: h(2),
                },
                expected: Operation::BooleanDifference,
                label: "Difference → BooleanDifference",
            },
            Case {
                op: GeometryOp::Intersection {
                    left: h(1),
                    right: h(2),
                },
                expected: Operation::BooleanIntersection,
                label: "Intersection → BooleanIntersection",
            },
            // Modify
            Case {
                op: GeometryOp::Fillet {
                    target: h(1),
                    edges: vec![],
                    radius: r(0.001),
                },
                expected: Operation::ModifyFillet,
                label: "Fillet → ModifyFillet",
            },
            Case {
                op: GeometryOp::Chamfer {
                    target: h(1),
                    edges: vec![],
                    distance: r(0.001),
                },
                expected: Operation::ModifyChamfer,
                label: "Chamfer → ModifyChamfer",
            },
            Case {
                op: GeometryOp::Shell {
                    target: h(1),
                    thickness: r(0.001),
                    faces_to_remove: vec![0],
                    open_face_handles: vec![],
                },
                expected: Operation::ModifyShell,
                label: "Shell → ModifyShell",
            },
            Case {
                op: GeometryOp::Draft {
                    target: h(1),
                    faces: vec![],
                    angle: r(0.1),
                    plane: h(2),
                },
                expected: Operation::ModifyDraft,
                label: "Draft → ModifyDraft",
            },
            Case {
                op: GeometryOp::Thicken {
                    target: h(1),
                    offset: r(0.001),
                },
                expected: Operation::ModifyThicken,
                label: "Thicken → ModifyThicken",
            },
            Case {
                op: GeometryOp::ZoneSlab {
                    target: h(1),
                    width: r(0.002),
                },
                expected: Operation::ModifyZoneSlab,
                label: "ZoneSlab → ModifyZoneSlab",
            },
            Case {
                op: GeometryOp::OffsetSolid {
                    target: h(1),
                    distance: r(0.002),
                },
                expected: Operation::ModifyOffsetSolid,
                label: "OffsetSolid → ModifyOffsetSolid",
            },
            Case {
                op: GeometryOp::OffsetCurve {
                    target: h(1),
                    distance: r(0.002),
                    reference: None,
                    direction: None,
                },
                expected: Operation::ModifyOffsetCurve,
                label: "OffsetCurve → ModifyOffsetCurve",
            },
            // Transform
            Case {
                op: GeometryOp::Translate {
                    target: h(1),
                    dx: 0.0,
                    dy: 0.0,
                    dz: 0.01,
                },
                expected: Operation::TransformTranslate,
                label: "Translate → TransformTranslate",
            },
            Case {
                op: GeometryOp::Rotate {
                    target: h(1),
                    axis: [0.0, 0.0, 1.0],
                    angle_rad: 0.5,
                },
                expected: Operation::TransformRotate,
                label: "Rotate → TransformRotate",
            },
            Case {
                op: GeometryOp::Scale {
                    target: h(1),
                    factor: 2.0,
                },
                expected: Operation::TransformScale,
                label: "Scale → TransformScale",
            },
            Case {
                op: GeometryOp::ScaleNonUniform {
                    target: h(1),
                    sx: 2.0,
                    sy: 1.0,
                    sz: 0.5,
                },
                expected: Operation::TransformScale,
                label: "ScaleNonUniform → TransformScale",
            },
            Case {
                op: GeometryOp::RotateAround {
                    target: h(1),
                    point: [0.0, 0.0, 0.0],
                    axis: [0.0, 0.0, 1.0],
                    angle_rad: 0.5,
                },
                expected: Operation::TransformRotateAround,
                label: "RotateAround → TransformRotateAround",
            },
            // Pattern
            Case {
                op: GeometryOp::LinearPattern {
                    target: h(1),
                    direction: [1.0, 0.0, 0.0],
                    count: 3,
                    spacing: r(0.01),
                },
                expected: Operation::PatternLinear,
                label: "LinearPattern → PatternLinear",
            },
            Case {
                op: GeometryOp::CircularPattern {
                    target: h(1),
                    axis_origin: [0.0, 0.0, 0.0],
                    axis_dir: [0.0, 0.0, 1.0],
                    count: 4,
                    angle: r(1.57),
                },
                expected: Operation::PatternCircular,
                label: "CircularPattern → PatternCircular",
            },
            Case {
                op: GeometryOp::Mirror {
                    target: h(1),
                    plane_origin: [0.0, 0.0, 0.0],
                    plane_normal: [1.0, 0.0, 0.0],
                },
                expected: Operation::PatternMirror,
                label: "Mirror → PatternMirror",
            },
            Case {
                op: GeometryOp::LinearPattern2D {
                    target: h(1),
                    direction1: [1.0, 0.0, 0.0],
                    count1: 3,
                    spacing1: r(0.01),
                    direction2: [0.0, 1.0, 0.0],
                    count2: 3,
                    spacing2: r(0.01),
                },
                expected: Operation::PatternLinear2D,
                label: "LinearPattern2D → PatternLinear2D",
            },
            Case {
                op: GeometryOp::ArbitraryPattern {
                    target: h(1),
                    transforms: vec![([1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0])],
                },
                expected: Operation::PatternArbitrary,
                label: "ArbitraryPattern → PatternArbitrary",
            },
            // Sweep (single-profile)
            Case {
                op: GeometryOp::Extrude {
                    profile: h(1),
                    distance: r(0.01),
                },
                expected: Operation::SweepExtrude,
                label: "Extrude → SweepExtrude",
            },
            Case {
                op: GeometryOp::ExtrudeSymmetric {
                    profile: h(1),
                    distance: r(0.01),
                },
                expected: Operation::SweepExtrudeSymmetric,
                label: "ExtrudeSymmetric → SweepExtrudeSymmetric",
            },
            Case {
                op: GeometryOp::ExtrudeInfinite {
                    profile: h(1),
                    axis: [0.0, 0.0, 1.0],
                    both: false,
                },
                expected: Operation::SweepExtrudeInfinite,
                label: "ExtrudeInfinite → SweepExtrudeInfinite",
            },
            Case {
                op: GeometryOp::Revolve {
                    profile: h(1),
                    axis_origin: [0.0, 0.0, 0.0],
                    axis_dir: [0.0, 0.0, 1.0],
                    angle_rad: 1.0,
                },
                expected: Operation::SweepRevolve,
                label: "Revolve → SweepRevolve",
            },
            Case {
                op: GeometryOp::Sweep {
                    profile: h(1),
                    path: h(2),
                },
                expected: Operation::SweepSweep,
                label: "Sweep → SweepSweep",
            },
            Case {
                op: GeometryOp::SweepGuided {
                    profile: h(1),
                    path: h(2),
                    guide: h(3),
                },
                expected: Operation::SweepSweepGuided,
                label: "SweepGuided → SweepSweepGuided",
            },
            Case {
                op: GeometryOp::Pipe {
                    path: h(1),
                    radius: r(0.005),
                },
                expected: Operation::SweepPipe,
                label: "Pipe → SweepPipe",
            },
            // Loft (multi-profile)
            Case {
                op: GeometryOp::Loft {
                    profiles: vec![h(1), h(2)],
                },
                expected: Operation::SweepLoft,
                label: "Loft → SweepLoft",
            },
            Case {
                op: GeometryOp::LoftGuided {
                    profiles: vec![h(1), h(2)],
                    guides: vec![h(3)],
                },
                expected: Operation::SweepLoftGuided,
                label: "LoftGuided → SweepLoftGuided",
            },
            // Curves
            Case {
                op: GeometryOp::LineSegment {
                    x1: 0.0,
                    y1: 0.0,
                    z1: 0.0,
                    x2: 1.0,
                    y2: 0.0,
                    z2: 0.0,
                },
                expected: Operation::CurveLineSegment,
                label: "LineSegment → CurveLineSegment",
            },
            Case {
                op: GeometryOp::Arc {
                    center: [0.0, 0.0, 0.0],
                    radius: 0.01,
                    start_angle: 0.0,
                    end_angle: 1.57,
                    axis: [0.0, 0.0, 1.0],
                },
                expected: Operation::CurveArc,
                label: "Arc → CurveArc",
            },
            Case {
                op: GeometryOp::Helix {
                    radius: 0.01,
                    pitch: 0.005,
                    height: 0.05,
                },
                expected: Operation::CurveHelix,
                label: "Helix → CurveHelix",
            },
            Case {
                op: GeometryOp::InterpCurve {
                    points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                },
                expected: Operation::CurveInterpCurve,
                label: "InterpCurve → CurveInterpCurve",
            },
            Case {
                op: GeometryOp::BezierCurve {
                    control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                },
                expected: Operation::CurveBezierCurve,
                label: "BezierCurve → CurveBezierCurve",
            },
            Case {
                op: GeometryOp::NurbsCurve {
                    control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                    weights: vec![1.0, 1.0],
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    degree: 1,
                },
                expected: Operation::CurveNurbsCurve,
                label: "NurbsCurve → CurveNurbsCurve",
            },
            // Profiles (task-4160)
            Case {
                op: GeometryOp::RectangleProfile {
                    width: r(0.02),
                    height: r(0.01),
                },
                expected: Operation::ProfileRectangle,
                label: "RectangleProfile → ProfileRectangle",
            },
            Case {
                op: GeometryOp::CircleProfile { radius: r(0.008) },
                expected: Operation::ProfileCircle,
                label: "CircleProfile → ProfileCircle",
            },
            // Profiles (task-4161)
            Case {
                op: GeometryOp::PolygonProfile {
                    points: vec![[0.0, 0.0], [0.01, 0.0], [0.01, 0.01], [0.0, 0.01]],
                },
                expected: Operation::ProfilePolygon,
                label: "PolygonProfile → ProfilePolygon",
            },
            Case {
                op: GeometryOp::EllipseProfile {
                    semi_major: r(0.010),
                    semi_minor: r(0.005),
                },
                expected: Operation::ProfileEllipse,
                label: "EllipseProfile → ProfileEllipse",
            },
            // NurbsSurface (task #4191)
            Case {
                op: GeometryOp::NurbsSurface {
                    control_points: vec![
                        vec![[0.0, 0.0, 0.0], [0.0, 0.01, 0.0]],
                        vec![[0.01, 0.0, 0.0], [0.01, 0.01, 0.005]],
                    ],
                    weights: vec![vec![1.0, 1.0], vec![1.0, 1.0]],
                    u_knots: vec![0.0, 0.0, 1.0, 1.0],
                    v_knots: vec![0.0, 0.0, 1.0, 1.0],
                    u_degree: 1,
                    v_degree: 1,
                },
                expected: Operation::SurfaceNurbs,
                label: "NurbsSurface → SurfaceNurbs",
            },
            // Previously missing from coverage (task 4671 step-1):
            Case {
                op: GeometryOp::ChamferAsymmetric {
                    target: h(1),
                    edges: vec![],
                    d1: r(0.001),
                    d2: r(0.002),
                },
                expected: Operation::ModifyChamfer,
                label: "ChamferAsymmetric → ModifyChamfer (reuses the ModifyChamfer capability)",
            },
            Case {
                op: GeometryOp::ApplyTransform {
                    target: h(1),
                    rotation: [1.0, 0.0, 0.0, 0.0],
                    translation: [0.0, 0.0, 0.0],
                },
                expected: Operation::TransformApplyTransform,
                label: "ApplyTransform → TransformApplyTransform",
            },
            Case {
                op: GeometryOp::AffineApply {
                    target: h(1),
                    linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                    translation: [0.0, 0.0, 0.0],
                },
                expected: Operation::TransformAffineApply,
                label: "AffineApply → TransformAffineApply",
            },
            // Surface (isosurface / marching-cubes, task 4999)
            Case {
                op: GeometryOp::Surface {
                    grid: h(1),
                    iso_level: 0.0,
                    adaptive: false,
                },
                expected: Operation::Surface,
                label: "Surface → Operation::Surface (isosurface / marching-cubes)",
            },
        ];

        for case in &cases {
            let got = geometry_op_to_operation(&case.op);
            assert_eq!(got, case.expected, "{} (got {got:?})", case.label);
        }

        // Coverage-completeness assertion: every non-Split GeometryOpDiscriminants
        // must appear exactly once in the cases table. Adding a new variant and
        // forgetting to add it here turns this into a RED test-time failure before
        // it could ever reach an unreachable!() in production (DD-3 model).
        let seen: HashSet<GeometryOpDiscriminants> =
            cases.iter().map(|c| GeometryOpDiscriminants::from(&c.op)).collect();
        let all_non_split: HashSet<GeometryOpDiscriminants> = GeometryOpDiscriminants::iter()
            .filter(|d| *d != GeometryOpDiscriminants::Split)
            .collect();
        assert_eq!(
            seen,
            all_non_split,
            "geometry_op_to_operation coverage gap — missing discriminants: {:?}",
            all_non_split.difference(&seen).collect::<Vec<_>>()
        );
    }

    // ── compiled_geometry_op_to_operation unit tests (task #4999, step-3 RED) ──

    /// `CompiledGeometryOp::Isosurface` must classify to `Operation::Surface`
    /// — the same coarse key `geometry_op_to_operation` assigns to the
    /// runtime-IR `GeometryOp::Surface`, keeping the compiled-IR and
    /// runtime-IR classifiers in agreement for the isosurface builtin.
    ///
    /// RED: `CompiledGeometryOp::Isosurface` does not exist yet.
    #[test]
    fn compiled_geometry_op_to_operation_isosurface_maps_to_surface() {
        let op = CompiledGeometryOp::Isosurface {
            grid: GeomRef::Step(0),
            args: vec![],
        };
        assert_eq!(
            compiled_geometry_op_to_operation(&op),
            Operation::Surface,
            "CompiledGeometryOp::Isosurface must classify as Operation::Surface"
        );
    }

    // ── plan_output_repr unit tests ──────────────────────────────────────────

    /// Pins the `plan_output_repr` produced-repr derivation helper
    /// (task ε / 3436, PRD §8 step-5/6).
    ///
    /// The helper takes a borrowed-view registry, a [`DispatchPlan`] (whose
    /// `kernel` field names the chosen kernel), and an [`Operation`], and
    /// returns the `ReprKind` that kernel produces for `op` — i.e. the second
    /// element of the matching entry in `descriptor.supports`. This is the
    /// value `execute_realization_ops` (step-10) will write into the
    /// realization graph node's `produced_repr` field.
    ///
    /// Two synthetic kernels exercise both reprs the v0.3 dispatcher recognises:
    /// (a) a BRep-native kernel supporting `(BooleanUnion, BRep)` → `BRep`,
    /// (b) a Mesh-native kernel supporting `(BooleanUnion, Mesh)` → `Mesh`.
    /// Each plan names exactly one kernel and contains zero conversions
    /// (the ε baseline; non-empty chains are deferred to ζ/η/θ).
    ///
    /// A third sub-case pins the `None` fallback when the named kernel does
    /// not support `op` for any repr — defensible against an invariant
    /// violation where dispatch is given an inconsistent registry.
    ///
    /// RED before step-6 impl: `plan_output_repr` does not exist yet.
    #[test]
    fn plan_output_repr_returns_kernel_descriptor_output_repr() {
        // (a) BRep-native kernel.
        let occt = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut brep_registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        brep_registry.insert("occt".to_string(), &occt);
        let brep_plan = DispatchPlan {
            kernel: "occt".to_string(),
            conversions: vec![],
        };
        assert_eq!(
            plan_output_repr(&brep_registry, &brep_plan, Operation::BooleanUnion),
            Some(ReprKind::BRep),
            "occt supports (BooleanUnion, BRep) → plan_output_repr must return BRep",
        );

        // (b) Mesh-native kernel.
        let manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut mesh_registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        mesh_registry.insert("manifold".to_string(), &manifold);
        let mesh_plan = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![],
        };
        assert_eq!(
            plan_output_repr(&mesh_registry, &mesh_plan, Operation::BooleanUnion),
            Some(ReprKind::Mesh),
            "manifold supports (BooleanUnion, Mesh) → plan_output_repr must return Mesh",
        );

        // (c) Defensive fallback: plan names a kernel whose descriptor has
        // no entry for the requested op. plan_output_repr must return None
        // so the caller (execute_realization_ops in step-10) can surface a
        // diagnostic rather than fabricate a repr.
        let empty = CapabilityDescriptor { supports: vec![] };
        let mut empty_registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        empty_registry.insert("empty".to_string(), &empty);
        let empty_plan = DispatchPlan {
            kernel: "empty".to_string(),
            conversions: vec![],
        };
        assert_eq!(
            plan_output_repr(&empty_registry, &empty_plan, Operation::BooleanUnion),
            None,
            "kernel with no matching supports entry → plan_output_repr must return None",
        );

        // (d) Plan kernel missing from registry — also None (defensive).
        let mut occt_only: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        occt_only.insert("occt".to_string(), &occt);
        let missing_plan = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![],
        };
        assert_eq!(
            plan_output_repr(&occt_only, &missing_plan, Operation::BooleanUnion),
            None,
            "plan.kernel absent from registry → plan_output_repr must return None",
        );
    }

    // ── compute_tessellation_budgets unit tests ───────────────────────────────

    /// Pins the return type of `compute_tessellation_budgets`:
    /// `Vec<Vec<f64>>` indexed `[template_idx][realization_idx]`.
    ///
    /// Two sub-scenarios share the same module fixture (1 template `EntityA`,
    /// 1 realization) and registry `{occt: [(BooleanUnion, BRep)]}`:
    ///
    /// (a) **No demanded tol / fallback path**: `demanded_tols[0][0]` is
    ///     `None` (no tolerance contributor) → helper falls back to
    ///     `effective_tessellation_tolerance(module)` (default `1e-4`) and
    ///     routes it through the v0.2 single-kernel registry which yields a
    ///     0-conversion plan → budget equals the fallback value.
    ///
    /// (b) **Seeded active-tolerance scope / Some-branch**: `EntityA` is
    ///     inserted into `active_tolerance_scope` with value `5e-7`.
    ///     Asserts (i) `demanded_b[0][0] == Some(5e-7)` — the scope entry
    ///     surfaces through the chain — and (ii) `budgets_b[0][0] == 5e-7`
    ///     bit-exactly — the v0.2 0-conversion DispatchPlan passes the
    ///     demand through `compute_realization_tolerance_budget` unchanged.
    #[test]
    fn compute_tessellation_budgets_returns_positionally_indexed_vec_of_vec() {
        use reify_core::ModulePath;
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        let checker = MockConstraintChecker::new();
        // `mut` required for sub-scenario (b) where we seed
        // `active_tolerance_scope` directly (crate-private field, accessible
        // from `mod tests` within the same crate).
        let mut engine = crate::Engine::new(Box::new(checker), None);

        let template_a = TopologyTemplateBuilder::new("EntityA")
            .realization("EntityA", 0, vec![])
            .build();
        let module = CompiledModuleBuilder::new(ModulePath::single("test_budgets"))
            .template(template_a)
            .build();

        let occt = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), occt);

        // ── (a) no demanded tol → fallback path ─────────────────────────────
        let demanded = engine.compute_demanded_tols(&module);
        let budgets: Vec<Vec<f64>> =
            engine.compute_tessellation_budgets(&module, &demanded, &registry);

        assert_eq!(
            budgets.len(),
            1,
            "outer Vec must have one entry per template"
        );
        assert_eq!(budgets[0].len(), 1, "template A has 1 realization");
        assert_eq!(
            budgets[0][0],
            Engine::effective_tessellation_tolerance(&module),
            "no demanded tol → falls back to module default; 0-conversion DispatchPlan \
             passes it through bit-exactly",
        );

        // ── (b) seeded active-tolerance scope → Some-branch ─────────────────
        //
        // Seed `active_tolerance_scope` (crate-private field) so that
        // `active_tolerance_for("EntityA")` returns `Some(5e-7)`.  This
        // drives `compute_demanded_tols` into `Some(5e-7)`, which in turn
        // drives `compute_tessellation_budgets` into the
        // `compute_realization_tolerance_budget` Some-branch.  Under the v0.2
        // single-kernel registry the dispatcher returns a 0-conversion
        // DispatchPlan, so `per_stage_tolerance_for_plan` passes the demanded
        // tolerance through unchanged — budget == demanded bit-exactly.
        engine
            .active_tolerance_scope
            .insert("EntityA".to_string(), 5e-7_f64);

        let demanded_b = engine.compute_demanded_tols(&module);
        let budgets_b: Vec<Vec<f64>> =
            engine.compute_tessellation_budgets(&module, &demanded_b, &registry);

        assert_eq!(
            demanded_b[0][0],
            Some(5e-7),
            "EntityA scope entry must surface as Some(5e-7) in demanded_tols[0][0] \
             (precondition for the Some-branch budget assertion below)",
        );
        assert_eq!(
            budgets_b[0][0], 5e-7,
            "0-conversion DispatchPlan: compute_realization_tolerance_budget must \
             pass the demanded tolerance through unchanged (bit-exact). \
             Demand: 5e-7",
        );
    }

    // ── compute_realization_tolerance_budget unit tests ───────────────────────

    /// Pins the new 3-arg signature of `compute_realization_tolerance_budget`:
    /// the caller supplies the `&HashSet<ReprKind>` rather than the helper
    /// synthesising it from `BUDGET_QUERY_TRIPLE_V02.2` on every call.
    ///
    /// Fixture: single-kernel registry with `{(BooleanUnion, BRep)}`, demand
    /// `1e-6`, available `{BRep}`. The v0.2 single-kernel registry yields a
    /// 0-conversion `DispatchPlan`, so `per_stage_tolerance_for_plan` returns
    /// the demanded tolerance bit-exactly.
    #[test]
    fn compute_realization_tolerance_budget_accepts_caller_supplied_available_set() {
        use reify_test_support::MockConstraintChecker;

        let checker = MockConstraintChecker::new();
        let engine = crate::Engine::new(Box::new(checker), None);

        let occt = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut single: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
        single.insert("occt".to_string(), occt);
        let registry_borrowed: BTreeMap<String, &CapabilityDescriptor> =
            single.iter().map(|(k, v)| (k.clone(), v)).collect();

        // Derive `available` from the same const that production code uses so a
        // future change to `BUDGET_QUERY_TRIPLE_V02.2` is caught here automatically.
        let available: HashSet<ReprKind> =
            Engine::BUDGET_QUERY_TRIPLE_V02.2.iter().copied().collect();
        // Verify the public helper returns the identical set — every external
        // consumer greps `budget_available_set`, so this folds the helper's
        // coverage into the same test that pins the const's contents.
        assert_eq!(
            Engine::budget_available_set(),
            available,
            "budget_available_set() must match BUDGET_QUERY_TRIPLE_V02.2 exactly; \
             if this fails, update all `budget_available_set` consumers",
        );
        let demand = 1e-6_f64;

        assert_eq!(
            engine.compute_realization_tolerance_budget(&registry_borrowed, &available, demand),
            demand,
            "single-kernel registry yields a 0-conversion DispatchPlan; \
             per_stage_tolerance_for_plan on an empty chain must return demanded_tol \
             bit-exactly. Demand: {demand}",
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

    // ── tessellate_from_values fail-fast indexing tests ───────────────────────

    /// Pins that an out-of-bounds `demanded_tols` lookup in
    /// `tessellate_from_values` is a panic, not a silent `None` fallback.
    ///
    /// Passes `demanded_tols = &[]` (empty slice) with a 1-template /
    /// 1-realization module.  After step 6 replaces the defensive
    /// `.get(t_idx).and_then(...).unwrap_or(None)` with direct slice indexing
    /// `demanded_tols[t_idx][r_idx]`, the first realization triggers an OOB
    /// panic.  Currently RED: the call returns silently because
    /// `demanded_tols.get(0)` returns `None` and `.unwrap_or(None)` swallows
    /// the missing entry.
    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn tessellate_from_values_panics_on_oob_demanded_tols_lookup() {
        use reify_test_support::mocks::MockGeometryKernel;

        let module = module_with_one_box_realization();
        // Task ε (3436): wrap the mock kernel into the new multi-handle map
        // under the synthetic default-kernel name. `default_kernel_name` is
        // threaded through as the resolution key the helper indexes by.
        let mut geometry_kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        geometry_kernels.insert(
            Engine::DEFAULT_KERNEL_NAME.to_string(),
            Box::new(MockGeometryKernel::new()),
        );
        let mut values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let mut realization_cache: RealizationCache<KernelHandle> = RealizationCache::new();

        // `demanded_tols = &[]` is the OOB trigger: the producer would have
        // generated `&[vec![None]]` for a 1-template/1-realization module, but
        // passing an empty slice causes `demanded_tols[0][...]` to panic.
        // `tessellation_budgets` is correctly shaped so we can confirm the
        // panic originates at the demanded_tol lookup, not the budget lookup.
        let desc = dispatch_test_descriptor_all_brep();
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert(Engine::DEFAULT_KERNEL_NAME.to_string(), &desc);
        let mut achieved_repr_tol = std::collections::BTreeMap::new();
        Engine::tessellate_from_values(
            &mut geometry_kernels,
            &registry,
            Some(Engine::DEFAULT_KERNEL_NAME),
            &module,
            &mut values,
            &functions,
            &mut diagnostics,
            &meta_map,
            &mut topology_attribute_table,
            &mut swept_kind_table,
            &mut realization_cache,
            &[],               // ← OOB: empty demanded_tols
            &[vec![1e-4_f64]], // correctly shaped tessellation_budgets
            &mut 0usize,
            &mut HashMap::new(),
            false,
            &mut achieved_repr_tol,
            None,              // unified_pass: LegacyMultiPass (no schedule)
            &std::collections::HashSet::new(), // realization_read_cells: empty
            None,              // demand_seed: full scope (not testing selective demand)
        );
    }

    /// Pins that an out-of-bounds `tessellation_budgets` lookup in
    /// `tessellate_from_values` is a panic, not a silent module-pragma fallback.
    ///
    /// Passes `tessellation_budgets = &[]` (empty slice) with a 1-template /
    /// 1-realization module and correctly-shaped `demanded_tols = &[vec![None]]`.
    /// After step 8 replaces the defensive `.get(t_idx).and_then(...).unwrap_or_else(...)`
    /// with direct slice indexing `tessellation_budgets[t_idx][r_idx]`, control
    /// reaches the budget lookup and panics.  Currently RED: the call returns
    /// silently with `budget = effective_tessellation_tolerance(module)` via the
    /// `unwrap_or_else` fallback.
    #[test]
    #[should_panic(expected = "index out of bounds: the len is 0 but the index is 0")]
    fn tessellate_from_values_panics_on_oob_tessellation_budgets_lookup() {
        use reify_test_support::mocks::MockGeometryKernel;

        let module = module_with_one_box_realization();
        // Task ε (3436): wrap the mock kernel into the multi-handle map under
        // the synthetic default-kernel name (sibling test mirror).
        let mut geometry_kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        geometry_kernels.insert(
            Engine::DEFAULT_KERNEL_NAME.to_string(),
            Box::new(MockGeometryKernel::new()),
        );
        let mut values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let mut realization_cache: RealizationCache<KernelHandle> = RealizationCache::new();

        // `demanded_tols` is correctly shaped; `tessellation_budgets = &[]` is
        // the OOB trigger.  The Box primitive in module_with_one_box_realization
        // produces at least one handle after `execute_realization_ops`, so
        // the `if step_handles.len() > handle_start` guard at line 1276 is true
        // and execution reaches the budget lookup.
        let desc = dispatch_test_descriptor_all_brep();
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert(Engine::DEFAULT_KERNEL_NAME.to_string(), &desc);
        let mut achieved_repr_tol = std::collections::BTreeMap::new();
        Engine::tessellate_from_values(
            &mut geometry_kernels,
            &registry,
            Some(Engine::DEFAULT_KERNEL_NAME),
            &module,
            &mut values,
            &functions,
            &mut diagnostics,
            &meta_map,
            &mut topology_attribute_table,
            &mut swept_kind_table,
            &mut realization_cache,
            &[vec![None]], // correctly shaped demanded_tols
            &[],           // ← OOB: empty tessellation_budgets
            &mut 0usize,
            &mut HashMap::new(),
            false,
            &mut achieved_repr_tol,
            None,          // unified_pass: LegacyMultiPass (no schedule)
            &std::collections::HashSet::new(), // realization_read_cells: empty
            None,          // demand_seed: full scope (not testing selective demand)
        );
    }

    // ── collect_centroids_with_failure_summary unit tests ─────────────────────

    /// All handles produce kernel query errors → exactly one coalesced warning
    /// naming the count, the realization_id, and the first error message.
    #[test]
    fn collect_centroids_with_failure_summary_coalesces_query_errors() {
        use reify_core::Severity;
        use reify_ir::Role;
        use reify_test_support::mocks::MockGeometryKernel;

        let realization_id = RealizationNodeId::new("TestEntity", 0);
        let feature_id = FeatureId::from(&realization_id);

        let attr0 = TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        };
        let attr1 = TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index: 1,
            user_label: None,
            mod_history: Vec::new(),
        };
        let h0 = GeometryHandleId(101);
        let h1 = GeometryHandleId(102);
        let realization_attrs: Vec<(GeometryHandleId, &TopologyAttribute)> =
            vec![(h0, &attr0), (h1, &attr1)];

        // No centroid fixtures → query() returns QueryError::QueryFailed for both handles.
        let kernel = MockGeometryKernel::new();

        // Capture the actual error text the kernel will produce for h0 so that
        // the assertion below is decoupled from MockGeometryKernel's exact message
        // format — a mock cleanup won't break this test.
        let expected_first_err = kernel
            .query(&GeometryQuery::Centroid(h0))
            .unwrap_err()
            .to_string();

        let (centroids, diagnostics) =
            collect_centroids_with_failure_summary(&realization_attrs, &kernel, &realization_id);

        assert!(
            centroids.is_empty(),
            "expected no successful centroids when all queries fail, got: {centroids:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 coalesced warning, got {}: {diagnostics:?}",
            diagnostics.len()
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            Severity::Warning,
            "diagnostic must be a Warning, got: {diag:?}"
        );
        assert!(
            diag.message
                .contains("topology-attribute centroid query failed for 2 handle(s)"),
            "message must contain the count phrase, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("TestEntity#realization[0]"),
            "message must contain the realization_id display form, got: {}",
            diag.message
        );
        // Assert the first error's text is preserved verbatim using the sentinel
        // captured above — decoupled from the mock's internal format.
        assert!(
            diag.message
                .contains(&format!("(first: {expected_first_err}")),
            "message must embed the first error text, got: {}",
            diag.message
        );
    }

    /// Both handles produce `Ok(Value::Real(0.0))` from the kernel, which
    /// `parse_xyz_value` rejects as a non-string value → exactly one coalesced
    /// parse-fail warning, no query-fail warning.
    #[test]
    fn collect_centroids_with_failure_summary_coalesces_parse_errors() {
        use reify_core::Severity;
        use reify_ir::{Role, Value};
        use reify_test_support::mocks::MockGeometryKernel;

        let realization_id = RealizationNodeId::new("TestEntity", 0);
        let feature_id = FeatureId::from(&realization_id);

        let attr0 = TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        };
        let attr1 = TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index: 1,
            user_label: None,
            mod_history: Vec::new(),
        };
        let h0 = GeometryHandleId(101);
        let h1 = GeometryHandleId(102);
        let realization_attrs: Vec<(GeometryHandleId, &TopologyAttribute)> =
            vec![(h0, &attr0), (h1, &attr1)];

        // Value::Real is not a string → parse_xyz_value returns Err for both.
        let kernel = MockGeometryKernel::new()
            .with_centroid_result(h0, Value::Real(0.0))
            .with_centroid_result(h1, Value::Real(0.0));

        let (centroids, diagnostics) =
            collect_centroids_with_failure_summary(&realization_attrs, &kernel, &realization_id);

        assert!(
            centroids.is_empty(),
            "expected no successful centroids when all parses fail, got: {centroids:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 coalesced parse-fail warning, got {}: {diagnostics:?}",
            diagnostics.len()
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            Severity::Warning,
            "diagnostic must be a Warning, got: {diag:?}"
        );
        assert!(
            diag.message
                .contains("topology-attribute centroid parse failed for 2 handle(s)"),
            "message must contain the parse-fail count phrase, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("TestEntity#realization[0]"),
            "message must contain the realization_id display form, got: {}",
            diag.message
        );
        // Assert that the first-error text is present and contains the locally-
        // owned query label ("local_index_reassignment_centroid" is defined in
        // engine_build.rs and passed to parse_xyz_value — stable regardless of
        // how QueryError formats its Display prefix).
        assert!(
            diag.message.contains("(first: "),
            "message must embed the first parse-error text, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("local_index_reassignment_centroid"),
            "first-error text must name the query label, got: {}",
            diag.message
        );
    }

    /// Mixed failure classes: one query-error handle (201), one parse-error
    /// handle (202), one success handle (203). Asserts:
    ///   - centroids map has exactly the success handle's xyz
    ///   - exactly two warnings: one per failure class
    ///   - each warning names the FIRST handle of its class (201 / 202)
    ///   - the parse-fail warning does NOT appear in the query-fail warning
    ///     and vice-versa (classes are separated)
    #[test]
    fn collect_centroids_with_failure_summary_separates_failure_classes_and_preserves_first_message()
     {
        use reify_core::Severity;
        use reify_ir::{Role, Value};
        use reify_test_support::mocks::MockGeometryKernel;

        let realization_id = RealizationNodeId::new("TestEntity", 0);
        let feature_id = FeatureId::from(&realization_id);

        let make_attr = |local_index: u32| TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index,
            user_label: None,
            mod_history: Vec::new(),
        };
        let attr0 = make_attr(0); // handle 201 — no kernel fixture → query Err
        let attr1 = make_attr(1); // handle 202 — Real(0.0) → parse Err
        let attr2 = make_attr(2); // handle 203 — valid xyz JSON → success

        let h_err = GeometryHandleId(201);
        let h_parse = GeometryHandleId(202);
        let h_ok = GeometryHandleId(203);

        // Construct in deterministic order so "first message" is well-defined.
        let realization_attrs: Vec<(GeometryHandleId, &TopologyAttribute)> =
            vec![(h_err, &attr0), (h_parse, &attr1), (h_ok, &attr2)];

        let kernel = MockGeometryKernel::new()
            // h_err: no fixture → returns QueryError::QueryFailed("no mock result …")
            .with_centroid_result(h_parse, Value::Real(0.0))
            .with_centroid_result(
                h_ok,
                Value::String("{\"x\":1.5,\"y\":2.5,\"z\":3.5}".into()),
            );

        // Capture the actual error text for h_err so the assertion below is
        // decoupled from MockGeometryKernel's exact message format.
        let expected_query_err = kernel
            .query(&GeometryQuery::Centroid(h_err))
            .unwrap_err()
            .to_string();

        let (centroids, diagnostics) =
            collect_centroids_with_failure_summary(&realization_attrs, &kernel, &realization_id);

        // Success handle returns the parsed xyz.
        assert_eq!(
            centroids.len(),
            1,
            "exactly one successful centroid expected, got: {centroids:?}"
        );
        assert_eq!(
            centroids.get(&h_ok),
            Some(&[1.5_f64, 2.5, 3.5]),
            "centroids map must hold the success handle's xyz"
        );

        // Two warnings — one per failure class.
        assert_eq!(
            diagnostics.len(),
            2,
            "expected exactly 2 warnings (one per failure class), got {}: {diagnostics:?}",
            diagnostics.len()
        );
        assert!(
            diagnostics.iter().all(|d| d.severity == Severity::Warning),
            "all diagnostics must be Warnings, got: {diagnostics:?}"
        );

        // Find the query-fail warning and the parse-fail warning.
        let query_warn = diagnostics
            .iter()
            .find(|d| d.message.contains("centroid query failed"))
            .expect("must have a query-fail warning");
        let parse_warn = diagnostics
            .iter()
            .find(|d| d.message.contains("centroid parse failed"))
            .expect("must have a parse-fail warning");

        // Query-fail warning: count=1, first error text matches sentinel
        // captured from the kernel before the call — decoupled from mock format.
        assert!(
            query_warn
                .message
                .contains("centroid query failed for 1 handle(s)"),
            "query-fail count must be 1, got: {}",
            query_warn.message
        );
        assert!(
            query_warn
                .message
                .contains(&format!("(first: {expected_query_err}")),
            "query-fail first must contain the captured error text, got: {}",
            query_warn.message
        );

        // Parse-fail warning: count=1, first-error text names the locally-owned
        // query label ("local_index_reassignment_centroid") — stable regardless
        // of how QueryError formats its Display prefix.
        assert!(
            parse_warn
                .message
                .contains("centroid parse failed for 1 handle(s)"),
            "parse-fail count must be 1, got: {}",
            parse_warn.message
        );
        assert!(
            parse_warn.message.contains("(first: "),
            "parse-fail message must embed the first error text, got: {}",
            parse_warn.message
        );
        assert!(
            parse_warn
                .message
                .contains("local_index_reassignment_centroid"),
            "parse-fail first-error must name the query label, got: {}",
            parse_warn.message
        );
    }

    // ── probe_realization_cache direct unit tests (task 5059 η / INV-BUILD-3) ──
    //
    // `Engine::probe_realization_cache` is the extracted cache-hit
    // short-circuit that used to live inline in `execute_realization_ops`
    // (task 2874 step-8; extracted task 5059 η, zero behavior change). These
    // tests drive the helper directly — bypassing the op loop entirely — to
    // pin its contract (PRD §5.1 / INV-BUILD-3) as a reviewable unit.

    /// step-1 (RED): a primary cache hit must apply the FULL side-effect set
    /// documented on `probe_realization_cache` — not just push a handle.
    ///
    /// Pre-seeds a `RealizationCache` at `(entity, BRep, tol)` with a known
    /// handle, then probes at the SAME repr/tol with the terminal+named+tol
    /// guard satisfied. Asserts every write the cold path's success branch
    /// would have made: `step_handles`, `named_steps`, `named_step_reprs`,
    /// and `produced_repr_out` — plus the `Some(CacheHit)` return itself.
    #[test]
    fn probe_realization_cache_primary_hit_applies_full_side_effect_set() {
        let realization_id = RealizationNodeId::new("PrimaryHit", 0);
        let tol = 1e-4_f64;
        let seeded = KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(7),
        };

        let mut cache = RealizationCache::<KernelHandle>::new();
        cache.insert(
            &realization_id.entity,
            ReprKind::BRep,
            tol,
            NO_OPTIONS,
            seeded,
        );

        let mut step_handles: Vec<KernelHandle> = Vec::new();
        let mut named_steps: HashMap<String, KernelHandle> = HashMap::new();
        let mut named_step_reprs: HashMap<String, ReprKind> = HashMap::new();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let mut produced_repr_out: Option<ReprKind> = None;

        let mut outputs = RealizationOutputs::new(
            &mut step_handles,
            &mut named_steps,
            &mut named_step_reprs,
            &mut topology_attribute_table,
            &mut swept_kind_table,
            &mut produced_repr_out,
        );

        let hit = Engine::probe_realization_cache(
            &cache,
            &realization_id,
            Some("part"),
            ReprKind::BRep,
            Some(tol),
            true,
            &mut outputs,
        );

        assert_eq!(
            hit,
            Some(CacheHit {
                handle: seeded,
                resolved_repr: ReprKind::BRep,
            }),
            "primary hit must report the seeded handle at the demanded (BRep) repr"
        );
        assert_eq!(
            step_handles,
            vec![seeded],
            "cached handle must be pushed onto step_handles"
        );
        assert_eq!(
            named_steps.get("part"),
            Some(&seeded),
            "named_steps[name] must resolve to the cached handle"
        );
        assert_eq!(
            named_step_reprs.get("part"),
            Some(&ReprKind::BRep),
            "named_step_reprs[name] must record the resolved repr"
        );
        assert_eq!(
            produced_repr_out,
            Some(ReprKind::BRep),
            "produced_repr_out must surface the resolved repr"
        );
    }

    /// step-3: pin invariant #1 (probes ONLY when `is_terminal_realization &&
    /// demanded_tol.is_some() && realization_name.is_some()`).
    ///
    /// Pre-seeds the cache at `(entity, BRep, tol)` — the exact key a real hit
    /// would probe — and pre-seeds `topology_attribute_table` at the cached
    /// handle's id (so an accidental unconditional eviction would also be
    /// caught). Exercises the three ways to leave exactly one guard term
    /// unmet; each must miss with `None` AND perform ZERO side effects — the
    /// guard wraps the ENTIRE hit body, so a failed guard must not push/insert
    /// into any output view or evict the attribute table.
    #[test]
    fn probe_realization_cache_guard_requires_terminal_named_and_tol() {
        use reify_ir::{FeatureId, Role};

        let realization_id = RealizationNodeId::new("GuardEntity", 0);
        let tol = 1e-4_f64;
        let seeded = KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(9),
        };

        // Each case leaves exactly one of the three guard terms unmet.
        let cases: [(bool, Option<&str>, Option<f64>, &str); 3] = [
            (false, Some("part"), Some(tol), "is_terminal_realization = false"),
            (true, None, Some(tol), "realization_name = None"),
            (true, Some("part"), None, "demanded_tol = None"),
        ];

        for (is_terminal, name, tol_arg, label) in cases {
            let mut cache = RealizationCache::<KernelHandle>::new();
            cache.insert(
                &realization_id.entity,
                ReprKind::BRep,
                tol,
                NO_OPTIONS,
                seeded,
            );

            let mut step_handles: Vec<KernelHandle> = Vec::new();
            let mut named_steps: HashMap<String, KernelHandle> = HashMap::new();
            let mut named_step_reprs: HashMap<String, ReprKind> = HashMap::new();
            let mut topology_attribute_table = TopologyAttributeTable::default();
            topology_attribute_table.record(
                seeded.id,
                TopologyAttribute {
                    feature_id: FeatureId::from(&realization_id),
                    role: Role::Side,
                    local_index: 0,
                    user_label: None,
                    mod_history: Vec::new(),
                },
            );
            let mut swept_kind_table = SweptKindTable::default();
            let mut produced_repr_out: Option<ReprKind> = None;

            let mut outputs = RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut named_step_reprs,
                &mut topology_attribute_table,
                &mut swept_kind_table,
                &mut produced_repr_out,
            );

            let hit = Engine::probe_realization_cache(
                &cache,
                &realization_id,
                name,
                ReprKind::BRep,
                tol_arg,
                is_terminal,
                &mut outputs,
            );

            assert_eq!(hit, None, "guard case [{label}] must miss (return None)");
            assert!(
                step_handles.is_empty(),
                "guard case [{label}] must not push step_handles"
            );
            assert!(
                named_steps.is_empty(),
                "guard case [{label}] must not insert named_steps"
            );
            assert!(
                named_step_reprs.is_empty(),
                "guard case [{label}] must not insert named_step_reprs"
            );
            assert_eq!(
                produced_repr_out, None,
                "guard case [{label}] must not write produced_repr_out"
            );
            assert!(
                topology_attribute_table.lookup(seeded.id).is_some(),
                "guard case [{label}] must NOT evict the pre-seeded \
                 topology_attribute_table entry (eviction only runs on a hit)"
            );
        }
    }

    // ── Task 4349: cross-kernel GeometryHandleId collision regression tests ─────

    /// Shared scaffolding for the two cross-kernel `GeometryHandleId` collision
    /// regression tests (task 4349).
    ///
    /// Builds `DispatchTestState`, pre-seeds the realization cache with
    /// `{Occt, GeometryHandleId(1)}`, calls the `pre_seed` closure (so each
    /// test can populate its own table at `GeometryHandleId(1)` to simulate the
    /// colliding sibling op), then drives the cache-hit short-circuit via
    /// `state.run_demand` with `operations=&[]`. Returns the state after the
    /// call for post-condition assertions.
    fn run_cross_kernel_cache_hit_short_circuit(
        entity_name: &str,
        pre_seed: impl FnOnce(&mut DispatchTestState, &RealizationNodeId),
    ) -> DispatchTestState {
        use reify_test_support::mocks::MockGeometryKernel;

        let realization_id = RealizationNodeId::new(entity_name, 0);
        let tol = 1e-4_f64;

        let desc = dispatch_test_descriptor_all_brep();
        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let registry = dispatch_test_single_default_registry(&desc);

        let mut state = DispatchTestState::default();

        // Pre-seed the cache: the prior build stored {Occt, GeometryHandleId(1)}
        // as the terminal handle for this entity.
        state.realization_cache.insert(
            &realization_id.entity,
            ReprKind::BRep,
            tol,
            NO_OPTIONS,
            KernelHandle {
                kernel: KernelId::Occt,
                id: GeometryHandleId(1),
            },
        );

        // Allow each test to pre-seed its specific table before the run.
        pre_seed(&mut state, &realization_id);

        // Drive the cache-hit short-circuit: operations=&[] ensures the function
        // returns BEFORE the op loop. demanded_tol=Some(tol) and
        // realization_name=Some("part") together enable the cache probe path.
        state.run_demand(
            &mut kernels,
            &registry,
            "default",
            &[], // empty ops — cache-hit fires before the op loop
            &realization_id,
            Some("part"),
            SourceSpan::new(0, 0),
            ReprKind::BRep,
            Some(tol),
            None,
        );

        state
    }

    /// Regression test for cross-kernel `GeometryHandleId` collision at the
    /// cache-hit short-circuit — `topology_attribute_table` path (task 4349).
    ///
    /// # Background
    ///
    /// OCCT and Manifold both mint `GeometryHandleId(1)` for their first
    /// geometry handle (each kernel's counter starts at 1). Within a single
    /// build a Manifold op can record
    /// `topology_attribute_table.record(GeometryHandleId(1), attr)` while a
    /// later cache-hit short-circuit returns the cached
    /// `{Occt, GeometryHandleId(1)}` from a prior build. The former
    /// `debug_assert!(topology_attribute_table.lookup(cached_handle.id).is_none())`
    /// fires because key `GeometryHandleId(1)` is occupied by the Manifold
    /// entry — two distinct `KernelHandle`s collapsing onto one kernel-blind key.
    ///
    /// # Invariant (build-mode independent)
    ///
    /// After the cache-hit short-circuit, `topology_attribute_table.lookup(id)`
    /// for the cached handle must return `None` — regardless of debug vs release
    /// build mode.  The `None` post-condition is the meaningful guarantee; in
    /// debug builds the former `debug_assert!` also fired before the fix, but
    /// the test's value is not limited to that panic path.
    #[test]
    fn cache_hit_short_circuit_tolerates_cross_kernel_topology_attribute_id_collision() {
        use reify_ir::{FeatureId, Role};

        let state = run_cross_kernel_cache_hit_short_circuit(
            "CrossKernelEntity2",
            |state, realization_id| {
                // Pre-seed topology_attribute_table at GeometryHandleId(1),
                // simulating a cross-kernel sibling Mesh op that recorded its
                // first handle's attribute earlier in this same build.
                state.topology_attribute_table.record(
                    GeometryHandleId(1),
                    TopologyAttribute {
                        feature_id: FeatureId::from(realization_id),
                        role: Role::Side,
                        local_index: 0,
                        user_label: None,
                        mod_history: Vec::new(),
                    },
                );
            },
        );

        // Post-condition: the cached handle must read None from topology_attribute_table.
        assert!(
            state
                .topology_attribute_table
                .lookup(GeometryHandleId(1))
                .is_none(),
            "topology_attribute_table must have no entry for the cached handle id \
             after cache-hit short-circuit: cross-kernel sibling's colliding entry \
             must be removed (not left behind as a foreign kernel's attribute)"
        );
    }

    // ── step-1 (task 4538): pass-ordering regression test ─────────────────────

    /// Regression guard (task 4538): `run_post_processes` must populate `mp`
    /// with real mass-props when the body is a selector-produced
    /// `Value::GeometryHandle`.
    ///
    /// Before task 4538 this test would have failed: `post_process_body_mass_props`
    /// ran before the selector passes, so `sel_body` was still `Value::Undef` when
    /// the mass-props pass read it → body arg had no geometry handle → all three
    /// geometric fields (`mass`/`com`/`inertia`) stayed `Value::Undef`.
    /// The reorder (step-2) placed the selector passes first; this test now guards
    /// the corrected order — a future re-reordering would immediately fail here.
    ///
    /// A SANITY PRECONDITION (`post_process_topology_selectors` on a clone)
    /// verifies that the selector expression is correctly constructed; a RED
    /// failure in the MAIN assertion is therefore unambiguously about ordering.
    ///
    /// Template:
    ///   `sel_body` = `single(edges(s))` — a selector-produced geometry handle
    ///   `mp`       = `body_mass_props(sel_body, rho)` — reads sel_body
    ///
    /// MockGeometryKernel:
    ///   `extract_edges(parent_id)` → `[edge_id]`    (one edge → single() unwraps)
    ///   `Volume(edge_id)` → `Real(3.0)`              (mass = 2000 × 3 = 6000)
    ///   `CenterOfMass(edge_id, 2000.0)` → JSON CoM
    ///   `InertiaTensor(edge_id, 2000.0)` → nested list inertia
    #[test]
    fn run_post_processes_selector_produced_body_gets_real_mass_props() {
        use reify_core::{ContentHash, DimensionVector, RealizationNodeId, Type, ValueCellId};
        use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, Value};
        use reify_test_support::{builders::TopologyTemplateBuilder, mocks::MockGeometryKernel};

        // ── geometry-handle fixture IDs ───────────────────────────────────────
        let parent_id = GeometryHandleId(100);
        let edge_id = GeometryHandleId(101);
        let parent_rr = RealizationNodeId::new("Design", 0);
        let parent_hash: [u8; 32] = [0xAA; 32];

        // ── value-cell IDs ────────────────────────────────────────────────────
        let s_cell = ValueCellId::new("Design", "s");
        let sel_body_cell = ValueCellId::new("Design", "sel_body");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mp_cell = ValueCellId::new("Design", "mp");

        // ── local helper: build a one-arg FunctionCall CompiledExpr ──────────
        //
        // Follows the pattern of `call_expr` in dynamics_ops::tests:674 and
        // `topology_selector_call_one_value_ref` in geometry_ops::tests:11837.
        fn one_arg_call(fn_name: &str, arg: CompiledExpr, result_type: Type) -> CompiledExpr {
            let content_hash = ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
                .combine(ContentHash::of_str(fn_name))
                .combine(arg.content_hash);
            CompiledExpr {
                kind: CompiledExprKind::FunctionCall {
                    function: ResolvedFunction {
                        name: fn_name.to_string(),
                        qualified_name: fn_name.to_string(),
                    },
                    args: vec![arg],
                },
                result_type,
                content_hash,
            }
        }

        // ── local helper: build a two-arg FunctionCall CompiledExpr ──────────
        fn two_arg_call(
            fn_name: &str,
            a1: CompiledExpr,
            a2: CompiledExpr,
            result_type: Type,
        ) -> CompiledExpr {
            let content_hash = ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
                .combine(ContentHash::of_str(fn_name))
                .combine(a1.content_hash)
                .combine(a2.content_hash);
            CompiledExpr {
                kind: CompiledExprKind::FunctionCall {
                    function: ResolvedFunction {
                        name: fn_name.to_string(),
                        qualified_name: fn_name.to_string(),
                    },
                    args: vec![a1, a2],
                },
                result_type,
                content_hash,
            }
        }

        // ── default_expr for sel_body: single(edges(s)) ──────────────────────
        //
        // `edges(s)` → FunctionCall("edges", [ValueRef(s_cell, Geometry)])
        //   `try_eval_topology_selector` returns Value::Selector(Edge, All)
        //
        // `single(edges(s))` → FunctionCall("single", [edges_expr])
        //   `try_eval_resolve_selector` matches the `single` arm (geometry_ops
        //   :3031), resolves the inner selector via `resolve_selector_to_list`,
        //   gets a one-element list, and unwraps the sole Value::GeometryHandle.
        //
        // Note: the inner arg is a bare FunctionCall (not wrapped in
        // ResolveSelector) — handled by the "Defensive" arm at geometry_ops:3037.
        let s_vref = CompiledExpr::value_ref(s_cell.clone(), Type::Geometry);
        let edges_expr = one_arg_call("edges", s_vref, Type::List(Box::new(Type::Geometry)));
        let single_edges_expr = one_arg_call("single", edges_expr, Type::Geometry);

        // ── default_expr for mp: body_mass_props(sel_body, rho) ──────────────
        //
        // Mirrors the call_expr helper in dynamics_ops::tests:674.  The body
        // arg is a ValueRef to sel_body — which starts as Undef (no
        // GeometryHandle) and is patched to a GeometryHandle by the selector
        // pass if the ordering is correct.
        let sel_body_vref = CompiledExpr::value_ref(sel_body_cell.clone(), Type::Geometry);
        let rho_vref = CompiledExpr::value_ref(rho_cell.clone(), Type::dimensionless_scalar());
        let mp_expr = two_arg_call(
            "body_mass_props",
            sel_body_vref,
            rho_vref,
            Type::StructureRef("MassProperties".to_string()),
        );

        // ── TopologyTemplate: two Let cells ──────────────────────────────────
        //
        // `sel_body` — post_process_topology_selectors patches this Undef →
        //              GeometryHandle{edge_id} via try_eval_resolve_selector
        // `mp`       — post_process_body_mass_props reads sel_body and
        //              assembles the MassProperties instance
        //
        // `s` and `rho` are seeded directly in the ValueMap; the selector and
        // mass-props passes read them from `values` without needing template
        // cells (only cells with default_expr are iterated by the passes).
        let template = TopologyTemplateBuilder::new("Design")
            .let_binding(
                "Design",
                "sel_body",
                Type::Geometry,
                single_edges_expr.clone(),
            )
            .let_binding(
                "Design",
                "mp",
                Type::StructureRef("MassProperties".to_string()),
                mp_expr,
            )
            .build();

        // ── initial ValueMap ──────────────────────────────────────────────────
        // sel_body and mp start as Undef (the pure eval_expr left them there);
        // the post-process passes must promote them to real values.
        let mut values = ValueMap::new();
        values.insert(
            s_cell.clone(),
            Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_id),
            },
        );
        values.insert(sel_body_cell.clone(), Value::Undef);
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 2000.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
        );
        values.insert(mp_cell.clone(), Value::Undef);

        // ── MockGeometryKernel fixture ────────────────────────────────────────
        // Volume = 3.0 m³ → expected mass = 2000.0 × 3.0 = 6000.0 kg
        // CoM injected as JSON; inertia as nested list with distinct diagonal.
        let injected_com = Value::String("{\"x\":0.01,\"y\":0.02,\"z\":0.03}".to_string());
        let injected_inertia = Value::List(vec![
            Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
        ]);
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(parent_id, vec![edge_id])
            .with_volume_result(edge_id, Value::Real(3.0))
            .with_center_of_mass_result(edge_id, 2000.0, injected_com)
            .with_inertia_tensor_result(edge_id, 2000.0, injected_inertia);

        let named_steps: HashMap<String, KernelHandle> = HashMap::new();
        let functions: Vec<CompiledFunction> = Vec::new();
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let table = TopologyAttributeTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        // ── SANITY PRECONDITION ───────────────────────────────────────────────
        //
        // Run `post_process_topology_selectors` alone on a fresh clone to
        // confirm that the single(edges(s)) expression is correctly built and
        // resolves to a Value::GeometryHandle{edge_id}. If this assertion fires,
        // the bug is in the selector expression itself; if it passes, any
        // failure in the MAIN assertion below is unambiguously about ordering.
        {
            let mut values_clone = values.clone();
            let mut kernel2 =
                MockGeometryKernel::new().with_extracted_edges(parent_id, vec![edge_id]);
            let mut diags2: Vec<Diagnostic> = Vec::new();
            Engine::post_process_topology_selectors(
                &template,
                &named_steps,
                &mut values_clone,
                &mut kernel2 as &mut dyn GeometryKernel,
                &table,
                &HashMap::new(),
                &mut diags2,
            );
            let patched = values_clone
                .get(&sel_body_cell)
                .expect("sel_body must be present after post_process_topology_selectors");
            assert!(
                matches!(
                    patched,
                    Value::GeometryHandle { kernel_handle, .. }
                        if *kernel_handle == Some(edge_id)
                ),
                "SANITY: post_process_topology_selectors must patch sel_body to \
                 GeometryHandle{{kernel_handle: {edge_id:?}}}; got: {patched:?}"
            );
        }

        // ── MAIN ASSERTION ────────────────────────────────────────────────────
        //
        // Before task 4538 (post_process_body_mass_props ran BEFORE selectors):
        //   body_mass_props read sel_body = Undef → no handle → mp's mass/
        //   com/inertia fields stayed Undef — this assertion would have failed.
        //
        // Fixed order (task 4538 / step-2):
        //   post_process_topology_selectors runs first → sel_body becomes
        //   GeometryHandle{edge_id} → body_mass_props queries the kernel →
        //   mass = density × Volume = 2000.0 × 3.0 = 6000.0.
        Engine::run_post_processes(
            &template,
            &named_steps,
            &mut values,
            &functions,
            &meta_map,
            &mut kernel as &mut dyn GeometryKernel,
            &table,
            &SweptKindTable::default(),
            &HashMap::new(),
            &mut diagnostics,
            &[],
        );

        let mp_val = values
            .get(&mp_cell)
            .expect("mp must be present in values after run_post_processes");
        let data = match mp_val {
            Value::StructureInstance(d) => d,
            other => panic!(
                "mp must be a MassProperties StructureInstance after \
                 run_post_processes; got {other:?}"
            ),
        };
        assert_eq!(data.type_name, "MassProperties");

        // `mass` must not be Undef: this is the ordering-contract assertion.
        // On the RED path the selector pass hasn't run yet so there is no
        // GeometryHandle → mass stays Undef.
        let mass_field = data
            .fields
            .get("mass")
            .expect("MassProperties must have a `mass` field");
        assert!(
            !matches!(mass_field, Value::Undef),
            "ordering regression: `post_process_body_mass_props` ran before the \
             selector passes populated `sel_body` (task 4538 fix). \
             Expected mass = density × volume = 2000.0 × 3.0 = 6000.0; \
             got: {mass_field:?}"
        );
        let mass = match mass_field {
            Value::Scalar { si_value, .. } => *si_value,
            Value::Real(m) => *m,
            other => panic!("mass must be a numeric Scalar or Real; got {other:?}"),
        };
        assert!(
            (mass - 6000.0_f64).abs() < 1e-9,
            "mass = density × volume = 2000.0 × 3.0 = 6000.0; got {mass}"
        );

        // CoM and inertia: assert non-Undef (real kernel values).
        let com_field = data
            .fields
            .get("com")
            .expect("MassProperties must have a `com` field");
        assert!(
            !matches!(com_field, Value::Undef),
            "com must not be Undef after run_post_processes; got {com_field:?}"
        );

        let inertia_field = data
            .fields
            .get("inertia")
            .expect("MassProperties must have an `inertia` field");
        let inertia = crate::dynamics_psd::inertia_3x3_from_value(inertia_field)
            .expect("inertia must parse as 3×3 via inertia_3x3_from_value");
        assert!(
            (inertia[0][0] - 1.0).abs() < 1e-9,
            "inertia[0][0] must be 1.0; got {}",
            inertia[0][0]
        );
        assert!(
            (inertia[1][1] - 2.0).abs() < 1e-9,
            "inertia[1][1] must be 2.0; got {}",
            inertia[1][1]
        );
        assert!(
            (inertia[2][2] - 3.0).abs() < 1e-9,
            "inertia[2][2] must be 3.0; got {}",
            inertia[2][2]
        );

        // Explicit density arg → no E_DynamicsNoDensity error.
        assert!(
            diagnostics.iter().all(|d| {
                !matches!(d.code, Some(reify_core::DiagnosticCode::DynamicsNoDensity))
            }),
            "explicit density must not emit E_DynamicsNoDensity; \
             diagnostics: {diagnostics:?}"
        );
    }

    /// Regression guard (task 4538, direct-body path): a body cell that already
    /// holds a `Value::GeometryHandle` before `run_post_processes` runs must
    /// still produce real mass-props in the new (last) ordering.
    ///
    /// The reorder moved `post_process_body_mass_props` to the end of
    /// `run_post_processes`; this test confirms the common pre-existing case
    /// (a directly let-bound body, not produced by a selector) is unaffected —
    /// real values arrive regardless of whether mass-props runs first or last
    /// relative to the selector passes.
    #[test]
    fn run_post_processes_direct_body_gets_real_mass_props() {
        use reify_core::{ContentHash, DimensionVector, RealizationNodeId, Type, ValueCellId};
        use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, Value};
        use reify_test_support::{builders::TopologyTemplateBuilder, mocks::MockGeometryKernel};

        let body_id = GeometryHandleId(200);
        let body_rr = RealizationNodeId::new("Design", 0);
        let body_hash: [u8; 32] = [0xBBu8; 32];

        let body_cell = ValueCellId::new("Design", "body");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mp_cell = ValueCellId::new("Design", "mp");

        // ── two-arg helper (mirrors the one in the selector-produced test) ────
        fn two_arg_call(
            fn_name: &str,
            a1: CompiledExpr,
            a2: CompiledExpr,
            result_type: Type,
        ) -> CompiledExpr {
            let content_hash = ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
                .combine(ContentHash::of_str(fn_name))
                .combine(a1.content_hash)
                .combine(a2.content_hash);
            CompiledExpr {
                kind: CompiledExprKind::FunctionCall {
                    function: ResolvedFunction {
                        name: fn_name.to_string(),
                        qualified_name: fn_name.to_string(),
                    },
                    args: vec![a1, a2],
                },
                result_type,
                content_hash,
            }
        }

        // ── default_expr for mp: body_mass_props(body, rho) ──────────────────
        let body_vref = CompiledExpr::value_ref(body_cell.clone(), Type::Geometry);
        let rho_vref = CompiledExpr::value_ref(rho_cell.clone(), Type::dimensionless_scalar());
        let mp_expr = two_arg_call(
            "body_mass_props",
            body_vref,
            rho_vref,
            Type::StructureRef("MassProperties".to_string()),
        );

        // Only `mp` needs a template cell; body and rho are seeded in the
        // ValueMap and read directly by `post_process_body_mass_props`.
        let template = TopologyTemplateBuilder::new("Design")
            .let_binding(
                "Design",
                "mp",
                Type::StructureRef("MassProperties".to_string()),
                mp_expr,
            )
            .build();

        let mut values = ValueMap::new();
        values.insert(
            body_cell.clone(),
            Value::GeometryHandle {
                realization_ref: body_rr,
                upstream_values_hash: body_hash,
                kernel_handle: Some(body_id),
            },
        );
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 2000.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
        );
        values.insert(mp_cell.clone(), Value::Undef);

        // Volume = 5.0 m³ → expected mass = 2000.0 × 5.0 = 10000.0 kg
        let injected_com = Value::String("{\"x\":0.1,\"y\":0.2,\"z\":0.3}".to_string());
        let injected_inertia = Value::List(vec![
            Value::List(vec![Value::Real(4.0), Value::Real(0.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(5.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(6.0)]),
        ]);
        let mut kernel = MockGeometryKernel::new()
            .with_volume_result(body_id, Value::Real(5.0))
            .with_center_of_mass_result(body_id, 2000.0, injected_com)
            .with_inertia_tensor_result(body_id, 2000.0, injected_inertia);

        let named_steps: HashMap<String, KernelHandle> = HashMap::new();
        let functions: Vec<CompiledFunction> = Vec::new();
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let table = TopologyAttributeTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        Engine::run_post_processes(
            &template,
            &named_steps,
            &mut values,
            &functions,
            &meta_map,
            &mut kernel as &mut dyn GeometryKernel,
            &table,
            &SweptKindTable::default(),
            &HashMap::new(),
            &mut diagnostics,
            &[],
        );

        let mp_val = values
            .get(&mp_cell)
            .expect("mp must be present after run_post_processes");
        let data = match mp_val {
            Value::StructureInstance(d) => d,
            other => panic!(
                "direct body: mp must be a MassProperties StructureInstance; \
                 got {other:?}"
            ),
        };
        assert_eq!(data.type_name, "MassProperties");

        let mass_field = data
            .fields
            .get("mass")
            .expect("MassProperties must have a `mass` field");
        let mass = match mass_field {
            Value::Scalar { si_value, .. } => *si_value,
            Value::Real(m) => *m,
            other => panic!("direct body: mass must be a numeric Scalar or Real; got {other:?}"),
        };
        assert!(
            (mass - 10_000.0_f64).abs() < 1e-9,
            "direct body: mass = density × volume = 2000.0 × 5.0 = 10000.0; \
             got {mass}"
        );

        let com_field = data
            .fields
            .get("com")
            .expect("MassProperties must have a `com` field");
        assert!(
            !matches!(com_field, Value::Undef),
            "direct body: com must not be Undef after run_post_processes; \
             got {com_field:?}"
        );
    }

    // ── parse_bbox_midpoint unit tests (task #4734 amendment) ────────────────────

    /// parse_bbox_midpoint: midpoint of a known bounding box.
    #[test]
    fn parse_bbox_midpoint_happy_path() {
        use reify_ir::Value;
        let payload = Value::String(
            r#"{"xmin":0.0,"ymin":0.0,"zmin":0.0,"xmax":10.0,"ymax":20.0,"zmax":30.0}"#
                .to_string(),
        );
        let mid = parse_bbox_midpoint(&payload).unwrap();
        assert_eq!(mid, [5.0, 10.0, 15.0]);
    }

    /// parse_bbox_midpoint: non-string value → Err.
    #[test]
    fn parse_bbox_midpoint_non_string_value() {
        use reify_ir::{QueryError, Value};
        let result = parse_bbox_midpoint(&Value::Bool(true));
        assert!(
            matches!(result, Err(QueryError::QueryFailed(_))),
            "expected QueryFailed for non-string value"
        );
    }

    /// parse_bbox_midpoint: missing brace wrapper → Err.
    #[test]
    fn parse_bbox_midpoint_malformed_json_no_braces() {
        use reify_ir::{QueryError, Value};
        let payload = Value::String("xmin:0.0,xmax:10.0".to_string());
        let result = parse_bbox_midpoint(&payload);
        assert!(
            matches!(result, Err(QueryError::QueryFailed(_))),
            "expected QueryFailed for missing braces"
        );
    }

    /// parse_bbox_midpoint: missing a required axis key → Err.
    #[test]
    fn parse_bbox_midpoint_missing_axis_key() {
        use reify_ir::{QueryError, Value};
        // Missing zmax
        let payload = Value::String(
            r#"{"xmin":0.0,"ymin":0.0,"zmin":0.0,"xmax":10.0,"ymax":20.0}"#.to_string(),
        );
        let result = parse_bbox_midpoint(&payload);
        assert!(
            matches!(result, Err(QueryError::QueryFailed(_))),
            "expected QueryFailed for missing zmax"
        );
    }
