//! Which export-format spellings the GUI export surfaces accept, and what each spelling reaches.

use std::path::Path;

use reify_ir::ExportFormat;

use crate::commands::{export_impl, parse_export_format};

/// One accepted spelling per format. No wildcard arm: a new `ExportFormat`
/// variant does not compile here until it is given a spelling.
fn a_spelling_of(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Step => "step",
        ExportFormat::Stl => "stl",
        ExportFormat::Obj => "obj",
        ExportFormat::ThreeMF => "3mf",
    }
}

fn assert_written(path: &Path) {
    let written = std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("the export target {} must exist: {e}", path.display()));
    assert!(written.len() > 0, "the exported file must not be empty");
}

#[test]
fn parse_export_format_names_every_export_format() {
    for format in [
        ExportFormat::Step,
        ExportFormat::Stl,
        ExportFormat::Obj,
        ExportFormat::ThreeMF,
    ] {
        let spelling = a_spelling_of(format);
        assert_eq!(
            parse_export_format(spelling),
            Ok(format),
            "the spelling {spelling:?} must name {format:?}"
        );
    }
    assert_eq!(parse_export_format("stp"), Ok(ExportFormat::Step));
}

#[test]
fn parse_export_format_refuses_an_unknown_spelling_by_name() {
    let err = parse_export_format("dxf").expect_err("an unknown spelling must be refused");
    assert!(
        err.contains("dxf"),
        "the refusal must name the offending spelling, got: {err}"
    );
}

#[test]
fn export_impl_writes_a_3mf_export() {
    let engine = crate::tests::make_test_engine();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bracket.3mf");

    export_impl(&engine, "3mf", path.to_str().unwrap()).expect("export_impl('3mf') must succeed");

    assert_written(&path);
}

#[test]
fn mcp_tool_context_export_writes_a_3mf_export() {
    use reify_mcp::ReifyToolContext;

    let ctx =
        crate::mcp_context::TauriToolContext::builder(crate::tests::make_test_engine()).build();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bracket.3mf");

    assert_eq!(
        ReifyToolContext::export(&ctx, "3mf", path.to_str().unwrap()),
        Ok(true)
    );

    assert_written(&path);
}

#[cfg(feature = "gui")]
#[tokio::test]
async fn reify_export_tool_writes_a_3mf_export() {
    const DESIGN_SOURCE: &str = r#"structure def Part {
    param width: Length = 80mm

    let body = box(width, width, width)
}"#;

    // The tool confines its target to the open design's directory, so the
    // engine must be launched from a file in that directory.
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("part.ri");
    std::fs::write(&part, DESIGN_SOURCE).expect("write part.ri");
    let canonical = std::fs::canonicalize(&part).expect("canonical part.ri");
    let engine = crate::tests::make_test_engine();
    crate::engine_lock::with_engine_lock(&engine, |s| s.load_file(&canonical))
        .and_then(std::convert::identity)
        .expect("load_file launch must succeed");
    let s0 = crate::engine_lock::with_engine_lock(&engine, |s| s.build_gui_state())
        .and_then(std::convert::identity)
        .expect("build_gui_state must succeed");
    let last_state = std::sync::Mutex::new(Some(s0));
    let target = canonical.with_extension("3mf");

    crate::debug_server::reify_export_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        "3mf",
        target.to_str().unwrap(),
    )
    .await
    .expect("reify_export('3mf') must succeed");

    assert_written(&target);
}

#[cfg(feature = "gui")]
mod production_kernel {
    //! Only the production kernel tells formats apart: `MockGeometryKernel`
    //! writes the same bytes whichever format reaches it.

    use std::sync::Mutex;

    use reify_constraints::SimpleConstraintChecker;

    use crate::commands::export_impl;
    use crate::engine::EngineSession;

    /// The ZIP local-file-header magic that opens every 3MF (OPC) package.
    const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

    fn occt_box_engine() -> Option<Mutex<EngineSession>> {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("Skipping: OCCT not available in this build (stub mode)");
            return None;
        }
        let mut session = EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker));
        session
            .load_from_source(
                "structure S { let b = box(10mm, 10mm, 10mm) }",
                "export_format_box",
            )
            .expect("load_from_source should succeed with the registered OCCT kernel");
        Some(Mutex::new(session))
    }

    #[test]
    fn production_kernel_writes_3mf_as_a_zip_package() {
        let Some(engine) = occt_box_engine() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("box.3mf");

        export_impl(&engine, "3mf", path.to_str().unwrap())
            .expect("export_impl('3mf') must succeed");

        let data = std::fs::read(&path).expect("the 3MF export must be readable");
        assert!(
            data.starts_with(ZIP_MAGIC),
            "a 3MF export must be a ZIP package, got leading bytes {:?}",
            &data[..data.len().min(8)]
        );
    }

    #[test]
    fn production_kernel_refuses_obj_and_writes_nothing() {
        let Some(engine) = occt_box_engine() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("box.obj");

        let err = export_impl(&engine, "obj", path.to_str().unwrap())
            .expect_err("no production kernel writes OBJ today");

        assert!(
            !err.starts_with("Unknown export format"),
            "'obj' must reach the kernel, not be refused as an unknown spelling, got: {err}"
        );
        assert!(
            err.contains("Obj"),
            "the kernel's refusal must name the format it cannot write, got: {err}"
        );
        assert!(
            !path.exists(),
            "a refused export must not leave a file behind"
        );
    }
}
