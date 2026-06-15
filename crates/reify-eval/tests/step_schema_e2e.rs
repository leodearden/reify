//! io-export ε (task 4288) — B8 end-to-end acceptance: a STEPOutput
//! occurrence's DSL `version : STEPVersion` selects the written OCCT STEP
//! schema, observable in the produced bytes' FILE_SCHEMA.
//!
//! This drives the FULL chain through real components — DSL `version`
//! → [`extract_output_export_spec`] → [`reify_ir::ExportOptions`]
//! → `OcctKernelHandle::export_with_options` → OCCT `write.step.schema` — that
//! the mock-kernel unit tests in `engine_build.rs` / `tolerance_combine.rs`
//! cannot reach (they stop at the kernel seam). It is guarded on
//! [`reify_kernel_occt::OCCT_AVAILABLE`] so it is a no-op when this build was
//! compiled without a linked OCCT.
//!
//! Schema identifiers are the actual OCCT EXPRESS names (verified against the
//! linked OCCT 7.9.3): AP203 → `CONFIG_CONTROL_DESIGN`, AP214 (the DSL default)
//! → `AUTOMOTIVE_DESIGN`. OCCT never writes the literal token "AP203", so the
//! EXPRESS schema name is the user-observable signal.

use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};

/// Two `STEPOutput` occurrences on the same `box` solid — one declaring
/// `version: STEPVersion.AP203`, one leaving `version` at its DSL default
/// (`STEPVersion.AP214`) — must produce two STEP artifacts whose written
/// FILE_SCHEMA differs: the AP203 file carries `CONFIG_CONTROL_DESIGN` and the
/// default file carries `AUTOMOTIVE_DESIGN`. This proves the declared version,
/// not a hardcoded default, reached the real OCCT writer.
#[test]
fn step_version_selects_occt_step_schema_end_to_end() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping step_version_selects_occt_step_schema_end_to_end: OCCT not available");
        return;
    }

    let module = parse_and_compile_with_stdlib(
        r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub a = STEPOutput(subject: part, version: STEPVersion.AP203, path: "ap203.step")
    sub d = STEPOutput(subject: part, path: "def.step")
}"#,
    );

    // Real OCCT, behind the Send+Sync actor handle (its GeometryKernel trait
    // override threads ExportOptions; the inherent OcctKernel impl would only be
    // reached via the trait's default method, which ignores options).
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );

    // A unique per-run temp dir (auto-removed when `out_dir` drops) rather than
    // a shared, never-cleaned hardcoded `/tmp` path. `build_outputs` uses it
    // only to resolve the occurrences' relative paths and the assertions below
    // read the in-memory `artifact.bytes`, but a unique dir keeps repeated and
    // concurrent test runs from racing on a world-shared destination. Keep
    // `out_dir` bound through the assertions so the directory outlives the call.
    let out_dir = tempfile::tempdir().expect("create a unique temp dir for the e2e exports");
    let artifacts = engine.build_outputs(&module, out_dir.path(), None);

    // Locate each artifact by its resolved destination filename.
    let find = |suffix: &str| -> &reify_eval::ExportArtifact {
        artifacts
            .iter()
            .find(|a| a.path.ends_with(suffix))
            .unwrap_or_else(|| {
                panic!(
                    "no ExportArtifact for `{suffix}`; produced paths were {:?}",
                    artifacts.iter().map(|a| a.path.clone()).collect::<Vec<_>>()
                )
            })
    };
    let ap203 = find("ap203.step");
    let default = find("def.step");

    let ap203_str =
        String::from_utf8(ap203.bytes.clone()).expect("AP203 STEP bytes must be valid UTF-8");
    let default_str =
        String::from_utf8(default.bytes.clone()).expect("default STEP bytes must be valid UTF-8");

    assert!(
        !ap203_str.is_empty() && !default_str.is_empty(),
        "both STEP exports must have written bytes (AP203 {} B, default {} B)",
        ap203_str.len(),
        default_str.len()
    );

    // The declared AP203 version selected the AP203 EXPRESS schema.
    assert!(
        ap203_str.contains("CONFIG_CONTROL_DESIGN"),
        "the `version: STEPVersion.AP203` occurrence must write the AP203 \
         CONFIG_CONTROL_DESIGN EXPRESS schema"
    );
    assert!(
        !ap203_str.contains("AUTOMOTIVE_DESIGN"),
        "the AP203 occurrence must NOT write the AP214 AUTOMOTIVE_DESIGN schema"
    );

    // The default-version occurrence wrote the AP214 schema.
    assert!(
        default_str.contains("AUTOMOTIVE_DESIGN"),
        "the default-version STEPOutput must write the AP214 AUTOMOTIVE_DESIGN schema"
    );

    // End-to-end proof the schema really varied with the DSL `version`.
    assert_ne!(
        ap203_str, default_str,
        "the AP203 and default-AP214 STEP files must differ in their FILE_SCHEMA"
    );
}
