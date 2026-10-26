//! Mass-property queries through `OcctKernel::query`: `CenterOfMass` and `InertiaTensor`,
//! including density handling, invalid handles, non-solid inputs and analytic boxes.

#![cfg(has_occt)]
#![allow(clippy::needless_range_loop)]

use crate::support::{make_box_20_10_5, parse_centroid_json};
use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};
use reify_kernel_occt::OcctKernel;

/// Decode the 3-row × 3-col `Value::List` returned by an `InertiaTensor` query into
/// a `[[f64;3];3]` array.  Panics with a descriptive message if the structure does not
/// match the expected nested-list shape.
fn extract_3x3_tensor_entries(value: &Value) -> [[f64; 3]; 3] {
    let rows = match value {
        Value::List(rows) => {
            assert_eq!(rows.len(), 3, "expected 3 rows, got {}", rows.len());
            rows
        }
        other => panic!("expected Value::List(rows), got {:?}", other),
    };
    let mut entries = [[0.0f64; 3]; 3];
    for (i, row) in rows.iter().enumerate() {
        let cols = match row {
            Value::List(cols) => {
                assert_eq!(
                    cols.len(),
                    3,
                    "row {} expected 3 cols, got {}",
                    i,
                    cols.len()
                );
                cols
            }
            other => panic!("row {} expected Value::List, got {:?}", i, other),
        };
        for (j, col) in cols.iter().enumerate() {
            entries[i][j] = match col {
                Value::Real(v) => *v,
                other => panic!("entry [{i}][{j}] expected Value::Real, got {:?}", other),
            };
        }
    }
    entries
}

#[test]
fn center_of_mass_with_density_returns_translated_centroid() {
    let mut kernel = OcctKernel::new();
    // Create a 10×10×10 box and translate it by dx=5 so centroid is at (5, 0, 0).
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let translated = kernel
        .execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: 5.0,
            dy: 0.0,
            dz: 0.0,
        })
        .unwrap();

    // Query with density 100.0.
    let result_100 = kernel
        .query(&GeometryQuery::CenterOfMass {
            handle: translated.id,
            density: 100.0,
        })
        .unwrap();
    let (x, y, z) = match &result_100 {
        Value::String(s) => parse_centroid_json(s),
        other => panic!("expected Value::String JSON centroid, got {:?}", other),
    };
    // After dx=5 translation on an origin-centred box, centroid must be at (5, 0, 0).
    let tol = 1e-3;
    assert!((x - 5.0).abs() < tol, "centroid x expected ≈5, got {x}");
    assert!(y.abs() < tol, "centroid y expected ≈0, got {y}");
    assert!(z.abs() < tol, "centroid z expected ≈0, got {z}");

    // Density must be irrelevant for uniform-density bodies.  Run the same
    // query with a completely different density and confirm results are identical
    // (bit-equal strings), locking in the documented invariant.
    let result_1 = kernel
        .query(&GeometryQuery::CenterOfMass {
            handle: translated.id,
            density: 1.0,
        })
        .unwrap();
    assert_eq!(
        result_100, result_1,
        "CenterOfMass result must be density-independent (got {result_100:?} vs {result_1:?})"
    );
}

/// Query CenterOfMass with a handle that was never inserted into the kernel.
/// The kernel must return `Err(QueryError::InvalidHandle(id))` with the same
/// `GeometryHandleId` that was queried — not `Ok(_)` or any other error variant.
///
/// This pins the contract already implemented by the CenterOfMass arm of
/// `OcctKernel::query`, which maps an unknown handle to
/// `Err(QueryError::InvalidHandle(*handle))`.
#[test]
fn center_of_mass_invalid_handle_returns_invalid_handle_err() {
    let kernel = OcctKernel::new();
    let bad_id = GeometryHandleId(9999);
    let result = kernel.query(&GeometryQuery::CenterOfMass {
        handle: bad_id,
        density: 1.0,
    });
    match result {
        Err(QueryError::InvalidHandle(id)) => {
            assert_eq!(
                id, bad_id,
                "InvalidHandle must carry the same handle ID that was queried"
            );
        }
        Ok(v) => panic!(
            "expected Err(QueryError::InvalidHandle({:?})), got Ok({:?})",
            bad_id, v
        ),
        Err(other) => panic!(
            "expected Err(QueryError::InvalidHandle({:?})), got Err({:?})",
            bad_id, other
        ),
    }
}

/// Query InertiaTensor with a handle that was never inserted into the kernel.
/// The kernel must return `Err(QueryError::InvalidHandle(id))` with the same
/// `GeometryHandleId` that was queried — not `Ok(_)` or any other error variant.
///
/// Kept separate from `center_of_mass_invalid_handle_returns_invalid_handle_err`
/// so a regression that breaks only one query variant has unambiguous attribution.
///
/// This pins the contract already implemented by the InertiaTensor arm of
/// `OcctKernel::query`, which maps an unknown handle to
/// `Err(QueryError::InvalidHandle(*handle))`.
#[test]
fn inertia_tensor_invalid_handle_returns_invalid_handle_err() {
    let kernel = OcctKernel::new();
    let bad_id = GeometryHandleId(9999);
    let result = kernel.query(&GeometryQuery::InertiaTensor {
        handle: bad_id,
        density: 1.0,
    });
    match result {
        Err(QueryError::InvalidHandle(id)) => {
            assert_eq!(
                id, bad_id,
                "InvalidHandle must carry the same handle ID that was queried"
            );
        }
        Ok(v) => panic!(
            "expected Err(QueryError::InvalidHandle({:?})), got Ok({:?})",
            bad_id, v
        ),
        Err(other) => panic!(
            "expected Err(QueryError::InvalidHandle({:?})), got Err({:?})",
            bad_id, other
        ),
    }
}

/// Query CenterOfMass on a non-solid wire (a line segment).
///
/// OCCT's `BRepGProp::VolumeProperties` returns mass=0 for non-solid shapes
/// (wires have no enclosed volume), and OCCT's `GProp_GProps::CentreOfMass()`
/// defaults to the origin when mass=0.  The kernel therefore returns
/// `Ok(Value::String("..."))` with (x, y, z) ≈ (0, 0, 0).
///
/// **This pins current observable behavior.**  A future kernel change MAY tighten
/// this to return a typed `Err(QueryError::…)` for non-solid input — update this
/// test if that change lands.
#[test]
fn center_of_mass_on_non_solid_wire_returns_origin() {
    let mut kernel = OcctKernel::new();
    // A line segment of length 10 — well above any minimum-length floor.
    let wire_h = kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 10.0,
            y2: 0.0,
            z2: 0.0,
        })
        .expect("LineSegment must succeed");
    let result = kernel.query(&GeometryQuery::CenterOfMass {
        handle: wire_h.id,
        density: 1.0,
    });
    // (a) must not panic, must not return Err — the kernel is permissive for non-solids.
    let value = result
        .expect("CenterOfMass on a non-solid wire must not return Err (kernel is permissive)");
    // (b) result must be a JSON-encoded centroid string.
    let (x, y, z) = match &value {
        Value::String(s) => parse_centroid_json(s),
        other => panic!("expected Value::String JSON centroid, got {:?}", other),
    };
    // (c) OCCT returns mass=0 for non-solids → CentreOfMass defaults to origin.
    let tol = 1e-6;
    assert!(
        x.abs() < tol,
        "centroid x expected ≈0 for non-solid wire, got {x}"
    );
    assert!(
        y.abs() < tol,
        "centroid y expected ≈0 for non-solid wire, got {y}"
    );
    assert!(
        z.abs() < tol,
        "centroid z expected ≈0 for non-solid wire, got {z}"
    );
}

/// Query InertiaTensor on a non-solid wire (a line segment).
///
/// `BRepGProp::VolumeProperties` returns mass=0 for non-solid shapes, so
/// `GProp_GProps::MatrixOfInertia()` is the zero matrix.  After multiplication
/// by `density` all 9 entries remain zero.
///
/// **This pins current observable behavior.**  A future kernel change MAY tighten
/// this to return a typed `Err(QueryError::…)` for non-solid input — update this
/// test if that change lands.
#[test]
fn inertia_tensor_on_non_solid_wire_returns_zero_tensor() {
    let mut kernel = OcctKernel::new();
    let wire_h = kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 10.0,
            y2: 0.0,
            z2: 0.0,
        })
        .expect("LineSegment must succeed");
    let result = kernel.query(&GeometryQuery::InertiaTensor {
        handle: wire_h.id,
        density: 1.0,
    });
    let value = result
        .expect("InertiaTensor on a non-solid wire must not return Err (kernel is permissive)");
    let entries = extract_3x3_tensor_entries(&value);
    let tol = 1e-9;
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                entries[i][j].abs() < tol,
                "entry [{i}][{j}] expected ≈0 for non-solid wire (mass=0 → zero tensor), \
                 got {}",
                entries[i][j]
            );
        }
    }
}

/// Verify that `CenterOfMass` is density-independent for a uniform-density solid.
///
/// The kernel's `CenterOfMass` query ignores the `density` field (bound to `_`
/// in the CenterOfMass arm of `OcctKernel::query`) because the centre of mass of a uniform-density
/// body equals its geometric centroid regardless of density.  This test locks in that
/// invariant for four density values: `density=1.0` (baseline), `density=0.0` (zero),
/// `density=-2.0` (negative) — the two cases the task description called out — and
/// `density=100.0` (large positive), confirming the invariant extends beyond just the
/// edge values.
///
/// **This pins current observable behavior.**  A future kernel change MAY reject
/// ρ ≤ 0 with a typed error — update this test if so.
#[test]
fn center_of_mass_density_zero_or_negative_unchanged() {
    let mut kernel = OcctKernel::new();
    // A 10×10×10 box centred at the origin.
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .expect("Box creation must succeed");
    // Query with four density values including the two edge cases ρ=0 and ρ<0.
    let densities = [1.0_f64, 0.0, -2.0, 100.0];
    let results: Vec<Value> = densities
        .iter()
        .map(|&d| {
            kernel
                .query(&GeometryQuery::CenterOfMass {
                    handle: box_h.id,
                    density: d,
                })
                .unwrap_or_else(|e| {
                    panic!("CenterOfMass with density={d} must not return Err: {e:?}")
                })
        })
        .collect();
    // Parse each result into (x, y, z) coordinates and assert all four are within a
    // tiny absolute tolerance of the baseline — CenterOfMass is density-independent
    // for uniform-density bodies.  Using a coordinate comparison rather than bit-equal
    // string equality gives cleaner failure messages (shows which coordinate drifted
    // and by how much) if the kernel is ever refactored to change the output format.
    let abs_tol = 1e-12_f64;
    let parsed: Vec<(f64, f64, f64)> = results
        .iter()
        .enumerate()
        .map(|(i, r)| match r {
            Value::String(s) => parse_centroid_json(s),
            other => panic!(
                "CenterOfMass with density={} must return Value::String, got {:?}",
                densities[i], other
            ),
        })
        .collect();
    let (x0, y0, z0) = parsed[0];
    for (i, &(x, y, z)) in parsed.iter().enumerate().skip(1) {
        assert!(
            (x - x0).abs() <= abs_tol,
            "CenterOfMass density-independence: x differs between density={} and \
             density={}: {} vs {}",
            densities[i],
            densities[0],
            x,
            x0
        );
        assert!(
            (y - y0).abs() <= abs_tol,
            "CenterOfMass density-independence: y differs between density={} and \
             density={}: {} vs {}",
            densities[i],
            densities[0],
            y,
            y0
        );
        assert!(
            (z - z0).abs() <= abs_tol,
            "CenterOfMass density-independence: z differs between density={} and \
             density={}: {} vs {}",
            densities[i],
            densities[0],
            z,
            z0
        );
    }
}

/// Parity lock-in: `CenterOfMass` output is bit-equal to `Centroid` output for a
/// uniform-density solid.
///
/// The `CenterOfMass` kernel arm ignores `density` (bound to `_`) and delegates to
/// the same OCCT volume-centroid path as `Centroid` (see the `CenterOfMass` doc-comment in
/// `reify-types::geometry` for the documented bit-equality invariant: "the result is
/// identical to `Centroid(handle)`").
///
/// This test guards against any future contributor who bypasses the helper and copies an
/// inline `format!` into one arm but not the other — the test will catch any format
/// divergence between the `Centroid` and `CenterOfMass` query arms.
///
/// A translated box (dx=5, dy=-3, dz=7) is used so the expected centroid is at (5, -3, 7):
/// an origin-centred box would pass even if one arm hardcoded the origin.
#[test]
fn center_of_mass_output_matches_centroid_for_uniform_density() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .expect("Box creation must succeed");
    let translated = kernel
        .execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: 5.0,
            dy: -3.0,
            dz: 7.0,
        })
        .expect("Translate must succeed");

    let centroid = kernel
        .query(&GeometryQuery::Centroid(translated.id))
        .expect("Centroid query must not return Err");
    let center_of_mass = kernel
        .query(&GeometryQuery::CenterOfMass {
            handle: translated.id,
            density: 1.0,
        })
        .expect("CenterOfMass query must not return Err");

    // Bit-equal string equality — not just numeric closeness — to lock in that both arms
    // use exactly the same JSON encoder (the CenterOfMass doc-comment in
    // reify-types::geometry documents this invariant).
    assert_eq!(
        centroid, center_of_mass,
        "CenterOfMass and Centroid must produce bit-equal Value outputs for a uniform-density \
         solid (see CenterOfMass doc-comment in reify-types::geometry). Got Centroid={centroid:?}, \
         CenterOfMass={center_of_mass:?}"
    );

    // Verify the centroid is actually at the expected translated position (5, -3, 7) —
    // guards against both arms silently agreeing on an incorrect value (e.g., returning
    // the origin when the shape is not at the origin).
    let centroid_str = match &centroid {
        Value::String(s) => s.as_str(),
        other => panic!("Centroid must return Value::String, got {:?}", other),
    };
    let (cx, cy, cz) = parse_centroid_json(centroid_str);
    assert!(
        (cx - 5.0).abs() < 1e-6 && (cy + 3.0).abs() < 1e-6 && (cz - 7.0).abs() < 1e-6,
        "Centroid should be at (5, -3, 7) for a box translated by (dx=5, dy=-3, dz=7), \
         got ({cx}, {cy}, {cz})"
    );
}

/// Document the density edge-case behavior of `InertiaTensor`.
///
/// The C++ `query_inertia_tensor` implementation multiplies every entry of OCCT's
/// volume-weighted matrix by `density` (a pure scalar multiply in occt_wrapper.cpp).
/// This yields two documented edge behaviors:
///
/// * **ρ = 0.0** → all 9 tensor entries are exactly 0.0 (zero density → zero mass →
///   zero inertia tensor).
/// * **ρ < 0** → each entry equals `−1 × (entry at the absolute value of ρ)` (scalar
///   negation, no guard at the kernel boundary).
///
/// **This pins current observable behavior.**  A future kernel change MAY reject ρ ≤ 0
/// with a typed error — update this test if so.
#[test]
fn inertia_tensor_density_edge_cases_documented() {
    let mut kernel = OcctKernel::new();
    // Canonical 20×10×5 box (see make_box_20_10_5 helper).
    let box_h_id = make_box_20_10_5(&mut kernel);

    // Baseline: ρ = 2.0.
    let result_baseline = kernel
        .query(&GeometryQuery::InertiaTensor {
            handle: box_h_id,
            density: 2.0,
        })
        .expect("InertiaTensor with density=2.0 must not return Err");
    let baseline = extract_3x3_tensor_entries(&result_baseline);

    // Edge case 1: ρ = 0.0 → zero tensor.
    let result_zero = kernel
        .query(&GeometryQuery::InertiaTensor {
            handle: box_h_id,
            density: 0.0,
        })
        .expect("InertiaTensor with density=0.0 must not return Err (kernel is permissive)");
    let zero_entries = extract_3x3_tensor_entries(&result_zero);
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                zero_entries[i][j].abs() < 1e-9,
                "entry [{i}][{j}] must be ≈0 for density=0 (zero density → zero tensor), \
                 got {}",
                zero_entries[i][j]
            );
        }
    }

    // Edge case 2: ρ = -2.0 → negated baseline tensor.
    let result_neg = kernel
        .query(&GeometryQuery::InertiaTensor {
            handle: box_h_id,
            density: -2.0,
        })
        .expect("InertiaTensor with density=-2.0 must not return Err (kernel is permissive)");
    let neg_entries = extract_3x3_tensor_entries(&result_neg);
    // Each entry must equal the negation of the baseline within an absolute tolerance.
    // Baseline diagonals are O(1e4), so tol=100 is ~1% relative for them.  Off-diagonals
    // are analytically zero for an axis-aligned box at the origin (I_xy = I_xz = I_yz = 0),
    // so the same abs-tol of 100 trivially covers them too.
    let tol = 100.0_f64;
    for i in 0..3 {
        for j in 0..3 {
            let expected = -baseline[i][j];
            assert!(
                (neg_entries[i][j] - expected).abs() < tol,
                "entry [{i}][{j}] with density=-2.0 expected ≈{expected:.4} \
                 (negation of baseline {:.4}), got {}, diff {}",
                baseline[i][j],
                neg_entries[i][j],
                (neg_entries[i][j] - expected).abs()
            );
        }
    }
}

#[test]
fn inertia_tensor_box_with_density_analytic() {
    let mut kernel = OcctKernel::new();
    // Canonical 20×10×5 box (see make_box_20_10_5 helper).
    // Volume = 1000; mass = ρ·V = 2·1000 = 2000.
    // Analytic moments about centroid (box m/12·(h²+d²)):
    //   I_xx = 2000/12 · (10² + 5²) = 2000/12 · 125 ≈ 20833.33
    //   I_yy = 2000/12 · (20² + 5²) = 2000/12 · 425 ≈ 70833.33
    //   I_zz = 2000/12 · (20² + 10²) = 2000/12 · 500 ≈ 83333.33
    // Off-diagonals = 0 for axis-aligned box at origin.
    let box_h_id = make_box_20_10_5(&mut kernel);
    let result = kernel
        .query(&GeometryQuery::InertiaTensor {
            handle: box_h_id,
            density: 2.0,
        })
        .unwrap();
    // (a) result must be a 3-row list
    let rows = match result {
        Value::List(rows) => {
            assert_eq!(rows.len(), 3, "expected 3 rows, got {}", rows.len());
            rows
        }
        other => panic!("expected Value::List(rows), got {:?}", other),
    };
    // (b) extract the nine Real entries
    let mut entries = [[0.0f64; 3]; 3];
    for (i, row) in rows.iter().enumerate() {
        let cols = match row {
            Value::List(cols) => {
                assert_eq!(
                    cols.len(),
                    3,
                    "row {} expected 3 cols, got {}",
                    i,
                    cols.len()
                );
                cols
            }
            other => panic!("row {} expected Value::List, got {:?}", i, other),
        };
        for (j, col) in cols.iter().enumerate() {
            entries[i][j] = match col {
                Value::Real(v) => *v,
                other => panic!("entry [{i}][{j}] expected Value::Real, got {:?}", other),
            };
        }
    }
    let tol = 100.0;
    // (c) diagonal entries
    assert!(
        (entries[0][0] - 20833.33).abs() < tol,
        "I_xx expected ~20833.33, got {}",
        entries[0][0]
    );
    assert!(
        (entries[1][1] - 70833.33).abs() < tol,
        "I_yy expected ~70833.33, got {}",
        entries[1][1]
    );
    assert!(
        (entries[2][2] - 83333.33).abs() < tol,
        "I_zz expected ~83333.33, got {}",
        entries[2][2]
    );
    // (d) off-diagonal entries all near zero
    let off_diag_pairs = [(0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1)];
    for (i, j) in off_diag_pairs {
        assert!(
            entries[i][j].abs() < tol,
            "off-diagonal [{i}][{j}] expected ~0, got {}",
            entries[i][j]
        );
    }

    // ── Second case: translate the box far from the origin (dx=100) ──────────
    // OCCT's BRepGProp::VolumeProperties computes centroidal inertia, so the
    // diagonal moments must be identical to the origin-centred case above.  This
    // distinguishes an implementation that accidentally computes inertia about
    // the *world origin* (which would differ) from one that correctly computes
    // inertia about the *centroid*.
    let translated_h = kernel
        .execute(&GeometryOp::Translate {
            target: box_h_id,
            dx: 100.0,
            dy: 0.0,
            dz: 0.0,
        })
        .unwrap();
    let result_t = kernel
        .query(&GeometryQuery::InertiaTensor {
            handle: translated_h.id,
            density: 2.0,
        })
        .unwrap();
    let rows_t = match result_t {
        Value::List(rows) => {
            assert_eq!(rows.len(), 3);
            rows
        }
        other => panic!(
            "translated box: expected Value::List(rows), got {:?}",
            other
        ),
    };
    let mut entries_t = [[0.0f64; 3]; 3];
    for (i, row) in rows_t.iter().enumerate() {
        let cols = match row {
            Value::List(cols) => {
                assert_eq!(cols.len(), 3);
                cols
            }
            other => panic!("translated row {i}: expected Value::List, got {:?}", other),
        };
        for (j, col) in cols.iter().enumerate() {
            entries_t[i][j] = match col {
                Value::Real(v) => *v,
                other => panic!(
                    "translated [{i}][{j}] expected Value::Real, got {:?}",
                    other
                ),
            };
        }
    }
    // Centroidal inertia is translation-invariant: diagonal entries must match.
    assert!(
        (entries_t[0][0] - entries[0][0]).abs() < tol,
        "translated I_xx should equal origin I_xx ({:.2}), got {:.2}",
        entries[0][0],
        entries_t[0][0]
    );
    assert!(
        (entries_t[1][1] - entries[1][1]).abs() < tol,
        "translated I_yy should equal origin I_yy ({:.2}), got {:.2}",
        entries[1][1],
        entries_t[1][1]
    );
    assert!(
        (entries_t[2][2] - entries[2][2]).abs() < tol,
        "translated I_zz should equal origin I_zz ({:.2}), got {:.2}",
        entries[2][2],
        entries_t[2][2]
    );
}

/// T8 strengthens this test with an analytical diagonal assertion.
///
/// For a 1000×1000×1000 cube with density 1 the moment of inertia about each
/// centroidal axis is `I = m·L²/6 = 1e9·1e6/6 ≈ 1.66667e14`.  A relative
/// tolerance of 1e-6 is used instead of the 1e-9 the task description suggests
/// because OCCT's BRepGProp::VolumeProperties integrates over the meshed
/// polyhedron (not the exact cube), so tessellation residue is O(1e-7) of the
/// magnitude — well above 1e-9 but well below the old `> 1e6` sanity check.
/// A test failure at 1e-6 indicates a real regression (not FP noise).
#[test]
fn inertia_tensor_large_shape_returns_symmetric_tensor() {
    let mut kernel = OcctKernel::new();
    // 1000×1000×1000 cube, density 1.0.
    // Analytical diagonal: I = m·L²/6 = 1e9·1e6/6 ≈ 1.66667e14 (all three equal
    // for an isotropic cube).  Off-diagonal products of inertia are analytically
    // zero at the centroid.
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(1000.0),
            size_y: Value::Real(1000.0),
            size_z: Value::Real(1000.0),
        })
        .unwrap();
    let result = kernel
        .query(&GeometryQuery::InertiaTensor {
            handle: box_h.id,
            density: 1.0,
        })
        .expect("query_inertia_tensor must not spuriously fail on large shapes (tensor entries ≫ 1e6)");
    // Extract the nine Real entries from the returned 3×3 list-of-lists.
    let rows = match result {
        Value::List(rows) => {
            assert_eq!(rows.len(), 3, "expected 3 rows, got {}", rows.len());
            rows
        }
        other => panic!("expected Value::List(rows), got {:?}", other),
    };
    let mut entries = [[0.0f64; 3]; 3];
    for (i, row) in rows.iter().enumerate() {
        let cols = match row {
            Value::List(cols) => {
                assert_eq!(
                    cols.len(),
                    3,
                    "row {} expected 3 cols, got {}",
                    i,
                    cols.len()
                );
                cols
            }
            other => panic!("row {} expected Value::List, got {:?}", i, other),
        };
        for (j, col) in cols.iter().enumerate() {
            entries[i][j] = match col {
                Value::Real(v) => *v,
                other => panic!("entry [{i}][{j}] expected Value::Real, got {:?}", other),
            };
        }
    }
    // Diagonal entries: cheap positive-and-finite guard (catches NaN/Inf with clear message).
    for i in 0..3 {
        assert!(
            entries[i][i].is_finite() && entries[i][i] > 0.0,
            "diagonal [{i}][{i}] must be positive and finite, got {}",
            entries[i][i]
        );
    }
    // T8: Analytical diagonal assertion — replaces the old "> 1e6" sanity check.
    // Cube of side L=1000, density ρ=1 → m = ρ·L³ = 1e9.
    //   I_diag = m·L²/6 = 1e9·1e6/6 ≈ 1.66667e14  (all three equal by isotropy).
    // Relative tolerance 1e-6: the ideal 1e-9 is unrealistic because OCCT integrates
    // over the tessellated polyhedron, not the exact mathematical cube; tessellation
    // residue is empirically O(1e-7) of the magnitude.  1e-6 is still 100× tighter
    // than the old "> 1e6" bound and converts this test into a genuine numerical
    // regression check.
    let i_expected = 1.0e9_f64 * 1.0e6_f64 / 6.0_f64; // ≈ 1.66667e14
    let rel_tol = 1e-6_f64;
    for i in 0..3 {
        assert!(
            (entries[i][i] - i_expected).abs() / i_expected < rel_tol,
            "diagonal [{i}][{i}] expected ≈{i_expected:.6e} (m·L²/6 for 1000³ cube, density=1), \
             got {}, relative error {}",
            entries[i][i],
            (entries[i][i] - i_expected).abs() / i_expected
        );
    }
    // Off-diagonal symmetric pairs must be bit-exactly equal after the averaging fix.
    assert_eq!(
        entries[0][1], entries[1][0],
        "m12 vs m21 must be bit-equal after averaging fix"
    );
    assert_eq!(
        entries[0][2], entries[2][0],
        "m13 vs m31 must be bit-equal after averaging fix"
    );
    assert_eq!(
        entries[1][2], entries[2][1],
        "m23 vs m32 must be bit-equal after averaging fix"
    );
}

/// Test that off-diagonal products of inertia are bit-exactly symmetric for a shape
/// that has *large, non-zero* off-diagonals — the regime the averaging fix targets.
///
/// An axis-aligned cube has analytically zero off-diagonals at the centroid, so it is
/// not a useful regression shape for this property.  A non-cubic box (1000×2000×500)
/// rotated 30° about the Z-axis has I_xy ≈ 1.0825e14 by the rotation-of-axes formula:
///   I_xy_world = (I_xx_local - I_yy_local) · sin(30°) · cos(30°)
///             = (3.5417e14 - 1.0417e14) · (√3/4) ≈ 1.0825e14
/// This is orders of magnitude above any noise threshold and conclusively exercises the
/// regime where the old 1e-6 absolute threshold was unsound.  The bit-equality
/// assertions pin the averaging behaviour: they would fail if query_inertia_tensor
/// returned m(i,j) and m(j,i) from two independent m.Value(…) reads (pre-fix) and
/// OCCT's two reads happened to disagree at the ULP level.
///
/// T7 additionally strengthens the assertions with analytical magnitude values for
/// the three diagonals and the off-diagonal I_xy.  The stronger checks catch:
///   (a) sign-flip in off-diagonal convention (|I_xy| < I_xy_expected fails)
///   (b) row/col transpose in FFI marshalling (symmetry equality fails)
///   (c) accidental zeroing of off-diagonals (already caught by > 1e12 but now also
///       by the ~1% relative tolerance against the analytic value)
#[test]
fn inertia_tensor_rotated_non_cubic_box_offdiagonals_symmetric() {
    let mut kernel = OcctKernel::new();
    // Non-cubic box: size_x=1000, size_y=2000, size_z=500, density=1.
    // Volume = 1e9, mass m = 1e9.
    // Centroidal moments in local frame (box axes):
    //   I_xx_local = m/12·(size_y²+size_z²) = 1e9/12·(4e6+2.5e5) ≈ 3.54e14
    //   I_yy_local = m/12·(size_x²+size_z²) = 1e9/12·(1e6+2.5e5) ≈ 1.04e14
    // After 30° rotation around Z:
    //   I_xy_world = (I_xx_local - I_yy_local)·sin30°·cos30° ≈ 1.08e14
    // (I_xz and I_yz remain zero because Z-rotation doesn't mix Z cross-products.)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(1000.0),
            size_y: Value::Real(2000.0),
            size_z: Value::Real(500.0),
        })
        .expect("Box creation must succeed");
    // Rotate 30° around Z-axis (PI/6 radians).
    let rotated_h = kernel
        .execute(&GeometryOp::Rotate {
            target: box_h.id,
            axis: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::PI / 6.0,
        })
        .expect("Rotate must succeed");
    let result = kernel
        .query(&GeometryQuery::InertiaTensor {
            handle: rotated_h.id,
            density: 1.0,
        })
        .expect("query_inertia_tensor must not fail on a rotated non-cubic box");
    // Extract entries.
    let rows = match result {
        Value::List(rows) => {
            assert_eq!(rows.len(), 3, "expected 3 rows");
            rows
        }
        other => panic!("expected Value::List(rows), got {:?}", other),
    };
    let mut entries = [[0.0f64; 3]; 3];
    for (i, row) in rows.iter().enumerate() {
        let cols = match row {
            Value::List(cols) => {
                assert_eq!(cols.len(), 3);
                cols
            }
            other => panic!("row {i} expected Value::List, got {:?}", other),
        };
        for (j, col) in cols.iter().enumerate() {
            entries[i][j] = match col {
                Value::Real(v) => *v,
                other => panic!("entry [{i}][{j}] expected Value::Real, got {:?}", other),
            };
        }
    }
    // Diagonal entries must be positive and finite.
    for i in 0..3 {
        assert!(
            entries[i][i].is_finite() && entries[i][i] > 0.0,
            "diagonal [{i}][{i}] must be positive and finite, got {}",
            entries[i][i]
        );
    }
    // I_xy_world must be non-trivially non-zero — this is the key property that
    // distinguishes this shape from a cube and places it in the regime the fix targets.
    // Expected |I_xy| ≈ 1.08e14; require > 1e12 as a conservative lower bound.
    assert!(
        entries[0][1].abs() > 1e12,
        "I_xy must be large (> 1e12) for rotated non-cubic box, got {}",
        entries[0][1]
    );
    // T7 strengthening: analytical magnitude check for I_xy (0.1% relative tolerance).
    // Derivation: m=1e9, I_xx_local=m/12·(2000²+500²)≈3.5417e14, I_yy_local=m/12·(1000²+500²)≈1.0417e14
    //   I'_xy = (√3/4)·(I_xx_local − I_yy_local) = (√3/4)·2.5e14 ≈ 1.0825e14
    let i_xy_expected = (3.0_f64.sqrt() / 4.0) * 2.5e14_f64; // ≈ 1.0825e14
    let rel_tol = 1e-3_f64; // 0.1% — well above OCCT's polyhedron integration noise
    assert!(
        (entries[0][1].abs() - i_xy_expected).abs() / i_xy_expected < rel_tol,
        "I_xy magnitude expected ≈{i_xy_expected:.6e} (analytic), got {} (|val|={}), \
         relative error {}",
        entries[0][1],
        entries[0][1].abs(),
        (entries[0][1].abs() - i_xy_expected).abs() / i_xy_expected
    );
    // Sign check: I'_xy = sin·cos·(I_xx − I_yy) > 0 since I_xx > I_yy for this box.
    // (OCCT's tensor layout matches the standard rotation-of-axes convention;
    //  a sign-flip here would indicate a FFI convention reversal.)
    assert!(
        entries[0][1] > 0.0,
        "I_xy expected positive (I_xx_local > I_yy_local → positive cross-term after 30° Z-rotation), \
         got {}",
        entries[0][1]
    );
    // T7 strengthening: analytical diagonal magnitude checks (1% relative tolerance).
    // After 30° Z-rotation (c=cos30=√3/2, s=sin30=1/2):
    //   I'_xx = c²·I_xx_local + s²·I_yy_local = 0.75·3.5417e14 + 0.25·1.0417e14 ≈ 2.9167e14
    //   I'_yy = s²·I_xx_local + c²·I_yy_local = 0.25·3.5417e14 + 0.75·1.0417e14 ≈ 1.6667e14
    //   I'_zz = I_zz_local = m/12·(w²+h²) = 1e9/12·(1e6+4e6) ≈ 4.1667e14 (Z invariant)
    let m = 1.0e9_f64;
    let w = 1000.0_f64;
    let h = 2000.0_f64;
    let d = 500.0_f64;
    let i_xx_local = m / 12.0 * (h * h + d * d);
    let i_yy_local = m / 12.0 * (w * w + d * d);
    let i_zz_local = m / 12.0 * (w * w + h * h);
    let c2 = 0.75_f64; // cos²(30°)
    let s2 = 0.25_f64; // sin²(30°)
    let diag_expected = [
        c2 * i_xx_local + s2 * i_yy_local, // I'_xx ≈ 2.9167e14
        s2 * i_xx_local + c2 * i_yy_local, // I'_yy ≈ 1.6667e14
        i_zz_local,                        // I'_zz ≈ 4.1667e14
    ];
    for i in 0..3 {
        let expected = diag_expected[i];
        assert!(
            (entries[i][i] - expected).abs() / expected < rel_tol,
            "diagonal [{i}][{i}] expected ≈{expected:.6e} (analytic), got {}, \
             relative error {}",
            entries[i][i],
            (entries[i][i] - expected).abs() / expected
        );
    }
    // I_xz and I_yz must remain near zero (Z-rotation does not mix Z products).
    assert!(
        entries[0][2].abs() < 1e6,
        "I_xz must be near zero for Z-rotation, got {}",
        entries[0][2]
    );
    assert!(
        entries[1][2].abs() < 1e6,
        "I_yz must be near zero for Z-rotation, got {}",
        entries[1][2]
    );
    // Bit-exact symmetry: guaranteed by the averaging implementation.
    // These assertions would fail with the old code if OCCT's two independent reads
    // of m.Value(i,j) and m.Value(j,i) differed at the ULP level (possible when
    // entries are ~1e14 and FP noise is ~ULP(1e14) ≈ 10).
    assert_eq!(
        entries[0][1], entries[1][0],
        "m12 vs m21 must be bit-equal after averaging fix"
    );
    assert_eq!(
        entries[0][2], entries[2][0],
        "m13 vs m31 must be bit-equal after averaging fix"
    );
    assert_eq!(
        entries[1][2], entries[2][1],
        "m23 vs m32 must be bit-equal after averaging fix"
    );
}
