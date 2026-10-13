//! `SingleKernelHolder` must forward every `GeometryKernel` method to its
//! inner kernel, and with no kernel registered must return exactly what the
//! trait default returns. Trait defaults silently mask a missing forward, so
//! this guard enumerates the trait's methods from reify-ir's source: a new
//! trait method fails `harness_calls_every_trait_method_exactly_once` until
//! `call_every_method` calls it, and fails
//! `holder_forwards_every_method_to_inner_kernel` until `RecordingKernel`
//! overrides it and the holder forwards it.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use reify_core::hash::ContentHash;
use reify_geometry::SingleKernelHolder;
use reify_ir::{
    AttributeHistory, BRepKind, ElementOrderTag, ExportError, ExportFormat, ExportOptions,
    ExportWarning, FeatureId, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, KernelAttributeHook, KernelAttributeOutcome, Mesh, QueryError,
    SampledField, TessError, TopologyAttributeTable, Value, VolumeConnectivity, VolumeMesh,
    VoxelResolution,
};

const GEOMETRY_KERNEL_SOURCE: &str = include_str!("../../reify-ir/src/geometry.rs");

fn trait_method_names() -> BTreeSet<String> {
    let file = syn::parse_file(GEOMETRY_KERNEL_SOURCE)
        .expect("crates/reify-ir/src/geometry.rs must parse as Rust");
    let kernel_trait = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Trait(t) if t.ident == "GeometryKernel" => Some(t),
            _ => None,
        })
        .expect(
            "trait GeometryKernel is no longer defined in crates/reify-ir/src/geometry.rs; \
             point GEOMETRY_KERNEL_SOURCE at the file that now defines it",
        );
    let names: BTreeSet<String> = kernel_trait
        .items
        .iter()
        .filter_map(|item| match item {
            syn::TraitItem::Fn(f) => Some(f.sig.ident.to_string()),
            _ => None,
        })
        .collect();
    for required in ["execute", "query", "export", "tessellate"] {
        assert!(
            names.contains(required),
            "enumerated GeometryKernel methods {names:?} lack required method `{required}`"
        );
    }
    names
}

/// Records every method invoked on it, and answers each with a value that is
/// not the trait default's and is derived from its arguments, so a forward
/// that drops the call, substitutes the default, or passes the wrong argument
/// changes the observed output.
struct RecordingKernel {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl RecordingKernel {
    fn new() -> (Self, Arc<Mutex<Vec<&'static str>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        (Self { log: log.clone() }, log)
    }

    fn record(&self, name: &'static str) {
        self.log.lock().unwrap().push(name);
    }

    /// An answer for a method whose success type has no room for its
    /// arguments: the error message echoes them verbatim.
    fn echoed(method: &str, args: &dyn std::fmt::Debug) -> GeometryError {
        GeometryError::OperationFailed(format!("{method} {args:?}"))
    }

    fn solid(id: u64) -> GeometryHandle {
        GeometryHandle {
            id: GeometryHandleId(id),
            repr: Some(BRepKind::Solid),
        }
    }

    fn tets(vertices: Vec<f32>, order: ElementOrderTag) -> VolumeMesh {
        VolumeMesh {
            vertices,
            connectivity: VolumeConnectivity::Tet {
                indices: vec![],
                order,
            },
            normals: None,
            boundary: None,
        }
    }
}

impl KernelAttributeHook for RecordingKernel {
    fn propagate_attributes(
        &self,
        _table: &mut TopologyAttributeTable,
        _op: &GeometryOp,
        _parent_handles: &[GeometryHandleId],
        _result_handle: GeometryHandleId,
        _splitting_feature_id: &FeatureId,
    ) -> Result<KernelAttributeOutcome, QueryError> {
        Ok(KernelAttributeOutcome::Propagated)
    }
}

impl GeometryKernel for RecordingKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.record("execute");
        Err(Self::echoed("execute", op))
    }

    fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        self.record("execute_with_history");
        Err(Self::echoed("execute_with_history", op))
    }

    fn reset(&mut self) {
        self.record("reset");
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        self.record("query");
        Ok(Value::String(format!("query {query:?}")))
    }

    fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
        self.record("query_many");
        Ok(queries
            .iter()
            .map(|q| Value::String(format!("query_many {q:?}")))
            .collect())
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        self.record("export");
        write!(writer, "export {handle:?} {format:?}")
            .map_err(|e| ExportError::IoError(e.to_string()))
    }

    fn export_with_options(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        options: &ExportOptions,
        writer: &mut dyn std::io::Write,
    ) -> Result<Vec<ExportWarning>, ExportError> {
        self.record("export_with_options");
        write!(
            writer,
            "export_with_options {handle:?} {format:?} {options:?}"
        )
        .map_err(|e| ExportError::IoError(e.to_string()))?;
        Ok(Vec::new())
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        self.record("tessellate");
        Ok(Mesh {
            vertices: vec![handle.0 as f32, tolerance as f32],
            indices: vec![],
            normals: None,
        })
    }

    fn extract_edges(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.record("extract_edges");
        Ok(vec![handle, GeometryHandleId(10)])
    }

    fn extract_faces(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.record("extract_faces");
        Ok(vec![handle, GeometryHandleId(11)])
    }

    fn extract_vertices(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.record("extract_vertices");
        Ok(vec![handle, GeometryHandleId(12)])
    }

    fn densify_grid_to_sampled(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<SampledField, QueryError> {
        self.record("densify_grid_to_sampled");
        Err(QueryError::QueryFailed(format!(
            "recorded densify for {handle:?}"
        )))
    }

    fn execute_split(&mut self, op: &GeometryOp) -> Result<Vec<GeometryHandleId>, GeometryError> {
        self.record("execute_split");
        Err(Self::echoed("execute_split", op))
    }

    fn make_compound(
        &mut self,
        handles: &[GeometryHandleId],
    ) -> Result<GeometryHandle, GeometryError> {
        self.record("make_compound");
        Err(Self::echoed("make_compound", &handles))
    }

    fn ingest_mesh(&mut self, mesh: &Mesh) -> Result<GeometryHandle, GeometryError> {
        self.record("ingest_mesh");
        Ok(Self::solid(30 + mesh.vertices.len() as u64))
    }

    fn ingest_mesh_at_resolution(
        &mut self,
        mesh: &Mesh,
        resolution: VoxelResolution,
    ) -> Result<GeometryHandle, GeometryError> {
        self.record("ingest_mesh_at_resolution");
        Err(Self::echoed(
            "ingest_mesh_at_resolution",
            &(resolution, &mesh.vertices),
        ))
    }

    fn register_mesh_handle(&mut self, mesh: &Mesh) -> Result<GeometryHandle, GeometryError> {
        self.record("register_mesh_handle");
        Ok(Self::solid(mesh.vertices.len() as u64))
    }

    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        self.record("attribute_hook");
        Some(self)
    }

    fn measure_mesh_deviation(&self, handle: GeometryHandleId, mesh: &Mesh) -> Option<f64> {
        self.record("measure_mesh_deviation");
        Some(handle.0 as f64 + mesh.vertices.len() as f64)
    }

    fn volume_mesh(&self, handle: GeometryHandleId) -> Result<VolumeMesh, QueryError> {
        self.record("volume_mesh");
        Ok(Self::tets(vec![handle.0 as f32], ElementOrderTag::P1))
    }

    fn mesh_surface_to_volume(
        &self,
        surface: &Mesh,
        element_order: ElementOrderTag,
    ) -> Result<VolumeMesh, GeometryError> {
        self.record("mesh_surface_to_volume");
        Ok(Self::tets(surface.vertices.clone(), element_order))
    }

    fn mesh_surface_to_volume_attributed(
        &self,
        surface: &Mesh,
        element_order: ElementOrderTag,
        face_anchors: &[(GeometryHandleId, [f64; 3])],
        match_tolerance: f64,
    ) -> Result<VolumeMesh, GeometryError> {
        self.record("mesh_surface_to_volume_attributed");
        let mut vertices = surface.vertices.clone();
        vertices.extend(face_anchors.iter().map(|(h, p)| h.0 as f32 + p[0] as f32));
        vertices.push(match_tolerance as f32);
        Ok(Self::tets(vertices, element_order))
    }

    fn store_volume_mesh(&self, vm: VolumeMesh) -> Result<GeometryHandleId, GeometryError> {
        self.record("store_volume_mesh");
        Ok(GeometryHandleId(100 + vm.vertices.len() as u64))
    }

    fn closest_point_on_shape(
        &self,
        handle: GeometryHandleId,
        point: [f64; 3],
    ) -> Result<[f64; 3], QueryError> {
        self.record("closest_point_on_shape");
        Ok([point[0] + handle.0 as f64, point[1], point[2]])
    }

    fn vertex_point(&self, handle: GeometryHandleId) -> Result<[f64; 3], QueryError> {
        self.record("vertex_point");
        Ok([handle.0 as f64, 0.0, 0.0])
    }

    fn realize_mesh_from_voxel(
        &self,
        handle: GeometryHandleId,
        iso_level: f64,
        adaptive: bool,
    ) -> Result<Mesh, GeometryError> {
        self.record("realize_mesh_from_voxel");
        Ok(Mesh {
            vertices: vec![handle.0 as f32, iso_level as f32, adaptive as u8 as f32],
            indices: vec![],
            normals: None,
        })
    }

    fn surface_options_content_hash(&self, iso_level: f64, adaptive: bool) -> ContentHash {
        self.record("surface_options_content_hash");
        ContentHash(1 + iso_level.to_bits() as u128 + adaptive as u128)
    }
}

/// Defaults-only reference kernel: its four required methods return exactly
/// the holder's no-kernel errors, so every inherited default is what an empty
/// holder must return.
struct Unregistered;

const NO_KERNEL: &str = "no geometry kernel registered";

impl GeometryKernel for Unregistered {
    fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(NO_KERNEL.to_string()))
    }

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(NO_KERNEL.to_string()))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(NO_KERNEL.to_string()))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(NO_KERNEL.to_string()))
    }
}

/// One call per `GeometryKernel` method, in trait order, labelled with the
/// method's name and Debug-rendered (the outputs share no `PartialEq`).
fn call_every_method(k: &mut dyn GeometryKernel) -> Vec<(&'static str, String)> {
    let handle = GeometryHandleId(7);
    let op = GeometryOp::Box {
        width: Value::length(0.01),
        height: Value::length(0.02),
        depth: Value::length(0.03),
    };
    let query = GeometryQuery::Volume(handle);
    let mesh = Mesh {
        vertices: vec![0.0, 1.0, 2.0],
        indices: vec![],
        normals: None,
    };
    let face_anchors = [(handle, [0.5, 0.5, 0.5])];
    let volume = VolumeMesh {
        vertices: vec![0.0; 6],
        connectivity: VolumeConnectivity::Tet {
            indices: vec![],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    };

    let mut calls: Vec<(&'static str, String)> = Vec::new();
    calls.push(("execute", format!("{:?}", k.execute(&op))));
    calls.push((
        "execute_with_history",
        format!("{:?}", k.execute_with_history(&op)),
    ));
    k.reset();
    calls.push(("reset", "()".to_string()));
    calls.push(("query", format!("{:?}", k.query(&query))));
    calls.push((
        "query_many",
        format!("{:?}", k.query_many(std::slice::from_ref(&query))),
    ));
    let mut written = Vec::new();
    let exported = k.export(handle, ExportFormat::Step, &mut written);
    calls.push(("export", format!("{exported:?} wrote {written:?}")));
    let mut written = Vec::new();
    let exported = k.export_with_options(
        handle,
        ExportFormat::Step,
        &ExportOptions::default(),
        &mut written,
    );
    calls.push((
        "export_with_options",
        format!("{exported:?} wrote {written:?}"),
    ));
    calls.push(("tessellate", format!("{:?}", k.tessellate(handle, 0.1))));
    calls.push(("extract_edges", format!("{:?}", k.extract_edges(handle))));
    calls.push(("extract_faces", format!("{:?}", k.extract_faces(handle))));
    calls.push((
        "extract_vertices",
        format!("{:?}", k.extract_vertices(handle)),
    ));
    calls.push((
        "densify_grid_to_sampled",
        format!("{:?}", k.densify_grid_to_sampled(handle)),
    ));
    calls.push(("execute_split", format!("{:?}", k.execute_split(&op))));
    calls.push((
        "make_compound",
        format!("{:?}", k.make_compound(&[handle, GeometryHandleId(8)])),
    ));
    calls.push(("ingest_mesh", format!("{:?}", k.ingest_mesh(&mesh))));
    calls.push((
        "ingest_mesh_at_resolution",
        format!(
            "{:?}",
            k.ingest_mesh_at_resolution(&mesh, VoxelResolution::MinFeature(1.0))
        ),
    ));
    calls.push((
        "register_mesh_handle",
        format!("{:?}", k.register_mesh_handle(&mesh)),
    ));
    calls.push((
        "attribute_hook",
        format!("{:?}", k.attribute_hook().is_some()),
    ));
    calls.push((
        "measure_mesh_deviation",
        format!("{:?}", k.measure_mesh_deviation(handle, &mesh)),
    ));
    calls.push(("volume_mesh", format!("{:?}", k.volume_mesh(handle))));
    calls.push((
        "mesh_surface_to_volume",
        format!("{:?}", k.mesh_surface_to_volume(&mesh, ElementOrderTag::P2)),
    ));
    calls.push((
        "mesh_surface_to_volume_attributed",
        format!(
            "{:?}",
            k.mesh_surface_to_volume_attributed(&mesh, ElementOrderTag::P2, &face_anchors, 1e-3)
        ),
    ));
    calls.push((
        "store_volume_mesh",
        format!("{:?}", k.store_volume_mesh(volume)),
    ));
    calls.push((
        "closest_point_on_shape",
        format!("{:?}", k.closest_point_on_shape(handle, [1.0, 2.0, 3.0])),
    ));
    calls.push(("vertex_point", format!("{:?}", k.vertex_point(handle))));
    calls.push((
        "realize_mesh_from_voxel",
        format!("{:?}", k.realize_mesh_from_voxel(handle, 0.25, true)),
    ));
    calls.push((
        "surface_options_content_hash",
        format!("{:?}", k.surface_options_content_hash(0.25, true)),
    ));
    calls
}

fn labels(calls: &[(&'static str, String)]) -> Vec<&'static str> {
    calls.iter().map(|(name, _)| *name).collect()
}

/// Names of the methods whose renderings differ between two harness runs.
fn differing(left: &[(&'static str, String)], right: &[(&'static str, String)]) -> Vec<String> {
    left.iter()
        .zip(right)
        .filter(|(l, r)| l != r)
        .map(|((name, l), (_, r))| format!("{name}:\n    left:  {l}\n    right: {r}"))
        .collect()
}

#[test]
fn harness_calls_every_trait_method_exactly_once() {
    let (mut kernel, _log) = RecordingKernel::new();
    let called = labels(&call_every_method(&mut kernel));

    let called_set: BTreeSet<String> = called.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        called_set.len(),
        called.len(),
        "duplicate harness labels: {called:?}"
    );

    let trait_methods = trait_method_names();
    let missing: Vec<&String> = trait_methods.difference(&called_set).collect();
    let extra: Vec<&String> = called_set.difference(&trait_methods).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "call_every_method must call every GeometryKernel method once; \
         missing from the harness: {missing:?}; labels that are not trait methods: {extra:?}"
    );
}

#[test]
fn recording_kernel_answers_differ_from_trait_defaults() {
    let (mut kernel, _log) = RecordingKernel::new();
    let recorded = call_every_method(&mut kernel);
    let defaults = call_every_method(&mut Unregistered);

    let indistinguishable: Vec<&str> = recorded
        .iter()
        .zip(&defaults)
        .filter(|((_, r), (_, d))| r == d && r != "()")
        .map(|((name, _), _)| *name)
        .collect();
    assert!(
        indistinguishable.is_empty(),
        "RecordingKernel must answer differently from the trait default, or a holder that \
         falls back to the default cannot be told apart from one that forwards: \
         {indistinguishable:?}"
    );
}

#[test]
fn holder_forwards_every_method_to_inner_kernel() {
    let (inner, holder_log) = RecordingKernel::new();
    let mut holder = SingleKernelHolder::new();
    holder.register_kernel(Box::new(inner));
    let through_holder = call_every_method(&mut holder);

    let (mut bare, bare_log) = RecordingKernel::new();
    let direct = call_every_method(&mut bare);

    let expected = labels(&direct);
    assert_eq!(
        *bare_log.lock().unwrap(),
        expected,
        "harness labels must name the methods RecordingKernel actually received"
    );
    assert_eq!(
        *holder_log.lock().unwrap(),
        expected,
        "SingleKernelHolder must forward each method to the same method of its inner kernel"
    );

    let mismatched = differing(&through_holder, &direct);
    assert!(
        mismatched.is_empty(),
        "SingleKernelHolder must return its inner kernel's answer unchanged \
         (left: through holder, right: bare kernel):\n{}",
        mismatched.join("\n")
    );
}

#[test]
fn empty_holder_reproduces_every_trait_default() {
    let through_holder = call_every_method(&mut SingleKernelHolder::new());
    let defaults: Vec<(&'static str, String)> = call_every_method(&mut Unregistered)
        .into_iter()
        .map(|(name, rendering)| {
            let normalized = rendering.replace(
                std::any::type_name::<Unregistered>(),
                std::any::type_name::<SingleKernelHolder>(),
            );
            (name, normalized)
        })
        .collect();

    let mismatched = differing(&through_holder, &defaults);
    assert!(
        mismatched.is_empty(),
        "an empty SingleKernelHolder must return exactly the trait default's output \
         (left: empty holder, right: trait default):\n{}",
        mismatched.join("\n")
    );
}
