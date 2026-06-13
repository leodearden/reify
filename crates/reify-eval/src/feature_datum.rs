//! Feature → datum projection (geometric-relations ε).
//!
//! Builds the deduplicated bundle of datums a realized feature projects onto
//! (the "real missing bridge", PRD §7.2 / design §2.2): `feature.<projection>
//! : Datum`, total downward per the datum lattice. The provenance of a bundle
//! is the union of
//!
//!   * **analytic classification** — `BRepAdaptor_*` → `GeomAbs_*` → `.Axis()`
//!     extraction per sub-face / sub-edge (via the kernel's
//!     [`FaceAnalyticDatum`] / [`EdgeAnalyticDatum`] queries), and
//!   * **construction-history datum-traits** — `Revolute → Axis`,
//!     `Extruded → Direction` read from the topology attribute table,
//!
//! canonicalized by geometric equivalence (coaxial / coplanar / coincident
//! merge within `tol = max(confusion_floor, localTol(A), localTol(B))`).
//!
//! This module owns ε's datum-equivalence + dedup primitive so that ζ (which
//! depends on both γ and ε) can reuse the **same** primitive — fulfilling the
//! design §2.3 coherence law by shared code rather than duplicated magic
//! numbers.
//!
//! # Status
//!
//! Pre-2 scaffolding: this is the registered module path the ε GREEN steps
//! (6 / 8 / 10 / 12) fill in. The equivalence predicates (`axes_coaxial`,
//! `planes_coplanar`, `points_coincident`), the `Datum` carrier, `dedup_datums`,
//! `dedup_tolerance`, and `feature_datum_bundle` land in those steps alongside
//! their RED tests.
//!
//! [`FaceAnalyticDatum`]: reify_ir::GeometryQuery::FaceAnalyticDatum
//! [`EdgeAnalyticDatum`]: reify_ir::GeometryQuery::EdgeAnalyticDatum
