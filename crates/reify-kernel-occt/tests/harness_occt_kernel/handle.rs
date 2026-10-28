//! `OcctKernelHandle` through its request channel: sync and async execute,
//! query, export and tessellate, drop and shutdown, warm-start transfer, and
//! topology extraction.

#![cfg(has_occt)]

use reify_ir::{BRepKind, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernelHandle;

/// Compile-time assertion: OcctKernelHandle must be Send + Sync.
const _: fn() = || {
    fn must_be_send_sync<T: Send + Sync>() {}
    must_be_send_sync::<OcctKernelHandle>();
};

#[test]
fn spawn_returns_handle_without_panic() {
    let handle = OcctKernelHandle::spawn();
    // Just verifying spawn() returns successfully without panic.
    drop(handle);
}

#[test]
fn execute_creates_box_and_returns_handle() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let result = handle.execute(&op).unwrap();
    assert_eq!(result.id, GeometryHandleId(1));
    assert_eq!(result.repr, Some(BRepKind::Solid));
}

#[test]
fn query_volume_returns_correct_value() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute(&op).unwrap();
    let result = handle
        .query(&reify_ir::GeometryQuery::Volume(gh.id))
        .unwrap();
    match result {
        Value::Real(v) => {
            // 10 * 20 * 30 = 6000
            assert!((v - 6000.0).abs() < 1.0, "expected ~6000, got {v}");
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn query_invalid_handle_returns_error() {
    let handle = OcctKernelHandle::spawn();
    let result = handle.query(&reify_ir::GeometryQuery::Volume(GeometryHandleId(999)));
    assert!(result.is_err());
    match result.unwrap_err() {
        reify_ir::QueryError::InvalidHandle(id) => {
            assert_eq!(id, GeometryHandleId(999));
        }
        other => panic!("expected InvalidHandle, got {:?}", other),
    }
}

/// Channel-routed `reset()` on `OcctKernelHandle`: evicts the underlying
/// shape on the dedicated kernel thread (old handle → error via the #5211
/// `get_shape` guard, never a panic or stale value) and a subsequent
/// execute mints a strictly-greater (monotonic) id. Also exercises the
/// polymorphic trait path (`&mut dyn GeometryKernel`) so the engine's
/// `Box<dyn GeometryKernel>` reset call is covered.
#[test]
fn reset_evicts_shape_and_keeps_next_id_monotonic() {
    let mut handle = OcctKernelHandle::spawn();
    let box_h = handle
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();
    assert!(
        handle.query(&GeometryQuery::Volume(box_h.id)).is_ok(),
        "box should be queryable before reset"
    );

    // Reset over the actor channel — evicts the resident shape.
    handle.reset();

    // The old handle is now absent from the shape table, so `get_shape`
    // surfaces InvalidHandle (a clean error, not a crash or a stale read).
    let after = handle.query(&GeometryQuery::Volume(box_h.id));
    assert!(
        matches!(after, Err(reify_ir::QueryError::InvalidHandle(_))),
        "old handle must surface as InvalidHandle after reset (shape \
         evicted from the table), got {after:?}"
    );

    // A fresh execute gets a strictly-greater id — reset kept next_id
    // monotonic on the kernel thread, so ids are never reused.
    let box_h2 = handle
        .execute(&GeometryOp::Box {
            size_x: Value::Real(1.0),
            size_y: Value::Real(1.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    assert!(
        box_h2.id.0 > box_h.id.0,
        "next_id must stay monotonic across channel-routed reset: {} !> {}",
        box_h2.id.0,
        box_h.id.0
    );

    // Polymorphic path: reset() through &mut dyn GeometryKernel (the shape
    // the engine's Box<dyn GeometryKernel> reset call takes) must evict too.
    let dyn_kernel: &mut dyn reify_ir::GeometryKernel = &mut handle;
    dyn_kernel.reset();
    let after2 = handle.query(&GeometryQuery::Volume(box_h2.id));
    assert!(
        matches!(after2, Err(reify_ir::QueryError::InvalidHandle(_))),
        "polymorphic reset() through &mut dyn GeometryKernel must also \
         evict the shape, got {after2:?}"
    );
}

#[test]
fn query_many_returns_ordered_values_for_heterogeneous_batch() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute(&op).unwrap();
    let result = handle
        .query_many(&[
            GeometryQuery::Volume(gh.id),
            GeometryQuery::SurfaceArea(gh.id),
        ])
        .expect("query_many should succeed for valid handles");
    assert_eq!(result.len(), 2, "expected one Value per query");
    match (&result[0], &result[1]) {
        (Value::Real(vol), Value::Real(area)) => {
            // 10 * 20 * 30 = 6000
            assert!(
                (vol - 6000.0).abs() < 1.0,
                "expected volume ~6000, got {vol}"
            );
            // 2 * (10*20 + 10*30 + 20*30) = 2200
            assert!(
                (area - 2200.0).abs() < 1.0,
                "expected surface area ~2200, got {area}"
            );
        }
        other => panic!("expected two Value::Real, got {:?}", other),
    }
}

#[test]
fn query_many_short_circuits_on_first_invalid_handle() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute(&op).unwrap();
    let result = handle.query_many(&[
        GeometryQuery::Volume(GeometryHandleId(999)),
        GeometryQuery::SurfaceArea(gh.id),
    ]);
    assert!(result.is_err(), "query_many must propagate the bad handle");
    match result.unwrap_err() {
        reify_ir::QueryError::InvalidHandle(id) => {
            assert_eq!(id, GeometryHandleId(999));
        }
        other => panic!("expected InvalidHandle, got {:?}", other),
    }
}

#[test]
fn query_many_empty_batch_returns_ok_empty_vec() {
    // The empty-batch fast path should return Ok(Vec::new()) without
    // routing through the actor channel. Observable behaviour is the
    // empty Ok result; the channel skip is documented in the doc-comment.
    let handle = OcctKernelHandle::spawn();
    let result = handle
        .query_many(&[])
        .expect("empty query_many should succeed");
    assert!(
        result.is_empty(),
        "empty batch should return empty Vec, got {:?}",
        result
    );
}

#[test]
fn export_unsupported_format_returns_error() {
    // Stl is now wired; use Obj (explicitly unsupported) to pin the
    // error-path contract for an unsupported format.
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute(&op).unwrap();
    let mut buf = Vec::new();
    let result = handle.export(gh.id, reify_ir::ExportFormat::Obj, &mut buf);
    assert!(result.is_err());
}

#[test]
fn handle_implements_geometry_kernel_trait() {
    use reify_ir::GeometryKernel;
    let mut handle = OcctKernelHandle::spawn();
    // Use it through the trait interface as Box<dyn GeometryKernel>
    let kernel: &mut dyn GeometryKernel = &mut handle;
    let op = GeometryOp::Box {
        size_x: Value::Real(5.0),
        size_y: Value::Real(5.0),
        size_z: Value::Real(5.0),
    };
    let gh = kernel.execute(&op).unwrap();
    assert_eq!(gh.id, GeometryHandleId(1));
}

#[test]
fn tessellate_returns_valid_mesh() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute(&op).unwrap();
    let mesh = handle.tessellate(gh.id, 0.1).unwrap();
    assert!(!mesh.vertices.is_empty(), "mesh should have vertices");
    assert!(!mesh.indices.is_empty(), "mesh should have indices");
    assert_eq!(
        mesh.indices.len() % 3,
        0,
        "indices should be divisible by 3 (triangles)"
    );
    assert!(mesh.normals.is_some(), "mesh should have normals");
}

#[test]
fn chamfer_all_edges_through_channel() {
    let handle = OcctKernelHandle::spawn();
    let box_op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(10.0),
        size_z: Value::Real(10.0),
    };
    let gh = handle.execute(&box_op).unwrap();
    let chamfer_op = GeometryOp::Chamfer {
        target: gh.id,
        edges: vec![],
        distance: Value::Real(1.0),
    };
    let result = handle.execute(&chamfer_op);
    assert!(
        result.is_ok(),
        "chamfer should succeed, got: {:?}",
        result.unwrap_err()
    );
    let chamfered = result.unwrap();
    assert!(
        chamfered.id.0 > 0,
        "chamfered shape should have a valid handle id, got {:?}",
        chamfered.id
    );
    // Verify the resulting shape is exportable and topologically valid by
    // exporting to STEP and checking the ISO-10303-21 header is present.
    let mut buf = Vec::new();
    handle
        .export(chamfered.id, reify_ir::ExportFormat::Step, &mut buf)
        .expect("chamfered shape should be exportable to STEP");
    let content = String::from_utf8(buf).expect("STEP output should be valid UTF-8");
    assert!(
        content.contains("ISO-10303-21"),
        "chamfered STEP export should contain ISO-10303-21 header"
    );
}

#[test]
fn export_invalid_handle_returns_error() {
    let handle = OcctKernelHandle::spawn();
    let mut buf = Vec::new();
    let result = handle.export(
        GeometryHandleId(999),
        reify_ir::ExportFormat::Step,
        &mut buf,
    );
    assert!(result.is_err());
    match result.unwrap_err() {
        reify_ir::ExportError::InvalidHandle(id) => {
            assert_eq!(id, GeometryHandleId(999));
        }
        other => panic!("expected InvalidHandle, got {:?}", other),
    }
}

/// Through the `GeometryKernel` seam production builds export from, an
/// unknown handle is `InvalidHandle` whatever the format.
#[test]
fn export_with_options_of_an_unknown_handle_reports_invalid_handle_for_every_format() {
    use reify_ir::{ExportError, ExportFormat, ExportOptions, GeometryKernel};
    let handle = OcctKernelHandle::spawn();
    let kernel: &dyn GeometryKernel = &handle;
    for format in [ExportFormat::Step, ExportFormat::Stl, ExportFormat::ThreeMF] {
        let mut buf = Vec::new();
        match kernel.export_with_options(
            GeometryHandleId(999),
            format,
            &ExportOptions::default(),
            &mut buf,
        ) {
            Err(ExportError::InvalidHandle(id)) => assert_eq!(id, GeometryHandleId(999)),
            other => panic!("{format:?}: expected InvalidHandle(999), got {other:?}"),
        }
    }
}

#[test]
fn tessellate_invalid_handle_returns_error() {
    let handle = OcctKernelHandle::spawn();
    let result = handle.tessellate(GeometryHandleId(999), 0.1);
    assert!(result.is_err());
    match result.unwrap_err() {
        reify_ir::TessError::InvalidHandle(id) => {
            assert_eq!(id, GeometryHandleId(999));
        }
        other => panic!("expected InvalidHandle, got {:?}", other),
    }
}

#[test]
fn drop_handle_exits_thread_cleanly() {
    let handle = OcctKernelHandle::spawn();
    // Execute an op to ensure kernel thread is alive and working
    let op = GeometryOp::Box {
        size_x: Value::Real(1.0),
        size_y: Value::Real(1.0),
        size_z: Value::Real(1.0),
    };
    handle.execute(&op).unwrap();
    // Drop should not panic — thread exits cleanly
    drop(handle);
}

#[test]
fn multiple_sequential_handles() {
    for _ in 0..3 {
        let handle = OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            size_x: Value::Real(5.0),
            size_y: Value::Real(5.0),
            size_z: Value::Real(5.0),
        };
        let gh = handle.execute(&op).unwrap();
        // Each handle starts with its own id counter
        assert_eq!(gh.id, GeometryHandleId(1));
        drop(handle);
    }
}

#[test]
fn multi_operation_sequence() {
    let handle = OcctKernelHandle::spawn();

    // Create box
    let box_h = handle
        .execute(&GeometryOp::Box {
            size_x: Value::Real(100.0),
            size_y: Value::Real(60.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    assert_eq!(box_h.id, GeometryHandleId(1));

    // Create cylinder
    let cyl_h = handle
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(20.0),
        })
        .unwrap();
    assert_eq!(cyl_h.id, GeometryHandleId(2));

    // Boolean union
    let union_h = handle
        .execute(&GeometryOp::Union {
            left: box_h.id,
            right: cyl_h.id,
        })
        .unwrap();
    assert_eq!(union_h.id, GeometryHandleId(3));

    // Fillet
    let fillet_h = handle
        .execute(&GeometryOp::Fillet {
            target: union_h.id,
            edges: vec![],
            radius: Value::Real(2.0),
        })
        .unwrap();
    assert_eq!(fillet_h.id, GeometryHandleId(4));

    // Query volume
    let vol = handle
        .query(&reify_ir::GeometryQuery::Volume(fillet_h.id))
        .unwrap();
    match vol {
        Value::Real(v) => assert!(v > 0.0, "volume should be positive, got {v}"),
        other => panic!("expected Value::Real, got {:?}", other),
    }

    // Tessellate
    let mesh = handle.tessellate(fillet_h.id, 0.1).unwrap();
    assert!(!mesh.vertices.is_empty());
    assert!(!mesh.indices.is_empty());

    // Export STEP
    let mut buf = Vec::new();
    handle
        .export(fillet_h.id, reify_ir::ExportFormat::Step, &mut buf)
        .unwrap();
    let content = String::from_utf8(buf).unwrap();
    assert!(content.contains("ISO-10303-21"));
}

// --- Async companion method tests (step-21) ---

#[tokio::test]
async fn execute_async_creates_box() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let result = handle.execute_async(&op).await.unwrap();
    assert_eq!(result.id, GeometryHandleId(1));
    assert_eq!(result.repr, Some(BRepKind::Solid));
}

#[tokio::test]
async fn query_async_volume() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute_async(&op).await.unwrap();
    let result = handle
        .query_async(&GeometryQuery::Volume(gh.id))
        .await
        .unwrap();
    match result {
        Value::Real(v) => {
            assert!((v - 6000.0).abs() < 1.0, "expected ~6000, got {v}");
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[tokio::test]
async fn query_async_invalid_handle() {
    let handle = OcctKernelHandle::spawn();
    let result = handle
        .query_async(&GeometryQuery::Volume(GeometryHandleId(999)))
        .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        reify_ir::QueryError::InvalidHandle(id) => {
            assert_eq!(id, GeometryHandleId(999));
        }
        other => panic!("expected InvalidHandle, got {:?}", other),
    }
}

#[tokio::test]
async fn export_async_returns_step_bytes() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute_async(&op).await.unwrap();
    let bytes = handle
        .export_async(gh.id, reify_ir::ExportFormat::Step)
        .await
        .unwrap();
    let content = String::from_utf8(bytes).unwrap();
    assert!(
        content.contains("ISO-10303-21"),
        "STEP export should contain ISO-10303-21 header"
    );
}

#[tokio::test]
async fn export_async_invalid_handle() {
    let handle = OcctKernelHandle::spawn();
    let result = handle
        .export_async(GeometryHandleId(999), reify_ir::ExportFormat::Step)
        .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        reify_ir::ExportError::InvalidHandle(id) => {
            assert_eq!(id, GeometryHandleId(999));
        }
        other => panic!("expected InvalidHandle, got {:?}", other),
    }
}

#[tokio::test]
async fn tessellate_async_returns_valid_mesh() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    let gh = handle.execute_async(&op).await.unwrap();
    let mesh = handle.tessellate_async(gh.id, 0.1).await.unwrap();
    assert!(!mesh.vertices.is_empty(), "mesh should have vertices");
    assert!(!mesh.indices.is_empty(), "mesh should have indices");
    assert_eq!(
        mesh.indices.len() % 3,
        0,
        "indices should be divisible by 3 (triangles)"
    );
    assert!(mesh.normals.is_some(), "mesh should have normals");
}

#[tokio::test]
async fn async_multi_op_sequence() {
    let handle = OcctKernelHandle::spawn();

    // Create box
    let box_h = handle
        .execute_async(&GeometryOp::Box {
            size_x: Value::Real(100.0),
            size_y: Value::Real(60.0),
            size_z: Value::Real(10.0),
        })
        .await
        .unwrap();
    assert_eq!(box_h.id, GeometryHandleId(1));

    // Create cylinder
    let cyl_h = handle
        .execute_async(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(20.0),
        })
        .await
        .unwrap();
    assert_eq!(cyl_h.id, GeometryHandleId(2));

    // Boolean union
    let union_h = handle
        .execute_async(&GeometryOp::Union {
            left: box_h.id,
            right: cyl_h.id,
        })
        .await
        .unwrap();
    assert_eq!(union_h.id, GeometryHandleId(3));

    // Fillet
    let fillet_h = handle
        .execute_async(&GeometryOp::Fillet {
            target: union_h.id,
            edges: vec![],
            radius: Value::Real(2.0),
        })
        .await
        .unwrap();
    assert_eq!(fillet_h.id, GeometryHandleId(4));

    // Query volume via async
    let vol = handle
        .query_async(&GeometryQuery::Volume(fillet_h.id))
        .await
        .unwrap();
    match vol {
        Value::Real(v) => assert!(v > 0.0, "volume should be positive, got {v}"),
        other => panic!("expected Value::Real, got {:?}", other),
    }

    // Tessellate via async
    let mesh = handle.tessellate_async(fillet_h.id, 0.1).await.unwrap();
    assert!(!mesh.vertices.is_empty());
    assert!(!mesh.indices.is_empty());

    // Export STEP via async (returns Vec<u8>)
    let bytes = handle
        .export_async(fillet_h.id, reify_ir::ExportFormat::Step)
        .await
        .unwrap();
    let content = String::from_utf8(bytes).unwrap();
    assert!(content.contains("ISO-10303-21"));
}

// --- Async-safe Drop and shutdown tests (step-23) ---

#[tokio::test]
async fn drop_in_async_context_does_not_block() {
    // Dropping OcctKernelHandle inside an async context must not block
    // the tokio worker thread (i.e., must not call thread.join()).
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(5.0),
        size_y: Value::Real(5.0),
        size_z: Value::Real(5.0),
    };
    handle.execute_async(&op).await.unwrap();
    // Drop inside async context — must complete without blocking
    drop(handle);
}

#[tokio::test]
async fn shutdown_completes_cleanly() {
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(5.0),
        size_y: Value::Real(5.0),
        size_z: Value::Real(5.0),
    };
    handle.execute_async(&op).await.unwrap();
    // Explicit async shutdown — should complete cleanly
    handle.shutdown().await;
    // After shutdown, spawning a new handle should work (kernel thread exited)
    let handle2 = OcctKernelHandle::spawn();
    let result = handle2.execute_async(&op).await.unwrap();
    assert_eq!(result.id, GeometryHandleId(1)); // fresh kernel, fresh ids
}

// --- Warm-start tests ---

#[test]
fn handle_warm_state_returns_some_after_op() {
    use reify_ir::WarmStartable;
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    handle.execute(&op).unwrap();
    let state = handle.warm_state();
    assert!(state.is_some(), "handle with shapes should have warm state");
    assert!(
        state.unwrap().estimated_size_bytes() > 0,
        "estimated size should be positive"
    );
}

#[test]
fn cross_handle_warm_start_transfer() {
    use reify_ir::WarmStartable;
    // Handle A: create box
    let handle_a = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    handle_a.execute(&op).unwrap();

    // Extract warm state from handle A
    let state = handle_a.warm_state().expect("should have warm state");

    // Handle B: restore warm state
    let mut handle_b = OcctKernelHandle::spawn();
    handle_b.with_warm_state(state);

    // Query volume on handle B using handle ID 1
    let vol = handle_b
        .query(&GeometryQuery::Volume(GeometryHandleId(1)))
        .unwrap();
    match vol {
        Value::Real(v) => {
            assert!((v - 6000.0).abs() < 1.0, "expected volume ~6000, got {v}");
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[tokio::test]
async fn async_warm_start_roundtrip() {
    let handle_a = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    handle_a.execute_async(&op).await.unwrap();

    // Extract warm state via async
    let state = handle_a
        .warm_state_async()
        .await
        .expect("should have warm state");

    // Restore on new handle via async
    let handle_b = OcctKernelHandle::spawn();
    handle_b.with_warm_state_async(state).await;

    // Query volume via async
    let vol = handle_b
        .query_async(&GeometryQuery::Volume(GeometryHandleId(1)))
        .await
        .unwrap();
    match vol {
        Value::Real(v) => {
            assert!((v - 6000.0).abs() < 1.0, "expected volume ~6000, got {v}");
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn handle_warm_state_none_on_empty_kernel() {
    use reify_ir::WarmStartable;
    let handle = OcctKernelHandle::spawn();
    // No ops executed — warm_state should return None
    let state = handle.warm_state();
    assert!(state.is_none(), "empty kernel should have no warm state");
}

#[tokio::test]
async fn warm_startable_trait_safe_in_async_context() {
    use reify_ir::WarmStartable;
    // Calling the sync WarmStartable trait methods from an async context
    // must not panic (previously used blocking_send/blocking_recv which
    // panicked inside tokio runtime).
    let handle_a = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(20.0),
        size_z: Value::Real(30.0),
    };
    handle_a.execute_async(&op).await.unwrap();

    // Call sync warm_state() from async context — must not panic
    let state = handle_a.warm_state().expect("should have warm state");

    // Call sync with_warm_state() from async context — must not panic
    let mut handle_b = OcctKernelHandle::spawn();
    handle_b.with_warm_state(state);

    // Verify restored state works
    let vol = handle_b
        .query_async(&GeometryQuery::Volume(GeometryHandleId(1)))
        .await
        .unwrap();
    match vol {
        Value::Real(v) => {
            assert!((v - 6000.0).abs() < 1.0, "expected volume ~6000, got {v}");
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn concurrent_export_step_from_multiple_handles() {
    // Regression test: spawn N OcctKernelHandle instances, each on its own
    // dedicated thread, create a box on each, and export to STEP concurrently.
    // This reliably triggers the OCCT global STEP writer state race condition
    // when the C++ export_step() function is not guarded by a mutex.
    const N: usize = 4;
    std::thread::scope(|s| {
        let threads: Vec<_> = (0..N)
            .map(|_| {
                s.spawn(|| {
                    let handle = OcctKernelHandle::spawn();
                    let op = GeometryOp::Box {
                        size_x: Value::Real(10.0),
                        size_y: Value::Real(20.0),
                        size_z: Value::Real(30.0),
                    };
                    let gh = handle.execute(&op).unwrap();
                    let mut buf = Vec::new();
                    handle
                        .export(gh.id, reify_ir::ExportFormat::Step, &mut buf)
                        .expect("STEP export should succeed under concurrent access");
                    let content = String::from_utf8(buf).unwrap();
                    assert!(
                        content.contains("ISO-10303-21"),
                        "STEP export should contain ISO-10303-21 header"
                    );
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
    });
}

#[test]
fn kernel_thread_responsive_after_errors() {
    let handle = OcctKernelHandle::spawn();

    // 1. Create a valid box (id=1) — should succeed
    let box_h = handle
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();
    assert_eq!(box_h.id, GeometryHandleId(1));

    // 2. Union with invalid handles — should return Err(InvalidReference)
    let union_result = handle.execute(&GeometryOp::Union {
        left: GeometryHandleId(999),
        right: GeometryHandleId(998),
    });
    assert!(union_result.is_err());

    // 3. Box with zero width — should return Err(OperationFailed) from validation
    let zero_result = handle.execute(&GeometryOp::Box {
        size_x: Value::Real(0.0),
        size_y: Value::Real(10.0),
        size_z: Value::Real(10.0),
    });
    assert!(zero_result.is_err());

    // 4. Query volume on invalid handle — should return Err
    let query_result = handle.query(&GeometryQuery::Volume(GeometryHandleId(999)));
    assert!(query_result.is_err());

    // 5. Create another valid box — proves kernel thread is still alive
    let box2_h = handle
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .unwrap();

    // 6. Query volume of the new box — should return correct value
    let vol = handle.query(&GeometryQuery::Volume(box2_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!((v - 6000.0).abs() < 1.0, "expected ~6000, got {v}");
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn sync_drop_still_joins_thread() {
    // Sync (non-async) Drop should preserve existing join behavior
    let handle = OcctKernelHandle::spawn();
    let op = GeometryOp::Box {
        size_x: Value::Real(5.0),
        size_y: Value::Real(5.0),
        size_z: Value::Real(5.0),
    };
    handle.execute(&op).unwrap();
    // Drop outside async context — should join the thread
    drop(handle);
    // No panic means success
}

// --- Topology extraction through the handle channel (step-21) ---

/// Helper: build a box on a fresh handle and return both.
fn handle_with_box(
    size_x: f64,
    size_y: f64,
    size_z: f64,
) -> (OcctKernelHandle, GeometryHandleId) {
    let handle = OcctKernelHandle::spawn();
    let gh = handle
        .execute(&GeometryOp::Box {
            size_x: Value::Real(size_x),
            size_y: Value::Real(size_y),
            size_z: Value::Real(size_z),
        })
        .expect("Box execute should succeed");
    (handle, gh.id)
}

/// Assert all ids in `ids` are pairwise distinct, none are
/// `GeometryHandleId::INVALID`, and none equal `excluded`.
fn assert_distinct_valid(ids: &[GeometryHandleId], excluded: GeometryHandleId) {
    for id in ids {
        assert_ne!(*id, GeometryHandleId::INVALID, "id should not be INVALID");
        assert_ne!(*id, excluded, "extracted id should not equal source handle");
    }
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        assert!(seen.insert(*id), "duplicate id in extracted vec: {id:?}");
    }
}

#[test]
fn extract_edges_through_handle_channel_returns_twelve_handles() {
    let (handle, box_id) = handle_with_box(10.0, 20.0, 30.0);
    let edges = handle
        .extract_edges(box_id)
        .expect("extract_edges through channel should succeed");
    assert_eq!(
        edges.len(),
        12,
        "a box should have 12 edges, got {}",
        edges.len()
    );
    assert_distinct_valid(&edges, box_id);
}

#[tokio::test]
async fn extract_edges_async_through_handle_channel_returns_twelve_handles() {
    let handle = OcctKernelHandle::spawn();
    let gh = handle
        .execute_async(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .await
        .unwrap();
    let edges = handle
        .extract_edges_async(gh.id)
        .await
        .expect("extract_edges_async through channel should succeed");
    assert_eq!(
        edges.len(),
        12,
        "a box should have 12 edges, got {}",
        edges.len()
    );
    assert_distinct_valid(&edges, gh.id);
}

#[test]
fn extract_faces_through_handle_channel_returns_six_handles() {
    let (handle, box_id) = handle_with_box(10.0, 20.0, 30.0);
    let faces = handle
        .extract_faces(box_id)
        .expect("extract_faces through channel should succeed");
    assert_eq!(
        faces.len(),
        6,
        "a box should have 6 faces, got {}",
        faces.len()
    );
    assert_distinct_valid(&faces, box_id);
}

#[tokio::test]
async fn extract_faces_async_through_handle_channel_returns_six_handles() {
    let handle = OcctKernelHandle::spawn();
    let gh = handle
        .execute_async(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .await
        .unwrap();
    let faces = handle
        .extract_faces_async(gh.id)
        .await
        .expect("extract_faces_async through channel should succeed");
    assert_eq!(
        faces.len(),
        6,
        "a box should have 6 faces, got {}",
        faces.len()
    );
    assert_distinct_valid(&faces, gh.id);
}

#[test]
fn extract_vertices_through_handle_channel_returns_eight_handles() {
    let (handle, box_id) = handle_with_box(10.0, 20.0, 30.0);
    let vertices = handle
        .extract_vertices(box_id)
        .expect("extract_vertices through channel should succeed");
    assert_eq!(
        vertices.len(),
        8,
        "a box should have 8 vertices, got {}",
        vertices.len()
    );
    assert_distinct_valid(&vertices, box_id);
}

#[tokio::test]
async fn extract_vertices_async_through_handle_channel_returns_eight_handles() {
    let handle = OcctKernelHandle::spawn();
    let gh = handle
        .execute_async(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(30.0),
        })
        .await
        .unwrap();
    let vertices = handle
        .extract_vertices_async(gh.id)
        .await
        .expect("extract_vertices_async through channel should succeed");
    assert_eq!(
        vertices.len(),
        8,
        "a box should have 8 vertices, got {}",
        vertices.len()
    );
    assert_distinct_valid(&vertices, gh.id);
}
