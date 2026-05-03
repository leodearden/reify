//! FEA load constructors for the stdlib.
//!
//! Provides `point_load`, `pressure_load`, `traction_load`, `body_force`, and
//! `gravity` constructors.  Each returns a `Value::Map` with a `kind`
//! discriminator field, matching the joints/coupling constructor pattern.
//!
//! ## Selector-target validation
//!
//! The topology-selector stdlib bindings (PRD `topology-selectors.md` task 5)
//! have not yet landed — there is no `Value::Face` / `Value::Edge` / `Value::Body`
//! variant today.  The `validate_selector_target` helper therefore only rejects
//! obvious primitive non-selector values (`Value::Real`, `Value::Int`,
//! `Value::Bool`, `Value::Undef`); any other shape (Map, List, String, …) is
//! accepted as an opaque pass-through.  Full topology-kind validation belongs
//! in the FEA evaluation pipeline (PRD task 16) when the engine resolves
//! selectors against the kernel and can produce diagnostics with source spans.

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use crate::test_macros::make_scalar_vec3;
    use reify_types::{DimensionVector, Value};
    use std::collections::BTreeMap;

    // ── point_load constructor: happy path ───────────────────────────────────

    #[test]
    fn point_load_returns_map_with_correct_fields() {
        // Opaque selector stub: a Map that is clearly not a primitive.
        let selector = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("point_stub".to_string()),
            );
            m
        });
        let force = make_scalar_vec3([5000.0, 0.0, 0.0], DimensionVector::FORCE);

        let result = eval_builtin("point_load", &[selector.clone(), force.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("point_load".to_string())),
            "kind field should be 'point_load'"
        );
        assert_eq!(
            map.get(&Value::String("point".to_string())),
            Some(&selector),
            "point field should round-trip the selector input"
        );
        assert_eq!(
            map.get(&Value::String("force".to_string())),
            Some(&force),
            "force field should round-trip the force input"
        );
    }
}
