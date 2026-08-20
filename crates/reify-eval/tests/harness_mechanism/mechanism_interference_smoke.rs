//! End-to-end smoke test for the kinematic-query stdlib helpers
//! `interferes`, `interferes_with`, `min_clearance` (task 2531 / PRD task 8).
//!
//! Drives the full pipeline:
//!   parse → `compile_with_stdlib` → `Engine::build` (with real OCCT kernel)
//!   → assert the `BuildResult.values` carry the kernel-resolved
//!   `Value::List(...)` / `Value::Bool(_)` / length-`Value::Scalar`.
//!
//! The kernel-aware dispatch lives in
//! `reify_eval::geometry_ops::try_eval_kinematic_query`, invoked as a
//! post-process from `engine_build.rs::post_process_kinematic_queries`. These
//! tests pin the wire-up: with the post-process disconnected, every cell
//! would read back as `Value::Undef` (the stdlib stub return).
//!
//! Gated on `OCCT_AVAILABLE` (same convention as the topology / conformance
//! e2e tests); skipped on builds without OCCT.
//!
//! ## FK placement applied via ApplyTransform (task 3906 T8)
//!
//! The Snapshot's per-body `world_transform` IS applied to the OCCT shape
//! before the distance probe, via the shared `GeometryOp::ApplyTransform`
//! primitive (same path as T5 static `at` placement). The first three fixtures
//! use `fixed()` joints and `translate`-positioned source lets so their
//! `world_transform` is identity — the identity short-circuit means no extra
//! kernel op and distances are unchanged. The `fk_posed_cubes_*` test (added
//! by T8) proves the full FK-posed path: two unit cubes at the source-let
//! origin, with cube_b's prismatic joint bound to +40mm, are probed as disjoint.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_compiler::compile_with_stdlib;
use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("mechanism_interference_smoke"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:#?}", errors);
    compiled
}

fn engine_with_occt() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(OcctKernelHandle::spawn())))
}

fn cell<'a>(values: &'a reify_ir::ValueMap, entity: &str, name: &str) -> &'a Value {
    let id = ValueCellId::new(entity, name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("{entity}.{name} not found in eval result"))
}

fn read_si_f64(v: &Value, label: &str) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("{label}: expected numeric, got {other:?}"),
    }
}

// ─── disjoint cubes: 30mm gap between two 20mm cubes ──────────────────────────
//
// Box A at origin spans [0,20]×[0,20]×[0,20] (mm); Box B at +30mm on x spans
// [30,50]×[0,20]×[0,20]. Closest face-to-face distance is 10mm.
//
// Expected:
//   pairs           → empty list
//   collide_ab      → false
//   clearance_ab    → 0.010 m (10mm) — strictly positive
const DISJOINT_SOURCE: &str = r#"
structure def Disjoint {
    let cube_a = box(20mm, 20mm, 20mm)
    let cube_b = translate(box(20mm, 20mm, 20mm), 30mm, 0mm, 0mm)

    let m0 = mechanism()
    let m1 = body(m0, "cube_a", fixed())
    let m2 = body(m1, "cube_b", fixed())
    let s = snapshot(m2, [])

    let id_a = body_id_of(m2, "cube_a")
    let id_b = body_id_of(m2, "cube_b")

    let pairs = interferes(s)
    let collide_ab = interferes_with(s, id_a, id_b)
    let clearance_ab = min_clearance(s, id_a, id_b)
}
"#;

#[test]
fn disjoint_cubes_no_pairs_and_positive_clearance() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(DISJOINT_SOURCE);
    let mut engine = engine_with_occt();
    let result = engine.build(&compiled, ExportFormat::Step);

    let pairs = cell(&result.values, "Disjoint", "pairs");
    let collide_ab = cell(&result.values, "Disjoint", "collide_ab");
    let clearance_ab = cell(&result.values, "Disjoint", "clearance_ab");

    match pairs {
        Value::List(items) => {
            assert!(
                items.is_empty(),
                "interferes(s) must be empty for two disjoint cubes, got {items:?}"
            );
        }
        other => panic!("interferes(s) must be Value::List, got {other:?}"),
    }
    assert_eq!(
        collide_ab,
        &Value::Bool(false),
        "interferes_with(s, a, b) must be false for disjoint cubes, got {collide_ab:?}"
    );
    let clearance_m = read_si_f64(clearance_ab, "clearance_ab");
    // Tolerate sub-µm OCCT noise on the 10mm expected clearance.
    let expected = 0.010_f64;
    assert!(
        (clearance_m - expected).abs() < 1e-6,
        "min_clearance must be ~{expected} m, got {clearance_m}",
    );
}

// ─── overlapping cubes: 5mm penetration ───────────────────────────────────────
//
// Box A at origin spans [0,20]³ (mm); Box B translated +15mm on x spans
// [15,35]×[0,20]×[0,20]. Cubes overlap on x in [15,20] — a 5mm penetration
// in x, 20mm × 20mm overlap in y/z.
//
// Expected:
//   pairs           → list with one Map { "a": 0, "b": 1 }
//   collide_ab      → true
//   clearance_ab    → 0.0 m (≤0 — Distance for intersecting shapes is reported
//                     as 0 by `BRepExtrema_DistShapeShape`)
const OVERLAPPING_SOURCE: &str = r#"
structure def Overlapping {
    let cube_a = box(20mm, 20mm, 20mm)
    let cube_b = translate(box(20mm, 20mm, 20mm), 15mm, 0mm, 0mm)

    let m0 = mechanism()
    let m1 = body(m0, "cube_a", fixed())
    let m2 = body(m1, "cube_b", fixed())
    let s = snapshot(m2, [])

    let id_a = body_id_of(m2, "cube_a")
    let id_b = body_id_of(m2, "cube_b")

    let pairs = interferes(s)
    let collide_ab = interferes_with(s, id_a, id_b)
    let clearance_ab = min_clearance(s, id_a, id_b)
}
"#;

#[test]
fn overlapping_cubes_one_pair_and_zero_clearance() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(OVERLAPPING_SOURCE);
    let mut engine = engine_with_occt();
    let result = engine.build(&compiled, ExportFormat::Step);

    let pairs = cell(&result.values, "Overlapping", "pairs");
    let collide_ab = cell(&result.values, "Overlapping", "collide_ab");
    let clearance_ab = cell(&result.values, "Overlapping", "clearance_ab");

    match pairs {
        Value::List(items) => {
            assert_eq!(
                items.len(),
                1,
                "interferes(s) must contain exactly one pair for overlapping cubes, got {items:?}"
            );
            // Pair-record contract: { "a": Int(id_lower), "b": Int(id_higher) }.
            let pair_map = match &items[0] {
                Value::Map(m) => m,
                other => panic!("pair entry must be Value::Map, got {other:?}"),
            };
            assert_eq!(
                pair_map.get(&Value::String("a".to_string())),
                Some(&Value::Int(0)),
                "first pair's `a` must be Int(0), got {pair_map:?}"
            );
            assert_eq!(
                pair_map.get(&Value::String("b".to_string())),
                Some(&Value::Int(1)),
                "first pair's `b` must be Int(1), got {pair_map:?}"
            );
        }
        other => panic!("interferes(s) must be Value::List, got {other:?}"),
    }
    assert_eq!(
        collide_ab,
        &Value::Bool(true),
        "interferes_with(s, a, b) must be true for overlapping cubes, got {collide_ab:?}"
    );
    let clearance_m = read_si_f64(clearance_ab, "clearance_ab");
    assert!(
        clearance_m <= 1e-6,
        "min_clearance must be ≤0 (≤1µm tolerance) for overlapping cubes, got {clearance_m}",
    );
}

// ─── single body: self-pair must be excluded by `i < j` iteration ─────────────
//
// One cube → one body → no pairs. `i < j` upper-triangular iteration excludes
// `(0, 0)` self-pairs. `interferes_with(s, id_a, id_a)` short-circuits to
// `Bool(false)` per the same self-pair rule. `min_clearance(s, id_a, id_a)`
// returns Undef — clearance to self is undefined.
const SELF_PAIR_SOURCE: &str = r#"
structure def SelfPair {
    let cube_a = box(20mm, 20mm, 20mm)

    let m0 = mechanism()
    let m1 = body(m0, "cube_a", fixed())
    let s = snapshot(m1, [])

    let id_a = body_id_of(m1, "cube_a")

    let pairs = interferes(s)
    let collide_self = interferes_with(s, id_a, id_a)
    let clearance_self = min_clearance(s, id_a, id_a)
}
"#;

#[test]
fn single_body_self_pair_excluded() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(SELF_PAIR_SOURCE);
    let mut engine = engine_with_occt();
    let result = engine.build(&compiled, ExportFormat::Step);

    let pairs = cell(&result.values, "SelfPair", "pairs");
    let collide_self = cell(&result.values, "SelfPair", "collide_self");
    let clearance_self = cell(&result.values, "SelfPair", "clearance_self");

    match pairs {
        Value::List(items) => {
            assert!(
                items.is_empty(),
                "interferes(s) must be empty for single-body snapshot (self-pair excluded), got {items:?}"
            );
        }
        other => panic!("interferes(s) must be Value::List, got {other:?}"),
    }
    assert_eq!(
        collide_self,
        &Value::Bool(false),
        "interferes_with(s, a, a) must be false (self-pair exclusion), got {collide_self:?}"
    );
    assert!(
        clearance_self.is_undef(),
        "min_clearance(s, a, a) must be Undef (self-clearance is undefined), got {clearance_self:?}"
    );
}

// ─── FK-posed cubes: world_transform routes geometry through ApplyTransform ───
//
// Two 20mm unit cubes whose SOURCE-lets both sit at the origin (fully overlapping
// if FK is ignored). cube_b is mounted on a prismatic joint bound to +40mm along X,
// so its FK world_transform poses it to span [40,60]mm — 20mm clear of cube_a's
// [0,20]mm span.
//
// T8 acceptance signal: after FK world_transform is applied via the shared
// ApplyTransform path, interference queries operate on posed geometry.
//
// Expected (POSED, post-T8):
//   pairs           → empty list (cubes are 20mm apart)
//   collide_ab      → false
//   clearance_ab    → 0.020 m (20mm) — strictly positive
//
// RED on main (world_transform ignored):
//   source-lets both at origin → cubes fully overlap → pairs=[{a,b}], collide=true,
//   clearance≈0 — all three assertions below fail.
const FK_POSED_SOURCE: &str = r#"
structure def FkPosed {
    let cube_a = box(20mm, 20mm, 20mm)
    let cube_b = box(20mm, 20mm, 20mm)

    let j = prismatic(vec3(1, 0, 0), 0mm .. 100mm)

    let m0 = mechanism()
    let m1 = body(m0, "cube_a", fixed())
    let m2 = body(m1, "cube_b", j)

    let binding = bind(j, 40mm)
    let s = snapshot(m2, [binding])

    let id_a = body_id_of(m2, "cube_a")
    let id_b = body_id_of(m2, "cube_b")

    let pairs = interferes(s)
    let collide_ab = interferes_with(s, id_a, id_b)
    let clearance_ab = min_clearance(s, id_a, id_b)
}
"#;

#[test]
fn fk_posed_cubes_no_interference_and_correct_clearance() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(FK_POSED_SOURCE);
    let mut engine = engine_with_occt();
    let result = engine.build(&compiled, ExportFormat::Step);

    let pairs = cell(&result.values, "FkPosed", "pairs");
    let collide_ab = cell(&result.values, "FkPosed", "collide_ab");
    let clearance_ab = cell(&result.values, "FkPosed", "clearance_ab");

    // FK world_transform applied: cube_b posed to [40,60]mm → no interference.
    match pairs {
        Value::List(items) => {
            assert!(
                items.is_empty(),
                "interferes(s) must be empty when FK-posed cubes are 20mm apart, got {items:?}"
            );
        }
        other => panic!("interferes(s) must be Value::List, got {other:?}"),
    }
    assert_eq!(
        collide_ab,
        &Value::Bool(false),
        "interferes_with must be false when FK-posed cubes are clear, got {collide_ab:?}"
    );
    let clearance_m = read_si_f64(clearance_ab, "clearance_ab");
    let expected = 0.020_f64;
    assert!(
        (clearance_m - expected).abs() < 1e-6,
        "min_clearance must be ~{expected} m (20mm FK-posed gap), got {clearance_m}",
    );
}

// ─── swept clearance: flat_map over FK-swept snapshots → monotonic to zero ───
//
// head: box(20mm,20mm,20mm) — centered; right face at x = v + 10mm (half-width 10mm).
//       Moved by prismatic j (X-axis, 0..300mm).
// dock: translate(box(200mm,20mm,20mm), 200mm, 0mm, 0mm) — fixed.
//       box() is centered → spans [−100, +100] in x; translate +200mm → [100, 300] mm.
//
// 7 sweep steps: v = 0, 50, 100, 150, 200, 250, 300 mm
// clearance(v) = max(0, dock_left(100) − head_right(v+10)) = max(0, 90 − v) mm
//
// Steps 0mm (90mm), 50mm (40mm): strictly positive.
// Steps 100mm onward: head right face (v+10) ≥ 110 > 100 = dock left → 0 / interfering.
//
// Acceptance (PRD §11.1): clearances[0] > 1mm, sequence monotone non-increasing,
// clearances.last() ≈ 0 (interfering); at least one positive AND one near-zero entry.
//
// RED on current main: `try_eval_kinematic_query` returns None for the `flat_map`
// cell → clearances stays as List([Undef,…]) → `read_si_f64` panics on Undef.
const SWEPT_CLEARANCE_SOURCE: &str = r#"
structure def SweptClearance {
    let head = box(20mm, 20mm, 20mm)
    let dock = translate(box(200mm, 20mm, 20mm), 200mm, 0mm, 0mm)

    let j = prismatic(vec3(1, 0, 0), 0mm .. 300mm)

    let m0 = mechanism()
    let m1 = body(m0, "head", j)
    let m2 = body(m1, "dock", fixed())

    let id_head = body_id_of(m2, "head")
    let id_dock = body_id_of(m2, "dock")

    let snaps = sweep(m2, j, 0mm .. 300mm, 7)
    let clearances = flat_map(snaps, |s| [min_clearance(s, id_head, id_dock)])
}
"#;

#[test]
fn swept_min_clearance_monotonic_to_interference() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors(SWEPT_CLEARANCE_SOURCE);
    let mut engine = engine_with_occt();
    let result = engine.build(&compiled, ExportFormat::Step);

    let clearances = cell(&result.values, "SweptClearance", "clearances");

    let items = match clearances {
        Value::List(v) => v,
        other => panic!("clearances must be Value::List, got {other:?}"),
    };
    assert_eq!(
        items.len(),
        7,
        "clearances must have 7 entries (one per sweep step), got {}",
        items.len()
    );

    // Read all clearances as SI metres — panics on Value::Undef (RED signal).
    let vals: Vec<f64> = items
        .iter()
        .enumerate()
        .map(|(i, v)| read_si_f64(v, &format!("clearances[{i}]")))
        .collect();

    // clearances[0]: head right face at x=10mm (half-width 10mm, v=0),
    // dock left face at x=100mm (centered box +200mm → spans [100,300]); gap = 90mm.
    assert!(
        vals[0] > 1e-3,
        "clearances[0] must be strictly positive (>1mm), got {:.6} m",
        vals[0]
    );

    // Monotone non-increasing: each step <= previous + 1µm tolerance.
    for i in 1..vals.len() {
        assert!(
            vals[i] <= vals[i - 1] + 1e-6,
            "clearances not monotone non-increasing at step {i}: \
             vals[{i}]={:.6} > vals[{}]={:.6}",
            vals[i],
            i - 1,
            vals[i - 1]
        );
    }

    // Last entry ≈ 0 (head fully inside dock at x=300mm → interfering).
    assert!(
        vals[vals.len() - 1] < 1e-6,
        "clearances.last() must be near-zero (interfering), got {:.6} m",
        vals[vals.len() - 1]
    );

    // Must have at least one strictly-positive entry AND at least one near-zero entry
    // (the monotone transition from clear to interfering must be present).
    assert!(
        vals.iter().any(|&v| v > 1e-3),
        "clearances must have at least one strictly-positive entry (>1mm)"
    );
    assert!(
        vals.iter().any(|&v| v < 1e-6),
        "clearances must have at least one near-zero entry (interfering)"
    );
}
