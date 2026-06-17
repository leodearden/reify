//! B integration gate: end-to-end DFM thickness example (task 4427 η).
//!
//! Loads `examples/process/std_process_dfm_thickness.ri` (the shipped
//! user-observable example), builds it with OCCT+OpenVDB kernels (via
//! `Engine::with_registered_kernels`), runs `engine.check()`, and asserts
//! the auto-emitted `{I,W,E}_DFM_MIN_WALL` / `{I,W,E}_DFM_MIN_FEATURE`
//! diagnostic multiset.
//!
//! # Kernel requirement
//!
//! `realize_solid_sdf` (the solid→SDF wire used by `measure_dfm_rules`)
//! needs BOTH a BRep kernel (OCCT, for tessellation) AND an OpenVDB kernel
//! (for Mesh→Voxel voxelisation + medial-axis extraction). Only
//! `Engine::with_registered_kernels` (plural) loads the full inventory
//! (OCCT+OpenVDB). The singular `with_registered_kernel` picks only OCCT
//! → thickness rules degrade to Indeterminate and no `_DFM_MIN_WALL` /
//! `_DFM_MIN_FEATURE` diagnostics are emitted (proved by
//! `process_dfm_measure.rs::occt_no_openvdb_no_thickness_diagnostics`).
//!
//! # Guard
//!
//! All tests are double-gated:
//!  1. `#[cfg(has_openvdb)]` — compile-time; the whole function is omitted
//!     in stub-OpenVDB environments (mirrors `imported_field_e2e.rs`).
//!  2. Runtime `if !reify_kernel_occt::OCCT_AVAILABLE { return; }` — skip on
//!     hosts where the OCCT native libraries are absent (mirrors
//!     `process_dfm_metrology_example.rs`).
//!
//! Compile coverage for `std_process_dfm_thickness.ri` (kernel-independent)
//! is provided by `crates/reify-compiler/tests/examples_smoke.rs` which
//! discovers `examples/**/*.ri` automatically and gates on Error-severity
//! compile diagnostics.
//!
//! # Diagnostic multiset expected from the final example file (all slices)
//!
//! Three violating DFMRules (W, E, I) each with a distinct thin subject in
//! the Goldilocks zone (2h ≤ t < min_feature_size); one thick conformer
//! that is silent:
//!
//! | Rule                    | W_MIN_WALL | W_MIN_FEATURE | E_MIN_WALL | E_MIN_FEATURE | I_MIN_WALL | I_MIN_FEATURE |
//! |-------------------------|-----------|--------------|-----------|--------------|-----------|--------------|
//! | Warning (thin slab)     |         1 |            1 |           |              |           |              |
//! | Error   (thin rib)      |           |              |         1 |            1 |           |              |
//! | Info    (thin web)      |           |              |           |              |         1 |            1 |
//! | Conformer (thick block) |         0 |            0 |         0 |            0 |         0 |            0 |
//! | **Total**               |     **1** |        **1** |     **1** |        **1** |     **1** |        **1** |

// ── helpers (cfg(has_openvdb) — only used in the real test bodies) ─────────────

/// Assert that `result.diagnostics` contains exactly `count` entries whose
/// `message` contains `substr`.
#[cfg(has_openvdb)]
fn assert_dfm_diagnostic_count(result: &reify_eval::CheckResult, substr: &str, count: usize) {
    let matching: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains(substr))
        .collect();
    assert_eq!(
        matching.len(),
        count,
        "expected {count} diagnostic(s) containing {:?}, but got {}: {:#?}",
        substr,
        matching.len(),
        matching
    );
}

/// Read `examples/process/std_process_dfm_thickness.ri` from the workspace
/// root and compile it with the stdlib prelude.
///
/// RED until the example file is created (step-2): panics on `read_to_string`.
#[cfg(has_openvdb)]
fn load_and_compile_example() -> reify_compiler::CompiledModule {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/process/std_process_dfm_thickness.ri"
    );
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read example file {path}: {e}"));
    reify_test_support::parse_and_compile_with_stdlib(&source)
}

/// Build an `Engine` with the full kernel inventory (OCCT + OpenVDB) loaded.
///
/// Uses `Engine::with_registered_kernels` (plural), which factory-instantiates
/// every adapter in the inventory — OCCT (for BRep tessellation to Mesh) and
/// OpenVDB (for Mesh→Voxel conversion). This is the only path that enables
/// `realize_solid_sdf`, which is required for the `measure_dfm_rules` thickness
/// arm.
///
/// The `openvdb_capability_descriptor()` call is a **linker anchor**: it forces
/// the linker to include `register.rs` from the `reify-kernel-openvdb` rlib so
/// the `inventory::submit!` registration fires at startup. Without it the rlib
/// can be dead-stripped and the OpenVDB adapter never appears in the registry.
/// Pattern mirrors `manifold_cross_kernel_real.rs`.
#[cfg(has_openvdb)]
fn make_occt_openvdb_engine() -> reify_eval::Engine {
    // Linker anchor — forces inventory::submit! in reify-kernel-openvdb/src/register.rs.
    let _anchor = reify_kernel_openvdb::register::openvdb_capability_descriptor();
    reify_eval::Engine::with_registered_kernels(Box::new(
        reify_constraints::SimpleConstraintChecker,
    ))
}

// ── step-1 / step-2: Warning slice ───────────────────────────────────────────

/// Loads the shipped example, builds with OCCT+OpenVDB, checks, and asserts
/// that exactly one `W_DFM_MIN_WALL` and one `W_DFM_MIN_FEATURE` diagnostic
/// are emitted — one per arm — from the Warning-severity DFMRule whose subject
/// is a thin-walled box in the Goldilocks zone (2h ≤ t < min_feature_size).
///
/// Both the `_MIN_WALL` arm (d⁺+d⁻ medial sum ≈ t) and the `_MIN_FEATURE` arm
/// (ridge 2|ϕ| ≈ t) fire on a thin slab because t is both above the resolution
/// floor and below the process `min_feature_size`.
///
/// RED (step-1): example file does not exist yet → `load_and_compile_example`
///   panics on `read_to_string` (observable on hosts with OCCT+OpenVDB).
/// GREEN (step-2): example file's Warning-severity DFMRule makes this pass.
#[cfg(has_openvdb)]
#[test]
fn example_emits_min_wall_and_min_feature_warning() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping example_emits_min_wall_and_min_feature_warning: OCCT not available"
        );
        return;
    }

    let compiled = load_and_compile_example();
    let mut engine = make_occt_openvdb_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    assert_dfm_diagnostic_count(&result, "W_DFM_MIN_WALL", 1);
    assert_dfm_diagnostic_count(&result, "W_DFM_MIN_FEATURE", 1);
}

/// Skip-stub: `has_openvdb` is not set in this build environment.
/// Compile coverage is provided by `examples_smoke.rs` (kernel-independent).
#[cfg(not(has_openvdb))]
#[test]
fn example_emits_min_wall_and_min_feature_warning() {
    eprintln!(
        "SKIP: has_openvdb not set — skipping DFM thickness Warning e2e test"
    );
}
