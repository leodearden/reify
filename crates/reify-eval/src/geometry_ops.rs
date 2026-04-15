// Geometry operation compilation: evaluates CompiledGeometryOp into runtime GeometryOp.
//
// Free functions with no Engine coupling — they take values, functions, meta_map
// as plain arguments.

#[cfg(test)]
mod tests {
    use super::compile_geometry_op;
    use reify_types::{GeometryHandleId, ValueMap};
    use std::collections::HashMap;

    /// Smoke test: compile_geometry_op is accessible from this module and can
    /// evaluate a trivial Box primitive.
    #[test]
    fn smoke_compile_geometry_op_box() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};

        let op = CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".to_string(), reify_types::CompiledExpr::literal(reify_types::Value::Real(1.0), reify_types::Type::Real)),
                ("height".to_string(), reify_types::CompiledExpr::literal(reify_types::Value::Real(2.0), reify_types::Type::Real)),
                ("depth".to_string(), reify_types::CompiledExpr::literal(reify_types::Value::Real(3.0), reify_types::Type::Real)),
            ],
        };
        let values = ValueMap::new();
        let step_handles: Vec<GeometryHandleId> = vec![];
        let functions = vec![];
        let meta_map = HashMap::new();
        let mut diagnostics = Vec::new();

        let result = compile_geometry_op(&op, &values, &step_handles, &functions, &meta_map, &mut diagnostics);
        assert!(result.is_some(), "Box with valid args should compile to a GeometryOp");
    }
}
