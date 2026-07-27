    use std::collections::HashMap;

    use reify_core::{DimensionVector, ValueCellId};
    use reify_ir::{GeometryHandleId, KernelHandle, KernelId, Value, ValueMap};
    use reify_test_support::{mocks::MockGeometryKernel, parse_and_compile_with_stdlib};

    use super::Engine;

    /// `MiniChild` exposes a DIRECT geometry-query cell (`v = volume(body)`,
    /// `try_eval_geometry_query` case (a)) and a NESTED one (`doubled =
    /// volume(body) * 2`, case (b) — mirrors the real `mass = volume(geometry)
    /// * material.density` shape) so both dispatch branches are pinned.
    /// `MiniParent` cross-subs a single non-collection instance; `MiniCollectionParent`
    /// cross-subs a `List<MiniChild>` — collection subs are out of scope for this
    /// pass (mirrors `seed_cross_sub_named_steps`'s no-collection-subs contract).
    const MINI_SOURCE: &str = r#"
structure def MiniChild {
    let body = box(10mm, 20mm, 30mm)
    let v = volume(body)
    let doubled = volume(body) * 2
}

structure def MiniParent {
    sub c = MiniChild()
}

structure def MiniCollectionParent {
    sub cs : List<MiniChild>
    constraint cs.count == 2
}
"#;

    /// Fixed kernel handle standing in for the sub-instance's realized `body`
    /// geometry (the compound `named_steps["<sub>.body"]` key
    /// `seed_cross_sub_named_steps`, task 3441, would populate in a real build).
    const HANDLE_ID: GeometryHandleId = GeometryHandleId(1);

    /// box(10mm, 20mm, 30mm) = 6.0e-6 m^3.
    const BOX_VOLUME: f64 = 0.010 * 0.020 * 0.030;

    fn compile_mini() -> reify_compiler::CompiledModule {
        parse_and_compile_with_stdlib(MINI_SOURCE)
    }

    /// Seed the compound-key convention `resolve_geometry_handle_arg` reads:
    /// `"<sub_name>.body"` → the sub-instance's realized geometry handle.
    fn named_steps_with_body_handle(sub_name: &str) -> HashMap<String, KernelHandle> {
        let mut named_steps = HashMap::new();
        named_steps.insert(
            format!("{sub_name}.body"),
            KernelHandle {
                kernel: KernelId::Occt,
                id: HANDLE_ID,
            },
        );
        named_steps
    }

    /// task 4725 amendment: direct-call unit pin (no OCCT required) for
    /// `Engine::post_process_cross_sub_value_cells`'s rescope
    /// (`CompiledExpr::map_value_refs`) + dispatch path. The OCCT-gated
    /// `cross_entity_aggregate_folds_via_fixpoint` /
    /// `total_mass_computed` integration pins only exercise this pass when
    /// OCCT is available; this test drives it directly with a
    /// `MockGeometryKernel` so the fold logic is pinned on every runner.
    ///
    /// Both `v = volume(body)` (direct) and `doubled = volume(body) * 2`
    /// (nested) rescope their child-scoped `body` `ValueRef` onto
    /// `MiniParent.c` and dispatch against `named_steps["c.body"]`.
    #[test]
    fn post_process_cross_sub_value_cells_folds_direct_and_nested_scoped_cells() {
        let compiled = compile_mini();
        let template = reify_compiler::find_template(&compiled.templates, "MiniParent")
            .expect("MiniParent template must compile");

        let named_steps = named_steps_with_body_handle("c");
        let mut values = ValueMap::new();
        let meta_map = HashMap::new();
        let mut diagnostics = Vec::new();
        let kernel = MockGeometryKernel::new().with_volume_result(HANDLE_ID, Value::Real(BOX_VOLUME));

        Engine::post_process_cross_sub_value_cells(
            template,
            &named_steps,
            &mut values,
            &compiled.functions,
            &meta_map,
            &kernel,
            &mut diagnostics,
            &compiled.templates,
        );

        match values.get(&ValueCellId::new("MiniParent.c", "v")) {
            Some(Value::Scalar {
                si_value,
                dimension,
            }) => {
                assert_eq!(*dimension, DimensionVector::VOLUME);
                assert!(
                    (*si_value - BOX_VOLUME).abs() < 1e-12,
                    "expected v \u{2248} {BOX_VOLUME}, got {si_value}"
                );
            }
            other => panic!("MiniParent.c.v must fold to Scalar<Volume>, got {other:?}"),
        }

        match values.get(&ValueCellId::new("MiniParent.c", "doubled")) {
            Some(Value::Scalar {
                si_value,
                dimension,
            }) => {
                assert_eq!(*dimension, DimensionVector::VOLUME);
                let expected = BOX_VOLUME * 2.0;
                assert!(
                    (*si_value - expected).abs() < 1e-12,
                    "expected doubled \u{2248} {expected}, got {si_value}"
                );
            }
            other => panic!(
                "MiniParent.c.doubled must fold to Scalar<Volume> (\u{2248}2\u{00d7}v), got {other:?}"
            ),
        }

        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected on the success path; got: {diagnostics:?}"
        );
    }

    /// Collection subs (`sub cs : List<MiniChild>`) are out of scope for this
    /// pass — mirrored from `seed_cross_sub_named_steps`'s no-collection-subs
    /// contract (a collection has N instances, not one scoped entity to fold
    /// into). `named_steps` is seeded with a `"cs.body"` entry that WOULD
    /// resolve if the `is_collection` guard were removed, so this pins the
    /// guard itself rather than passing vacuously.
    #[test]
    fn post_process_cross_sub_value_cells_skips_collection_subs() {
        let compiled = compile_mini();
        let template = reify_compiler::find_template(&compiled.templates, "MiniCollectionParent")
            .expect("MiniCollectionParent template must compile");

        let named_steps = named_steps_with_body_handle("cs");
        let mut values = ValueMap::new();
        let meta_map = HashMap::new();
        let mut diagnostics = Vec::new();
        let kernel = MockGeometryKernel::new().with_volume_result(HANDLE_ID, Value::Real(BOX_VOLUME));

        Engine::post_process_cross_sub_value_cells(
            template,
            &named_steps,
            &mut values,
            &compiled.functions,
            &meta_map,
            &kernel,
            &mut diagnostics,
            &compiled.templates,
        );

        assert!(
            values.get(&ValueCellId::new("MiniCollectionParent.cs", "v")).is_none(),
            "collection subs must be skipped even though a resolvable named_steps \
             entry was seeded; got {:?}",
            values.get(&ValueCellId::new("MiniCollectionParent.cs", "v"))
        );
    }

    /// A scoped cell that already holds a non-`Undef` value (e.g. a `Param`
    /// cell populated by `elaborate_child_params_only`) must be left
    /// untouched — the pass must not re-dispatch and overwrite it. Seeds
    /// `named_steps`/`kernel` so a fresh dispatch WOULD produce a different
    /// value, proving the guard (not coincidence) preserves the original.
    #[test]
    fn post_process_cross_sub_value_cells_leaves_already_folded_cell_untouched() {
        let compiled = compile_mini();
        let template = reify_compiler::find_template(&compiled.templates, "MiniParent")
            .expect("MiniParent template must compile");

        let named_steps = named_steps_with_body_handle("c");
        let mut values = ValueMap::new();
        let sentinel = Value::Scalar {
            si_value: 999.0,
            dimension: DimensionVector::VOLUME,
        };
        values.insert(ValueCellId::new("MiniParent.c", "v"), sentinel.clone());
        let meta_map = HashMap::new();
        let mut diagnostics = Vec::new();
        // Would fold `v` to BOX_VOLUME (≠ 999.0) if the already-folded guard
        // were absent.
        let kernel = MockGeometryKernel::new().with_volume_result(HANDLE_ID, Value::Real(BOX_VOLUME));

        Engine::post_process_cross_sub_value_cells(
            template,
            &named_steps,
            &mut values,
            &compiled.functions,
            &meta_map,
            &kernel,
            &mut diagnostics,
            &compiled.templates,
        );

        assert_eq!(
            values.get(&ValueCellId::new("MiniParent.c", "v")),
            Some(&sentinel),
            "an already-folded scoped cell must not be overwritten"
        );
    }
