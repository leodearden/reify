//! SIR-α (task 3540) — `Value::StructureInstance` boundary tests.
//!
//! This file is the primary boundary-test surface for the SIR-α foundation
//! slice. It covers PRD §7.1 (producer-side: variant round-trip, cache-key
//! determinism, engine-restart stability) and §7.2 (consumer-side: `.ri`
//! constructor evaluation, trait-typed-param admission, nested member-access
//! chains, Map-vs-Structure distinction, nominal-conformance enforcement).
//!
//! Step-9 seeded this file with `structure_instance_is_constructible` (the
//! workspace-exhaustiveness probe) and step-19 added
//! `point_load_in_source_lowers_to_structure_instance` (the wave-1 stdlib
//! swap end-to-end pin). Both are preserved below. Step-21 adds the full
//! PRD §7.1/§7.2 scenario suite (RED until step-22 wires the remaining
//! plumbing and authors `examples/structure-instance.ri`). Step-23 appends
//! the `reify eval` golden test.

#![allow(clippy::mutable_key_type)]

use std::sync::atomic::{AtomicUsize, Ordering};

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_test_support::{
    collect_errors, compile_source_with_stdlib, make_simple_engine, parse_and_compile_with_stdlib,
};

/// `PersistentMap<String, Value>::get` is keyed by `&String`; this lets the
/// scenarios index `StructureInstance.fields` with a string literal.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

/// Scenario-9 call counter: incremented by `identity_fn` each time it runs.
///
/// Provides a direct execution proof for the ComputeNode-trampoline test:
/// because the inline-fallback body is also `{ x }` (identity), the result
/// cell value alone cannot distinguish trampoline-accept from inline-fallback.
/// The ComputeNode-presence check (assertion b) is an indirect inference; this
/// counter makes assertion (d) a first-class execution witness — if lowering
/// ever changed to insert a ComputeNode on inline-fallback, (b) would silently
/// lose discriminating power while (d) would still fail correctly.
static SCENARIO_9_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Synthetic identity trampoline used by scenario 9.
/// Mirrors compute_dispatch_registry.rs identity_fn (task γ / 3422 pattern).
/// Increments `SCENARIO_9_CALL_COUNT` so the test can assert direct execution.
fn identity_fn(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    SCENARIO_9_CALL_COUNT.fetch_add(1, Ordering::Relaxed);
    ComputeOutcome::Completed {
        result: value_inputs.first().cloned().unwrap_or(Value::Undef),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
    }
}

/// No-op constructor: proves `Value::StructureInstance` is reachable from a
/// test binary. Compilation of the whole `reify-eval` test target is the
/// real assertion here (step-9 RED → step-10 GREEN).
#[test]
fn structure_instance_is_constructible() {
    let fields: PersistentMap<String, Value> = [("youngs_modulus".to_string(), Value::Real(205e9))]
        .into_iter()
        .collect();
    let v = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Steel_AISI_1045".to_string(),
        version: 1,
        fields,
    }));
    match &v {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "Steel_AISI_1045");
            assert_eq!(data.version, 1);
        }
        other => panic!("expected StructureInstance, got {other:?}"),
    }
}

/// task 3540 step-19: end-to-end check of the wave-1 stdlib swap.
///
/// Compiles a tiny structure that calls `PointLoad()` (the structure-def
/// constructor landed in step-20 in `crates/reify-compiler/stdlib/fea_multi_case.ri`).
/// Asserts the bound cell value is a `Value::StructureInstance` whose
/// `type_name` is `"PointLoad"`.
#[test]
fn point_load_in_source_lowers_to_structure_instance() {
    const SOURCE: &str = r#"
structure def PointLoadFixture {
    let load = PointLoad()
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("PointLoadFixture", "load");
    let load = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("PointLoadFixture.load cell missing from eval result"));

    match load {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "PointLoad",
                "expected type_name=\"PointLoad\" (the wave-1 SIR-α stdlib structure_def), \
                 got {:?}",
                data.type_name
            );
        }
        other => panic!(
            "expected Value::StructureInstance for PointLoadFixture.load — \
             got {other:?}"
        ),
    }
}

// ── PRD §7.2 — consumer-side scenarios ───────────────────────────────────────

/// Scenario 1: a flat `Steel_AISI_1045()` constructor evaluates to a
/// `Value::StructureInstance` carrying the declared default fields.
#[test]
fn flat_construction_evaluates_to_structure_instance() {
    const SOURCE: &str = r#"
structure def FlatPart {
    let steel = Steel_AISI_1045()
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let id = ValueCellId::new("FlatPart", "steel");
    let steel = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("FlatPart.steel cell missing from eval result"));

    match steel {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "Steel_AISI_1045");
            assert!(
                field(&data.fields, "youngs_modulus").is_some(),
                "Steel_AISI_1045 instance must carry its `youngs_modulus` default field; \
                 fields present: {:?}",
                data.fields.iter().map(|(k, _)| k).collect::<Vec<_>>()
            );
            assert!(
                !matches!(field(&data.fields, "youngs_modulus"), Some(Value::Undef)),
                "youngs_modulus default (205GPa) must evaluate to a non-Undef value"
            );
        }
        other => panic!("expected Value::StructureInstance for FlatPart.steel, got {other:?}"),
    }
}

/// Scenario 2: nested compositional construction with a member-access chain
/// through `sub` children (`assembly.primary.material.youngs_modulus`).
#[test]
fn nested_compositional_construction_member_access() {
    const SOURCE: &str = r#"
structure def Beam {
    param material : ElasticMaterial = Steel_AISI_1045()
    param length : Length = 1m
}

structure def NestedAssembly {
    sub primary   = Beam(length: 1m)
    sub secondary = Beam(length: 2m)
    let primary_E = self.primary.material.youngs_modulus
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // The `sub primary` cell is itself a Beam structure instance whose
    // `material` field is a nested Steel_AISI_1045 structure instance.
    let primary = result
        .values
        .get(&ValueCellId::new("NestedAssembly", "primary"))
        .unwrap_or_else(|| panic!("NestedAssembly.primary cell missing"));
    match primary {
        Value::StructureInstance(data) => {
            assert_eq!(data.type_name, "Beam");
            match field(&data.fields, "material") {
                Some(Value::StructureInstance(mat)) => {
                    assert_eq!(mat.type_name, "Steel_AISI_1045");
                    assert!(
                        field(&mat.fields, "youngs_modulus").is_some(),
                        "nested material must carry youngs_modulus"
                    );
                }
                other => panic!(
                    "expected nested Value::StructureInstance for Beam.material, got {other:?}"
                ),
            }
        }
        other => {
            panic!("expected Value::StructureInstance for NestedAssembly.primary, got {other:?}")
        }
    }

    // The source-level member-access chain must resolve to the same scalar.
    let primary_e = result
        .values
        .get(&ValueCellId::new("NestedAssembly", "primary_E"))
        .unwrap_or_else(|| panic!("NestedAssembly.primary_E cell missing"));
    assert!(
        matches!(primary_e, Value::Scalar { .. }),
        "self.primary.material.youngs_modulus must resolve to a Scalar (205GPa), got {primary_e:?}"
    );
}

/// Scenario 3: a trait-typed param admits a conforming concrete structure,
/// and the nested member reads through.
#[test]
fn trait_typed_param_admits_conforming_structure() {
    const SOURCE: &str = r#"
structure def BeamT {
    param mat : ElasticMaterial = Steel_AISI_1045()
}

structure def UseBeam {
    sub b = BeamT()
    let e = self.b.mat.youngs_modulus
}
"#;
    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "a conforming structure (Steel_AISI_1045) must be admitted for an \
         ElasticMaterial-typed param without diagnostics; got: {errors:?}"
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let b = result
        .values
        .get(&ValueCellId::new("UseBeam", "b"))
        .unwrap_or_else(|| panic!("UseBeam.b cell missing"));
    match b {
        Value::StructureInstance(data) => match field(&data.fields, "mat") {
            Some(Value::StructureInstance(mat)) => assert_eq!(mat.type_name, "Steel_AISI_1045"),
            other => panic!("expected BeamT.mat to be a nested StructureInstance, got {other:?}"),
        },
        other => panic!("expected Value::StructureInstance for UseBeam.b, got {other:?}"),
    }
}

/// Scenario 4: a `Map` value and a `StructureInstance` value coexisting in
/// one fixture stay structurally distinct and hash distinctly (no
/// conflation through the content-hash / cache path).
#[test]
fn linguistic_map_vs_structure_distinction() {
    const SOURCE: &str = r#"
structure def MapVsStruct {
    let m = map{"youngs_modulus" => 205}
    let s = Steel_AISI_1045()
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let m = result
        .values
        .get(&ValueCellId::new("MapVsStruct", "m"))
        .unwrap_or_else(|| panic!("MapVsStruct.m cell missing"));
    let s = result
        .values
        .get(&ValueCellId::new("MapVsStruct", "s"))
        .unwrap_or_else(|| panic!("MapVsStruct.s cell missing"));

    assert!(
        matches!(m, Value::Map(_)),
        "a `map{{...}}` literal must remain a Value::Map, got {m:?}"
    );
    assert!(
        matches!(s, Value::StructureInstance(_)),
        "a structure constructor must produce a Value::StructureInstance, got {s:?}"
    );
    assert_ne!(
        m.content_hash().0,
        s.content_hash().0,
        "a Map and a StructureInstance must never share a content hash"
    );
}

/// Scenario 5: nominal-conformance enforcement — a non-conforming structure
/// passed where a trait-typed param is required is rejected at compile time.
#[test]
fn nominal_conformance_enforcement_negative() {
    const SOURCE: &str = r#"
structure def BeamN {
    param mat : ElasticMaterial = Steel_AISI_1045()
}

structure def BadAsm {
    sub b = BeamN(mat: PointLoad())
}
"#;
    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("does not conform to trait")
                && d.message.contains("ElasticMaterial")),
        "passing a non-conforming `PointLoad()` to an ElasticMaterial-typed param \
         must produce a trait-conformance error; got: {errors:?}"
    );
}

/// Scenario 6: the content hash is deterministic across field-insertion
/// order (sort-by-key invariant, PRD §5).
#[test]
fn cache_key_deterministic_across_field_order() {
    let a: PersistentMap<String, Value> = [
        ("youngs_modulus".to_string(), Value::Real(205e9)),
        ("poisson_ratio".to_string(), Value::Real(0.29)),
        ("density".to_string(), Value::Real(7850.0)),
    ]
    .into_iter()
    .collect();
    // Same entries, reversed insertion order.
    let b: PersistentMap<String, Value> = [
        ("density".to_string(), Value::Real(7850.0)),
        ("poisson_ratio".to_string(), Value::Real(0.29)),
        ("youngs_modulus".to_string(), Value::Real(205e9)),
    ]
    .into_iter()
    .collect();

    let va = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Steel_AISI_1045".to_string(),
        version: 1,
        fields: a,
    }));
    // A different type_id must NOT change the content hash (per-Engine,
    // ephemeral; cache must survive an Engine restart).
    let vb = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(99),
        type_name: "Steel_AISI_1045".to_string(),
        version: 1,
        fields: b,
    }));
    assert_eq!(
        va.content_hash().0,
        vb.content_hash().0,
        "content_hash must be invariant under field-insertion order and type_id"
    );
}

/// Scenario 7: bumping the structure `version` changes the content hash so
/// a `@version(N)` redefinition invalidates the persistent cache.
#[test]
fn cache_key_changes_on_version_bump() {
    let fields: PersistentMap<String, Value> = [("youngs_modulus".to_string(), Value::Real(205e9))]
        .into_iter()
        .collect();
    let v1 = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Steel_AISI_1045".to_string(),
        version: 1,
        fields: fields.clone(),
    }));
    let v2 = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Steel_AISI_1045".to_string(),
        version: 2,
        fields,
    }));
    assert_ne!(
        v1.content_hash().0,
        v2.content_hash().0,
        "a version bump (1 → 2) must change the content hash"
    );
}

/// Scenario 8: cache key is stable across an Engine restart — re-evaluating
/// the same source in a fresh Engine yields a StructureInstance with an
/// identical content hash even though the per-Engine `type_id` may differ.
#[test]
fn engine_restart_cache_stability() {
    const SOURCE: &str = r#"
structure def RestartFixture {
    let steel = Steel_AISI_1045()
}
"#;
    let id = ValueCellId::new("RestartFixture", "steel");

    let compiled_a = parse_and_compile_with_stdlib(SOURCE);
    let mut engine_a = make_simple_engine();
    let result_a = engine_a.eval(&compiled_a);
    let hash_a = result_a
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("RestartFixture.steel missing (engine A)"))
        .content_hash()
        .0;
    drop(engine_a);

    let compiled_b = parse_and_compile_with_stdlib(SOURCE);
    let mut engine_b = make_simple_engine();
    let result_b = engine_b.eval(&compiled_b);
    let hash_b = result_b
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("RestartFixture.steel missing (engine B)"))
        .content_hash()
        .0;

    assert_eq!(
        hash_a, hash_b,
        "the persistent cache key must be name+version-derived (type_id-independent), \
         so it survives an Engine restart"
    );
}

/// Scenario 9: the ComputeNode trampoline accepts a `Value::StructureInstance`
/// argument (PRD `docs/prds/v0_3/compute-node-contract.md` §7.2 / scenario 9).
///
/// The seam required by this scenario — task γ / 3422: synthetic `test::identity`
/// trampoline pattern, per-Engine `register_compute_fn` registry, and
/// `@optimized(...)`→ComputeNode lowering wire — landed on main and is live.
/// This test pins that the trampoline path (not the inline-fallback body) accepts
/// and round-trips a `Value::StructureInstance` end-to-end through a compiled
/// `.ri` source. Defence-in-depth for compute-node-contract.md §7.2; not the
/// SIR-α user-observable signal.
#[test]
fn compute_node_trampoline_arm_accepts_structure_instance() {
    // The `@optimized` fn receives a trait-typed param reference so the call
    // site is an exact ElasticMaterial-to-ElasticMaterial match (no concrete
    // StructureRef at the call site — avoids the trait-conformance-at-call-site
    // wrinkle documented in solver_elastic.ri:410-426).
    const SOURCE: &str = r#"
@optimized("test::identity")
fn identity_compute_test(x: ElasticMaterial) -> ElasticMaterial { x }

structure def StructInstanceTrampolineFixture {
    param mat : ElasticMaterial = Steel_AISI_1045()
    let result = identity_compute_test(mat)
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::identity", identity_fn as ComputeFn);
    // Reset before eval so any prior (re-)run of this test in the same process
    // doesn't bleed a stale count into assertion (d).
    SCENARIO_9_CALL_COUNT.store(0, Ordering::Relaxed);
    let eval_result = engine.eval(&compiled);

    // (a) The result cell must be a Steel_AISI_1045 StructureInstance,
    //     proving the StructureInstance round-tripped through the trampoline.
    let result_id = ValueCellId::new("StructInstanceTrampolineFixture", "result");
    let result_val = eval_result
        .values
        .get(&result_id)
        .unwrap_or_else(|| panic!("StructInstanceTrampolineFixture.result cell missing"));
    match result_val {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "Steel_AISI_1045",
                "expected result to be a Steel_AISI_1045 StructureInstance \
                 (round-tripped via trampoline), got type_name: {:?}",
                data.type_name
            );
        }
        other => panic!(
            "expected Value::StructureInstance for StructInstanceTrampolineFixture.result, \
             got {other:?}"
        ),
    }

    // (b) A ComputeNode with target "test::identity" must exist in the graph,
    //     proving the value came from the TRAMPOLINE (not the inline-fallback
    //     body). CRITICAL: the body is `{ x }` (also identity), so without (b)
    //     we cannot distinguish trampoline-accept from inline-fallback.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, data)| data.target == "test::identity");
    assert!(
        compute_node.is_some(),
        "expected a ComputeNode with target==\"test::identity\" in the graph \
         (proves trampoline path, not inline-fallback); found compute nodes: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| &d.target)
            .collect::<Vec<_>>()
    );

    // (c) No Error diagnostics — secondary guard confirming the trampoline
    //     registration gotcha was honored (unregistered targets emit Error + inline).
    let error_diags: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected no Error diagnostics (trampoline was registered), \
         got: {:?}",
        error_diags
    );

    // (d) Direct execution proof: identity_fn's call counter was incremented.
    //     Unlike (b), this asserts the fn body actually ran — if lowering
    //     behaviour ever changed to insert a ComputeNode even on inline-fallback,
    //     (b) alone would silently lose its discriminating power; (d) would not.
    assert_eq!(
        SCENARIO_9_CALL_COUNT.load(Ordering::Relaxed),
        1,
        "identity_fn must have been called exactly once by the trampoline dispatch \
         (direct execution proof, not inferred from graph structure)"
    );
}

// ── SIR-α user-observable signal (step-23) ───────────────────────────────────

/// `reify eval examples/structure-instance.ri` must print inspectable
/// structure-shaped values (not `undef`), and its stdout must match the
/// committed golden. Regenerate with `REIFY_REGENERATE_GOLDEN=1`.
///
/// `CARGO_BIN_EXE_reify` is only injected for `reify-cli`'s own integration
/// tests, so this cross-crate test execs the pre-built `reify` binary
/// directly. It deliberately does NOT use `cargo run`: even when the binary
/// is already compiled, `cargo run` re-fingerprints the entire workspace and
/// blocks on the global cargo build-lock before exec, and under concurrent
/// multi-worktree verify load that overhead can push the test past its time
/// budget (esc-4340-32, exit 124). The merge gate's debug `--workspace` pass
/// builds all `[[bin]]` targets (including `reify`) at `target/debug/reify`;
/// its release pass is scoped to release-sensitive crates and does NOT rebuild
/// `reify-cli`, so the resolution below prefers the profile-local bin and falls
/// back to the debug-profile one when it is absent. The cargo runner
/// (`.cargo/run-with-occt.sh`) exports `LD_LIBRARY_PATH` into this test
/// process's environment, which the spawned child inherits, so OCCT shared
/// libraries resolve without going through cargo.
#[test]
fn cli_reify_eval_prints_inspectable_structure_values() {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../crates/reify-eval
    let workspace_root = std::path::Path::new(manifest)
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above crates/reify-eval")
        .to_path_buf();
    let example = workspace_root.join("examples/structure-instance.ri");
    let golden = std::path::Path::new(manifest).join("tests/golden/structure_instance.txt");

    // Resolve the prebuilt `reify` binary from this test binary's own location.
    // The integration-test binary lives at `…/target/<profile>/deps/<testbin>`,
    // so its grandparent is `…/target/<profile>` and the `reify` bin sits beside
    // it at `…/target/<profile>/reify`.
    //
    // Cross-task seam (task/4390 HAS LANDED): the merge gate's RELEASE pass
    // (verify.sh, DF_VERIFY_ROLE=merge --profile both) is scoped to
    // release-sensitive crates and deliberately does NOT build `reify-cli`, so
    // `target/release/reify` is absent during the release test pass. The
    // preceding DEBUG pass runs the full `--workspace` (building
    // `target/debug/reify`), and the reify CLI's golden output is
    // profile-independent (the release pass exists to re-check reify-eval's own
    // overflow/debug-assert behaviour, not the spawned CLI). So prefer the
    // profile-local bin but fall back to the debug-profile sibling when it is
    // absent. (Per-task verifies are unaffected: a reify-eval change pulls
    // `reify-cli` into the affected set as a reverse-dep, so the debug bin is
    // built.)
    let test_bin = std::env::current_exe().expect("current_exe");
    let profile_dir = test_bin
        .parent()
        .and_then(|p| p.parent())
        .expect("test binary lives in target/<profile>/deps");
    let profile_local = profile_dir.join("reify");
    let reify_bin = if profile_local.exists() {
        profile_local
    } else {
        // Release pass: target/release/reify is absent (reify-cli not built);
        // fall back to the debug-profile bin the debug pass built.
        profile_dir
            .parent()
            .map(|target_dir| target_dir.join("debug").join("reify"))
            .filter(|p| p.exists())
            .unwrap_or(profile_local)
    };

    let output = std::process::Command::new(&reify_bin)
        .current_dir(&workspace_root)
        .arg("eval")
        .arg(&example)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn pre-built reify binary at {}: {e}; is it built? \
                 The gated verify pass builds it when it compiles `reify-cli` \
                 (`cargo test -p reify-cli`, or the merge gate's debug \
                 `--workspace` pass that builds all `[[bin]]` targets). Note: an \
                 ad-hoc `cargo test -p reify-eval` alone does NOT build the \
                 `reify` bin.",
                reify_bin.display()
            )
        });

    assert!(
        output.status.success(),
        "`reify eval` exited non-zero.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");

    if std::env::var("REIFY_REGENERATE_GOLDEN").is_ok() {
        std::fs::write(&golden, &stdout).expect("failed to write golden file");
        return;
    }

    let expected = std::fs::read_to_string(&golden).expect(
        "golden crates/reify-eval/tests/golden/structure_instance.txt missing; \
         run once with REIFY_REGENERATE_GOLDEN=1",
    );
    assert_eq!(
        stdout, expected,
        "`reify eval examples/structure-instance.ri` stdout drifted from the golden; \
         re-run with REIFY_REGENERATE_GOLDEN=1 to update"
    );
    assert!(
        stdout.contains("Steel_AISI_1045 {"),
        "the SIR-α signal requires an inspectable Steel_AISI_1045 structure value \
         (not `undef`) in `reify eval` output; got:\n{stdout}"
    );
}

// ── RBD-α (task 3822) — MassProperties PSD inertia validation ────────────────

/// Scenario: non-PSD inertia tensor → `E_DynamicsInertiaNotPSD` + `Value::Undef`.
///
/// `inertia: [[1,0,0],[0,1,0],[0,0,-1]]` has minimum eigenvalue −1.  The
/// engine_eval PSD hook must:
///   (a) emit a `Diagnostic` with `code == Some(DiagnosticCode::DynamicsInertiaNotPSD)`, and
///   (b) replace the `mp` cell value with `Value::Undef`.
///
/// Note: `origin` is a zero `Frame3` (task 4547 retargeted `MassProperties.origin`
/// from the old `Real` placeholder to the `Frame3` struct declared in std.ports).
/// `point3(0mm, 0mm, 0mm)` builds the CoM Point3<Length>. The nested-list
/// literal `[[1,0,0],[0,1,0],[0,0,-1]]` is accepted by the structure ctor
/// (structure ctors accept any value — no call-site type check).
#[test]
fn mass_properties_non_psd_inertia_emits_diagnostic_and_undef() {
    const SOURCE: &str = r#"
structure def NonPsdFixture {
    let mp = MassProperties(
        mass: 1kg,
        com: point3(0mm, 0mm, 0mm),
        inertia: [[1,0,0],[0,1,0],[0,0,-1]],
        origin: Frame3(origin: vec3(0mm, 0mm, 0mm), x_axis: vec3(0mm, 0mm, 0mm), y_axis: vec3(0mm, 0mm, 0mm), z_axis: vec3(0mm, 0mm, 0mm))
    )
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // (a) A DynamicsInertiaNotPSD diagnostic must be present.
    let psd_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DynamicsInertiaNotPSD))
        .collect();
    assert!(
        !psd_diags.is_empty(),
        "expected a DynamicsInertiaNotPSD diagnostic for non-PSD inertia \
         [[1,0,0],[0,1,0],[0,0,-1]] (min eigenvalue ≈ -1), got diagnostics: {:?}",
        result.diagnostics
    );

    // (b) The `mp` cell must be Value::Undef (the PSD hook replaces the instance).
    let mp_id = ValueCellId::new("NonPsdFixture", "mp");
    let mp_val = result
        .values
        .get(&mp_id)
        .unwrap_or_else(|| panic!("NonPsdFixture.mp cell missing from eval result"));
    assert!(
        matches!(mp_val, Value::Undef),
        "NonPsdFixture.mp should be Value::Undef after non-PSD rejection, got: {:?}",
        mp_val
    );
}

// ── task 4245 — PointLoad.direction field ────────────────────────────────────

/// task 4245 step-1 RED — `PointLoad.direction` field default and round-trip.
///
/// Compiles two `PointLoad` instances:
///   (a) default constructor — `direction` must carry the default `[0.0, 0.0, -1.0]`.
///   (b) explicit override  — `direction: [0.0, -1.0, 0.0]` must round-trip.
///
/// RED: the `direction` field does not exist yet in `fea_multi_case.ri`, so
/// `field(..., "direction")` is `None` and the assertions fail.
#[test]
fn point_load_direction_field_default_and_override() {
    const SOURCE: &str = r#"
structure def DirectionFixture {
    let a = PointLoad()
    // `direction` is the LAST param (ctors bind positionally), so an explicit
    // override must also supply point/force in declaration order.
    let b = PointLoad(point: "", force: 0.0, direction: [0.0, -1.0, 0.0])
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // ── (a) default: direction = [0.0, 0.0, -1.0] ────────────────────────────
    let a = result
        .values
        .get(&ValueCellId::new("DirectionFixture", "a"))
        .unwrap_or_else(|| panic!("DirectionFixture.a cell missing from eval result"));
    match a {
        Value::StructureInstance(data) => match field(&data.fields, "direction") {
            Some(Value::List(items)) => {
                assert_eq!(
                    items.len(),
                    3,
                    "PointLoad().direction must have 3 elements; got {:?}",
                    items
                );
                assert_eq!(
                    items[0],
                    Value::Real(0.0),
                    "PointLoad().direction[0] must be 0.0 (default -Z)"
                );
                assert_eq!(
                    items[1],
                    Value::Real(0.0),
                    "PointLoad().direction[1] must be 0.0 (default -Z)"
                );
                assert_eq!(
                    items[2],
                    Value::Real(-1.0),
                    "PointLoad().direction[2] must be -1.0 (default -Z)"
                );
            }
            other => panic!(
                "expected Value::List for PointLoad().direction, got {:?}",
                other
            ),
        },
        other => panic!("expected Value::StructureInstance for DirectionFixture.a, got {other:?}"),
    }

    // ── (b) override: direction = [0.0, -1.0, 0.0] ───────────────────────────
    let b = result
        .values
        .get(&ValueCellId::new("DirectionFixture", "b"))
        .unwrap_or_else(|| panic!("DirectionFixture.b cell missing from eval result"));
    match b {
        Value::StructureInstance(data) => match field(&data.fields, "direction") {
            Some(Value::List(items)) => {
                assert_eq!(
                    items.len(),
                    3,
                    "PointLoad(direction:[0,-1,0]).direction must have 3 elements; got {:?}",
                    items
                );
                assert_eq!(
                    items[0],
                    Value::Real(0.0),
                    "PointLoad(direction:[0,-1,0]).direction[0] must be 0.0"
                );
                assert_eq!(
                    items[1],
                    Value::Real(-1.0),
                    "PointLoad(direction:[0,-1,0]).direction[1] must be -1.0"
                );
                assert_eq!(
                    items[2],
                    Value::Real(0.0),
                    "PointLoad(direction:[0,-1,0]).direction[2] must be 0.0"
                );
            }
            other => panic!(
                "expected Value::List for PointLoad(direction:[0,-1,0]).direction, got {:?}",
                other
            ),
        },
        other => panic!("expected Value::StructureInstance for DirectionFixture.b, got {other:?}"),
    }
}

/// Scenario: PSD inertia tensor → `Value::StructureInstance` + no PSD diagnostic.
///
/// `inertia: [[1,0,0],[0,1,0],[0,0,1]]` (identity) has all eigenvalues = 1.
/// The engine_eval PSD hook must leave the instance untouched and emit no
/// `DynamicsInertiaNotPSD` diagnostic.
#[test]
fn mass_properties_psd_inertia_yields_structure_instance() {
    const SOURCE: &str = r#"
structure def PsdFixture {
    let mp = MassProperties(
        mass: 1kg,
        com: point3(0mm, 0mm, 0mm),
        inertia: [[1,0,0],[0,1,0],[0,0,1]],
        origin: Frame3(origin: vec3(0mm, 0mm, 0mm), x_axis: vec3(0mm, 0mm, 0mm), y_axis: vec3(0mm, 0mm, 0mm), z_axis: vec3(0mm, 0mm, 0mm))
    )
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // No DynamicsInertiaNotPSD diagnostic should appear.
    let psd_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DynamicsInertiaNotPSD))
        .collect();
    assert!(
        psd_diags.is_empty(),
        "PSD inertia [[1,0,0],[0,1,0],[0,0,1]] should produce no \
         DynamicsInertiaNotPSD diagnostic, got: {:?}",
        psd_diags
    );

    // The `mp` cell must be a StructureInstance with type_name "MassProperties".
    let mp_id = ValueCellId::new("PsdFixture", "mp");
    let mp_val = result
        .values
        .get(&mp_id)
        .unwrap_or_else(|| panic!("PsdFixture.mp cell missing from eval result"));
    match mp_val {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "MassProperties",
                "PsdFixture.mp should be a MassProperties instance, got type_name: {:?}",
                data.type_name
            );
        }
        other => panic!(
            "PsdFixture.mp should be Value::StructureInstance, got: {:?}",
            other
        ),
    }
}

// ── task 3754 — named-arg ctor binding regression guard ──────────────────────

/// Characterisation / regression guard for the by-name binder (task-4522,
/// commit 5eea7c3de8).  The by-name binder already landed on main; this test
/// is GREEN on arrival and exists to prevent silent regressions.
///
/// `Beam(material: Aluminium_6061_T6())` binds the named `material` arg to the
/// first param slot and leaves `length` to pick up its default (`1m`).  The
/// result must be a `Beam` `StructureInstance` whose `material` field is an
/// `Aluminium_6061_T6` instance and whose `length` field is a `Scalar`.
#[test]
fn named_arg_binding_for_ctor() {
    const SOURCE: &str = r#"
structure def Beam {
    param material : ElasticMaterial = Steel_AISI_1045()
    param length : Length = 1m
}

structure def NamedArgFixture {
    let beam = Beam(material: Aluminium_6061_T6())
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "Beam(material: Aluminium_6061_T6()) must compile without errors \
         (Aluminium_6061_T6 conforms to ElasticMaterial); got: {errors:?}"
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let beam_id = ValueCellId::new("NamedArgFixture", "beam");
    let beam_val = result
        .values
        .get(&beam_id)
        .unwrap_or_else(|| panic!("NamedArgFixture.beam cell missing from eval result"));

    match beam_val {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "Beam",
                "NamedArgFixture.beam must be a Beam instance; got type_name: {:?}",
                data.type_name
            );

            // Named arg `material: Aluminium_6061_T6()` must have bound to the
            // `material` param — the field must be an Aluminium_6061_T6 instance.
            match field(&data.fields, "material") {
                Some(Value::StructureInstance(mat)) => {
                    assert_eq!(
                        mat.type_name, "Aluminium_6061_T6",
                        "Beam.material must be Aluminium_6061_T6 (named arg override), \
                         got type_name: {:?}",
                        mat.type_name
                    );
                }
                other => {
                    panic!("expected a nested StructureInstance for Beam.material, got {other:?}")
                }
            }

            // `length` was NOT supplied — it must have taken the default (1m).
            match field(&data.fields, "length") {
                Some(Value::Scalar { .. }) => {} // default 1m applied ✓
                other => {
                    panic!("expected Value::Scalar for Beam.length (default 1m), got {other:?}")
                }
            }
        }
        other => {
            panic!("expected Value::StructureInstance for NamedArgFixture.beam, got {other:?}")
        }
    }
}
