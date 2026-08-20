    use std::collections::BTreeMap;

    use reify_core::identity::ValueCellId;
    use reify_core::{RealizationNodeId, Severity};
    use reify_ir::{GeometryHandleId, Value, ValueMap};
    use reify_test_support::mocks::MockGeometryKernel;

    use super::Engine;

    /// Fixed kernel handle for the geometry-backed body in these tests.
    const HANDLE_ID: GeometryHandleId = GeometryHandleId(77);

    /// Build a minimal mechanism `Value::Map`: kind="mechanism", bodies=[body]
    /// where body.solid is the given `solid_value`.
    fn one_body_mechanism(solid_value: Value) -> Value {
        let mut body = BTreeMap::new();
        body.insert(Value::String("id".to_string()), Value::Int(0));
        body.insert(Value::String("solid".to_string()), solid_value);

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("bodies".to_string()),
            Value::List(vec![Value::Map(body)]),
        );
        Value::Map(mech)
    }

    /// Build a `Value::GeometryHandle` for `HANDLE_ID`.
    fn geometry_handle() -> Value {
        Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("Design", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(HANDLE_ID),
        }
    }

    /// Build a `MockGeometryKernel` with injected Volume / CenterOfMass /
    /// InertiaTensor replies for `HANDLE_ID` at the water-default density
    /// (1000.0 kg/m³). Volume=6.0 → mass=6000.0 kg.
    fn mock_kernel() -> MockGeometryKernel {
        let inertia = Value::List(vec![
            Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
        ]);
        MockGeometryKernel::new()
            .with_volume_result(HANDLE_ID, Value::Real(6.0))
            .with_center_of_mass_result(
                HANDLE_ID,
                1000.0,
                Value::String("{\"x\":0.1,\"y\":0.2,\"z\":0.3}".to_string()),
            )
            .with_inertia_tensor_result(HANDLE_ID, 1000.0, inertia)
    }

    /// The engine pass must iterate over a ValueMap containing a mechanism cell,
    /// call `derive_mechanism_mass_props`, and write the patched mechanism back
    /// into the ValueMap so that values.get(cell_id) yields a mechanism whose
    /// first body carries `derived_mass_props`.
    ///
    /// RED: `Engine::post_process_mechanism_mass_props` does not exist yet.
    #[test]
    fn post_process_mechanism_mass_props_writes_derived_back_into_value_map() {
        let cell_id = ValueCellId::new("Design", "mech");
        let mut values = ValueMap::new();
        values.insert(cell_id.clone(), one_body_mechanism(geometry_handle()));

        let kernel = mock_kernel();
        let mut diags = Vec::new();

        Engine::post_process_mechanism_mass_props(&mut values, &kernel, &mut diags);

        // The mechanism cell must now hold a patched value.
        let patched = values
            .get(&cell_id)
            .expect("mechanism cell must still be present after pass");

        // Extract the first body from the patched mechanism.
        let mech_map = match patched {
            Value::Map(m) => m,
            other => panic!("mechanism cell must be a Map, got {other:?}"),
        };
        let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            _ => panic!("patched mechanism missing bodies"),
        };
        assert_eq!(bodies.len(), 1, "must have exactly one body");
        let body_map = match &bodies[0] {
            Value::Map(b) => b,
            other => panic!("body must be a Map, got {other:?}"),
        };

        // The body must carry `derived_mass_props` (additive write).
        let derived = body_map
            .get(&Value::String("derived_mass_props".to_string()))
            .expect("body must carry derived_mass_props after engine pass");

        // Must be a MassProperties StructureInstance.
        let data = match derived {
            Value::StructureInstance(d) => d,
            other => panic!("derived_mass_props must be a StructureInstance, got {other:?}"),
        };
        assert_eq!(
            data.type_name, "MassProperties",
            "derived_mass_props type_name must be MassProperties"
        );

        // The original `solid` GeometryHandle must still be present — additive write.
        assert!(
            body_map.contains_key(&Value::String("solid".to_string())),
            "body must still carry `solid` after additive pass"
        );

        // No diagnostics expected on the success path.
        assert!(
            diags.is_empty(),
            "no diagnostics expected on success path; got: {diags:?}"
        );
    }

    /// A ValueMap cell that is NOT a mechanism (e.g. a plain Real) must be
    /// left untouched by the pass.
    ///
    /// RED: `Engine::post_process_mechanism_mass_props` does not exist yet.
    #[test]
    fn post_process_mechanism_mass_props_leaves_non_mechanism_cells_untouched() {
        let cell_id = ValueCellId::new("Design", "x");
        let mut values = ValueMap::new();
        values.insert(cell_id.clone(), Value::Real(42.0));

        let kernel = mock_kernel();
        let mut diags = Vec::new();

        Engine::post_process_mechanism_mass_props(&mut values, &kernel, &mut diags);

        // Cell must still hold its original value.
        assert_eq!(
            values.get(&cell_id),
            Some(&Value::Real(42.0)),
            "non-mechanism cell must be left untouched"
        );
        assert!(diags.is_empty(), "no diagnostics for non-mechanism cells");
    }

    /// A geometry-backed body whose kernel query fails (no injected results) must
    /// cause the pass to skip that body (emit a Warning), and since no body was
    /// patched the mechanism cell is left with its original value unchanged.
    ///
    /// RED: `Engine::post_process_mechanism_mass_props` does not exist yet.
    #[test]
    fn post_process_mechanism_mass_props_emits_warning_on_kernel_failure() {
        let cell_id = ValueCellId::new("Design", "mech");
        let original = one_body_mechanism(geometry_handle());
        let mut values = ValueMap::new();
        values.insert(cell_id.clone(), original.clone());

        // Bare kernel — no replies injected, so Volume query will fail.
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        Engine::post_process_mechanism_mass_props(&mut values, &kernel, &mut diags);

        // Cell must be unchanged (no body was patched).
        assert_eq!(
            values.get(&cell_id),
            Some(&original),
            "mechanism cell must be unchanged when kernel fails for all bodies"
        );

        // A Warning diagnostic must be emitted.
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "must emit a Warning when kernel query fails; got: {diags:?}"
        );
    }
