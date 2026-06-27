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

use reify_core::{DiagnosticCode, Severity, ValueCellId};
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
