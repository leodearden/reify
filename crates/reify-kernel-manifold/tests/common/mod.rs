// Compile-time enforcement: this test binary requires the `stub_register`
// feature to be enabled.  The feature is activated for all integration test
// binaries in `tests/` via the self-dev-dep in `[dev-dependencies]`:
//   reify-kernel-manifold = { path = ".", features = ["stub_register"] }
// A regression that drops that activation re-exposes the production-safety
// bug (stub kernel registered in non-test builds) AND would produce a
// confusing runtime "manifold not in registry" failure here rather than a
// clear compile-time message.  This guard makes the failure mode actionable.
#[cfg(not(feature = "stub_register"))]
compile_error!(
    "The `stub_register` feature must be enabled for this test binary. \
     Add `reify-kernel-manifold = { path = \".\", features = [\"stub_register\"] }` to \
     `[dev-dependencies]` in `crates/reify-kernel-manifold/Cargo.toml`. \
     This self-dev-dep activates the feature for all integration test binaries in \
     `tests/` (which are separate compilation units and do not inherit `cfg(test)` \
     from the parent crate)."
);
