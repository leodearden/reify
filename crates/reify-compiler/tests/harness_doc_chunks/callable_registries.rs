//! The compiler name registries a documented call may belong to, and the one
//! wording every chunk module uses to report a name that belongs to none.

/// The compiler name registries a documented call form may legitimately belong
/// to, each paired with the const name to quote in a panic so the reader is told
/// WHERE to look rather than just that the lookup failed.
///
/// INCOMPLETE, AND KNOWABLY SO. `units::AFFINE_MAP_CONSTRUCTOR_NAMES` and
/// `math_signatures::MATH_CONSTRUCTION_NAMES` (which carries the `point2` /
/// `point3` / `vec2` / `vec3` prelude constructors geometry.md documents) are
/// real registries but are NOT re-exported from `reify_compiler`'s crate root —
/// `mod units` and `mod math_signatures` are both private, and lib.rs's
/// `pub use units::{…}` omits the affine one. Reaching them needs an edit to
/// `crates/reify-compiler/src/lib.rs`, outside task 5759's file scope. Callers
/// therefore carry an explicit, justified allowlist for names in those two
/// families; when the re-export lands, move those entries here and delete the
/// allowlist rather than growing it.
pub(crate) const CALLABLE_NAME_REGISTRIES: &[(&str, &[&str])] = &[
    (
        "GEOMETRY_FUNCTION_NAMES",
        reify_compiler::GEOMETRY_FUNCTION_NAMES,
    ),
    (
        "GEOMETRY_QUERY_HELPER_NAMES",
        reify_compiler::GEOMETRY_QUERY_HELPER_NAMES,
    ),
    (
        "GEOMETRY_KINEMATIC_QUERY_NAMES",
        reify_compiler::GEOMETRY_KINEMATIC_QUERY_NAMES,
    ),
    (
        "GEOMETRY_TOPOLOGY_SELECTOR_NAMES",
        reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES,
    ),
    ("GEOMETRY_QUERY_NAMES", reify_compiler::GEOMETRY_QUERY_NAMES),
];

/// The [`CALLABLE_NAME_REGISTRIES`] family `name` belongs to, or `None`.
pub(crate) fn registry_family(name: &str) -> Option<&'static str> {
    CALLABLE_NAME_REGISTRIES
        .iter()
        .find(|(_, names)| names.contains(&name))
        .map(|(family, _)| *family)
}

/// Panic text shared by every chunk module's registry check, so they cannot
/// drift apart on what a reader is told to do about a phantom name.
pub(crate) fn phantom_name_panic(chunk_path: &str, where_: &str, name: &str) -> String {
    format!(
        "{chunk_path} documents a call to `{name}(…)` in {where_}, but `{name}` is not a member \
         of ANY compiler name registry ({:?}). Either the chunk teaches a PHANTOM signature the \
         compiler was never shown — the failure mode that cost live probe cycles in the \
         2026-07-24 language review (`rotate(geo, axis, angle)`, `translate(geo, vector)`; tasks \
         #5347 / #5364) — or the name is a prelude/math constructor from one of the registries \
         `CALLABLE_NAME_REGISTRIES` documents as unreachable, in which case add it to THIS \
         call site's allowlist with a justification. Do not widen the allowlist to silence a \
         name you have not looked up.",
        CALLABLE_NAME_REGISTRIES
            .iter()
            .map(|(family, _)| *family)
            .collect::<Vec<_>>()
    )
}
