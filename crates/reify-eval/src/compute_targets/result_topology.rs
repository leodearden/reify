/// Task 4654 (R3a): carried-topology bundle for result values.
///
/// Defines [`CarriedTopology`] — the kernel-free, selector-resolvable topology
/// bundle that result values (ModalResult, ElasticResult, …) carry so the
/// eval-path resolver (R3b, a separate task — the CONSUMER) can resolve
/// `faces_by_normal(part,+Z,tol)` against baked data, never OCCT.
///
/// # Design decisions
///
/// * **PER-FACE normals** (Q3): keyed by `GeometryHandleId`, same as
///   `BoundaryAssociation::OnFace`. Only per-face normals reproduce
///   `faces_by_normal`'s kernel-side selection — a node on two faces has an
///   ambiguous per-node normal (PRD §7.2/§9).
///
/// * **One shared type**: `CarriedTopology` is not modal-only; the same
///   `from_realized_mesh` builder serves modal and FEA result models (DD-3).
///
/// * **Value round-trip via synthetic StructureInstance** (type_id u32::MAX):
///   synthetic result values bypass field-vs-.ri-def validation, matching the
///   `warm_started`/other undeclared-field precedents.
