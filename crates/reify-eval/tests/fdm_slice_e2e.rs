// SPDX-License-Identifier: AGPL-3.0-or-later

//! End-to-end stdlib + engine test for the `fdm::slice` ComputeNode (task η /
//! 3789, slice 2 of `docs/prds/v0_5/fdm-as-printed-fea.md`, step-19).
//!
//! Compiles + builds an inline `.ri` that calls `fdm_slice(body, FDMProcess())`
//! through a real OCCT-backed engine, asserting the user-observable signal
//! "FDMSlice on a body emits a Toolpath":
//!
//!   * the stdlib `fdm_slice(body, process, options) -> Toolpath` surface
//!     type-checks — `parse_and_compile_with_stdlib` panics on any compile error,
//!     so reaching the engine at all proves the `-> Toolpath` surface resolved;
//!   * the call lowers to a ComputeNode whose `target == "fdm::slice"` (the proven
//!     `@optimized` dispatch path, not body inlining — mirrors the δ
//!     `dynamics::inverse_dynamics` assertion);
//!   * the node's output cell holds a `Toolpath` `StructureInstance` Value; and
//!   * in this slicer-absent environment the node degrades honestly — an empty
//!     `Toolpath` plus a `Severity::Info` W_FDM_SLICER_UNAVAILABLE diagnostic
//!     (`DiagnosticCode::FdmSlicerUnavailable`), never an error (PRD open Q4).
//!     The "emits a Toolpath" signal holds even without a slicer.
//!
//! A second, skip-if-absent verify-and-lock test runs the REAL slicer twice on a
//! fixed body+settings and asserts an identical `Toolpath` (the e2e-observable
//! proxy for "byte-identical G-code" — identical G-code ⇒ identical parsed
//! Toolpath, since the ζ parser is pure/deterministic). When PrusaSlicer is not on
//! `$PATH` it `eprintln`-skips, mirroring `reify-kernel-gmsh/tests/rpath_smoke.rs`.
//!
//! Both tests gate on `reify_kernel_occt::OCCT_AVAILABLE`: the `body = box(...)`
//! realizes through OCCT before the body-consuming ComputeNode re-dispatches
//! (mirrors `as_printed_body_realization_e2e.rs`). OCCT is present in this
//! environment via `/opt/reify-deps`.
//!
//! RED until step-20 populates `crates/reify-compiler/stdlib/fdm_slice.ri` with the
//! `Toolpath` / `Bead` / `Layer` / `BeadRole` / `FDMSliceOptions` defs + the
//! `fdm_slice` `@optimized("fdm::slice")` surface; until then `fdm_slice` is an
//! unresolved name and `parse_and_compile_with_stdlib` panics on the compile error.

use reify_core::{DiagnosticCode, DimensionVector, Severity, ValueCellId};
use reify_eval::compute_targets::fdm_slice::toolpath_to_value;
use reify_eval::compute_targets::register_compute_fns;
use reify_ir::{ExportFormat, Value};

/// Inline FDM box: a `Solid` body fed to `fdm_slice` with a default `FDMProcess`.
/// Mirrors the `as_printed_body_realization_e2e.rs` / `fdm_bracket_e2e.rs` body
/// shape (`box(40mm, 40mm, 10mm)` + `FDMProcess()`).
const FDM_SLICE_SOURCE: &str = r#"
structure FdmSliceBox {
    let body = box(40mm, 40mm, 10mm)
    let toolpath = fdm_slice(body, FDMProcess())
}
"#;

/// The `FdmSliceBox.toolpath` value cell — the `fdm_slice` ComputeNode output.
fn toolpath_cell() -> ValueCellId {
    ValueCellId::new("FdmSliceBox", "toolpath")
}

/// Build a fresh `Engine` backed by a real OCCT kernel (mirrors
/// `as_printed_body_realization_e2e.rs::make_occt_engine`).
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Is a real PrusaSlicer binary discoverable on `$PATH`? Uses the SAME discovery
/// the trampoline uses (`discover_slicer` + `DEFAULT_SLICER_NAMES`), so this
/// answers exactly "did the engine just run a real slicer or take the degraded
/// path" for this process.
fn slicer_on_path() -> bool {
    let path_var = std::env::var("PATH").unwrap_or_default();
    reify_fdm::discover_slicer(&path_var, reify_fdm::DEFAULT_SLICER_NAMES).is_some()
}

/// Read the `beads` field of a `Toolpath` `StructureInstance` value as a slice,
/// asserting the value is a `Toolpath` structure carrying a `beads` List.
fn toolpath_beads(tp: &Value) -> &[Value] {
    let fields = match tp {
        Value::StructureInstance(d) => {
            assert_eq!(
                d.type_name, "Toolpath",
                "fdm_slice output must be a `Toolpath` StructureInstance, got type_name {}",
                d.type_name
            );
            &d.fields
        }
        other => panic!("fdm_slice output must be a StructureInstance, got {other:?}"),
    };
    match fields.get("beads") {
        Some(Value::List(items)) => items,
        other => panic!("Toolpath must carry a `beads` List field, got {other:?}"),
    }
}

/// Compile + build `FDM_SLICE_SOURCE` through a fresh OCCT engine and return the
/// `(toolpath value, full diagnostics)` pair. The toolpath value is read from the
/// post-build snapshot (the as-printed-realization-e2e idiom); diagnostics are the
/// `BuildResult`'s, which fold in the compute-node `Completed { diagnostics }` via
/// `build → check → eval` (so a degraded-slice Info diagnostic surfaces here).
fn build_toolpath() -> (Value, Vec<reify_core::Diagnostic>) {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(FDM_SLICE_SOURCE);

    let mut engine = make_occt_engine();
    register_compute_fns(&mut engine);
    let build = engine.build(&compiled, ExportFormat::Step);

    // The call must lower to a ComputeNode targeting "fdm::slice" — the
    // `@optimized` dispatch path, not body inlining.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after build()")
        .snapshot
        .clone();
    let slice_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, data)| data.target == "fdm::slice");
    assert!(
        slice_node.is_some(),
        "expected a ComputeNode with target==\"fdm::slice\" in the graph \
         (the @optimized dispatch path), found compute nodes: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| &d.target)
            .collect::<Vec<_>>()
    );

    let (tp_value, _det) = snapshot
        .values
        .get(&toolpath_cell())
        .unwrap_or_else(|| panic!("FdmSliceBox.toolpath value cell not found in snapshot"));
    (tp_value.clone(), build.diagnostics)
}

/// Primary e2e: `fdm_slice(body, FDMProcess())` type-checks, dispatches to the
/// `fdm::slice` ComputeNode, and emits a `Toolpath` value. In this slicer-absent
/// environment it degrades honestly — empty beads + a single `Severity::Info`
/// `FdmSlicerUnavailable` diagnostic, never an error (PRD open Q4).
#[test]
fn fdm_slice_dispatches_to_compute_node_and_emits_toolpath() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping fdm_slice_dispatches_to_compute_node_and_emits_toolpath: OCCT not available"
        );
        return;
    }

    let (toolpath, diagnostics) = build_toolpath();

    // "FDMSlice on a body emits a Toolpath" — holds regardless of slicer presence.
    let beads = toolpath_beads(&toolpath);

    // No `fdm::slice` diagnostic may ever be an error (PRD open Q4: absence is
    // informational). This invariant holds slicer-present OR slicer-absent.
    let fdm_unavailable: Vec<&reify_core::Diagnostic> = diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::FdmSlicerUnavailable))
        .collect();
    for d in &fdm_unavailable {
        assert_eq!(
            d.severity,
            Severity::Info,
            "W_FDM_SLICER_UNAVAILABLE must be Severity::Info, never an error; got {d:?}"
        );
    }

    if slicer_on_path() {
        // A real slicer ran — the determinism lock (other test) owns this path.
        // Here we only confirm the degraded diagnostic was NOT spuriously emitted.
        assert!(
            fdm_unavailable.is_empty(),
            "a slicer is on PATH, so no FdmSlicerUnavailable diagnostic should be emitted; \
             got {fdm_unavailable:?}"
        );
    } else {
        // Slicer-absent (this environment): the node degrades honestly.
        assert!(
            beads.is_empty(),
            "with no slicer on PATH the node must emit an EMPTY (degraded) Toolpath; \
             got {} beads",
            beads.len()
        );
        assert!(
            !fdm_unavailable.is_empty(),
            "the slicer-absent build must surface ≥1 W_FDM_SLICER_UNAVAILABLE Info diagnostic; \
             all diagnostics: {diagnostics:#?}"
        );
    }
}

/// Verify-and-lock (skip-if-absent): with a real PrusaSlicer on `$PATH`, two
/// independent builds (fresh engines, no shared warm-state) must produce an
/// identical `Toolpath` — the e2e-observable proxy for byte-identical G-code
/// (identical G-code ⇒ identical parsed Toolpath; the ζ parser is deterministic,
/// and `compose_slicer_args` pins `--threads 1` + a fixed flag order). When no
/// slicer is present it `eprintln`-skips (mirrors `rpath_smoke.rs`).
#[test]
fn real_slicer_build_is_deterministic_verify_and_lock() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping real_slicer_build_is_deterministic_verify_and_lock: OCCT not available"
        );
        return;
    }
    if !slicer_on_path() {
        eprintln!(
            "PrusaSlicer not on PATH; skipping the real-slicer verify-and-lock determinism check"
        );
        return;
    }

    let (toolpath1, _d1) = build_toolpath();
    let (toolpath2, _d2) = build_toolpath();

    // A present slicer produces a non-degraded Toolpath with beads.
    assert!(
        !toolpath_beads(&toolpath1).is_empty(),
        "a real slice of a solid box must yield a non-empty Toolpath"
    );
    // Determinism lock: two real-slicer runs → identical parsed Toolpath value.
    assert_eq!(
        toolpath1, toolpath2,
        "the real slicer must be deterministic: two builds must produce an identical Toolpath \
         (byte-identical G-code → identical Toolpath)"
    );
}

// ── Declared-unit regime (task #6301) ───────────────────────────────────────

/// Compile `src` with the stdlib and return its Error-severity diagnostics as
/// `(code, message)` pairs. Uses `compile_source_with_stdlib` (which does NOT
/// assert the absence of compile errors) so the negative half below can inspect
/// the errors rather than panicking on them.
fn compile_errors(src: &str) -> Vec<(Option<DiagnosticCode>, String)> {
    let module = reify_test_support::compile_source_with_stdlib(src);
    reify_test_support::errors_only(&module)
        .into_iter()
        .map(|d| (d.code, d.message.clone()))
        .collect()
}

/// Assert `src` compiles with no Error-severity diagnostics.
fn assert_compiles_clean(src: &str, what: &str) {
    let errors = compile_errors(src);
    assert!(
        errors.is_empty(),
        "{what}: expected a clean compile, got errors: {errors:?}"
    );
}

/// Assert `src` is REJECTED with a `ParamDefaultTypeMismatch` whose message
/// renders the initializer's type as `rendered`.
///
/// The escape hatch for types the binding check itself does not descend into:
/// the rejection message spells the initializer type out in full
/// (`Point3<Scalar[m]>` vs `Point3<Real>`), so a deliberate wrong-dimension
/// binding turns an otherwise-invisible element type into an observable string.
fn assert_rejection_renders_initializer_type(src: &str, rendered: &str, what: &str) {
    let errors = compile_errors(src);
    assert!(
        errors.iter().any(|(code, msg)| *code
            == Some(DiagnosticCode::ParamDefaultTypeMismatch)
            && msg.contains(rendered)),
        "{what}: expected a ParamDefaultTypeMismatch naming the initializer type \
         `{rendered}`, got: {errors:?}"
    );
}

/// Assert `src` is REJECTED with at least one `ParamDefaultTypeMismatch`.
fn assert_param_default_type_mismatch(src: &str, what: &str) {
    let errors = compile_errors(src);
    assert!(
        errors
            .iter()
            .any(|(code, _)| *code == Some(DiagnosticCode::ParamDefaultTypeMismatch)),
        "{what}: expected a ParamDefaultTypeMismatch error, got: {errors:?}"
    );
}

/// The stdlib `Bead` / `Layer` field types declare the SAME unit regime that
/// `toolpath_to_value` marshals into — SI and dimensioned.
///
/// This is the enforcement of `fdm_slice.ri`'s "Field-type ↔ marshalling
/// contract" header, which until now asserted that the declarations and the
/// marshaller "MUST stay aligned" with nothing actually checking it.
///
/// # Reach (what this test does and does not pin)
///
/// The SCALAR fields — `width` / `height` / `layer_z` / `speed` /
/// `nominal_temp` / `Layer.z` — are pinned directly and in both directions: the
/// positive half fails if a field stops being its own dimension, the negative
/// half fails if it reverts to bare `Real`.
///
/// `centerline` is NOT reachable that way. The `ParamDefaultTypeMismatch`
/// mechanism does not inspect List element types or `Point3` component types,
/// so `param c : List<Int> = Bead().centerline` and `param p : Point3<Real> =
/// Bead().centerline[0]` BOTH compile clean — a revert to `List<Point3<Real>>`
/// would sail past any binding-shaped assertion. What is expressible is the
/// type the compiler *renders* in a deliberate wrong-dimension rejection, so
/// that is the observation point used for it below. The marshaller-side
/// counterpart is `fdm_slice.rs`'s `assert_point3_length`.
///
/// Pure compile-level: no OCCT, no PrusaSlicer, no beads — so unlike the two
/// tests above it carries no `OCCT_AVAILABLE` / `slicer_on_path` guard and runs
/// in every environment.
///
/// # Why this shape
///
/// The obvious spelling — passing a unit-bearing literal to a constructor,
/// `Bead(width: 0.45mm)` — has NO discriminating power here: structure-
/// construction arguments are not dimension-checked, so even a flagrantly wrong
/// `Flatness(tolerance_value: 0.45kg)` compiles clean. Binding a field to a
/// declared param IS checked (`ParamDefaultTypeMismatch`), so reading the field
/// back out into a typed param is what actually observes its declared type.
#[test]
fn stdlib_bead_and_layer_fields_declare_the_si_dimensioned_regime() {
    // Positive half: each field binds cleanly to a param of its own dimension.
    // RED while the fields are declared `Real`, GREEN once they name their unit.
    assert_compiles_clean(
        "structure P { param w : Length = Bead().width }",
        "Bead.width is Length",
    );
    assert_compiles_clean(
        "structure P { param h : Length = Bead().height }",
        "Bead.height is Length",
    );
    assert_compiles_clean(
        "structure P { param z : Length = Bead().layer_z }",
        "Bead.layer_z is Length",
    );
    assert_compiles_clean(
        "structure P { param s : Velocity = Bead().speed }",
        "Bead.speed is Velocity",
    );
    assert_compiles_clean(
        "structure P { param t : Temperature = Bead().nominal_temp }",
        "Bead.nominal_temp is Temperature",
    );
    assert_compiles_clean(
        "structure P { param z : Length = Layer().z }",
        "Layer.z is Length",
    );

    // The dimensionless-by-nature fields stay `Int` — the regime covers the
    // dimensional fields, it does not sweep up the indices.
    assert_compiles_clean(
        "structure P { param i : Int = Bead().layer_index }",
        "Bead.layer_index stays Int",
    );
    assert_compiles_clean(
        "structure P { param i : Int = Layer().index }",
        "Layer.index stays Int",
    );

    // Negative half — the half that actually closes the silent bare-number
    // surface. Binding a now-dimensioned field to a bare `Real` param must be
    // REJECTED; this assertion fails if someone later reverts a field to `Real`.
    assert_param_default_type_mismatch(
        "structure P { param w : Real = Bead().width }",
        "Bead.width must no longer satisfy a bare Real param",
    );
    assert_param_default_type_mismatch(
        "structure P { param s : Real = Bead().speed }",
        "Bead.speed must no longer satisfy a bare Real param",
    );
    assert_param_default_type_mismatch(
        "structure P { param t : Real = Bead().nominal_temp }",
        "Bead.nominal_temp must no longer satisfy a bare Real param",
    );
    assert_param_default_type_mismatch(
        "structure P { param z : Real = Layer().z }",
        "Layer.z must no longer satisfy a bare Real param",
    );

    // Mechanism control: a WRONG-dimension binding is rejected, and stays
    // rejected either way. Without this, a compiler that silently accepted
    // everything would make the positive half above pass vacuously.
    assert_param_default_type_mismatch(
        "structure P { param m : Mass = Bead().width }",
        "a Mass param must never accept a Bead width",
    );

    // `centerline` — the one field whose type actually gates usability
    // (`resolve_point3_length_arg` rejects bare-`Real` components). Neither
    // binding half above can see it, so it is pinned by the RENDERED type in a
    // wrong-dimension rejection instead: `Point3<Scalar[m]>` today, which a
    // revert to `List<Point3<Real>>` turns into `Point3<Real>`.
    assert_rejection_renders_initializer_type(
        "structure P { param m : Mass = Bead().centerline[0] }",
        "Point3<Scalar[m]>",
        "Bead.centerline elements stay Point3<Length>",
    );
    // Control for that pin: the two spellings the mechanism CANNOT distinguish,
    // recorded so nobody mistakes them for coverage. Both compile clean today
    // and would still compile clean after a revert.
    assert_compiles_clean(
        "structure P { param c : List<Int> = Bead().centerline }",
        "List element types are not checked (so this pins nothing)",
    );
    assert_compiles_clean(
        "structure P { param p : Point3<Real> = Bead().centerline[0] }",
        "Point3 component types are not checked (so this pins nothing)",
    );
}
