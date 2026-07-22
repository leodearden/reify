    use super::*;
    use reify_compiler::{CompiledGeometryOp, GeomRef, PatternKind, SweepKind, TransformKind};
    use reify_ir::GeometryHandleId;

    /// Helper: build a CompiledExpr literal from a constant f64.
    fn literal_f64(v: f64) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(v),
            reify_core::Type::dimensionless_scalar(),
        )
    }

    /// Helper: build a CompiledExpr literal from a Scalar with LENGTH dimension.
    fn literal_length(meters: f64) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Scalar {
                si_value: meters,
                dimension: reify_core::DimensionVector::LENGTH,
            },
            reify_core::Type::length(),
        )
    }

    /// Helper: build a CompiledExpr literal from a Scalar with ANGLE dimension (radians).
    fn literal_angle(radians: f64) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Scalar {
                si_value: radians,
                dimension: reify_core::DimensionVector::ANGLE,
            },
            reify_core::Type::angle(),
        )
    }

    /// Helper: build an inline `CompiledExpr` literal from a `Value::Scalar`
    /// with an arbitrary `DimensionVector`. Used by task ε's inline-arg tests
    /// (the converted resolvers `eval_expr` the arg, so a `Literal` cell now
    /// flows through exactly like a `ValueRef → Scalar`). The `Type` carried by
    /// the literal is irrelevant to `eval_expr` (which clones the `Value`), so
    /// the dimension on the `Type::Scalar` simply mirrors the value's.
    fn literal_scalar(
        si_value: f64,
        dimension: reify_core::DimensionVector,
    ) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Scalar {
                si_value,
                dimension,
            },
            reify_core::Type::Scalar { dimension },
        )
    }

    /// Helper: wrap a bare `GeometryHandleId` in a `KernelHandle` with the
    /// default test kernel (`KernelId::Occt`).
    ///
    /// Bulk test fixtures use `kh(id)` to keep the named_steps map concise;
    /// contract tests that verify `.kernel` is ignored construct inline
    /// `KernelHandle { kernel: KernelId::Manifold/Fidget, id }` instead.
    fn kh(id: GeometryHandleId) -> reify_ir::KernelHandle {
        reify_ir::KernelHandle {
            kernel: reify_ir::KernelId::Occt,
            id,
        }
    }

    /// Bare `Value::Real` components in a `Value::Point` are NOT a valid
    /// production shape for a `Type::Point<Length>` cell.  The function MUST
    /// return `None` (returning `Some([...])` would silently reinterpret the
    /// raw floats as SI metres at the kernel boundary — exactly the hazard this
    /// closes).  All production mocks use `Value::length(...)` components (i.e.
    /// `Value::Scalar { dimension: LENGTH, .. }`).
    ///
    /// FLIP (task ε, evaluate-then-accept): the resolver now EVALUATES the arg
    /// and, on this defined-but-wrong shape, ALSO pushes exactly one
    /// `Severity::Warning` naming the builtin / arg / expected `Point<Length>`,
    /// instead of the prior silent fall-through to `None`.
    #[test]
    fn resolve_point3_length_arg_bare_real_components_return_none() {
        let cell = reify_core::ValueCellId::new("Bracket", "p");
        let expr = reify_ir::CompiledExpr::value_ref(
            cell.clone(),
            reify_core::Type::point3(reify_core::Type::length()),
        );
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            cell,
            reify_ir::Value::Point(vec![
                reify_ir::Value::Real(1.0),
                reify_ir::Value::Real(2.0),
                reify_ir::Value::Real(3.0),
            ]),
        );
        let mut diags: Vec<Diagnostic> = Vec::new();
        assert_eq!(
            super::resolve_point3_length_arg(&expr, &values, "closest_point", "point", &mut diags),
            None,
            "bare Value::Real components must produce None — production cells \
             carry Type::Point<Length> so components must be \
             Value::Scalar {{ dimension: LENGTH, .. }}; a bare Real slipping \
             through would be silently reinterpreted as metres at the kernel \
             boundary, hence the function must return None"
        );
        // FLIP (task ε): the defined-but-wrong shape now emits exactly one
        // Severity::Warning, not a silent None.
        assert_eq!(
            diags.len(),
            1,
            "bare-Real Point must push exactly 1 Warning (FLIP from silent), got: {diags:?}"
        );
        assert_eq!(diags[0].severity, reify_core::Severity::Warning);
        let msg = diags[0].message.to_lowercase();
        assert!(
            msg.contains("closest_point"),
            "warning must name the builtin, got: {:?}",
            diags[0].message
        );
        assert!(
            msg.contains("point<length>"),
            "warning must name expected Point<Length>, got: {:?}",
            diags[0].message
        );
    }

    /// Task ε (evaluate-then-accept): `resolve_point3_length_arg` now EVALUATES
    /// the arg expr (gaining a `diagnostics` sink + builtin/arg labels). A
    /// `Value::Point` of exactly three LENGTH-dimensioned Scalars — whether an
    /// inline `Literal` or a `ValueRef → Point<Length>` cell — resolves to its
    /// `[m, m, m]` SI components with 0 diagnostics; a defined-but-wrong value
    /// (non-Point, or wrong arity) is Rejected with exactly one
    /// `Severity::Warning` naming the builtin, the arg, and the expected
    /// `Point<Length>` type (byte-uniform wording with the density / vec3 /
    /// range paths). A `Value::Undef` (missing cell) degrades quietly.
    ///
    ///   (a) inline `Literal(Point[LENGTH×3])` → `Some([..])`, 0 diags.
    ///   (b) `ValueRef → Point[LENGTH×3]` cell → `Some([..])`, 0 diags.
    ///   (c) non-Point (`Value::Real`) → `None` + 1 Warning.
    ///   (d) wrong arity (`Point` of 2) → `None` + 1 Warning.
    ///   (e) missing-cell `ValueRef` → `Undef` → `None`, 0 diags (quiet).
    ///
    /// Compile-RED until step-10 adds the `(builtin, arg, &mut diags)` signature.
    #[test]
    fn resolve_point3_length_arg_eval_and_diagnostics() {
        // (a) inline Literal(Point[LENGTH×3]) → Some([..]), 0 diags.
        {
            let expr = reify_ir::CompiledExpr::literal(
                point3_length_value(0.01, 0.02, 0.03),
                reify_core::Type::point3(reify_core::Type::length()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_point3_length_arg(
                &expr,
                &values,
                "closest_point",
                "point",
                &mut diags,
            );
            assert_eq!(
                result,
                Some([0.01, 0.02, 0.03]),
                "(a) inline Point<Length> literal must be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(a) Point literal must produce no diags, got: {diags:?}"
            );
        }

        // (b) ValueRef → Point[LENGTH×3] cell → Some([..]), 0 diags.
        {
            let cell = reify_core::ValueCellId::new("Bracket", "p");
            let expr = reify_ir::CompiledExpr::value_ref(
                cell.clone(),
                reify_core::Type::point3(reify_core::Type::length()),
            );
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, point3_length_value(0.1, 0.2, 0.3));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_point3_length_arg(&expr, &values, "is_on", "point", &mut diags);
            assert_eq!(
                result,
                Some([0.1, 0.2, 0.3]),
                "(b) ValueRef Point<Length> must be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(b) ValueRef Point must produce no diags, got: {diags:?}"
            );
        }

        // (c) non-Point (Value::Real) → None + 1 Warning naming builtin/arg/Point<Length>.
        {
            let expr = literal_f64(1.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_point3_length_arg(&expr, &values, "contains", "point", &mut diags);
            assert_eq!(result, None, "(c) non-Point must return None");
            assert_eq!(
                diags.len(),
                1,
                "(c) non-Point must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("contains"),
                "(c) names builtin, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("point"),
                "(c) names arg, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("point<length>"),
                "(c) names expected Point<Length>, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("got"),
                "(c) names what it got, got: {:?}",
                diags[0].message
            );
        }

        // (d) wrong arity (Point of 2 LENGTH scalars) → None + 1 Warning.
        {
            let expr = reify_ir::CompiledExpr::literal(
                reify_ir::Value::Point(vec![
                    reify_ir::Value::length(0.01),
                    reify_ir::Value::length(0.02),
                ]),
                reify_core::Type::point3(reify_core::Type::length()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_point3_length_arg(&expr, &values, "normal", "point", &mut diags);
            assert_eq!(result, None, "(d) wrong-arity Point must return None");
            assert_eq!(
                diags.len(),
                1,
                "(d) wrong-arity Point must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("point<length>"),
                "(d) names expected Point<Length>, got: {:?}",
                diags[0].message
            );
        }

        // (e) missing-cell ValueRef → Undef → None, 0 diags (quiet).
        {
            let cell = reify_core::ValueCellId::new("Bracket", "missing_point");
            let expr = reify_ir::CompiledExpr::value_ref(
                cell,
                reify_core::Type::point3(reify_core::Type::length()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_point3_length_arg(&expr, &values, "curvature", "point", &mut diags);
            assert_eq!(result, None, "(e) missing cell must return None");
            assert!(
                diags.is_empty(),
                "(e) missing cell must be quiet, got: {diags:?}"
            );
        }
    }

    /// Task ε (evaluate-then-accept): `resolve_int_value_ref` (the kinematic
    /// body-id resolver for `interferes_with` / `min_clearance`) now EVALUATES
    /// the arg expr (gaining a `diagnostics` sink + builtin/arg labels) instead
    /// of shape-matching `CompiledExprKind::ValueRef`. A `Value::Int` — whether
    /// an inline `Literal` or a `ValueRef → Int` cell — resolves to its `i64`
    /// with 0 diagnostics; a defined-but-wrong value (non-Int) is Rejected with
    /// exactly one `Severity::Warning` naming the kinematic builtin, the arg,
    /// and the expected `Int` type (byte-uniform wording with the density /
    /// point / vec3 / range paths). A `Value::Undef` (missing cell) degrades
    /// quietly — behaviourally identical to the prior `values.get(id)` fall-through.
    ///
    ///   (a) inline `Literal(Int)` → `Some(n)`, 0 diags.
    ///   (b) `ValueRef → Int` cell → `Some(n)`, 0 diags.
    ///   (c) non-Int (`Value::Real`) → `None` + 1 Warning naming builtin/arg/Int/got.
    ///   (d) non-Int (`Value::Scalar`) → `None` + 1 Warning.
    ///   (e) missing-cell `ValueRef` → `Undef` → `None`, 0 diags (quiet).
    ///
    /// Compile-RED until step-12 adds the `(builtin, arg, &mut diags)` signature
    /// (today `resolve_int_value_ref` is `(expr, values) -> Option<i64>` and
    /// silently returns `None` on a non-Int, with no diagnostic).
    #[test]
    fn resolve_int_value_ref_eval_and_diagnostics() {
        // (a) inline Literal(Int) → Some(n), 0 diags.
        {
            let expr =
                reify_ir::CompiledExpr::literal(reify_ir::Value::Int(7), reify_core::Type::Int);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_int_value_ref(
                &expr,
                &values,
                "interferes_with",
                "body_a",
                &mut diags,
            );
            assert_eq!(result, Some(7), "(a) inline Int literal must be Accepted");
            assert!(
                diags.is_empty(),
                "(a) Int literal must produce no diags, got: {diags:?}"
            );
        }

        // (b) ValueRef → Int cell → Some(n), 0 diags.
        {
            let cell = reify_core::ValueCellId::new("Mech", "id_a");
            let expr = reify_ir::CompiledExpr::value_ref(cell.clone(), reify_core::Type::Int);
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, reify_ir::Value::Int(2));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_int_value_ref(&expr, &values, "min_clearance", "body_b", &mut diags);
            assert_eq!(result, Some(2), "(b) ValueRef Int must be Accepted");
            assert!(
                diags.is_empty(),
                "(b) ValueRef Int must produce no diags, got: {diags:?}"
            );
        }

        // (c) non-Int (Value::Real) → None + 1 Warning naming builtin/arg/Int/got.
        {
            let expr = literal_f64(1.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_int_value_ref(
                &expr,
                &values,
                "interferes_with",
                "body_a",
                &mut diags,
            );
            assert_eq!(result, None, "(c) non-Int must return None");
            assert_eq!(
                diags.len(),
                1,
                "(c) non-Int must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("interferes_with"),
                "(c) names builtin, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("body_a"),
                "(c) names arg, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("int"),
                "(c) names expected Int, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("got"),
                "(c) names what it got, got: {:?}",
                diags[0].message
            );
        }

        // (d) non-Int (Value::Scalar) → None + 1 Warning.
        {
            let expr = literal_length(0.05);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_int_value_ref(&expr, &values, "min_clearance", "body_b", &mut diags);
            assert_eq!(result, None, "(d) Scalar must return None");
            assert_eq!(
                diags.len(),
                1,
                "(d) Scalar must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("int"),
                "(d) names expected Int, got: {:?}",
                diags[0].message
            );
        }

        // (e) missing-cell ValueRef → Undef → None, 0 diags (quiet).
        {
            let cell = reify_core::ValueCellId::new("Mech", "missing_id");
            let expr = reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::Int);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_int_value_ref(
                &expr,
                &values,
                "interferes_with",
                "body_a",
                &mut diags,
            );
            assert_eq!(result, None, "(e) missing cell must return None");
            assert!(
                diags.is_empty(),
                "(e) missing cell must be quiet, got: {diags:?}"
            );
        }
    }

    /// Task ε (evaluate-then-accept): `resolve_string_literal_arg` (the selector
    /// name/label resolver for `face`/`edge`/`solid_body` and the ad-hoc
    /// `@face`/`@edge` base/label) now EVALUATES the arg expr (gaining a
    /// `diagnostics` sink + builtin/arg labels) and returns an OWNED `String`
    /// (was `Option<&str>` matching only `Literal(Value::String)`). A
    /// `Value::String` — whether an inline `Literal` or a `ValueRef → String`
    /// cell — resolves to its owned `String` with 0 diagnostics; a
    /// defined-but-wrong value (non-String) is Rejected with exactly one
    /// `Severity::Warning` naming the builtin, the arg, and the expected
    /// `String` type (byte-uniform wording with the density / point / vec3 /
    /// range / int paths). A `Value::Undef` (missing cell) degrades quietly.
    ///
    /// Both call contexts are covered via the builtin/arg labels: the named
    /// leaf selector (`face(body,"top")` → builtin `face`, arg `name`) and the
    /// ad-hoc selector (`@face("top")` → builtin `@face`, arg `label`).
    ///
    ///   (a) inline `Literal(String)` → `Some("top")`, 0 diags.
    ///   (b) `ValueRef → String` cell → `Some("side")`, 0 diags.
    ///   (c) non-String (`Value::Int`) → `None` + 1 Warning naming builtin/arg/String/got.
    ///   (d) missing-cell `ValueRef` → `Undef` → `None`, 0 diags (quiet).
    ///
    /// Compile-RED until step-14 changes the signature to
    /// `(expr, values, builtin, arg, &mut diags) -> Option<String>` (today
    /// `resolve_string_literal_arg(expr) -> Option<&str>` matches only an inline
    /// `Literal(Value::String)` and silently returns `None` otherwise).
    #[test]
    fn resolve_string_literal_arg_eval_and_diagnostics() {
        // (a) inline Literal(String) → Some("top"), 0 diags (named-leaf context).
        {
            let expr = reify_ir::CompiledExpr::literal(
                reify_ir::Value::String("top".to_string()),
                reify_core::Type::String,
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_string_literal_arg(&expr, &values, "face", "name", &mut diags);
            assert_eq!(
                result,
                Some("top".to_string()),
                "(a) inline String literal must be Accepted as an owned String"
            );
            assert!(
                diags.is_empty(),
                "(a) String literal must produce no diags, got: {diags:?}"
            );
        }

        // (b) ValueRef → String cell → Some("side"), 0 diags (ad-hoc label context).
        {
            let cell = reify_core::ValueCellId::new("Part", "label");
            let expr = reify_ir::CompiledExpr::value_ref(cell.clone(), reify_core::Type::String);
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, reify_ir::Value::String("side".to_string()));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_string_literal_arg(&expr, &values, "@face", "label", &mut diags);
            assert_eq!(
                result,
                Some("side".to_string()),
                "(b) ValueRef String must be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(b) ValueRef String must produce no diags, got: {diags:?}"
            );
        }

        // (c) non-String (Value::Int) → None + 1 Warning naming builtin/arg/String/got.
        {
            let expr =
                reify_ir::CompiledExpr::literal(reify_ir::Value::Int(5), reify_core::Type::Int);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_string_literal_arg(&expr, &values, "edge", "name", &mut diags);
            assert_eq!(result, None, "(c) non-String must return None");
            assert_eq!(
                diags.len(),
                1,
                "(c) non-String must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("edge"),
                "(c) names builtin, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("name"),
                "(c) names arg, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("string"),
                "(c) names expected String, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("got"),
                "(c) names what it got, got: {:?}",
                diags[0].message
            );
        }

        // (d) missing-cell ValueRef → Undef → None, 0 diags (quiet).
        {
            let cell = reify_core::ValueCellId::new("Part", "missing_label");
            let expr = reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::String);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_string_literal_arg(&expr, &values, "@edge", "label", &mut diags);
            assert_eq!(result, None, "(d) missing cell must return None");
            assert!(
                diags.is_empty(),
                "(d) missing cell must be quiet, got: {diags:?}"
            );
        }
    }

    /// Tests for `resolve_density_arg`: diagnostic behavior for the NEW
    /// Density-only contract (γ, task 4486).
    ///
    /// NEW contract under test:
    ///   (a) ValueRef → Scalar{MASS_DENSITY, 7850.0} → Some(7850.0), 0 diagnostics
    ///       [NEW accept — was Warning+None under the old contract].
    ///   (b) ValueRef → Value::Real(7850.0) → None + exactly 1 Severity::Warning
    ///       whose lowercased message contains "density" AND "7850kg/m^3"
    ///       [FLIP — was accepted silently].
    ///   (c) ValueRef → dimensionless Scalar → None + 1 Warning [FLIP — was accepted].
    ///   (d) ValueRef → Scalar{LENGTH} → None + 1 Warning [keep reject].
    ///   (e) ValueRef → Value::Bool(true) → None + 1 Warning [keep reject].
    ///   (f) Non-ValueRef expr (literal_f64) → None + exactly 1 Warning
    ///       [LOUD — was 0/silent under old "unsupported arg shape → silent" contract].
    ///
    /// Modelled on `resolve_point3_length_arg_bare_real_components_return_none` above
    /// — build a `value_ref` expr + a `ValueMap`, call the helper directly,
    /// assert the return value and diagnostic side-effect, compiler-independently.
    #[test]
    fn resolve_density_arg_diagnostics() {
        fn make_value_ref(cell: reify_core::ValueCellId) -> reify_ir::CompiledExpr {
            reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::dimensionless_scalar())
        }

        // (a) ValueRef → MASS_DENSITY Scalar → Some(7850.0), 0 diagnostics [NEW accept]
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(
                cell,
                reify_ir::Value::Scalar {
                    si_value: 7850.0,
                    dimension: reify_core::DimensionVector::MASS_DENSITY,
                },
            );
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(
                result,
                Some(7850.0),
                "(a) MASS_DENSITY Scalar must return Some(7850.0)"
            );
            assert!(
                diags.is_empty(),
                "(a) MASS_DENSITY Scalar must produce no diagnostics, got: {:?}",
                diags
            );
        }

        // (b) ValueRef → Value::Real(7850.0) → None + 1 Warning with "density" + "7850kg/m^3" [FLIP]
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho2");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, reify_ir::Value::Real(7850.0));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(b) Value::Real must return None");
            assert_eq!(
                diags.len(),
                1,
                "(b) Value::Real must push exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(b) diagnostic must be Warning severity"
            );
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("density"),
                "(b) warning must name 'density', got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("7850kg/m^3"),
                "(b) warning must contain '7850kg/m^3' migration hint, got: {:?}",
                diags[0].message
            );
        }

        // (c) ValueRef → dimensionless Scalar → None + 1 Warning [FLIP]
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho3");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(
                cell,
                reify_ir::Value::Scalar {
                    si_value: 7850.0,
                    dimension: reify_core::DimensionVector::DIMENSIONLESS,
                },
            );
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(
                result, None,
                "(c) dimensionless Scalar must return None (no longer accepted)"
            );
            assert_eq!(
                diags.len(),
                1,
                "(c) dimensionless Scalar must push exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(c) diagnostic must be Warning severity"
            );
        }

        // (d) ValueRef → Scalar{LENGTH} → None + 1 Warning [keep reject]
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho4");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(
                cell,
                reify_ir::Value::Scalar {
                    si_value: 1.0,
                    dimension: reify_core::DimensionVector::LENGTH,
                },
            );
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(d) LENGTH Scalar must return None");
            assert_eq!(
                diags.len(),
                1,
                "(d) LENGTH Scalar must push exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(d) diagnostic must be Warning severity"
            );
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("density"),
                "(d) warning must name 'density', got: {:?}",
                diags[0].message
            );
        }

        // (e) ValueRef → Value::Bool(true) → None + 1 Warning [keep reject]
        {
            let cell = reify_core::ValueCellId::new("TestDef", "rho5");
            let expr = make_value_ref(cell.clone());
            let mut values = reify_ir::ValueMap::new();
            values.insert(cell, reify_ir::Value::Bool(true));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(e) Bool must return None");
            assert_eq!(
                diags.len(),
                1,
                "(e) Bool must push exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(e) diagnostic must be Warning severity"
            );
        }

        // (f) Non-ValueRef (literal_f64) → None + 1 Warning [LOUD — was silent]
        {
            let expr = literal_f64(7850.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(f) Literal expr must return None");
            assert_eq!(
                diags.len(),
                1,
                "(f) Non-ValueRef literal must push exactly 1 Warning (γ=LOUD), got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(f) diagnostic must be Warning severity"
            );
        }
    }

    /// Task ε (evaluate-then-accept): `resolve_density_arg` now EVALUATES an
    /// inline (non-`ValueRef`) arg expression instead of warning "not yet
    /// supported". The headline `moment_of_inertia(b, 7850kg/m^3)` inline form
    /// must be ACCEPTED; an inline bare `Real` / wrong-dimension `Scalar` must
    /// be REJECTED with exactly one Warning carrying the same wording as the
    /// `ValueRef` path.
    ///
    ///   (a) inline `Literal(Scalar{MASS_DENSITY, 7850})` → `Some(7850.0)`,
    ///       0 diagnostics [RED before ε: the non-`ValueRef` branch warned + None].
    ///   (b) inline `Literal(Real(7850.0))` → `None` + 1 Warning naming
    ///       `density` + `7850kg/m^3`.
    ///   (c) inline `Literal(Scalar{PRESSURE})` → `None` + 1 Warning
    ///       (Pressure-as-density hole stays closed for the inline shape too).
    #[test]
    fn resolve_density_arg_inline_evaluates() {
        // (a) inline MASS_DENSITY literal → Some(7850.0), 0 diagnostics.
        {
            let expr = literal_scalar(7850.0, reify_core::DimensionVector::MASS_DENSITY);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(
                result,
                Some(7850.0),
                "(a) inline MASS_DENSITY literal must evaluate + be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(a) inline MASS_DENSITY literal must produce no diagnostics, got: {:?}",
                diags
            );
        }

        // (b) inline bare Real literal → None + 1 Warning naming density + hint.
        {
            let expr = literal_f64(7850.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(b) inline bare Real must return None");
            assert_eq!(
                diags.len(),
                1,
                "(b) inline bare Real must push exactly 1 Warning, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(b) diagnostic must be Warning severity"
            );
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("density"),
                "(b) warning must name 'density', got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("7850kg/m^3"),
                "(b) warning must contain '7850kg/m^3' migration hint, got: {:?}",
                diags[0].message
            );
        }

        // (c) inline Pressure Scalar literal → None + 1 Warning [closed hole].
        {
            let expr = literal_scalar(2.0e11, reify_core::DimensionVector::PRESSURE);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_density_arg(&expr, &values, "moment_of_inertia", &mut diags);
            assert_eq!(result, None, "(c) inline Pressure Scalar must return None");
            assert_eq!(
                diags.len(),
                1,
                "(c) inline Pressure Scalar must push exactly 1 Warning, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                reify_core::Severity::Warning,
                "(c) diagnostic must be Warning severity"
            );
        }
    }

    /// Helper: build a `CompiledExpr` literal from a `Value::Bool`.
    fn literal_bool(b: bool) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(reify_ir::Value::Bool(b), reify_core::Type::Bool)
    }

    /// Task ε (evaluate-then-accept): the scalar-bound wrappers
    /// `resolve_angle_scalar_arg` / `resolve_length_scalar_arg` now EVALUATE the
    /// arg expr and route the result through `accept_arg`, gaining a
    /// `diagnostics` sink + builtin/arg labels. An inline dimensioned literal of
    /// the expected dimension is Accepted (0 diags); a defined-but-wrong value
    /// (wrong dimension, dimensionless, or non-Scalar) is Rejected with exactly
    /// one Warning naming the builtin, the arg, and the expected type.
    #[test]
    fn resolve_scalar_bound_arg_eval_and_diagnostics() {
        // (a) inline ANGLE literal → Some(rad), 0 diagnostics.
        {
            let expr = literal_scalar(0.25, reify_core::DimensionVector::ANGLE);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_angle_scalar_arg(
                &expr,
                &values,
                "faces_by_normal",
                "tol",
                &mut diags,
            );
            assert_eq!(
                result,
                Some(0.25),
                "(a) inline ANGLE literal must be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(a) ANGLE literal must produce no diags, got: {diags:?}"
            );
        }

        // (b) inline LENGTH literal → Some(m), 0 diagnostics.
        {
            let expr = literal_scalar(0.005, reify_core::DimensionVector::LENGTH);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_length_scalar_arg(
                &expr,
                &values,
                "edges_at_height",
                "z",
                &mut diags,
            );
            assert_eq!(
                result,
                Some(0.005),
                "(b) inline LENGTH literal must be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(b) LENGTH literal must produce no diags, got: {diags:?}"
            );
        }

        // (c) wrong dimension (ANGLE where LENGTH expected) → None + 1 Warning.
        {
            let expr = literal_scalar(0.25, reify_core::DimensionVector::ANGLE);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_length_scalar_arg(
                &expr,
                &values,
                "edges_at_height",
                "z",
                &mut diags,
            );
            assert_eq!(
                result, None,
                "(c) ANGLE where LENGTH expected must return None"
            );
            assert_eq!(
                diags.len(),
                1,
                "(c) must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("edges_at_height"),
                "(c) names builtin, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("z"),
                "(c) names arg, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("length"),
                "(c) names expected Length, got: {:?}",
                diags[0].message
            );
        }

        // (d) non-Scalar (Bool) where ANGLE expected → None + 1 Warning.
        {
            let expr = literal_bool(true);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_angle_scalar_arg(
                &expr,
                &values,
                "faces_by_normal",
                "tol",
                &mut diags,
            );
            assert_eq!(
                result, None,
                "(d) Bool where ANGLE expected must return None"
            );
            assert_eq!(
                diags.len(),
                1,
                "(d) must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("faces_by_normal"),
                "(d) names builtin, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("angle"),
                "(d) names expected Angle, got: {:?}",
                diags[0].message
            );
        }

        // (e) Undef (missing cell ValueRef) → None, 0 diagnostics (quiet).
        {
            let cell = reify_core::ValueCellId::new("Bracket", "missing");
            let expr = reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::length());
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_length_scalar_arg(
                &expr,
                &values,
                "edges_at_height",
                "z",
                &mut diags,
            );
            assert_eq!(result, None, "(e) missing cell must return None");
            assert!(
                diags.is_empty(),
                "(e) missing cell must be quiet, got: {diags:?}"
            );
        }
    }

    /// Task ε (evaluate-then-accept): `resolve_vec3_arg` now EVALUATES the arg
    /// expr (gaining a `diagnostics` sink + builtin/arg labels). An inline
    /// `Literal(Value::Vector)` AND an inline `vec3(..)` FunctionCall both
    /// resolve to `Some([..])` with 0 diagnostics; a defined-but-wrong value
    /// (non-Vector, wrong length, or a dimensioned-Scalar component) is Rejected
    /// with exactly one `Severity::Warning` naming the builtin, the arg, and the
    /// expected `Vec3` type (byte-uniform wording with the density path).
    ///
    ///   (a) inline `Literal(Vector([Real,Real,Real]))` → `Some([..])`, 0 diags.
    ///   (b) inline `vec3(0,0,1)` FunctionCall → `Some([0,0,1])`, 0 diags
    ///       [RED before ε: the FunctionCall hit `resolve_vec3_arg`'s `_ => None`
    ///       arm → silent fall-through].
    ///   (c) inline `Literal(Real)` (non-Vector) → `None` + 1 Warning.
    ///   (d) inline `Literal(Vector)` of length 2 (wrong length) → `None` + 1 Warning.
    ///   (e) inline `Literal(Vector)` with a dimensioned-Scalar component → `None`
    ///       + 1 Warning.
    ///
    /// Compile-RED until step-6 adds the `(builtin, arg, &mut diags)` signature.
    #[test]
    fn resolve_vec3_arg_eval_and_diagnostics() {
        // (a) inline Literal(Vector([Real,Real,Real])) → Some([..]), 0 diags.
        {
            let expr = reify_ir::CompiledExpr::literal(
                vec3_value(0.0, 0.0, 1.0),
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_vec3_arg(&expr, &values, "faces_by_normal", "dir", &mut diags);
            assert_eq!(
                result,
                Some([0.0, 0.0, 1.0]),
                "(a) inline vector literal must be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(a) vector literal must produce no diags, got: {diags:?}"
            );
        }

        // (b) inline vec3(0,0,1) FunctionCall → Some([0,0,1]), 0 diags.
        {
            let arg_x = literal_f64(0.0);
            let arg_y = literal_f64(0.0);
            let arg_z = literal_f64(1.0);
            let mut ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
                .combine(reify_core::ContentHash::of_str("vec3"));
            ch = ch
                .combine(arg_x.content_hash)
                .combine(arg_y.content_hash)
                .combine(arg_z.content_hash);
            let expr = reify_ir::CompiledExpr {
                kind: reify_ir::CompiledExprKind::FunctionCall {
                    function: reify_ir::ResolvedFunction {
                        name: "vec3".to_string(),
                        qualified_name: "vec3".to_string(),
                    },
                    args: vec![arg_x, arg_y, arg_z],
                },
                result_type: reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
                content_hash: ch,
            };
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_vec3_arg(&expr, &values, "faces_by_normal", "dir", &mut diags);
            assert_eq!(
                result,
                Some([0.0, 0.0, 1.0]),
                "(b) inline vec3(0,0,1) FunctionCall must evaluate + be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(b) inline vec3 call must produce no diags, got: {diags:?}"
            );
        }

        // (c) non-Vector (Value::Real) → None + 1 Warning naming builtin/arg/Vec3.
        {
            let expr = literal_f64(1.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_vec3_arg(&expr, &values, "faces_by_normal", "dir", &mut diags);
            assert_eq!(result, None, "(c) non-Vector must return None");
            assert_eq!(
                diags.len(),
                1,
                "(c) non-Vector must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("faces_by_normal"),
                "(c) names builtin, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("dir"),
                "(c) names arg, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("vec3"),
                "(c) names expected Vec3, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("got"),
                "(c) names what it got, got: {:?}",
                diags[0].message
            );
        }

        // (d) wrong length (Vector of 2) → None + 1 Warning.
        {
            let expr = reify_ir::CompiledExpr::literal(
                reify_ir::Value::Vector(vec![
                    reify_ir::Value::Real(0.0),
                    reify_ir::Value::Real(1.0),
                ]),
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_vec3_arg(&expr, &values, "edges_parallel_to", "axis", &mut diags);
            assert_eq!(result, None, "(d) wrong-length Vector must return None");
            assert_eq!(
                diags.len(),
                1,
                "(d) wrong-length Vector must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("edges_parallel_to"),
                "(d) names builtin, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("axis"),
                "(d) names arg, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("vec3"),
                "(d) names expected Vec3, got: {:?}",
                diags[0].message
            );
        }

        // (e) dimensioned-Scalar component → None + 1 Warning.
        {
            let expr = reify_ir::CompiledExpr::literal(
                reify_ir::Value::Vector(vec![
                    reify_ir::Value::Scalar {
                        si_value: 1.0,
                        dimension: reify_core::DimensionVector::LENGTH,
                    },
                    reify_ir::Value::Real(0.0),
                    reify_ir::Value::Real(0.0),
                ]),
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            );
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result =
                super::resolve_vec3_arg(&expr, &values, "faces_by_normal", "dir", &mut diags);
            assert_eq!(result, None, "(e) dimensioned component must return None");
            assert_eq!(
                diags.len(),
                1,
                "(e) dimensioned component must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("vec3"),
                "(e) names expected Vec3, got: {:?}",
                diags[0].message
            );
        }
    }

    /// Helper: build an inline `CompiledExpr` literal carrying a `Value::Range`
    /// with the given optional `(lower, upper)` SI bounds, each a
    /// `Value::Scalar` of `dim`. `None` bounds model a half-open range. The
    /// `Type` is irrelevant to `eval_expr` (which clones the `Value`), so a
    /// `dimensionless_scalar` placeholder suffices.
    fn literal_range(
        lower: Option<f64>,
        upper: Option<f64>,
        dim: reify_core::DimensionVector,
    ) -> reify_ir::CompiledExpr {
        let mk = |si: f64| -> Box<reify_ir::Value> {
            Box::new(reify_ir::Value::Scalar {
                si_value: si,
                dimension: dim,
            })
        };
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Range {
                lower: lower.map(mk),
                upper: upper.map(mk),
                lower_inclusive: true,
                upper_inclusive: true,
            },
            reify_core::Type::dimensionless_scalar(),
        )
    }

    /// Task ε (evaluate-then-accept): `resolve_range_dim_arg` now EVALUATES the
    /// arg expr (gaining a `diagnostics` sink + builtin/arg labels). An inline
    /// `Range<dim>` with both bounds present and dimensioned `expected_dim`
    /// resolves to `Some((lo, hi))` with 0 diagnostics; a defined-but-wrong
    /// value — non-Range, half-open (one bound `None`), or bounds of the wrong
    /// dimension — is Rejected with exactly one `Severity::Warning` naming the
    /// builtin, the arg, and the expected `Range<dim>` type (byte-uniform
    /// wording with the density / vec3 paths). A `Value::Undef` (missing cell)
    /// degrades quietly.
    ///
    ///   (a) inline `Literal(Range{Some(LENGTH 0), Some(LENGTH 0.05)})`
    ///       → `Some((0.0, 0.05))`, 0 diags.
    ///   (b) inline `Literal(Real)` (non-Range) → `None` + 1 Warning.
    ///   (c) inline half-open `Range{Some, None}` → `None` + 1 Warning.
    ///   (d) inline wrong-dimension `Range{ANGLE, ANGLE}` where LENGTH expected
    ///       → `None` + 1 Warning.
    ///   (e) missing-cell `ValueRef` → `Value::Undef` → `None`, 0 diags (quiet).
    ///   (f) inline `Range<AREA>` (faces_by_area path) → `Some(..)`, 0 diags.
    ///
    /// Compile-RED until step-8 adds the
    /// `(expected_dim, builtin, arg, &mut diags)` signature.
    #[test]
    fn resolve_range_dim_arg_eval_and_diagnostics() {
        use reify_core::DimensionVector;

        // (a) inline Range<LENGTH> with both bounds → Some((0.0, 0.05)), 0 diags.
        {
            let expr = literal_range(Some(0.0), Some(0.05), DimensionVector::LENGTH);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::LENGTH,
                "edges_by_length",
                "length_range",
                &mut diags,
            );
            assert_eq!(
                result,
                Some((0.0, 0.05)),
                "(a) inline closed Range<Length> must be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(a) closed Range must produce no diags, got: {diags:?}"
            );
        }

        // (b) non-Range (Value::Real) → None + 1 Warning naming builtin/arg/Range.
        {
            let expr = literal_f64(1.0);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::LENGTH,
                "edges_by_length",
                "length_range",
                &mut diags,
            );
            assert_eq!(result, None, "(b) non-Range must return None");
            assert_eq!(
                diags.len(),
                1,
                "(b) non-Range must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("edges_by_length"),
                "(b) names builtin, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("length_range"),
                "(b) names arg, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("range"),
                "(b) names expected Range, got: {:?}",
                diags[0].message
            );
            assert!(
                msg.contains("got"),
                "(b) names what it got, got: {:?}",
                diags[0].message
            );
        }

        // (c) half-open Range (upper: None) → None + 1 Warning.
        {
            let expr = literal_range(Some(0.0), None, DimensionVector::LENGTH);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::LENGTH,
                "edges_by_length",
                "length_range",
                &mut diags,
            );
            assert_eq!(result, None, "(c) half-open Range must return None");
            assert_eq!(
                diags.len(),
                1,
                "(c) half-open Range must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("range"),
                "(c) names expected Range, got: {:?}",
                diags[0].message
            );
        }

        // (d) wrong-dimension bounds (ANGLE where LENGTH expected) → None + 1 Warning.
        {
            let expr = literal_range(Some(0.0), Some(0.25), DimensionVector::ANGLE);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::LENGTH,
                "edges_by_length",
                "length_range",
                &mut diags,
            );
            assert_eq!(result, None, "(d) wrong-dimension bounds must return None");
            assert_eq!(
                diags.len(),
                1,
                "(d) wrong-dimension bounds must push exactly 1 Warning, got: {diags:?}"
            );
            assert_eq!(diags[0].severity, reify_core::Severity::Warning);
            let msg = diags[0].message.to_lowercase();
            assert!(
                msg.contains("range"),
                "(d) names expected Range, got: {:?}",
                diags[0].message
            );
        }

        // (e) missing-cell ValueRef → Undef → None, 0 diags (quiet).
        {
            let cell = reify_core::ValueCellId::new("Bracket", "missing_range");
            let expr =
                reify_ir::CompiledExpr::value_ref(cell, reify_core::Type::dimensionless_scalar());
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::LENGTH,
                "edges_by_length",
                "length_range",
                &mut diags,
            );
            assert_eq!(result, None, "(e) missing cell must return None");
            assert!(
                diags.is_empty(),
                "(e) missing cell must be quiet, got: {diags:?}"
            );
        }

        // (f) inline Range<AREA> (faces_by_area path) → Some((0.0, 1.0)), 0 diags.
        {
            let expr = literal_range(Some(0.0), Some(1.0), DimensionVector::AREA);
            let values = reify_ir::ValueMap::new();
            let mut diags: Vec<Diagnostic> = Vec::new();
            let result = super::resolve_range_dim_arg(
                &expr,
                &values,
                DimensionVector::AREA,
                "faces_by_area",
                "area_range",
                &mut diags,
            );
            assert_eq!(
                result,
                Some((0.0, 1.0)),
                "(f) inline closed Range<Area> must be Accepted"
            );
            assert!(
                diags.is_empty(),
                "(f) closed Range<Area> must produce no diags, got: {diags:?}"
            );
        }
    }

    // Constants `DEGENERATE_LENGTH_M`, `DEGENERATE_ANGLE_RAD`, and
    // `GEOMETRY_EPSILON` (top of file) are not pinned by a standalone unit
    // test — that would just restate the `const` definitions. Their behavior
    // is pinned by the boundary tests that drive the guards they feed:
    //   - `build_extrude_distance_{just_below,at}_threshold_*` (geometry_error_handling.rs)
    //     → DEGENERATE_LENGTH_M (inclusive floor)
    //   - `build_revolve_angle_{just_below,negative_just_below}_threshold_rejected`
    //     → DEGENERATE_ANGLE_RAD (sign-symmetric floor)
    //   - `extrude_symmetric_{per_side,negative_per_side}_{just_below,at}_threshold_*`
    //     (extrude_symmetric_e2e.rs) → 2 * DEGENERATE_LENGTH_M (per-side floor)
    // Any numeric change to the constants will fail those boundary tests.

    #[test]
    fn compile_geometry_op_scale_produces_scale_variant() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(2.0))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for Scale");

        match result {
            reify_ir::GeometryOp::Scale { target, factor } => {
                assert_eq!(target, GeometryHandleId(42));
                assert!((factor - 2.0).abs() < 1e-12);
            }
            other => panic!("expected GeometryOp::Scale, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_rotate_around_produces_rotate_around_variant() {
        let step_handles = vec![GeometryHandleId(99)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::RotateAround,
            target: GeomRef::Step(0),
            args: vec![
                ("px".into(), literal_f64(0.05)),
                ("py".into(), literal_f64(0.0)),
                ("pz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::FRAC_PI_2)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for RotateAround");

        match result {
            reify_ir::GeometryOp::RotateAround {
                target,
                point,
                axis,
                angle_rad,
            } => {
                assert_eq!(target, GeometryHandleId(99));
                assert!((point[0] - 0.05).abs() < 1e-12);
                assert!((point[1]).abs() < 1e-12);
                assert!((point[2]).abs() < 1e-12);
                assert!((axis[0]).abs() < 1e-12);
                assert!((axis[1]).abs() < 1e-12);
                assert!((axis[2] - 1.0).abs() < 1e-12);
                assert!((angle_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
            }
            other => panic!("expected GeometryOp::RotateAround, got {:?}", other),
        }
    }

    // --- CompiledGeometryOp::Isosurface build-arm lowering (task #4999, step-3 RED) ---

    /// Bare `isosurface(g)` — empty `args` — must default `iso_level` to
    /// `0.0` exactly and `adaptive` to `false`, with no diagnostics. Defaults
    /// are applied silently (absence is the normal, expected shape for this
    /// optional pair), NOT via `eval_named_arg`'s "missing required argument"
    /// Warning path — mirroring the `edges`/`faces`/`third` optional-arg
    /// convention (fillet/chamfer/draft/offset_curve) rather than a required-arg helper.
    ///
    /// RED: `CompiledGeometryOp::Isosurface` does not exist yet.
    #[test]
    fn compile_geometry_op_isosurface_bare_defaults_iso_zero_adaptive_false() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Isosurface {
            grid: GeomRef::Step(0),
            args: vec![],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        let result = result.expect("compile_geometry_op should return Ok for bare Isosurface");

        match result {
            reify_ir::GeometryOp::Surface {
                grid,
                iso_level,
                adaptive,
            } => {
                assert_eq!(grid, GeometryHandleId(42));
                assert_eq!(iso_level, 0.0, "absent iso must default to exactly 0.0");
                assert!(!adaptive, "absent adaptive must default to false");
            }
            other => panic!("expected GeometryOp::Surface, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "bare isosurface(g) must emit no diagnostics, got: {:?}",
            diagnostics
        );
    }

    /// Named `isosurface(g, iso: 5mm, adaptive: true)` must decode `iso`
    /// through the same Length→f64 SI-metres path as every other Length-typed
    /// geometry arg (5mm → 0.005, within 1e-12) and `adaptive` to `true`.
    ///
    /// RED: `CompiledGeometryOp::Isosurface` does not exist yet.
    #[test]
    fn compile_geometry_op_isosurface_named_args_decode_iso_metres_and_adaptive_true() {
        let step_handles = vec![GeometryHandleId(7)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Isosurface {
            grid: GeomRef::Step(0),
            args: vec![
                ("iso".to_string(), literal_length(0.005)),
                ("adaptive".to_string(), literal_bool(true)),
            ],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        let result = result.expect("compile_geometry_op should return Ok for named Isosurface");

        match result {
            reify_ir::GeometryOp::Surface {
                grid,
                iso_level,
                adaptive,
            } => {
                assert_eq!(grid, GeometryHandleId(7));
                assert!(
                    (iso_level - 0.005).abs() < 1e-12,
                    "iso: 5mm must decode to 0.005 metres, got {iso_level}"
                );
                assert!(adaptive, "adaptive: true must decode to true");
            }
            other => panic!("expected GeometryOp::Surface, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "named isosurface(g, iso, adaptive) must emit no diagnostics, got: {:?}",
            diagnostics
        );
    }

    /// Helper: build a CompiledExpr literal from a Value::Transform
    /// (quaternion [w,x,y,z] and SI-metre translation [tx,ty,tz]).
    fn literal_transform(q: [f64; 4], t: [f64; 3]) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(transform_of(q, t), reify_core::Type::transform(3))
    }

    #[test]
    fn compile_geometry_op_apply_transform_happy_path() {
        // orient_axis_angle(z, 90°) quaternion: [w, x, y, z] = [cos45°, 0, 0, sin45°]
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::ApplyTransform,
            target: GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_f64(0.0)), // placeholder; target resolved via GeomRef
                (
                    "transform".into(),
                    literal_transform([w, 0.0, 0.0, w], [0.005, 0.0, 0.0]),
                ),
            ],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let geo_op =
            result.expect("compile_geometry_op should return Ok for ApplyTransform happy path");
        match geo_op {
            reify_ir::GeometryOp::ApplyTransform {
                target,
                rotation,
                translation,
            } => {
                assert_eq!(target, GeometryHandleId(42));
                assert!((rotation[0] - w).abs() < 1e-12, "rotation[0] (w) mismatch");
                assert!(rotation[1].abs() < 1e-12, "rotation[1] (x) mismatch");
                assert!(rotation[2].abs() < 1e-12, "rotation[2] (y) mismatch");
                assert!((rotation[3] - w).abs() < 1e-12, "rotation[3] (z) mismatch");
                assert!(
                    (translation[0] - 0.005).abs() < 1e-12,
                    "translation[0] mismatch"
                );
                assert!(translation[1].abs() < 1e-12, "translation[1] mismatch");
                assert!(translation[2].abs() < 1e-12, "translation[2] mismatch");
            }
            other => panic!("expected GeometryOp::ApplyTransform, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_apply_transform_malformed_arg_produces_warning_and_err() {
        let step_handles = vec![GeometryHandleId(7)];
        let values = ValueMap::new();

        // Pass a Real(5.0) instead of a Transform value — should be rejected.
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::ApplyTransform,
            target: GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_f64(0.0)),
                ("transform".into(), literal_f64(5.0)),
            ],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(result.is_err(), "malformed transform arg must return Err");
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic, got {:?}",
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic must be Warning severity"
        );
        assert!(
            diag.message.contains("transform"),
            "diagnostic message must name the 'transform' arg; got: {:?}",
            diag.message
        );
    }

    /// A non-unit quaternion (e.g. [2, 0, 0, 0]) must pass through the eval arm
    /// as `Ok(GeometryOp::ApplyTransform{…})` — the eval seam does NOT validate
    /// unit-length (see the layering note in the production arm above).
    ///
    /// The kernel (`build_trsf` in the OCCT layer) is responsible for rejecting
    /// non-unit quaternions; it surfaces the rejection as a kernel-level
    /// `OperationFailed` error, which the eval layer converts to a build diagnostic
    /// (not a panic).  This test confirms the eval-to-kernel handoff contract:
    /// eval always passes a structurally-valid Transform through, regardless of
    /// whether its rotation has unit norm.
    #[test]
    fn compile_geometry_op_apply_transform_non_unit_quaternion_passthrough() {
        // [2, 0, 0, 0] is structurally valid (Orientation variant) but not unit-norm.
        let step_handles = vec![GeometryHandleId(99)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::ApplyTransform,
            target: GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_f64(0.0)),
                (
                    "transform".into(),
                    literal_transform([2.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
                ),
            ],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // The eval arm must NOT panic and must return Ok — unit-norm is the kernel's job.
        let geo_op = result.expect("non-unit quaternion must pass through eval arm as Ok");
        match geo_op {
            reify_ir::GeometryOp::ApplyTransform {
                target,
                rotation,
                translation,
            } => {
                assert_eq!(target, GeometryHandleId(99));
                // Rotation passed through as-is (eval does not normalize).
                assert!((rotation[0] - 2.0).abs() < 1e-12, "rotation[0] must be 2.0");
                assert!(rotation[1].abs() < 1e-12);
                assert!(rotation[2].abs() < 1e-12);
                assert!(rotation[3].abs() < 1e-12);
                assert!(translation.iter().all(|&v| v.abs() < 1e-12));
            }
            other => panic!("expected GeometryOp::ApplyTransform, got {:?}", other),
        }
        // No diagnostic emitted by the eval arm for a structurally-valid Transform.
        assert!(
            diagnostics.is_empty(),
            "eval arm must not emit diagnostics for a structurally-valid (non-unit) Transform; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_sweep_resolves_distinct_profiles() {
        // Two distinct step handles representing two wire profiles
        let step_handles = vec![GeometryHandleId(100), GeometryHandleId(200)];
        let values = ValueMap::new();

        // Create a Loft sweep that references Step(0) and Step(1)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Loft,
            profiles: vec![GeomRef::Step(0), GeomRef::Step(1)],
            args: vec![],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for Loft");

        match result {
            reify_ir::GeometryOp::Loft { profiles } => {
                assert_eq!(
                    profiles,
                    vec![GeometryHandleId(100), GeometryHandleId(200)],
                    "Loft profiles should resolve Step(0) -> handle 100, Step(1) -> handle 200"
                );
            }
            other => panic!("expected GeometryOp::Loft, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_extrude_preserves_value_type() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_length(0.05))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for Extrude");

        match result {
            reify_ir::GeometryOp::Extrude { profile, distance } => {
                assert_eq!(profile, GeometryHandleId(10));
                // The distance must preserve Scalar type (not be converted to Value::Real)
                match distance {
                    reify_ir::Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!((si_value - 0.05).abs() < 1e-12, "SI value should be 0.05m");
                        assert_eq!(
                            dimension,
                            reify_core::DimensionVector::LENGTH,
                            "dimension should be LENGTH"
                        );
                    }
                    other => panic!(
                        "expected Value::Scalar, got {:?} — Extrude distance must preserve SI unit info",
                        other
                    ),
                }
            }
            other => panic!("expected GeometryOp::Extrude, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_revolve_missing_each_required_arg_returns_none() {
        // Table-driven coverage for all 7 required Revolve args. Revolve reads
        // ax, ay, az, angle, ox, oy, oz via f64_arg?; omitting any of them must
        // yield None (not silently treat as 0.0).
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Each iteration omits exactly one named arg; all other required args
        // remain present so that f64_arg? short-circuits on (and diagnoses)
        // only the omitted arg under test.
        let full_args: Vec<(&'static str, reify_ir::CompiledExpr)> = vec![
            ("ox", literal_f64(0.0)),
            ("oy", literal_f64(0.0)),
            ("oz", literal_f64(0.0)),
            ("ax", literal_f64(0.0)),
            ("ay", literal_f64(0.0)),
            ("az", literal_f64(1.0)),
            ("angle", literal_f64(std::f64::consts::PI)),
        ];

        for omit in ["ox", "oy", "oz", "ax", "ay", "az", "angle"] {
            let args: Vec<(String, reify_ir::CompiledExpr)> = full_args
                .iter()
                .filter(|(name, _)| *name != omit)
                .map(|(name, expr)| ((*name).into(), expr.clone()))
                .collect();

            let op = CompiledGeometryOp::Sweep {
                kind: SweepKind::Revolve,
                profiles: vec![GeomRef::Step(0)],
                args,
            };

            // Pin the observable contract: each missing required arg
            // must (a) return None and (b) emit exactly one warning
            // diagnostic naming the quoted arg (e.g. `'ox'`) and the
            // 'Revolve' op. Covers all seven required args including `ox`.
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            let result = compile_geometry_op(
                &op,
                &values,
                &step_handles,
                &[],
                &HashMap::new(),
                &HashMap::new(),
                &mut diagnostics,
            );
            assert!(
                result.is_err(),
                "missing '{omit}' should return None, got {:?}",
                result
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "missing '{omit}' should emit exactly one diagnostic, got: {:?}",
                diagnostics
            );
            assert_eq!(
                diagnostics[0].severity,
                reify_core::Severity::Warning,
                "missing '{omit}' should emit a Warning severity"
            );
            assert!(
                diagnostics[0].message.contains(&format!("'{omit}'")),
                "diagnostic for missing '{omit}' should mention \"'{omit}'\", got: {}",
                diagnostics[0].message
            );
            assert!(
                diagnostics[0].message.contains("revolve"),
                "diagnostic for missing '{omit}' should mention 'revolve', got: {}",
                diagnostics[0].message
            );
        }
    }

    #[test]
    fn compile_geometry_op_extrude_missing_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result.is_err(),
            "expected None for missing 'distance' arg, got {:?}",
            result
        );
    }

    #[test]
    fn compile_geometry_op_extrude_nan_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Extrude with NaN distance — should return None (runtime edge case, not invariant)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_f64(f64::NAN))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(result.is_err(), "NaN extrude distance should return None");
    }

    #[test]
    fn compile_geometry_op_extrude_inf_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Extrude with Inf distance — should return None (runtime edge case, not invariant)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_f64(f64::INFINITY))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(result.is_err(), "Inf extrude distance should return None");
    }

    #[test]
    fn compile_geometry_op_extrude_near_zero_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Extrude with a near-zero (1e-15 m) distance — should return None (degenerate geometry)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_length(1e-15))],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "near-zero extrude distance should return None"
        );
        // A warning diagnostic must be emitted so model authors see why the
        // op was dropped rather than only the caller's generic error.
        assert!(
            diagnostics
                .iter()
                .any(|d| matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("extrude dropped")
                    && d.message.contains("degenerate")),
            "expected degenerate-extrude warning, got {:?}",
            diagnostics,
        );
    }

    #[test]
    fn compile_geometry_op_revolve_zero_axis_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // All 7 args present and numeric, but ax=ay=az=0.0 (zero-length rotation axis)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(0.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
            ],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "zero-length rotation axis should return None"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("revolve dropped")
                    && d.message.contains("axis")),
            "expected degenerate-revolve-axis warning, got {:?}",
            diagnostics,
        );
    }

    #[test]
    fn compile_geometry_op_revolve_nan_axis_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // All 7 args present and numeric, but ax=NaN (non-finite rotation axis)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(f64::NAN)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(0.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_err(), "NaN rotation axis should return None");
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("non-numeric/non-finite")
                    && d.message.contains("ax")
                    && d.message.contains("revolve")
            }),
            "expected a Warning mentioning 'non-numeric/non-finite', 'ax', and 'revolve', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_revolve_near_zero_angle_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Revolve with a near-zero (1e-15 rad) angle — should return None (degenerate geometry)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(1e-15)),
            ],
        };

        let mut diagnostics = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "near-zero revolve angle should return None"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("revolve dropped")
                    && d.message.contains("angle")),
            "expected degenerate-revolve-angle warning, got {:?}",
            diagnostics,
        );
    }

    #[test]
    fn compile_geometry_op_revolve_produces_revolve_variant() {
        let step_handles = vec![GeometryHandleId(55)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::TAU)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result =
            result.expect("compile_geometry_op should return Ok for Revolve with valid axis");

        match result {
            reify_ir::GeometryOp::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle_rad,
            } => {
                assert_eq!(profile, GeometryHandleId(55));
                assert!((axis_origin[0]).abs() < 1e-12);
                assert!((axis_origin[1]).abs() < 1e-12);
                assert!((axis_origin[2]).abs() < 1e-12);
                assert!((axis_dir[0]).abs() < 1e-12);
                assert!((axis_dir[1]).abs() < 1e-12);
                assert!((axis_dir[2] - 1.0).abs() < 1e-12);
                assert!((angle_rad - std::f64::consts::TAU).abs() < 1e-12);
            }
            other => panic!("expected GeometryOp::Revolve, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_extrude_produces_extrude_variant() {
        let step_handles = vec![GeometryHandleId(77)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_length(0.03))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for Extrude");

        match result {
            reify_ir::GeometryOp::Extrude { profile, distance } => {
                assert_eq!(profile, GeometryHandleId(77));
                match distance {
                    reify_ir::Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!(
                            (si_value - 0.03).abs() < 1e-12,
                            "SI value should be 0.03m (30mm)"
                        );
                        assert_eq!(dimension, reify_core::DimensionVector::LENGTH);
                    }
                    other => panic!("expected Value::Scalar for distance, got {:?}", other),
                }
            }
            other => panic!("expected GeometryOp::Extrude, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_scale_negative_factor_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(-1.0))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "negative scale factor should return None (inside-out geometry)"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for negative scale factor, got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("scale dropped")
                    && d.message.contains("negative")
            }),
            "expected a Warning mentioning 'scale dropped' and 'negative', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_scale_zero_factor_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(0.0))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "zero scale factor should return None (degenerate geometry)"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for zero scale factor, got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("scale dropped")
                    && d.message.contains("degenerate")
            }),
            "expected a Warning mentioning 'scale dropped' and 'degenerate', got: {:?}",
            diagnostics
        );
    }

    // ── transform_affine_apply tests (task 3963 step-7) ─────────────────────

    /// Helper: build a `CompiledExpr` literal wrapping a `Value::AffineMap`.
    fn literal_affine_map(linear: [[f64; 3]; 3], translation: [f64; 3]) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::AffineMap { linear, translation },
            reify_core::Type::affine_map(3),
        )
    }

    #[test]
    fn transform_affine_apply_lowers_affine_map_arg_verbatim() {
        let linear = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]];
        let translation = [0.0, 0.0, 0.0];
        let args: Vec<(String, reify_ir::CompiledExpr)> =
            vec![("map".to_string(), literal_affine_map(linear, translation))];
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target_id = GeometryHandleId(7);

        let result = transform_affine_apply(
            &TransformKind::AffineApply,
            target_id,
            &args,
            &values,
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        match result {
            Ok(reify_ir::GeometryOp::AffineApply {
                target,
                linear: got_linear,
                translation: got_translation,
            }) => {
                assert_eq!(target, target_id);
                assert_eq!(got_linear, linear, "linear part must be carried verbatim");
                assert_eq!(got_translation, translation, "translation must be carried verbatim");
            }
            other => panic!("expected Ok(GeometryOp::AffineApply), got {:?}", other),
        }
    }

    #[test]
    fn transform_affine_apply_singular_map_is_dropped_with_diagnostic() {
        // det(linear) == 0: the third row is all zero.
        let linear = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]];
        let args: Vec<(String, reify_ir::CompiledExpr)> =
            vec![("map".to_string(), literal_affine_map(linear, [0.0, 0.0, 0.0]))];
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let result = transform_affine_apply(
            &TransformKind::AffineApply,
            GeometryHandleId(7),
            &args,
            &values,
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(result.is_err(), "singular linear part must be dropped (Err)");
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("affine_apply dropped: linear part is singular (det=0)")
            }),
            "expected a Warning containing 'affine_apply dropped: linear part is singular (det=0)', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn transform_affine_apply_negative_determinant_is_not_dropped() {
        // Reflection: det = -1 * 1 * 2 = -2 < 0, but non-zero — must pass through.
        let linear = [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]];
        let args: Vec<(String, reify_ir::CompiledExpr)> =
            vec![("map".to_string(), literal_affine_map(linear, [0.0, 0.0, 0.0]))];
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let result = transform_affine_apply(
            &TransformKind::AffineApply,
            GeometryHandleId(7),
            &args,
            &values,
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "negative-determinant (reflection) map must NOT be dropped, got {:?}",
            result
        );
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
    }

    #[test]
    fn transform_affine_apply_near_singular_map_is_dropped_with_diagnostic() {
        // det(linear) = 1e-14: nonzero but numerically degenerate — an exact
        // `== 0.0` comparison would let this slip through to the kernel
        // (amend: reviewer_comprehensive robustness finding). The
        // epsilon-based guard must still drop it, same diagnostic as the
        // exact-zero case.
        let linear = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1e-14]];
        let args: Vec<(String, reify_ir::CompiledExpr)> =
            vec![("map".to_string(), literal_affine_map(linear, [0.0, 0.0, 0.0]))];
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let result = transform_affine_apply(
            &TransformKind::AffineApply,
            GeometryHandleId(7),
            &args,
            &values,
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "near-singular (det=1e-14) linear part must be dropped (Err)"
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("affine_apply dropped: linear part is singular (det=0)")
            }),
            "expected a Warning containing 'affine_apply dropped: linear part is singular (det=0)', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn affine_apply_linear_det_matches_stdlib_determinant_builtin() {
        // `affine_apply_linear_det` hand-copies `reify_stdlib::matrix::mat3_det`
        // (pub(crate) there, so not directly reachable from this crate — see
        // the doc comment above `affine_apply_linear_det`). Cross-check the
        // two formulas agree via the public `determinant` builtin's
        // `AffineMap` arm, which calls `mat3_det` internally, so a future
        // divergence between the two hand-written expansions (e.g. a sign
        // fix applied to one but not the other) surfaces here instead of
        // silently desyncing the singular-guard semantics from the
        // `determinant` builtin's notion of singular (amend:
        // reviewer_comprehensive code_duplication finding).
        let samples: [[[f64; 3]; 3]; 5] = [
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]],
            [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]],
            [[2.0, 1.0, 0.0], [1.0, 3.0, 1.0], [0.0, 1.0, 4.0]],
        ];
        for m in samples {
            let local = affine_apply_linear_det(m);
            let stdlib = reify_stdlib::eval_builtin(
                "determinant",
                &[reify_ir::Value::AffineMap {
                    linear: m,
                    translation: [0.0, 0.0, 0.0],
                }],
            );
            match stdlib {
                reify_ir::Value::Real(v) => {
                    assert!(
                        (local - v).abs() < 1e-9,
                        "affine_apply_linear_det({:?}) = {} disagrees with stdlib determinant = {}",
                        m,
                        local,
                        v
                    );
                }
                other => panic!(
                    "expected Value::Real from the determinant builtin, got {:?}",
                    other
                ),
            }
        }
    }

    // ── transform_scale_non_uniform tests (task 4167 step-5) ─────────────────

    /// Helper: build a `CompiledExpr` literal wrapping a `Value::Vector` of
    /// three dimensionless Real components (mirrors `literal_affine_map`).
    fn literal_vec3(x: f64, y: f64, z: f64) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            vec3_value(x, y, z),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        )
    }

    #[test]
    fn compile_geometry_op_scale_non_uniform_produces_variant() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::ScaleNonUniform,
            target: GeomRef::Step(0),
            args: vec![("factors".into(), literal_vec3(2.0, 1.0, 0.5))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result = result.expect("compile_geometry_op should return Ok for ScaleNonUniform");

        match result {
            reify_ir::GeometryOp::ScaleNonUniform { target, sx, sy, sz } => {
                assert_eq!(target, GeometryHandleId(42));
                assert!((sx - 2.0).abs() < 1e-12);
                assert!((sy - 1.0).abs() < 1e-12);
                assert!((sz - 0.5).abs() < 1e-12);
            }
            other => panic!("expected GeometryOp::ScaleNonUniform, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_scale_non_uniform_zero_component_drops() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::ScaleNonUniform,
            target: GeomRef::Step(0),
            args: vec![("factors".into(), literal_vec3(0.0, 1.0, 1.0))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(result.is_err(), "zero factors component must be dropped (Err)");
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("scale dropped")
            }),
            "expected a Warning containing 'scale dropped', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_translate_missing_arg_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // Translate with only dx — missing dy, dz
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![("dx".into(), literal_f64(1.0))],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result.is_err(),
            "missing dy/dz should return None, not silently default to 0.0"
        );
    }

    #[test]
    fn compile_geometry_op_scale_nan_factor_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(f64::NAN))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_err(), "NaN scale factor should return None");
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for NaN scale factor, got: {:?}",
            diagnostics
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("non-numeric/non-finite")
                    && d.message.contains("factor")
                    && d.message.contains("scale")
            }),
            "expected a Warning mentioning 'non-numeric/non-finite', 'factor', and 'scale', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_rotate_around_missing_axis_returns_none() {
        let step_handles = vec![GeometryHandleId(99)];
        let values = ValueMap::new();

        // RotateAround with missing az
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::RotateAround,
            target: GeomRef::Step(0),
            args: vec![
                ("px".into(), literal_f64(0.0)),
                ("py".into(), literal_f64(0.0)),
                ("pz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(1.0)),
                // az deliberately omitted
                ("angle".into(), literal_f64(1.0)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(result.is_err(), "missing az should return Err");
    }

    #[test]
    fn compile_geometry_op_linear_pattern_missing_spacing_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // LinearPattern with dx/dy/dz/count but OMITS spacing
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                // spacing deliberately omitted
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result.is_err(),
            "missing spacing should return None, not silently default to Value::Undef"
        );
    }

    #[test]
    fn compile_geometry_op_circular_pattern_missing_angle_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // CircularPattern with ox/oy/oz/ax/ay/az/count but OMITS angle
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(4.0)),
                // angle deliberately omitted
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result.is_err(),
            "missing angle should return None, not silently default to Value::Undef"
        );
    }

    #[test]
    fn compile_geometry_op_linear_pattern_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                ("spacing".into(), literal_length(0.02)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::LinearPattern {
                target,
                direction,
                count,
                spacing,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(direction, [10.0, 0.0, 0.0]);
                assert_eq!(count, 3);
                // spacing should be a Scalar value, not Undef
                assert!(
                    !matches!(spacing, reify_ir::Value::Undef),
                    "spacing should not be Undef when arg is present"
                );
            }
            other => panic!("expected Some(LinearPattern), got {:?}", other),
        }
    }

    /// A BARE (dimensionless) `spacing` on a 1D `linear_pattern` must be
    /// REJECTED — same silent-SI-metres hazard as the 2D case. The op is
    /// dropped (Err) with a diagnostic naming the arg and the Length
    /// requirement.
    #[test]
    fn compile_geometry_op_linear_pattern_bare_spacing_rejected() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                // BARE dimensionless spacing — must be rejected.
                ("spacing".into(), literal_f64(0.02)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "bare (dimensionless) spacing must drop the op, got: {:?}",
            result
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("spacing") && d.message.contains("Length")),
            "a diagnostic must name the spacing arg and the Length units \
             requirement; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_circular_pattern_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(4.0)),
                // Use an explicitly-dimensioned angle literal to test the pass-through path.
                // A bare f64 would now trigger the degrees→radians conversion path instead.
                ("angle".into(), literal_angle(std::f64::consts::FRAC_PI_2)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        match result {
            Ok(reify_ir::GeometryOp::CircularPattern {
                target,
                axis_origin,
                axis_dir,
                count,
                angle,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(axis_origin, [0.0, 0.0, 0.0]);
                assert_eq!(axis_dir, [0.0, 0.0, 1.0]);
                assert_eq!(count, 4);
                // angle should be a Scalar value (with ANGLE dimension), not Undef
                assert!(
                    !matches!(angle, reify_ir::Value::Undef),
                    "angle should not be Undef when arg is present"
                );
                // The explicit-unit path must NOT emit a degree-conversion warning
                let has_deg_warning = diagnostics.iter().any(|d| {
                    d.severity == reify_core::Severity::Warning
                        && (d.message.contains("deg") || d.message.contains("degree"))
                });
                assert!(
                    !has_deg_warning,
                    "explicit angle unit should not trigger a degree-conversion warning, got: {:?}",
                    diagnostics
                );
            }
            other => panic!("expected Some(CircularPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_circular_pattern_bare_f64_converts_to_radians() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = reify_compiler::CompiledGeometryOp::Pattern {
            kind: reify_compiler::PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(6.0)),
                // Bare f64 without unit — should be interpreted as degrees and
                // converted to radians: 360° → 2π rad.
                ("angle".into(), literal_f64(360.0)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::CircularPattern { angle, .. }) => {
                let angle_f64 = angle.as_f64().expect("angle should be numeric");
                assert!(
                    (angle_f64 - std::f64::consts::TAU).abs() < 1e-9,
                    "360.0 (bare f64) should convert to 2π radians, got {}",
                    angle_f64
                );
            }
            other => panic!("expected Some(CircularPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_circular_pattern_bare_int_converts_to_radians() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // Bare integer 360 — should be interpreted as 360° and converted to 2π rad.
        let angle_int_expr =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Int(360), reify_core::Type::Int);

        let op = reify_compiler::CompiledGeometryOp::Pattern {
            kind: reify_compiler::PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(6.0)),
                ("angle".into(), angle_int_expr),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::CircularPattern { angle, .. }) => {
                let angle_f64 = angle.as_f64().expect("angle should be numeric");
                assert!(
                    (angle_f64 - std::f64::consts::TAU).abs() < 1e-9,
                    "Int(360) should convert to 2π radians, got {}",
                    angle_f64
                );
            }
            other => panic!("expected Some(CircularPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_circular_pattern_bare_number_emits_deprecation_warning() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let op = reify_compiler::CompiledGeometryOp::Pattern {
            kind: reify_compiler::PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(6.0)),
                ("angle".into(), literal_f64(360.0)),
            ],
        };

        let _result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let has_degree_warning = diagnostics.iter().any(|d| {
            d.severity == reify_core::Severity::Warning
                && (d.message.contains("deg") || d.message.contains("degree"))
        });
        assert!(
            has_degree_warning,
            "expected a Warning diagnostic about implicit degree conversion, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_circular_pattern_angle_scalar_passes_through() {
        // An explicitly-dimensioned angle (Value::Scalar with ANGLE dimension) must
        // pass through the CircularPattern arm unchanged — no double-conversion,
        // no degree-conversion warning.
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let op = reify_compiler::CompiledGeometryOp::Pattern {
            kind: reify_compiler::PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(6.0)),
                // Explicit angle unit: PI radians
                ("angle".into(), literal_angle(std::f64::consts::PI)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        match result {
            Ok(reify_ir::GeometryOp::CircularPattern { angle, .. }) => {
                let angle_f64 = angle.as_f64().expect("angle should be numeric");
                assert!(
                    (angle_f64 - std::f64::consts::PI).abs() < 1e-12,
                    "explicit PI rad angle should pass through as PI, got {}",
                    angle_f64
                );
                // No degree-conversion warning should be emitted for explicit units
                let has_deg_warning = diagnostics.iter().any(|d| {
                    d.severity == reify_core::Severity::Warning
                        && (d.message.contains("deg") || d.message.contains("degree"))
                });
                assert!(
                    !has_deg_warning,
                    "explicit angle unit should not trigger a degree-conversion warning, got: {:?}",
                    diagnostics
                );
            }
            other => panic!("expected Some(CircularPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_mirror_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Mirror,
            target: GeomRef::Step(0),
            args: vec![
                // Plane ORIGIN is length-semantic → must be dimensioned Length.
                ("ox".into(), literal_length(0.0)),
                ("oy".into(), literal_length(0.0)),
                ("oz".into(), literal_length(0.0)),
                // Plane NORMAL is a dimensionless unit vector → stays bare f64.
                ("nx".into(), literal_f64(1.0)),
                ("ny".into(), literal_f64(0.0)),
                ("nz".into(), literal_f64(0.0)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::Mirror {
                target,
                plane_origin,
                plane_normal,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(plane_origin, [0.0, 0.0, 0.0]);
                assert_eq!(plane_normal, [1.0, 0.0, 0.0]);
            }
            other => panic!("expected Some(Mirror), got {:?}", other),
        }
    }

    /// A BARE (dimensionless) mirror-plane origin component `ox` must be
    /// REJECTED — a plane ORIGIN is length-semantic, so a bare `0.0` would be
    /// silently read as 0 SI metres (and any non-zero bare value 1000× off).
    /// The op is dropped (Err) with a diagnostic naming `ox` and Length.
    #[test]
    fn compile_geometry_op_mirror_bare_origin_rejected() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Mirror,
            target: GeomRef::Step(0),
            args: vec![
                // BARE dimensionless ox — must be rejected.
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_length(0.0)),
                ("oz".into(), literal_length(0.0)),
                ("nx".into(), literal_f64(1.0)),
                ("ny".into(), literal_f64(0.0)),
                ("nz".into(), literal_f64(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "bare (dimensionless) mirror origin ox must drop the op, got: {:?}",
            result
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("ox") && d.message.contains("Length")),
            "a diagnostic must name the ox arg and the Length units requirement; \
             got: {:?}",
            diagnostics
        );
    }

    /// Locks the split: the mirror-plane ORIGIN must be a Length, but the plane
    /// NORMAL is a dimensionless unit vector — so a `Length` origin with BARE
    /// (dimensionless) normal components is accepted, no error.
    #[test]
    fn compile_geometry_op_mirror_length_origin_bare_normal_accepted() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Mirror,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_length(0.0)),
                ("oy".into(), literal_length(0.0)),
                ("oz".into(), literal_length(0.0)),
                // Normal stays a bare dimensionless unit vector.
                ("nx".into(), literal_f64(1.0)),
                ("ny".into(), literal_f64(0.0)),
                ("nz".into(), literal_f64(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        match result {
            Ok(reify_ir::GeometryOp::Mirror {
                target,
                plane_origin,
                plane_normal,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(plane_origin, [0.0, 0.0, 0.0]);
                assert_eq!(plane_normal, [1.0, 0.0, 0.0]);
            }
            other => panic!(
                "expected Ok(Mirror) for Length origin + bare normal, got {:?}",
                other
            ),
        }
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == reify_core::Severity::Error),
            "Length origin + dimensionless normal must not produce an Error \
             diagnostic; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_linear_pattern_2d_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear2D,
            target: GeomRef::Step(0),
            args: vec![
                ("dx1".into(), literal_f64(1.0)),
                ("dy1".into(), literal_f64(0.0)),
                ("dz1".into(), literal_f64(0.0)),
                ("count1".into(), literal_f64(3.0)),
                ("spacing1".into(), literal_length(0.02)),
                ("dx2".into(), literal_f64(0.0)),
                ("dy2".into(), literal_f64(1.0)),
                ("dz2".into(), literal_f64(0.0)),
                ("count2".into(), literal_f64(4.0)),
                ("spacing2".into(), literal_length(0.03)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::LinearPattern2D {
                target,
                direction1,
                count1,
                spacing1,
                direction2,
                count2,
                spacing2,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(direction1, [1.0, 0.0, 0.0]);
                assert_eq!(count1, 3);
                assert!(
                    !matches!(spacing1, reify_ir::Value::Undef),
                    "spacing1 should not be Undef"
                );
                assert_eq!(direction2, [0.0, 1.0, 0.0]);
                assert_eq!(count2, 4);
                assert!(
                    !matches!(spacing2, reify_ir::Value::Undef),
                    "spacing2 should not be Undef"
                );
            }
            other => panic!("expected Some(LinearPattern2D), got {:?}", other),
        }
    }

    /// A BARE (dimensionless) `spacing1` must be REJECTED — reading it via
    /// `Value::as_f64` would silently treat `0.02` as 0.02 SI **metres**, the
    /// exact silent-SI-metres hazard this task closes. The op is dropped
    /// (Err) with a diagnostic naming the arg and the Length requirement.
    #[test]
    fn compile_geometry_op_linear_pattern_2d_bare_spacing_rejected() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear2D,
            target: GeomRef::Step(0),
            args: vec![
                ("dx1".into(), literal_f64(1.0)),
                ("dy1".into(), literal_f64(0.0)),
                ("dz1".into(), literal_f64(0.0)),
                ("count1".into(), literal_f64(3.0)),
                // BARE dimensionless spacing1 — must be rejected.
                ("spacing1".into(), literal_f64(0.02)),
                ("dx2".into(), literal_f64(0.0)),
                ("dy2".into(), literal_f64(1.0)),
                ("dz2".into(), literal_f64(0.0)),
                ("count2".into(), literal_f64(4.0)),
                ("spacing2".into(), literal_length(0.03)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_err(),
            "bare (dimensionless) spacing1 must drop the op, got: {:?}",
            result
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("spacing1") && d.message.contains("Length")),
            "a diagnostic must name the spacing1 arg and the Length units \
             requirement; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_arbitrary_pattern_valid_3_transforms() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Arbitrary,
            target: GeomRef::Step(0),
            args: vec![
                ("t0_dx".into(), literal_f64(0.01)),
                ("t0_dy".into(), literal_f64(0.0)),
                ("t0_dz".into(), literal_f64(0.0)),
                ("t1_dx".into(), literal_f64(0.0)),
                ("t1_dy".into(), literal_f64(0.02)),
                ("t1_dz".into(), literal_f64(0.0)),
                ("t2_dx".into(), literal_f64(0.01)),
                ("t2_dy".into(), literal_f64(0.02)),
                ("t2_dz".into(), literal_f64(0.0)),
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        match result {
            Ok(reify_ir::GeometryOp::ArbitraryPattern { target, transforms }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(transforms.len(), 3);
                // Scalar-triple form: identity rotation quat per instance.
                assert_eq!(transforms[0], ([1.0, 0.0, 0.0, 0.0], [0.01, 0.0, 0.0]));
                assert_eq!(transforms[1], ([1.0, 0.0, 0.0, 0.0], [0.0, 0.02, 0.0]));
                assert_eq!(transforms[2], ([1.0, 0.0, 0.0, 0.0], [0.01, 0.02, 0.0]));
            }
            other => panic!("expected Some(ArbitraryPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_linear_pattern_2d_missing_spacing2_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear2D,
            target: GeomRef::Step(0),
            args: vec![
                ("dx1".into(), literal_f64(1.0)),
                ("dy1".into(), literal_f64(0.0)),
                ("dz1".into(), literal_f64(0.0)),
                ("count1".into(), literal_f64(3.0)),
                ("spacing1".into(), literal_length(0.02)),
                ("dx2".into(), literal_f64(0.0)),
                ("dy2".into(), literal_f64(1.0)),
                ("dz2".into(), literal_f64(0.0)),
                ("count2".into(), literal_f64(4.0)),
                // spacing2 deliberately omitted
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(result.is_err(), "missing spacing2 should return None");
    }

    #[test]
    fn compile_geometry_op_arbitrary_pattern_missing_transform_coord_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // Only 2 coords for what should be a complete triple
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Arbitrary,
            target: GeomRef::Step(0),
            args: vec![
                ("t0_dx".into(), literal_f64(0.01)),
                ("t0_dy".into(), literal_f64(0.0)),
                // t0_dz deliberately omitted
            ],
        };

        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result.is_err(),
            "missing transform coord should return None"
        );
    }

    // ── compile_geometry_op diagnostic tests ─────────────────────────────────

    #[test]
    fn compile_geometry_op_primitive_missing_arg_returns_none() {
        let step_handles: Vec<GeometryHandleId> = vec![];
        let values = ValueMap::new();

        // Box with height and depth present, but 'width' deliberately omitted
        let op = CompiledGeometryOp::Primitive {
            kind: reify_compiler::PrimitiveKind::Box,
            args: vec![
                ("height".into(), literal_length(0.05)),
                ("depth".into(), literal_length(0.04)),
                // width deliberately omitted
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // When a required arg is missing, compile_geometry_op should short-circuit and return None
        assert!(
            result.is_err(),
            "compile_geometry_op should return None when a required arg is missing"
        );

        // Exactly one diagnostic warning should have been emitted for the missing 'width'
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for missing 'width', got: {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "expected Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("width"),
            "diagnostic message should mention 'width', got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("box"),
            "diagnostic message should mention 'box', got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn compile_geometry_op_modify_missing_arg_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Fillet with target but 'radius' deliberately omitted
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                // radius deliberately omitted
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // When a required arg is missing, compile_geometry_op should short-circuit and return None
        assert!(
            result.is_err(),
            "compile_geometry_op should return None when a required arg is missing"
        );

        // Exactly one diagnostic warning should have been emitted for the missing 'radius'
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for missing 'radius', got: {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "expected Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("radius"),
            "diagnostic message should mention 'radius', got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("fillet"),
            "diagnostic message should mention 'fillet', got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn compile_geometry_op_present_args_emit_no_diagnostics() {
        let step_handles = vec![GeometryHandleId(1)];
        let values = ValueMap::new();

        // Primitive::Box with all required args present
        let box_op = CompiledGeometryOp::Primitive {
            kind: reify_compiler::PrimitiveKind::Box,
            args: vec![
                ("width".into(), literal_length(0.10)),
                ("height".into(), literal_length(0.05)),
                ("depth".into(), literal_length(0.04)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &box_op,
            &values,
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_ok(), "Box with all args should return Some");
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected when all Primitive args are present, got: {:?}",
            diagnostics
        );

        // Modify::Fillet with target and radius present
        let fillet_op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![("radius".into(), literal_length(0.005))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &fillet_op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_ok(), "Fillet with all args should return Some");
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected when all Modify args are present, got: {:?}",
            diagnostics
        );
    }

    // ── ι: offset_curve build-arm 3rd-arg dispatch (step-13/14, task 4193) ────────

    /// (a) 2-arg `offset_curve(curve, distance)` — no 3rd arg — builds
    /// `GeometryOp::OffsetCurve { reference: None, direction: None }` (the planar
    /// overload). Passes against the step-12 stub; pinned here so the full
    /// step-14 dispatch keeps the 2-arg path intact.
    #[test]
    fn compile_geometry_op_offset_curve_2arg_no_reference_no_direction() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::OffsetCurve,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![("distance".into(), literal_length(0.002))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        match result {
            Ok(reify_ir::GeometryOp::OffsetCurve {
                target,
                reference,
                direction,
                ..
            }) => {
                assert_eq!(target, GeometryHandleId(10), "target resolves to step 0");
                assert_eq!(reference, None, "2-arg form has no reference");
                assert_eq!(direction, None, "2-arg form has no direction");
            }
            other => panic!("expected Ok(OffsetCurve), got {:?}", other),
        }
    }

    /// (b) 3-arg `offset_curve(curve, distance, vec3(0,0,1))` — the 3rd arg is a
    /// `Value::Vector` → `direction: Some([0,0,1])`, `reference: None` (overload 3).
    ///
    /// RED until step-14: the step-12 stub ignores the 3rd arg and always yields
    /// `direction: None`.
    #[test]
    fn compile_geometry_op_offset_curve_3arg_vector_is_direction() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // A literal vec3(0,0,1). resolve_parent_geometry_handle_arg returns None
        // for a Literal (not a ValueRef), so the dispatch falls through to the
        // direction-decode path; point3_components reads the 3 components.
        let dir_expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::OffsetCurve,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("distance".into(), literal_length(0.002)),
                ("third".into(), dir_expr),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        match result {
            Ok(reify_ir::GeometryOp::OffsetCurve {
                reference,
                direction,
                ..
            }) => {
                assert_eq!(reference, None, "a vec3 3rd arg is NOT a reference");
                assert_eq!(
                    direction,
                    Some([0.0, 0.0, 1.0]),
                    "a vec3 3rd arg becomes the direction"
                );
            }
            other => panic!("expected Ok(OffsetCurve) with direction, got {:?}", other),
        }
    }

    /// (c) 3-arg `offset_curve(curve, distance, <faces() sub-handle>)` — the 3rd
    /// arg is a bound `Value::GeometryHandle` → `reference: Some(kernel_handle)`,
    /// `direction: None` (overload 2). The handle is resolved via
    /// `resolve_parent_geometry_handle_arg` exactly like split's solid arg.
    ///
    /// RED until step-14: the step-12 stub ignores the 3rd arg and always yields
    /// `reference: None`.
    #[test]
    fn compile_geometry_op_offset_curve_3arg_handle_is_reference() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

        let step_handles = vec![GeometryHandleId(10)];

        // Bind a Value::GeometryHandle (a faces() sub-handle shape) into the
        // values map, referenced by a ValueRef 3rd arg.
        let ref_handle = GeometryHandleId(42);
        let ref_cell = ValueCellId::new("E", "surf");
        let mut values = ValueMap::new();
        values.insert(
            ref_cell.clone(),
            reify_ir::Value::GeometryHandle {
                realization_ref: RealizationNodeId::new("E", 0),
                upstream_values_hash: [0x11; 32],
                kernel_handle: Some(ref_handle),
            },
        );
        let ref_expr = reify_ir::CompiledExpr::value_ref(ref_cell, Type::Geometry);

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::OffsetCurve,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("distance".into(), literal_length(0.002)),
                ("third".into(), ref_expr),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        match result {
            Ok(reify_ir::GeometryOp::OffsetCurve {
                reference,
                direction,
                ..
            }) => {
                assert_eq!(
                    reference,
                    Some(ref_handle),
                    "a bound GeometryHandle 3rd arg becomes the reference surface"
                );
                assert_eq!(direction, None, "a reference 3rd arg has no direction");
            }
            other => panic!("expected Ok(OffsetCurve) with reference, got {:?}", other),
        }
    }

    // ── Fillet eval-arm: anti-zero-edges + 2-arg back-compat (task 3205 step-9/10) ──

    /// Build a `CompiledExpr` literal that evaluates to an empty `Value::List`
    /// — a present-but-empty edge selector. Drives the anti-zero-edges
    /// (E_EMPTY_SELECTION) eval-arm path.
    fn empty_list_literal() -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(vec![]),
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        )
    }

    /// (a) ANTI-ZERO-EDGES: a 3-arg Fillet whose `edges` arg is PRESENT but
    /// evaluates to an empty `Value::List` must NOT silently fall through to
    /// the all-edges path. `compile_geometry_op` returns `Err`, pushes exactly
    /// one diagnostic carrying `DiagnosticCode::EmptyEdgeSelection`, and
    /// produces NO `GeometryOp::Fillet`. Closes the task-3295 fake-done trap.
    #[test]
    fn compile_geometry_op_fillet_empty_edge_selection_errors_with_code() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // 3-arg form: args carry "target" (the solid expr), an "edges" selector
        // that evaluates to Value::List(vec![]), and "radius".
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("edges".into(), empty_list_literal()),
                ("radius".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "a present edge selector resolving to zero edges must Err (never \
             fall through to all-edges), got {:?}",
            result
        );
        let empty_sel: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.code == Some(reify_core::DiagnosticCode::EmptyEdgeSelection))
            .collect();
        assert_eq!(
            empty_sel.len(),
            1,
            "expected exactly one EmptyEdgeSelection diagnostic, got diagnostics: {:?}",
            diagnostics
        );
    }

    /// (b) 2-arg back-compat: a Fillet with NO `edges` arg lowers to
    /// `GeometryOp::Fillet{edges: vec![], ..}` (the all-edges path) with NO
    /// `EmptyEdgeSelection` diagnostic — "no selector" is legitimately
    /// all-edges, distinct from "selector present but empty".
    #[test]
    fn compile_geometry_op_fillet_2arg_no_edges_arg_is_all_edges_back_compat() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("radius".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Fillet { target, edges, .. }) => {
                assert_eq!(
                    target,
                    GeometryHandleId(10),
                    "target must resolve via Step(0)"
                );
                assert!(
                    edges.is_empty(),
                    "2-arg fillet (no edges arg) must lower to empty edges \
                     (all-edges back-compat), got {:?}",
                    edges
                );
            }
            other => panic!(
                "expected Ok(GeometryOp::Fillet) for 2-arg fillet, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "2-arg fillet must NOT emit an EmptyEdgeSelection diagnostic, got: {:?}",
            diagnostics
        );
    }

    /// (c) INTERMEDIATE UX: on the legacy pipeline a 3-arg `fillet(solid, edges,
    /// radius)` reaches this eval arm with the `edges` selector still UNRESOLVED
    /// (runtime `Value::Undef` — the selector resolves in P4, after this P2 arm).
    /// That is NOT an empty selection, so the arm must NOT emit
    /// `EmptyEdgeSelection`; instead it returns a USER-ACTIONABLE `Err` (surfaced
    /// verbatim as `failed to compile geometry operation: <msg>`), not the old
    /// internal "did not resolve to a List" string. This pins the staging UX
    /// until engine-unified-build-dag η/ε (tasks 4360/4358) make curated
    /// selection reachable end-to-end. (Reviewer test_coverage note, task 3205.)
    #[test]
    fn compile_geometry_op_fillet_legacy_selector_unresolved_is_user_actionable() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // 3-arg form whose "edges" selector evaluates to `Value::Undef` — the
        // legacy-pipeline state where the selector has not yet resolved. Its
        // STATIC type is `List<Geometry>`, but its runtime value is `Undef`.
        let unresolved_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Undef,
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("edges".into(), unresolved_selector),
                ("radius".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "an unresolved legacy edge selector must Err (stays Undef for η \
                 to resolve in-loop), got Ok({:?})",
                other
            ),
        };
        // User-actionable: names the call form, points at the 2-arg fallback,
        // and does NOT leak the old internal "did not resolve to a List" string.
        assert!(
            msg.contains("fillet(solid, edges, radius)"),
            "diagnostic must name the 3-arg call form, got: {msg:?}"
        );
        assert!(
            msg.contains("2-arg fillet(solid, radius)"),
            "diagnostic must point the user at the 2-arg all-edges fallback, got: {msg:?}"
        );
        assert!(
            !msg.contains("did not resolve to a List"),
            "diagnostic must not surface the raw internal 'did not resolve to a \
             List' string, got: {msg:?}"
        );
        // The deferral is preserved: an unresolved selector is NOT an empty
        // selection, so it must NEVER trip the anti-zero-edges guard.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "an unresolved (non-List) selector must NOT emit EmptyEdgeSelection \
             (that would false-positive on every legacy 3-arg fillet), got: {:?}",
            diagnostics
        );
    }

    /// (d) MALFORMED ELEMENT: a 3-arg Fillet whose `edges` selector resolves to a
    /// List containing a NON-handle element must `Err` on the bad element rather
    /// than silently filleting only the surviving handle subset. This mirrors
    /// `resolve_subhandle_list`'s reject-non-handle strictness so the eval arm
    /// and the full resolver share one validation policy. The malformed case is
    /// distinct from an EMPTY selection, so it must NOT trip EmptyEdgeSelection.
    /// (Reviewer robustness note, task 3205.)
    #[test]
    fn compile_geometry_op_fillet_malformed_element_errors_not_empty_selection() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // "edges" resolves to a List with a non-handle element (a bare Real) —
        // a partially-malformed selector. The old `filter_map` would have
        // silently dropped it; the strict arm errors on it.
        let malformed_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(vec![reify_ir::Value::Real(1.0)]),
            reify_core::Type::List(Box::new(reify_core::Type::dimensionless_scalar())),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("edges".into(), malformed_selector),
                ("radius".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "a selector with a non-handle element must Err (never silently \
                 fillet the surviving subset), got Ok({:?})",
                other
            ),
        };
        assert!(
            msg.contains("not a Geometry sub-handle"),
            "diagnostic must flag the non-handle element, got: {msg:?}"
        );
        // A malformed element is NOT an empty selection — it must error on the
        // element, never reach (and so never trip) the anti-zero-edges guard.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a malformed-element selector must NOT emit EmptyEdgeSelection, got: {:?}",
            diagnostics
        );
    }

    // ── Chamfer eval-arm: curated edges resolution + anti-zero + 2-arg back-compat ──
    // These mirror the Fillet eval-arm tests above; ModifyKind::Chamfer is reused
    // (no new kind) for the symmetric 2/3-arg form.

    /// CHAMFER (a) ANTI-ZERO-EDGES: a 3-arg Chamfer whose `edges` arg is PRESENT
    /// but evaluates to an empty `Value::List` must NOT silently fall through to
    /// the all-edges path. `compile_geometry_op` returns `Err`, pushes exactly
    /// one diagnostic carrying `DiagnosticCode::EmptyEdgeSelection`, and produces
    /// NO `GeometryOp::Chamfer`. Mirrors the Fillet arm; closes the task-3295 trap.
    #[test]
    fn compile_geometry_op_chamfer_empty_edge_selection_errors_with_code() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // 3-arg form: args carry "target" (the solid expr), an "edges" selector
        // that evaluates to Value::List(vec![]), and "distance".
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Chamfer,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("edges".into(), empty_list_literal()),
                ("distance".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "a present chamfer edge selector resolving to zero edges must Err \
             (never fall through to all-edges), got {:?}",
            result
        );
        let empty_sel: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.code == Some(reify_core::DiagnosticCode::EmptyEdgeSelection))
            .collect();
        assert_eq!(
            empty_sel.len(),
            1,
            "expected exactly one EmptyEdgeSelection diagnostic, got diagnostics: {:?}",
            diagnostics
        );
    }

    /// CHAMFER (b) 2-arg back-compat: a Chamfer with NO `edges` arg lowers to
    /// `GeometryOp::Chamfer{edges: vec![], ..}` (the all-edges path) with NO
    /// `EmptyEdgeSelection` diagnostic — "no selector" is legitimately all-edges,
    /// distinct from "selector present but empty".
    #[test]
    fn compile_geometry_op_chamfer_2arg_back_compat_builds_empty_edges() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Chamfer,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("distance".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Chamfer { target, edges, .. }) => {
                assert_eq!(
                    target,
                    GeometryHandleId(10),
                    "target must resolve via Step(0)"
                );
                assert!(
                    edges.is_empty(),
                    "2-arg chamfer (no edges arg) must lower to empty edges \
                     (all-edges back-compat), got {:?}",
                    edges
                );
            }
            other => panic!(
                "expected Ok(GeometryOp::Chamfer) for 2-arg chamfer, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "2-arg chamfer must NOT emit an EmptyEdgeSelection diagnostic, got: {:?}",
            diagnostics
        );
    }

    /// CHAMFER (c) MALFORMED ELEMENT: a 3-arg Chamfer whose `edges` selector
    /// resolves to a List containing a NON-handle element must `Err` on the bad
    /// element rather than silently chamfering only the surviving handle subset.
    /// Mirrors the Fillet arm's reject-non-handle strictness so the chamfer eval
    /// arm and the full resolver share one validation policy. The malformed case
    /// is distinct from an EMPTY selection, so it must NOT trip EmptyEdgeSelection.
    #[test]
    fn compile_geometry_op_chamfer_malformed_element_errors_not_empty_selection() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // "edges" resolves to a List with a non-handle element (a bare Real) — a
        // partially-malformed selector that a `filter_map` would silently drop.
        let malformed_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(vec![reify_ir::Value::Real(1.0)]),
            reify_core::Type::List(Box::new(reify_core::Type::dimensionless_scalar())),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Chamfer,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("edges".into(), malformed_selector),
                ("distance".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "a selector with a non-handle element must Err (never silently \
                 chamfer the surviving subset), got Ok({:?})",
                other
            ),
        };
        assert!(
            msg.contains("not a Geometry sub-handle"),
            "diagnostic must flag the non-handle element, got: {msg:?}"
        );
        // A malformed element is NOT an empty selection — it must error on the
        // element, never reach (and so never trip) the anti-zero-edges guard.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a malformed-element chamfer selector must NOT emit EmptyEdgeSelection, got: {:?}",
            diagnostics
        );
    }

    // ── ChamferAsymmetric eval-arm: distinct d1/d2 + curated edges + anti-zero ──
    // The 4-arg `chamfer_asymmetric(solid, edges, d1, d2)` form lowers to the NEW
    // `ModifyKind::ChamferAsymmetric` → `GeometryOp::ChamferAsymmetric` (β, task 4185).
    // The edge-resolution + EmptyEdgeSelection logic is shared with the Chamfer arm.

    /// ASYMMETRIC (a) BUILDS VARIANT: a 4-arg ChamferAsymmetric whose `edges`
    /// selector resolves to a List of `Value::GeometryHandle` sub-handles threads
    /// the canonical edge ids (ascending kernel_handle order, deduped) and BOTH
    /// distinct setbacks `d1`/`d2` onto a `GeometryOp::ChamferAsymmetric`. Supplies
    /// two handles in REVERSE order so the canonical-sort is observable (h7 < h42 →
    /// [7, 42]); supplies distinct d1≠d2 so the two-distance threading is observable.
    ///
    /// RED until step-12 adds `ModifyKind::ChamferAsymmetric` + its eval arm.
    #[test]
    fn compile_geometry_op_chamfer_asymmetric_builds_variant() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::ChamferAsymmetric,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                (
                    "edges".into(),
                    geometry_handle_list_literal(vec![GeometryHandleId(42), GeometryHandleId(7)]),
                ),
                ("d1".into(), literal_length(0.001)),
                ("d2".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::ChamferAsymmetric {
                target,
                edges,
                d1,
                d2,
            }) => {
                assert_eq!(
                    target,
                    GeometryHandleId(10),
                    "target must resolve via Step(0)"
                );
                assert_eq!(
                    edges,
                    vec![GeometryHandleId(7), GeometryHandleId(42)],
                    "edges must be canonically sorted (ascending kernel_handle id), \
                     got {:?}",
                    edges
                );
                assert_eq!(
                    d1.as_f64(),
                    Some(0.001),
                    "d1 setback must thread through, got {:?}",
                    d1
                );
                assert_eq!(
                    d2.as_f64(),
                    Some(0.002),
                    "d2 setback must thread through (distinct from d1), got {:?}",
                    d2
                );
            }
            other => panic!(
                "expected Ok(GeometryOp::ChamferAsymmetric) for 4-arg \
                 chamfer_asymmetric with curated edges, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a curated-edges chamfer_asymmetric must NOT emit EmptyEdgeSelection, got: {:?}",
            diagnostics
        );
    }

    /// ASYMMETRIC (b) ANTI-ZERO-EDGES: a 4-arg ChamferAsymmetric whose `edges`
    /// arg is PRESENT but evaluates to an empty `Value::List` must NOT silently
    /// fall through to the all-edges path. `compile_geometry_op` returns `Err`,
    /// pushes exactly one diagnostic carrying `DiagnosticCode::EmptyEdgeSelection`,
    /// and produces NO `GeometryOp`. Shares the Chamfer arm's anti-zero guard;
    /// closes the task-3295 fake-done trap for the asymmetric form too.
    ///
    /// RED until step-12 adds `ModifyKind::ChamferAsymmetric` + its eval arm.
    #[test]
    fn compile_geometry_op_chamfer_asymmetric_empty_edge_selection_errors_with_code() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::ChamferAsymmetric,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("edges".into(), empty_list_literal()),
                ("d1".into(), literal_length(0.001)),
                ("d2".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "a present chamfer_asymmetric edge selector resolving to zero edges \
             must Err (never fall through to all-edges), got {:?}",
            result
        );
        let empty_sel: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.code == Some(reify_core::DiagnosticCode::EmptyEdgeSelection))
            .collect();
        assert_eq!(
            empty_sel.len(),
            1,
            "expected exactly one EmptyEdgeSelection diagnostic, got diagnostics: {:?}",
            diagnostics
        );
    }

    // ── Draft eval-arm: faces resolution + anti-zero + 3-arg back-compat ──

    /// Helper: build a `Value::GeometryHandle` sub-handle with the given
    /// kernel handle id, using a fixed test realization_ref and hash.
    fn geometry_handle_value(kernel_handle: GeometryHandleId) -> reify_ir::Value {
        reify_ir::Value::GeometryHandle {
            realization_ref: reify_core::identity::RealizationNodeId::new("test-solid", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(kernel_handle),
        }
    }

    /// Helper: build a `CompiledExpr` literal that evaluates to a
    /// `Value::List` of `Value::GeometryHandle` sub-handles.
    fn geometry_handle_list_literal(handles: Vec<GeometryHandleId>) -> reify_ir::CompiledExpr {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(handles.into_iter().map(geometry_handle_value).collect()),
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        )
    }

    /// (a) 4-arg draft: a "faces" selector that evaluates to a List of
    /// `Value::GeometryHandle` sub-handles threads the canonical face ids
    /// (ascending kernel_handle order, deduped) onto
    /// `GeometryOp::Draft.faces`. Supplies two handles in REVERSE order so
    /// the canonical-sort is observable (h7 < h42 → sorted [7, 42]).
    #[test]
    fn compile_geometry_op_draft_4arg_faces_threads_canonical_handles() {
        // step_handles[0] = target solid; step_handles.last() = plane
        // (the Draft eval arm resolves the plane via step_handles.last()).
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                (
                    "faces".into(),
                    geometry_handle_list_literal(vec![GeometryHandleId(42), GeometryHandleId(7)]),
                ),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Draft { target, faces, .. }) => {
                assert_eq!(
                    target,
                    GeometryHandleId(10),
                    "target must resolve via Step(0)"
                );
                assert_eq!(
                    faces,
                    vec![GeometryHandleId(7), GeometryHandleId(42)],
                    "faces must be canonically sorted (ascending kernel_handle id), \
                     got {:?}",
                    faces
                );
            }
            other => panic!(
                "expected Ok(GeometryOp::Draft) for 4-arg draft with curated faces, \
                 got {:?}",
                other
            ),
        }
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a curated-faces draft must NOT emit EmptyEdgeSelection, got: {:?}",
            diagnostics
        );
    }

    /// (b) ANTI-ZERO-FACES: a 4-arg Draft whose "faces" selector is PRESENT
    /// but evaluates to an empty List must NOT silently fall through to the
    /// all-faces path. `compile_geometry_op` returns `Err` and pushes exactly
    /// one diagnostic carrying `DiagnosticCode::EmptyEdgeSelection`.
    /// Closes the task-3295 fake-done trap for Draft.
    #[test]
    fn compile_geometry_op_draft_empty_face_selection_errors_with_code() {
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("faces".into(), empty_list_literal()),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "a present face selector resolving to zero faces must Err (never \
             fall through to all-faces), got {:?}",
            result
        );
        let empty_sel: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.code == Some(reify_core::DiagnosticCode::EmptyEdgeSelection))
            .collect();
        assert_eq!(
            empty_sel.len(),
            1,
            "expected exactly one EmptyEdgeSelection diagnostic, got diagnostics: {:?}",
            diagnostics
        );
    }

    /// (c) 3-arg back-compat: a Draft with NO "faces" arg lowers to
    /// `GeometryOp::Draft{faces: vec![], ..}` (the all-faces path) with NO
    /// `EmptyEdgeSelection` diagnostic — "no selector" is legitimately
    /// all-draftable-faces, distinct from "selector present but empty".
    #[test]
    fn compile_geometry_op_draft_3arg_no_faces_arg_is_all_faces_back_compat() {
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Draft { target, faces, .. }) => {
                assert_eq!(
                    target,
                    GeometryHandleId(10),
                    "target must resolve via Step(0)"
                );
                assert!(
                    faces.is_empty(),
                    "3-arg draft (no faces arg) must lower to empty faces \
                     (all-faces back-compat), got {:?}",
                    faces
                );
            }
            other => panic!(
                "expected Ok(GeometryOp::Draft) for 3-arg draft, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "3-arg draft must NOT emit an EmptyEdgeSelection diagnostic, got: {:?}",
            diagnostics
        );
    }

    /// (d) MALFORMED ELEMENT: a 4-arg Draft whose "faces" selector resolves
    /// to a List containing a NON-handle element must `Err` on the bad
    /// element rather than silently drafting only the surviving handle
    /// subset. A malformed element is distinct from an empty selection, so
    /// it must NOT trip EmptyEdgeSelection.
    #[test]
    fn compile_geometry_op_draft_malformed_element_errors_not_empty_selection() {
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        // "faces" resolves to a List with a non-handle element (a bare Real)
        let malformed_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(vec![reify_ir::Value::Real(1.0)]),
            reify_core::Type::List(Box::new(reify_core::Type::dimensionless_scalar())),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("faces".into(), malformed_selector),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "a selector with a non-handle element must Err (never silently \
                 draft the surviving subset), got Ok({:?})",
                other
            ),
        };
        assert!(
            msg.contains("not a Geometry sub-handle"),
            "diagnostic must flag the non-handle element, got: {msg:?}"
        );
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a malformed-element selector must NOT emit EmptyEdgeSelection, got: {:?}",
            diagnostics
        );
    }

    /// (e) NON-LIST SELECTOR: a 4-arg Draft whose "faces" selector evaluates
    /// to a non-List value (e.g., `Value::Undef` on the legacy pipeline)
    /// must return a user-actionable `Err` and must NOT emit
    /// `EmptyEdgeSelection` (that would false-positive on every legacy miss).
    #[test]
    fn compile_geometry_op_draft_legacy_selector_unresolved_is_user_actionable() {
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        // "faces" evaluates to `Value::Undef` — the legacy-pipeline state
        // where the selector has not yet resolved.
        let unresolved_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Undef,
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("faces".into(), unresolved_selector),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "an unresolved (non-List) faces selector must Err (stays \
                 Undef for future in-loop resolution), got Ok({:?})",
                other
            ),
        };
        // User-actionable: names the 4-arg call form and points at the
        // 3-arg all-faces fallback.
        assert!(
            msg.contains("draft(solid, faces, angle, neutral_plane)"),
            "diagnostic must name the 4-arg call form, got: {msg:?}"
        );
        assert!(
            msg.contains("3-arg draft(solid, angle, neutral_plane)"),
            "diagnostic must point the user at the 3-arg all-faces fallback, \
             got: {msg:?}"
        );
        // A non-List is NOT an empty selection — must never trip anti-zero
        // guard.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "an unresolved (non-List) selector must NOT emit EmptyEdgeSelection, \
             got: {:?}",
            diagnostics
        );
    }

    // TODO(#4727): The UnifiedDag cutover (Stage-4 #4362) has landed; the 4-arg
    // `draft(solid, faces, angle, neutral_plane)` face selector now resolves on
    // the active (default) pipeline. Add an end-to-end .ri-source test that
    // compiles a 4-arg draft, runs it through eval, and asserts the resulting
    // `GeometryOp::Draft.faces` vector is non-empty and the kernel produces a
    // drafted solid with positive volume — confirming the full
    // compiler → eval → kernel path works from authored .ri source.

    #[test]
    fn compile_geometry_op_transform_pattern_sweep_present_args_emit_no_diagnostics() {
        let step_handles = vec![GeometryHandleId(1)];
        let values = ValueMap::new();

        // Transform::Translate — all three required args present
        let translate_op = CompiledGeometryOp::Transform {
            kind: reify_compiler::TransformKind::Translate,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(1.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
            ],
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &translate_op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_ok(), "Translate with all args should return Some");
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for Translate with all args, got: {:?}",
            diagnostics
        );

        // Pattern::LinearPattern — all required args present
        let linear_op = CompiledGeometryOp::Pattern {
            kind: reify_compiler::PatternKind::Linear,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                ("spacing".into(), literal_length(0.02)),
            ],
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &linear_op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_ok(),
            "LinearPattern with all args should return Some"
        );
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for LinearPattern with all args, got: {:?}",
            diagnostics
        );

        // Sweep::Extrude — distance present
        let extrude_op = CompiledGeometryOp::Sweep {
            kind: reify_compiler::SweepKind::Extrude,
            profiles: vec![reify_compiler::GeomRef::Step(0)],
            args: vec![("distance".into(), literal_length(0.05))],
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &extrude_op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_ok(), "Extrude with all args should return Some");
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for Extrude with all args, got: {:?}",
            diagnostics
        );

        // Sweep::Revolve — all seven args present with a valid axis
        let revolve_op = CompiledGeometryOp::Sweep {
            kind: reify_compiler::SweepKind::Revolve,
            profiles: vec![reify_compiler::GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
            ],
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &revolve_op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(result.is_ok(), "Revolve with all args should return Some");
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for Revolve with all args, got: {:?}",
            diagnostics
        );
    }

    // ── missing-arg diagnostic tests for Transform/Pattern/Sweep ─────────────

    #[test]
    fn compile_geometry_op_sweep_extrude_missing_distance_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Extrude with no args at all — 'distance' is missing
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![reify_compiler::GeomRef::Step(0)],
            args: vec![],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // Still returns None
        assert!(
            result.is_err(),
            "missing 'distance' should still return None, got {:?}",
            result
        );

        // Exactly one diagnostic warning for the missing 'distance' arg
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for missing 'distance', got: {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "expected Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("distance"),
            "diagnostic message should mention 'distance', got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("extrude")
                && !diagnostics[0].message.contains("extrude_"),
            "diagnostic message should mention 'extrude' but not any underscore-suffixed sibling (extrude_*), got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn compile_geometry_op_pattern_linear_missing_spacing_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // LinearPattern with dx/dy/dz/count but OMITS spacing
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                // spacing deliberately omitted
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // Still returns None (Pattern short-circuits on missing args)
        assert!(
            result.is_err(),
            "missing spacing should still return None, got {:?}",
            result
        );

        // Exactly one diagnostic warning for the missing 'spacing' arg
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for missing 'spacing', got: {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "expected Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("spacing"),
            "diagnostic message should mention 'spacing', got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("linear")
                && !diagnostics[0].message.contains("linear_"),
            "diagnostic message should mention 'linear' but not any underscore-suffixed sibling (linear_*), got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn compile_geometry_op_transform_translate_missing_arg_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // Translate with only dx — missing dy, dz
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![("dx".into(), literal_f64(1.0))],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // Still returns None (Transform short-circuits on missing f64 args)
        assert!(
            result.is_err(),
            "missing dy/dz should still return None, got {:?}",
            result
        );

        // But now exactly one diagnostic warning should be emitted for the first missing arg 'dy'
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one diagnostic for missing 'dy', got: {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "expected Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("dy"),
            "diagnostic message should mention 'dy', got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("translate"),
            "diagnostic message should mention 'translate', got: {}",
            diagnostics[0].message
        );
    }

    // ── non-numeric/non-finite diagnostic tests ──────────────────────────────

    #[test]
    fn compile_geometry_op_translate_wrong_type_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // dx is a String value, not a numeric f64 — should trigger a non-numeric diagnostic
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![
                (
                    "dx".into(),
                    reify_ir::CompiledExpr::literal(
                        reify_ir::Value::String("oops".into()),
                        reify_core::Type::String,
                    ),
                ),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "wrong-type dx should return None, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("non-numeric/non-finite")
                    && d.message.contains("dx")
                    && d.message.contains("translate")
            }),
            "expected a Warning mentioning 'non-numeric/non-finite', 'dx', and 'translate', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_translate_nan_dx_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // dx is NaN — non-finite, should trigger a diagnostic
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(f64::NAN)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "NaN dx should return None, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("non-numeric/non-finite")
                    && d.message.contains("dx")
                    && d.message.contains("translate")
            }),
            "expected a Warning mentioning 'non-numeric/non-finite', 'dx', and 'translate', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_translate_infinity_dx_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // dx is +Infinity — non-finite, should trigger a diagnostic
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(f64::INFINITY)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "Infinity dx should return None, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("non-numeric/non-finite")
                    && d.message.contains("dx")
                    && d.message.contains("translate")
            }),
            "expected a Warning mentioning 'non-numeric/non-finite', 'dx', and 'translate', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_translate_finite_args_no_false_positive_warning() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // All finite args — should succeed with no non-numeric/non-finite warning
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(1.0)),
                ("dy".into(), literal_f64(2.0)),
                ("dz".into(), literal_f64(3.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "finite Translate args should return Some, got None; diagnostics: {:?}",
            diagnostics
        );
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("non-numeric/non-finite")),
            "no 'non-numeric/non-finite' warning expected for finite args, got: {:?}",
            diagnostics
        );
    }

    // ---------------------------------------------------------------------------
    // Tests: INVALID sentinel preserves step index alignment (task-612, step-9)
    // ---------------------------------------------------------------------------

    /// Verifies that an INVALID sentinel at step index 1 does not shift subsequent
    /// valid handles. With step_handles = [42, INVALID, 100]:
    /// - Boolean(Step(0), Step(2)) → Some(Union { left: 42, right: 100 })
    ///   Step(0) resolves to 42 and Step(2) resolves to 100, both correct.
    ///   The INVALID at index 1 is skipped; indices ≥ 2 are unaffected.
    /// - Boolean(Step(0), Step(1)) → None
    ///   Step(1) is INVALID, filtered out by the sentinel check, so the op fails.
    ///
    /// Together these two assertions confirm that:
    /// (a) the sentinel at index 1 maintains index alignment for subsequent handles,
    /// (b) the INVALID value correctly blocks resolution of its own index.
    #[test]
    fn compile_geometry_op_invalid_sentinel_preserves_index_alignment() {
        use reify_compiler::BooleanOp;
        let values = ValueMap::new();

        // step_handles[0] = 42 (valid sphere handle)
        // step_handles[1] = INVALID (sentinel for a failed op)
        // step_handles[2] = 100 (valid handle — must remain at index 2)
        let step_handles = vec![
            GeometryHandleId(42),
            GeometryHandleId::INVALID,
            GeometryHandleId(100),
        ];

        // (a) Union(Step(0), Step(2)): both resolve correctly despite sentinel at index 1
        let op_ok = CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(0),
            right: GeomRef::Step(2),
        };
        let result_ok = compile_geometry_op(
            &op_ok,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let result_ok = result_ok
            .expect("Boolean(Step(0), Step(2)) should succeed: both indices hold valid handles");
        match result_ok {
            reify_ir::GeometryOp::Union { left, right } => {
                assert_eq!(
                    left,
                    GeometryHandleId(42),
                    "Step(0) should resolve to handle 42 (not shifted by sentinel at index 1)"
                );
                assert_eq!(
                    right,
                    GeometryHandleId(100),
                    "Step(2) should resolve to handle 100 (aligned correctly despite sentinel at 1)"
                );
            }
            other => panic!(
                "expected GeometryOp::Union from Boolean(Step(0), Step(2)), got {:?}",
                other
            ),
        }

        // (b) Union(Step(0), Step(1)): Step(1) is INVALID → filtered out → returns None
        let op_fail = CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(0),
            right: GeomRef::Step(1),
        };
        let result_fail = compile_geometry_op(
            &op_fail,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            result_fail.is_err(),
            "Boolean(Step(0), Step(1)) should return Err: Step(1) is INVALID and filtered out"
        );
    }

    // ── Shell face index validation tests ────────────────────────────────────

    #[test]
    fn compile_geometry_op_shell_non_numeric_face_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 is a String value — should trigger a non-numeric diagnostic
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                (
                    "face_0".into(),
                    reify_ir::CompiledExpr::literal(
                        reify_ir::Value::String("oops".into()),
                        reify_core::Type::String,
                    ),
                ),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // Shell op itself should still succeed (non-numeric face is skipped)
        assert!(
            result.is_ok(),
            "Shell should return Some even when face_0 is non-numeric, got {:?}",
            result
        );
        // The bad face should produce a diagnostic mentioning 'non-numeric'
        // (precision assertion — that it does NOT say 'non-finite' — lives in the dedicated
        // compile_geometry_op_shell_string_face_diagnostic_excludes_non_finite test)
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("non-numeric")
            }),
            "expected a Warning mentioning 'face_0' and 'non-numeric', got: {:?}",
            diagnostics
        );
        // The resulting faces_to_remove should be empty (bad face skipped)
        match result.unwrap() {
            reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            } => {
                assert!(
                    faces_to_remove.is_empty(),
                    "faces_to_remove should be empty when face_0 is non-numeric, got {:?}",
                    faces_to_remove
                );
            }
            other => panic!("expected GeometryOp::Shell, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_shell_bool_face_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_1 is a Bool value — should trigger a non-numeric diagnostic
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                (
                    "face_1".into(),
                    reify_ir::CompiledExpr::literal(
                        reify_ir::Value::Bool(true),
                        reify_core::Type::Bool,
                    ),
                ),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell should return Some even when face_1 is Bool, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_1")
                    && d.message.contains("non-numeric")
                    && !d.message.contains("non-finite")
            }),
            "expected a Warning mentioning 'face_1' and 'non-numeric' (not 'non-finite'), got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_shell_negative_face_index_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = -1.0 — would wrap to usize::MAX without the guard
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(-1.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell should return Some even when face_0 is negative, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("negative")
                    && !d.message.contains("non-finite")
            }),
            "expected a Warning mentioning 'face_0' and 'negative' (not 'non-finite'), got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            } => {
                assert!(
                    faces_to_remove.is_empty(),
                    "faces_to_remove should be empty when face_0 is -1.0, got {:?}",
                    faces_to_remove
                );
            }
            other => panic!("expected GeometryOp::Shell, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_shell_nan_face_index_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = NaN — non-finite
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(f64::NAN)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with NaN face_0 should return Some, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("non-finite")
                    && !d.message.contains("negative")
            }),
            "expected a Warning mentioning 'non-finite' (not 'negative') for NaN face_0, got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            } => {
                assert!(
                    faces_to_remove.is_empty(),
                    "faces_to_remove should be empty for NaN face_0, got {:?}",
                    faces_to_remove
                );
            }
            other => panic!("expected GeometryOp::Shell, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_shell_infinity_face_index_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = +Infinity — non-finite
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(f64::INFINITY)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with INFINITY face_0 should return Some, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && d.message.contains("non-finite")
                    && !d.message.contains("negative")
            }),
            "expected a Warning mentioning 'non-finite' (not 'negative') for INFINITY face_0, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_shell_valid_faces_no_false_positive() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // All three faces are valid non-negative integers — no diagnostics should be emitted
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(0.0)),
                ("face_1".into(), literal_f64(2.0)),
                ("face_2".into(), literal_f64(5.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            }) => {
                assert_eq!(
                    faces_to_remove,
                    vec![0usize, 2, 5],
                    "valid faces should all be collected correctly"
                );
            }
            other => panic!("expected Some(Shell), got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "valid faces should produce no diagnostics, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn compile_geometry_op_shell_fractional_face_index_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = 2.7 — non-integer, should emit diagnostic and be skipped (not truncated to 2)
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(2.7)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with fractional face_0 should return Some, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && (d.message.contains("integer") || d.message.contains("fractional"))
            }),
            "expected a Warning about non-integer face_0, got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            } => {
                assert!(
                    faces_to_remove.is_empty(),
                    "fractional face index should be skipped (not truncated), got {:?}",
                    faces_to_remove
                );
            }
            other => panic!("expected GeometryOp::Shell, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_shell_huge_face_index_emits_diagnostic() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = 2e18 — far exceeds upper bound; Rust saturates f64→usize to usize::MAX
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(2e18)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with huge face_0 should return Some, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && d.message.contains("face_0")
                    && (d.message.contains("upper bound") || d.message.contains("exceeds"))
            }),
            "expected a Warning about face_0 exceeding upper bound, got: {:?}",
            diagnostics
        );
        match result.unwrap() {
            reify_ir::GeometryOp::Shell {
                faces_to_remove, ..
            } => {
                assert!(
                    faces_to_remove.is_empty(),
                    "huge face index should be skipped, got {:?}",
                    faces_to_remove
                );
            }
            other => panic!("expected GeometryOp::Shell, got {:?}", other),
        }
    }

    // ── Shell face index diagnostic precision tests ───────────────────────────

    #[test]
    fn compile_geometry_op_shell_string_face_diagnostic_excludes_non_finite() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 is a String — as_f64() returns None (non-numeric type, NOT non-finite)
        // Diagnostic should say 'non-numeric' only, NOT 'non-finite'
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                (
                    "face_0".into(),
                    reify_ir::CompiledExpr::literal(
                        reify_ir::Value::String("bad".into()),
                        reify_core::Type::String,
                    ),
                ),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell should return Some even when face_0 is String, got {:?}",
            result
        );
        let face_0_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                matches!(d.severity, reify_core::Severity::Warning) && d.message.contains("face_0")
            })
            .collect();
        assert_eq!(
            face_0_warnings.len(),
            1,
            "expected exactly one Warning mentioning 'face_0', got: {:?}",
            face_0_warnings
        );
        let diag = face_0_warnings[0];
        assert!(
            diag.message.contains("non-numeric"),
            "diagnostic should mention 'non-numeric', got: {:?}",
            diag.message
        );
        assert!(
            !diag.message.contains("non-finite"),
            "diagnostic should NOT mention 'non-finite' for a non-numeric type, got: {:?}",
            diag.message
        );
    }

    #[test]
    fn compile_geometry_op_shell_nan_face_diagnostic_excludes_negative() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = NaN — non-finite value; diagnostic should say 'non-finite', NOT 'negative'
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(f64::NAN)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with NaN face_0 should return Some, got {:?}",
            result
        );
        let face_0_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                matches!(d.severity, reify_core::Severity::Warning) && d.message.contains("face_0")
            })
            .collect();
        assert_eq!(
            face_0_warnings.len(),
            1,
            "expected exactly one Warning mentioning 'face_0', got: {:?}",
            face_0_warnings
        );
        let diag = face_0_warnings[0];
        assert!(
            diag.message.contains("non-finite"),
            "NaN diagnostic should mention 'non-finite', got: {:?}",
            diag.message
        );
        assert!(
            !diag.message.contains("negative"),
            "NaN diagnostic should NOT mention 'negative', got: {:?}",
            diag.message
        );
    }

    #[test]
    fn compile_geometry_op_shell_negative_face_diagnostic_excludes_non_finite() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = -1.0 — negative value; diagnostic should say 'negative', NOT 'non-finite'
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(-1.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with -1.0 face_0 should return Some, got {:?}",
            result
        );
        let face_0_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                matches!(d.severity, reify_core::Severity::Warning) && d.message.contains("face_0")
            })
            .collect();
        assert_eq!(
            face_0_warnings.len(),
            1,
            "expected exactly one Warning mentioning 'face_0', got: {:?}",
            face_0_warnings
        );
        let diag = face_0_warnings[0];
        assert!(
            diag.message.contains("negative"),
            "negative face diagnostic should mention 'negative', got: {:?}",
            diag.message
        );
        assert!(
            !diag.message.contains("non-finite"),
            "negative face diagnostic should NOT mention 'non-finite', got: {:?}",
            diag.message
        );
    }

    #[test]
    fn compile_geometry_op_shell_neg_infinity_face_diagnostic_says_non_finite() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // face_0 = -Infinity — satisfies both !is_finite() and < 0.0; should be classified
        // as 'non-finite' (not 'negative'), so the is_finite() arm must come first.
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.002)),
                ("face_0".into(), literal_f64(f64::NEG_INFINITY)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_ok(),
            "Shell with -Infinity face_0 should return Some, got {:?}",
            result
        );
        let face_0_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                matches!(d.severity, reify_core::Severity::Warning) && d.message.contains("face_0")
            })
            .collect();
        assert_eq!(
            face_0_warnings.len(),
            1,
            "expected exactly one Warning mentioning 'face_0', got: {:?}",
            face_0_warnings
        );
        let diag = face_0_warnings[0];
        assert!(
            diag.message.contains("non-finite"),
            "-Infinity diagnostic should mention 'non-finite', got: {:?}",
            diag.message
        );
        assert!(
            !diag.message.contains("negative"),
            "-Infinity diagnostic should NOT mention 'negative' (it is non-finite, not negative), got: {:?}",
            diag.message
        );
    }

    // ── Shell open curated-face eval-arm tests (γ step-3) ──────────────────

    /// (a) Shell with an `open_faces` arg whose expression evaluates to a
    /// `Value::List` of `Value::GeometryHandle` sub-handles threads the
    /// canonical face ids (ascending kernel_handle order, deduped) onto
    /// `GeometryOp::Shell.open_face_handles` and leaves `faces_to_remove`
    /// empty.  Supplies two handles in REVERSE order so the canonical-sort
    /// is observable (h7 < h42 → sorted [7, 42]).
    /// Mirrors `compile_geometry_op_draft_4arg_faces_threads_canonical_handles`.
    #[test]
    fn compile_geometry_op_shell_open_curated_faces_threads_canonical_handles() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.001)),
                (
                    "open_faces".into(),
                    geometry_handle_list_literal(vec![
                        GeometryHandleId(42),
                        GeometryHandleId(7),
                    ]),
                ),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Shell {
                open_face_handles,
                faces_to_remove,
                ..
            }) => {
                assert_eq!(
                    open_face_handles,
                    vec![GeometryHandleId(7), GeometryHandleId(42)],
                    "open_face_handles must be canonically sorted (ascending \
                     kernel_handle id), got {:?}",
                    open_face_handles
                );
                assert!(
                    faces_to_remove.is_empty(),
                    "curated shell_open must produce an empty faces_to_remove, \
                     got {:?}",
                    faces_to_remove
                );
            }
            other => panic!(
                "expected Ok(GeometryOp::Shell) for shell with curated open_faces, \
                 got {:?}",
                other
            ),
        }
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a curated-faces shell_open must NOT emit EmptyEdgeSelection, \
             got: {:?}",
            diagnostics
        );
    }

    /// (b) Dedup + ordering: duplicate and reverse-order open_face handles
    /// are canonicalized to a sorted, deduped Vec<GeometryHandleId>.
    /// Mirrors the draft test's reverse-order / dedup case.
    #[test]
    fn compile_geometry_op_shell_open_dedup_and_order_canonical() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // h99, h3, h99 (duplicate), h15 in arbitrary order
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.001)),
                (
                    "open_faces".into(),
                    geometry_handle_list_literal(vec![
                        GeometryHandleId(99),
                        GeometryHandleId(3),
                        GeometryHandleId(99),
                        GeometryHandleId(15),
                    ]),
                ),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::Shell {
                open_face_handles, ..
            }) => {
                assert_eq!(
                    open_face_handles,
                    vec![
                        GeometryHandleId(3),
                        GeometryHandleId(15),
                        GeometryHandleId(99)
                    ],
                    "deduped+sorted open_face_handles expected [3, 15, 99], \
                     got {:?}",
                    open_face_handles
                );
            }
            other => panic!(
                "expected Ok(GeometryOp::Shell) for shell_open dedup/order \
                 test, got {:?}",
                other
            ),
        }
    }

    /// (c) ANTI-ZERO-FACES (γ step-5): a shell_open whose "open_faces" arg
    /// is PRESENT but evaluates to an empty List must NOT silently fall
    /// through to the all-faces path or produce an empty open_face_handles.
    /// `compile_geometry_op` must return `Err` and push exactly one
    /// diagnostic carrying `DiagnosticCode::EmptyEdgeSelection`.
    /// Mirrors `compile_geometry_op_draft_empty_face_selection_errors_with_code`.
    #[test]
    fn compile_geometry_op_shell_open_empty_selection_errors_with_code() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.001)),
                ("open_faces".into(), empty_list_literal()),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "a present open_faces selector resolving to zero faces must Err \
             (never silently shell all faces), got {:?}",
            result
        );
        let empty_sel: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.code == Some(reify_core::DiagnosticCode::EmptyEdgeSelection))
            .collect();
        assert_eq!(
            empty_sel.len(),
            1,
            "expected exactly one EmptyEdgeSelection diagnostic, \
             got diagnostics: {:?}",
            diagnostics
        );
    }

    /// (d) NON-LIST SELECTOR (γ amendment — reviewer suggestion 1): when the
    /// `open_faces` arg evaluates to a non-List value (e.g. `Value::Undef`,
    /// the state on the legacy P2 pipeline before the in-loop build-DAG driver
    /// lands), `compile_geometry_op` must:
    ///   - return `Err` with a USER-ACTIONABLE message naming the 3-arg call
    ///     form and pointing at the η/ε tasks (4360/4358);
    ///   - NOT push `DiagnosticCode::EmptyEdgeSelection` (a non-List value is
    ///     NOT an empty selection — that code is reserved for a PRESENT but
    ///     EMPTY `Value::List([])`; emitting it here would false-positive on
    ///     every legacy shell_open run).
    ///
    /// Mirrors `compile_geometry_op_draft_legacy_selector_unresolved_is_user_actionable`.
    #[test]
    fn compile_geometry_op_shell_open_non_list_selector_is_user_actionable_err() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // open_faces evaluates to `Value::Undef` — the legacy-pipeline state
        // where the selector has not yet resolved (the driver that turns a
        // faces_by_normal(...) expression into a Value::List lands in tasks
        // 4360/4358, DOWNSTREAM of γ).
        let unresolved_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Undef,
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Shell,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("thickness".into(), literal_length(0.001)),
                ("open_faces".into(), unresolved_selector),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "a non-List open_faces selector must Err (stays Undef for \
                 future in-loop resolution), got Ok({:?})",
                other
            ),
        };
        // User-actionable: names the 3-arg shell_open call form and points at
        // the numeric fallback and the η/ε tasks.
        assert!(
            msg.contains("shell_open(solid, thickness, open_faces)"),
            "diagnostic must name the 3-arg call form, got: {msg:?}"
        );
        assert!(
            msg.contains("4360") || msg.contains("4358"),
            "diagnostic must point at η/ε tasks (4360/4358), got: {msg:?}"
        );
        // A non-List value is NOT an empty selection — must NOT trip the
        // anti-zero guard or mislead callers into thinking zero faces resolved.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a non-List selector must NOT emit EmptyEdgeSelection \
             (non-List ≠ empty selection), got: {:?}",
            diagnostics
        );
    }

    // ── validate_pattern_count upper-bound tests ──────────────────────────────

    #[test]
    fn validate_pattern_count_rejects_huge_count() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // count=1e15 is way above the upper bound and should be rejected
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(1.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(1e15)),
                ("spacing".into(), literal_length(0.01)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "count=1e15 should return None, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && (d.message.contains("upper bound") || d.message.contains("exceeds"))
            }),
            "expected a Warning mentioning 'upper bound' or 'exceeds', got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn validate_pattern_count_boundary_100000_succeeds() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // count=100_000 is exactly at the upper bound and should be accepted
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(1.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(100_000.0)),
                ("spacing".into(), literal_length(0.01)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        match result {
            Ok(reify_ir::GeometryOp::LinearPattern { count, .. }) => {
                assert_eq!(count, 100_000, "count=100_000 should be accepted");
            }
            other => panic!(
                "expected Some(LinearPattern) for count=100_000, got {:?}",
                other
            ),
        }
        // No upper-bound diagnostic should be emitted for a valid boundary value
        assert!(
            !diagnostics
                .iter()
                .any(|d| { d.message.contains("upper bound") || d.message.contains("exceeds") }),
            "count=100_000 should not emit an upper-bound diagnostic, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn validate_pattern_count_boundary_100001_rejected() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // count=100_001 exceeds the upper bound by one and should be rejected
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(1.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(100_001.0)),
                ("spacing".into(), literal_length(0.01)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_err(),
            "count=100_001 should return None, got {:?}",
            result
        );
        assert!(
            diagnostics.iter().any(|d| {
                matches!(d.severity, reify_core::Severity::Warning)
                    && (d.message.contains("upper bound") || d.message.contains("exceeds"))
            }),
            "expected a Warning for count=100_001, got: {:?}",
            diagnostics
        );
    }

    /// Drives the `Result<GeometryOp, String>` API: a missing required arg must
    /// cause `compile_geometry_op` to return `Err(msg)` where `msg` names both
    /// the missing argument and the op kind, so callers can emit a specific
    /// Error diagnostic instead of a generic one.
    ///
    /// Uses Revolve missing `ox` as the representative case because Revolve has
    /// the most required f64 args (7) and `ox` is the last one resolved, making
    /// it easy to isolate without triggering other validation guards.
    #[test]
    fn compile_geometry_op_missing_arg_returns_err_with_arg_name() {
        let step_handles = vec![GeometryHandleId(1)];
        let values = ValueMap::new();

        // Revolve with all required args EXCEPT ox — drives the Result API.
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
                // "ox" deliberately omitted — drives Result<_, String> API
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        // The Result-typed API must return Err containing the arg name and op kind.
        // This assertion fails to compile with the current Option<_> return type.
        assert!(
            result.is_err(),
            "missing 'ox' should return Err, got: {:?}",
            result
        );
        let err_msg = result.unwrap_err();
        assert!(
            err_msg.contains("ox"),
            "error message should mention the missing arg 'ox', got: {:?}",
            err_msg
        );
        assert!(
            err_msg.contains("revolve"),
            "error message should mention the op kind 'revolve', got: {:?}",
            err_msg
        );
    }

    // -----------------------------------------------------------------
    // eval_named_arg_f64: non-numeric / non-finite value coverage
    // -----------------------------------------------------------------
    //
    // These close the gap left by existing coverage (which only exercises
    // numeric paths through compile_geometry_op): the three branches of
    // `match value.as_f64() { Some(v) if v.is_finite() => ..., _ => warn; None }`
    // must all emit a Warning diagnostic naming the arg and kind.

    #[test]
    fn eval_named_arg_f64_undef_value_returns_none_with_warning() {
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        // Value::Undef is the universal no-value sentinel — `as_f64()` returns None.
        let undef_expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Undef,
            reify_core::Type::dimensionless_scalar(),
        );
        let args = vec![("width".to_string(), undef_expr)];

        let result = eval_named_arg_f64(
            "width",
            reify_compiler::PrimitiveKind::Box,
            &args,
            &values,
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(result.is_none(), "Undef value should return None");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == reify_core::Severity::Warning
                    && d.message.contains("width")
                    && d.message.contains("box")
                    && d.message.contains("non-numeric/non-finite")),
            "expected Warning mentioning 'width', 'box', and 'non-numeric/non-finite', \
             got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn eval_named_arg_f64_nan_value_returns_none_with_warning() {
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let nan_expr = literal_f64(f64::NAN);
        let args = vec![("width".to_string(), nan_expr)];

        let result = eval_named_arg_f64(
            "width",
            reify_compiler::PrimitiveKind::Box,
            &args,
            &values,
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(result.is_none(), "NaN value should return None");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == reify_core::Severity::Warning
                    && d.message.contains("width")
                    && d.message.contains("box")
                    && d.message.contains("non-numeric/non-finite")),
            "expected Warning mentioning 'width', 'box', and 'non-numeric/non-finite', \
             got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn eval_named_arg_f64_infinity_value_returns_none_with_warning() {
        let values = ValueMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let inf_expr = literal_f64(f64::INFINITY);
        let args = vec![("width".to_string(), inf_expr)];

        let result = eval_named_arg_f64(
            "width",
            reify_compiler::PrimitiveKind::Box,
            &args,
            &values,
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(result.is_none(), "infinity should return None");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == reify_core::Severity::Warning
                    && d.message.contains("width")
                    && d.message.contains("box")
                    && d.message.contains("non-numeric/non-finite")),
            "expected Warning mentioning 'width', 'box', and 'non-numeric/non-finite', \
             got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ── named_steps / GeomRef::Sub resolution tests ───────────────────────────

    /// Happy path: compile_geometry_op resolves GeomRef::Sub("body") and
    /// GeomRef::Sub("hole") from the named_steps map and produces the correct
    /// Difference op.
    ///
    /// This test intentionally fails to compile until step-2 adds the
    /// `named_steps` parameter to `compile_geometry_op`.
    #[test]
    fn compile_geometry_op_sub_ref_resolved_via_named_steps() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};

        let handle_a = GeometryHandleId(10);
        let handle_b = GeometryHandleId(20);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".into(), kh(handle_a));
        named_steps.insert("hole".into(), kh(handle_b));

        let op = CompiledGeometryOp::Boolean {
            op: BooleanOp::Difference,
            left: GeomRef::Sub("body".into()),
            right: GeomRef::Sub("hole".into()),
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &ValueMap::new(),
            &[], // no step handles — Sub refs must resolve via named_steps
            &[],
            &HashMap::new(),
            &named_steps,
            &mut diagnostics,
        );

        let geom_op = result.expect("Sub refs with known names should resolve successfully");
        match geom_op {
            reify_ir::GeometryOp::Difference { left, right } => {
                assert_eq!(left, handle_a, "left should be body handle");
                assert_eq!(right, handle_b, "right should be hole handle");
            }
            other => panic!("expected Difference, got {:?}", other),
        }

        // No warnings should be emitted — named_steps lookup is silent-success
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Warning)
            .collect();
        assert!(
            warnings.is_empty(),
            "no Warning diagnostics expected for successful Sub resolution, got: {:?}",
            warnings
        );
    }

    /// Unknown-name error path: compile_geometry_op with GeomRef::Sub("unknown")
    /// and an empty named_steps map must return Err whose message contains
    /// "unresolvable GeomRef::Sub('unknown')", and MUST NOT push any
    /// Warning-severity diagnostics (regression guard against the old
    /// warning+last()-fallback behavior).
    #[test]
    fn compile_geometry_op_sub_ref_unknown_name_returns_err_no_warning() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};

        let op = CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Sub("unknown".into()),
            right: GeomRef::Step(0),
        };

        let step_handles = vec![GeometryHandleId(5)];
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new(); // empty — "unknown" not present

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &ValueMap::new(),
            &step_handles,
            &[],
            &HashMap::new(),
            &named_steps,
            &mut diagnostics,
        );

        // Must return Err (not Ok with a fabricated default)
        let err_msg = result.expect_err("Sub ref with unknown name should return Err");
        assert!(
            err_msg.contains("unresolvable GeomRef::Sub('unknown')"),
            "error message should contain \"unresolvable GeomRef::Sub('unknown')\", got: {:?}",
            err_msg
        );

        // Must NOT emit any Warning-severity diagnostic — the old fallback
        // emitted a Warning before returning the last handle; that pattern is
        // explicitly forbidden by the feedback_silent_defaults_pattern norm.
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Warning)
            .collect();
        assert!(
            warnings.is_empty(),
            "no Warning diagnostics expected on unknown-name Sub resolution, got: {:?}",
            warnings
        );
    }

    /// Contract test (task 4142, Cluster A RED): compile_geometry_op resolves
    /// GeomRef::Sub via `KernelHandle.id`, ignoring `KernelHandle.kernel`.
    ///
    /// Uses deliberately non-default `KernelId` values (Manifold for "body",
    /// Fidget for "hole") to prove the GeomRef::Sub arm (geometry_ops.rs:368)
    /// keys only off `.id` and never consults `.kernel`.
    ///
    /// RED on current main: `compile_geometry_op` still takes
    /// `&HashMap<String, GeometryHandleId>`, so passing a
    /// `HashMap<String, KernelHandle>` causes a compile-time type mismatch.
    /// GREEN after step-2: signature changed + leaf projection updated.
    ///
    /// NOTE: Pins the leaf-projection contract only (`.kernel` unused in the current
    /// single-kernel-per-build design). When cross-kernel handle resolution lands,
    /// update to assert per-kernel dispatch rather than treating `.kernel` as ignored.
    #[test]
    fn compile_geometry_op_sub_ref_resolves_via_kernel_handle_id_ignoring_kernel_field() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body".into(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Manifold, // deliberately non-default
                id: GeometryHandleId(10),
            },
        );
        named_steps.insert(
            "hole".into(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Fidget, // deliberately non-default
                id: GeometryHandleId(20),
            },
        );

        let op = CompiledGeometryOp::Boolean {
            op: BooleanOp::Difference,
            left: GeomRef::Sub("body".into()),
            right: GeomRef::Sub("hole".into()),
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &ValueMap::new(),
            &[], // no step handles — Sub refs resolve via named_steps
            &[],
            &HashMap::new(),
            &named_steps,
            &mut diagnostics,
        );

        let geom_op =
            result.expect("Sub refs with known KernelHandle values should resolve successfully");
        match geom_op {
            reify_ir::GeometryOp::Difference { left, right } => {
                assert_eq!(
                    left,
                    GeometryHandleId(10),
                    "left must be body's .id (10), not influenced by .kernel (Manifold)"
                );
                assert_eq!(
                    right,
                    GeometryHandleId(20),
                    "right must be hole's .id (20), not influenced by .kernel (Fidget)"
                );
            }
            other => panic!("expected Difference, got {:?}", other),
        }

        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for successful KernelHandle Sub resolution, \
             got: {:?}",
            diagnostics
        );
    }

    // ── rewrite_geometry_queries FunctionCall-args recursion (task 4358 ε) ───
    //
    // Pins that `rewrite_geometry_queries` recurses into the ARGUMENTS of a
    // non-query outer FunctionCall, folding each inner geometry-query leaf to a
    // `Literal` while leaving the outer call's identity (function name + arity +
    // result type) intact. Before step-2 the `_ => expr.clone()` fallthrough
    // returns the outer call verbatim, so the inner `bounding_box(...)` leaves
    // never fold — the bug behind `fits_build_volume(bounding_box(..),
    // bounding_box(..))` constraints folding to Undef.

    /// Build a single-arg geometry-query call `<name>(<entity>.<member>)` whose
    /// sole arg is a `ValueRef`. Mirrors `conformance_call`'s manual
    /// `FunctionCall` construction (no public `function_call` constructor).
    fn geom_query_call(
        name: &str,
        entity: &str,
        member: &str,
        result_type: reify_core::Type,
    ) -> reify_ir::CompiledExpr {
        let arg = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member),
            reify_core::Type::Geometry,
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(name));
        content_hash = content_hash.combine(arg.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: name.to_string(),
                    qualified_name: name.to_string(),
                },
                args: vec![arg],
            },
            result_type,
            content_hash,
        }
    }

    /// Build an N-arg outer FunctionCall `<name>(args...)`.
    fn outer_function_call(
        name: &str,
        args: Vec<reify_ir::CompiledExpr>,
        result_type: reify_core::Type,
    ) -> reify_ir::CompiledExpr {
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(name));
        for a in &args {
            content_hash = content_hash.combine(a.content_hash);
        }
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: name.to_string(),
                    qualified_name: name.to_string(),
                },
                args,
            },
            result_type,
            content_hash,
        }
    }

    /// RED until step-2: `rewrite_geometry_queries` over a NON-query outer call
    /// `fits_build_volume(bounding_box(S.part), bounding_box(S.envelope))` must
    /// preserve the outer call (name + arity) but fold each inner
    /// `bounding_box(..)` leaf to a `Literal(Value::BoundingBox{..})`. Today the
    /// `_ => expr.clone()` arm returns the outer call verbatim (args still
    /// `FunctionCall` query nodes), so the per-arg `Literal(BoundingBox)`
    /// assertion fails.
    #[test]
    fn rewrite_geometry_queries_folds_function_call_args() {
        use reify_test_support::mocks::MockGeometryKernel;

        // Two handles, each answered with a valid bbox JSON wire reply
        // (`dispatch_bounding_box` → `parse_bbox_axis_extents` expects a
        // `Value::String` of `{"xmin":..,..,"zmax":..}`).
        let h1 = reify_ir::GeometryHandleId(11);
        let h2 = reify_ir::GeometryHandleId(22);
        let bbox_json_1 = reify_ir::Value::String(
            "{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":0.0,\"xmax\":0.01,\"ymax\":0.02,\"zmax\":0.03}"
                .to_string(),
        );
        let bbox_json_2 = reify_ir::Value::String(
            "{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":0.0,\"xmax\":0.1,\"ymax\":0.2,\"zmax\":0.3}"
                .to_string(),
        );
        let kernel = MockGeometryKernel::new()
            .with_bbox_result(h1, bbox_json_1)
            .with_bbox_result(h2, bbox_json_2);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("part".to_string(), kh(h1));
        named_steps.insert("envelope".to_string(), kh(h2));

        // Outer NON-query call: fits_build_volume(bounding_box(S.part),
        // bounding_box(S.envelope)).
        let arg1 = geom_query_call("bounding_box", "S", "part", reify_core::Type::Geometry);
        let arg2 = geom_query_call("bounding_box", "S", "envelope", reify_core::Type::Geometry);
        let outer = outer_function_call(
            "fits_build_volume",
            vec![arg1, arg2],
            reify_core::Type::Bool,
        );

        let mut diags: Vec<Diagnostic> = Vec::new();
        let rewritten = rewrite_geometry_queries(&outer, &named_steps, &kernel, &mut diags);

        match &rewritten.kind {
            reify_ir::CompiledExprKind::FunctionCall { function, args } => {
                assert_eq!(
                    function.name, "fits_build_volume",
                    "outer non-query call name must be preserved"
                );
                assert_eq!(args.len(), 2, "outer call arity must be preserved");
                for (i, arg) in args.iter().enumerate() {
                    match &arg.kind {
                        reify_ir::CompiledExprKind::Literal(reify_ir::Value::BoundingBox {
                            ..
                        }) => {}
                        other => panic!(
                            "arg {i} must fold to Literal(Value::BoundingBox); got {other:?}"
                        ),
                    }
                }
            }
            other => panic!("expected outer FunctionCall preserved, got {other:?}"),
        }
    }

    // ── resolve_geometry_handle_arg cross-sub resolution (task 4358 ε) ────────
    //
    // Pins that `resolve_geometry_handle_arg` resolves the cross-sub
    // `proc.build_volume` geometry-handle arg. Per the CORRECTED esc-4358-124
    // premise, that arg lowers (via try_resolve_cross_sub_geometry_value_ref,
    // reify-compiler/src/expr.rs) to one of two shapes, BOTH carrying a scoped
    // `<parent>.<sub>` entity stamp:
    //   * `CrossSubGeometryRef(ValueCellId)` — a genuine child realization.
    //   * a forward-declared scoped `ValueRef(ValueCellId)`.
    // Either way the live handle is keyed in `named_steps` under the composed
    // `<sub>.<member>` key that `seed_cross_sub_named_steps` stamps
    // ("proc.build_volume", engine_build.rs) — NOT the bare member. The arm must
    // reconstruct that composed key for any dotted-entity id while still
    // resolving a plain same-template `ValueRef` via its bare member and
    // returning None for a missing key.
    //
    // RED until step-4: resolve_geometry_handle_arg matches ONLY ValueRef and
    // looks up named_steps by the BARE member ("build_volume"), so the dotted
    // cross-sub entity misses the "proc.build_volume" key → None (shape b), and
    // the CrossSubGeometryRef shape isn't matched at all → None (shape a).
    #[test]
    fn resolve_geometry_handle_arg_resolves_cross_sub_composed_key() {
        let cross_handle = reify_ir::GeometryHandleId(91);
        let bare_handle = reify_ir::GeometryHandleId(7);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // Cross-sub handle keyed by the composed "<sub>.<member>" key, exactly as
        // seed_cross_sub_named_steps stamps it (engine_build.rs).
        named_steps.insert("proc.build_volume".to_string(), kh(cross_handle));
        // Same-template let-bound geometry keyed by its bare member.
        named_steps.insert("part".to_string(), kh(bare_handle));

        // The scoped ValueCellId both cross-sub shapes carry: entity
        // "<parent>.<sub>" ("Parent.proc"), member "build_volume".
        let scoped_id = reify_core::ValueCellId::new("Parent.proc", "build_volume");

        // (a) CrossSubGeometryRef shape (genuine child realization).
        let cross_ref = reify_ir::CompiledExpr::cross_sub_geometry_ref(
            scoped_id.clone(),
            reify_core::Type::Geometry,
        );
        assert_eq!(
            resolve_geometry_handle_arg(&cross_ref, &named_steps),
            Some(cross_handle),
            "CrossSubGeometryRef(Parent.proc.build_volume) must resolve via the \
             composed \"proc.build_volume\" named_steps key"
        );

        // (b) forward-declared scoped ValueRef shape (same scoped id).
        let fwd_ref = reify_ir::CompiledExpr::value_ref(scoped_id, reify_core::Type::Geometry);
        assert_eq!(
            resolve_geometry_handle_arg(&fwd_ref, &named_steps),
            Some(cross_handle),
            "forward-declared scoped ValueRef(Parent.proc.build_volume) must also \
             resolve via the composed \"proc.build_volume\" key"
        );

        // (c) regression: a plain same-template ValueRef (dot-free entity)
        // still resolves via its bare member.
        let plain_ref = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new("S", "part"),
            reify_core::Type::Geometry,
        );
        assert_eq!(
            resolve_geometry_handle_arg(&plain_ref, &named_steps),
            Some(bare_handle),
            "plain ValueRef(S.part) must still resolve via its bare member \"part\""
        );

        // (d) regression: a missing key returns None.
        let missing_ref = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new("S", "absent"),
            reify_core::Type::Geometry,
        );
        assert_eq!(
            resolve_geometry_handle_arg(&missing_ref, &named_steps),
            None,
            "a ValueRef whose member is absent from named_steps must return None"
        );
    }

    // ── try_eval_conformance_query unit tests (task 2320) ────────────────────
    //
    // These tests pin the contract of `try_eval_conformance_query`, the
    // kernel-aware eval-time dispatch surface for the `is_watertight`,
    // `is_manifold`, `is_orientable` stdlib helpers. Architecture rationale
    // is captured in the task 2320 plan; the function lives in this module
    // (rather than `eval_expr`) because the build pipeline owns both the
    // kernel and the per-realization name → handle map (`named_steps`).

    /// Build a `CompiledExpr` for `is_watertight(<entity>.<member>)`.
    fn conformance_call(helper_name: &str, entity: &str, member: &str) -> reify_ir::CompiledExpr {
        let arg = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member),
            reify_core::Type::Geometry,
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg],
            },
            result_type: reify_core::Type::Bool,
            content_hash,
        }
    }

    #[test]
    fn try_eval_conformance_query_kernel_reply_true() {
        use reify_test_support::mocks::MockGeometryKernel;
        let handle_id = reify_ir::GeometryHandleId(7);
        let kernel =
            MockGeometryKernel::new().with_query_result(handle_id, reify_ir::Value::Bool(true));

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "is_watertight(body) with kernel returning Bool(true) must produce Some(Bool(true))"
        );
    }

    /// Build a `CompiledExpr` for `is_watertight(<literal_real>)`.
    fn conformance_call_literal_arg(helper_name: &str) -> reify_ir::CompiledExpr {
        let arg = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(1.0),
            reify_core::Type::dimensionless_scalar(),
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg],
            },
            result_type: reify_core::Type::Bool,
            content_hash,
        }
    }

    #[test]
    fn try_eval_conformance_query_non_helper_name_returns_none_no_kernel_call() {
        let handle_id = reify_ir::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        // `volume` is a real stdlib function name but NOT one of the three
        // recognised conformance helpers. The dispatch must return None.
        let expr = conformance_call("volume", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert!(
            result.is_none(),
            "non-helper name 'volume' must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-helper names"
        );
    }

    #[test]
    fn try_eval_conformance_query_literal_arg_returns_none_no_kernel_call() {
        let handle_id = reify_ir::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();

        // `is_watertight(1.0)` — recognised helper name but the arg is a
        // literal, not a `ValueRef`. The dispatch must return None *and*
        // never consult the kernel.
        let expr = conformance_call_literal_arg("is_watertight");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert!(
            result.is_none(),
            "is_watertight(<literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_watertight_short_circuits() {
        // Kernel is configured to return Bool(false) — but the structure
        // declares `: Watertight`, so the dispatch must short-circuit to
        // Bool(true) WITHOUT consulting the kernel.
        let handle_id = reify_ir::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_watertight", "TrustedShell", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Watertight".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "user-asserted Watertight must override kernel reply"
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted when the structure asserts Watertight"
        );
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_manifold_short_circuits() {
        let handle_id = reify_ir::GeometryHandleId(11);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_manifold", "TrustedShell", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Manifold".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Bool(true)));
        assert_eq!(kernel.total_query_count(), 0);
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_orientable_short_circuits() {
        let handle_id = reify_ir::GeometryHandleId(13);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_orientable", "TrustedShell", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Orientable".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Bool(true)));
        assert_eq!(kernel.total_query_count(), 0);
    }

    // ── θ conformance predicates (task #4171): escape-hatch + kernel-reply ──

    /// Escape-hatch: `is_closed` on a structure that declares `: Closed`
    /// must return `Bool(true)` WITHOUT consulting the kernel (total_query_count == 0).
    /// RED until step-6 adds "is_closed" => "Closed" to try_eval_conformance_query.
    #[test]
    fn try_eval_conformance_query_user_assertion_closed_short_circuits_is_closed() {
        let handle_id = reify_ir::GeometryHandleId(21);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_closed", "ClosedShell", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Closed".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Bool(true)), "user-asserted Closed must override kernel reply");
        assert_eq!(kernel.total_query_count(), 0, "kernel must NOT be consulted when structure asserts Closed");
    }

    /// Kernel-reply: `is_closed` with no matching marker trait must delegate to
    /// the kernel and return its Bool reply.
    /// RED until step-6 adds "is_closed" to try_eval_conformance_query.
    #[test]
    fn try_eval_conformance_query_is_closed_kernel_reply_propagates() {
        let handle_id = reify_ir::GeometryHandleId(22);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_closed", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(result, Some(reify_ir::Value::Bool(false)), "is_closed kernel reply Bool(false) must propagate");
        assert_eq!(kernel.total_query_count(), 1, "kernel must be consulted exactly once");
    }

    /// Escape-hatch: `is_connected` on a structure that declares `: Connected`
    /// must return `Bool(true)` WITHOUT consulting the kernel.
    /// RED until step-6 adds "is_connected" => "Connected" to try_eval_conformance_query.
    #[test]
    fn try_eval_conformance_query_user_assertion_connected_short_circuits_is_connected() {
        let handle_id = reify_ir::GeometryHandleId(23);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_connected", "ConnectedPart", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Connected".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Bool(true)), "user-asserted Connected must override kernel reply");
        assert_eq!(kernel.total_query_count(), 0, "kernel must NOT be consulted when structure asserts Connected");
    }

    /// Kernel-reply: `is_connected` with no matching marker trait must delegate to
    /// the kernel and return its Bool reply.
    /// RED until step-6 adds "is_connected" to try_eval_conformance_query.
    #[test]
    fn try_eval_conformance_query_is_connected_kernel_reply_propagates() {
        let handle_id = reify_ir::GeometryHandleId(24);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_connected", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(result, Some(reify_ir::Value::Bool(true)), "is_connected kernel reply Bool(true) must propagate");
        assert_eq!(kernel.total_query_count(), 1, "kernel must be consulted exactly once");
    }

    /// Escape-hatch: `is_bounded` on a structure that declares `: Bounded`
    /// must return `Bool(true)` WITHOUT consulting the kernel.
    /// RED until step-6 adds "is_bounded" => "Bounded" to try_eval_conformance_query.
    #[test]
    fn try_eval_conformance_query_user_assertion_bounded_short_circuits_is_bounded() {
        let handle_id = reify_ir::GeometryHandleId(25);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_bounded", "BoundedPart", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Bounded".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Bool(true)), "user-asserted Bounded must override kernel reply");
        assert_eq!(kernel.total_query_count(), 0, "kernel must NOT be consulted when structure asserts Bounded");
    }

    /// Kernel-reply: `is_bounded` with no matching marker trait must delegate to
    /// the kernel and return its Bool reply.
    /// RED until step-6 adds "is_bounded" to try_eval_conformance_query.
    #[test]
    fn try_eval_conformance_query_is_bounded_kernel_reply_propagates() {
        let handle_id = reify_ir::GeometryHandleId(26);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_bounded", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(result, Some(reify_ir::Value::Bool(false)), "is_bounded kernel reply Bool(false) must propagate");
        assert_eq!(kernel.total_query_count(), 1, "kernel must be consulted exactly once");
    }

    #[test]
    fn try_eval_conformance_query_user_assertion_closed_does_not_short_circuit_is_watertight() {
        // Asymmetry per task 2320 design decision: `is_watertight` short-
        // circuits ONLY on `Watertight` — declaring the (refined) `Closed`
        // bound is not sufficient. The kernel must be consulted and its
        // Bool(false) reply honoured.
        let handle_id = reify_ir::GeometryHandleId(17);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(false));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_conformance_query(
            &expr,
            &["Closed".to_string()],
            &named_steps,
            &kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(false)),
            "is_watertight must NOT be short-circuited by ': Closed'"
        );
        assert_eq!(
            kernel.total_query_count(),
            1,
            "kernel must be consulted exactly once when no matching marker trait is declared"
        );
    }

    #[test]
    fn try_eval_conformance_query_unresolvable_member_returns_none_no_kernel_call() {
        let handle_id = reify_ir::GeometryHandleId(7);
        let inner = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Bool(true));
        let kernel = reify_test_support::mocks::CountingMockKernel::new(inner);

        // `named_steps` contains "body" but the call references "ghost",
        // which is not present. The dispatch must return None and never
        // consult the kernel.
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_watertight", "Bracket", "ghost");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert!(
            result.is_none(),
            "unresolvable cell-member 'ghost' must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted when the cell-member name is absent"
        );
    }

    /// Failure-mode contract (amend, task 2320): when the kernel returns
    /// `Ok(value)` with a non-`Bool` value (e.g. a stray `Value::Real`),
    /// `try_eval_conformance_query` must defensively downgrade to
    /// `Some(Value::Undef)` and emit exactly one Warning diagnostic naming
    /// the helper. Pins the `Ok(other)` arm in the source so a regression
    /// that swaps the downgrade for a panic (or drops the diagnostic)
    /// would be caught.
    #[test]
    fn try_eval_conformance_query_kernel_returns_non_bool_downgrades_with_warning() {
        let handle_id = reify_ir::GeometryHandleId(23);
        // Seed a non-Bool kernel reply for the IsWatertight query.
        let kernel = reify_test_support::mocks::MockGeometryKernel::new()
            .with_query_result(handle_id, reify_ir::Value::Real(1.0));

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "non-Bool kernel reply must downgrade to Some(Value::Undef), got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Bool kernel reply must emit exactly one diagnostic, got {}",
            diagnostics.len()
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "non-Bool kernel reply must emit a Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("is_watertight"),
            "diagnostic must mention the helper name, got: {}",
            diag.message
        );
    }

    /// Failure-mode contract (amend, task 2320): when the kernel returns
    /// `Err(QueryError)`, `try_eval_conformance_query` must defensively
    /// downgrade to `Some(Value::Undef)` and emit exactly one Warning
    /// diagnostic naming the helper and surfacing the error message. Pins
    /// the `Err(err)` arm so a regression swapping the downgrade for a
    /// panic (or losing the error context in the diagnostic) would fail.
    #[test]
    fn try_eval_conformance_query_kernel_query_error_downgrades_with_warning() {
        let handle_id = reify_ir::GeometryHandleId(29);
        // No `with_query_result` seeding → MockGeometryKernel.query() returns
        // `Err(QueryError::QueryFailed("no mock result for …"))` for any handle.
        let kernel = reify_test_support::mocks::MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(handle_id));

        let expr = conformance_call("is_manifold", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "kernel Err must downgrade to Some(Value::Undef), got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one diagnostic, got {}",
            diagnostics.len()
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "kernel Err must emit a Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("is_manifold"),
            "diagnostic must mention the helper name, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("kernel query failed"),
            "diagnostic must indicate the kernel failure, got: {}",
            diag.message
        );
    }

    // ── task 4142 Cluster B RED contract tests ───────────────────────────────
    //
    // These three tests pin the "resolve via KernelHandle.id, ignore
    // KernelHandle.kernel" contract on the three remaining leaf families.
    // They fail to compile on current main (Cluster B helpers still take
    // `&HashMap<String, GeometryHandleId>`), and go GREEN when step-4 lands.

    /// Contract test (task 4142, Cluster B RED — conformance leaf):
    /// `try_eval_conformance_query` resolves the geometry handle via
    /// `KernelHandle.id`, ignoring `KernelHandle.kernel`.
    ///
    /// Uses `KernelId::Manifold` (non-default) to prove the leaf at
    /// geometry_ops.rs:1399 keys only off `.id`.
    ///
    /// RED on current main: `try_eval_conformance_query` still takes
    /// `&HashMap<String, GeometryHandleId>` → E0308 type mismatch.
    ///
    /// NOTE: Pins the leaf-projection contract only (`.kernel` unused in the current
    /// single-kernel-per-build design). When cross-kernel handle resolution lands,
    /// update to assert per-kernel dispatch rather than treating `.kernel` as ignored.
    #[test]
    fn try_eval_conformance_query_resolves_via_kernel_handle_id() {
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_id = reify_ir::GeometryHandleId(7);
        let kernel =
            MockGeometryKernel::new().with_query_result(handle_id, reify_ir::Value::Bool(true));

        // Map "body" to a KernelHandle with deliberately non-default kernel.
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Manifold, // non-default: must be ignored
                id: handle_id,
            },
        );

        let expr = conformance_call("is_watertight", "Bracket", "body");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result =
            super::try_eval_conformance_query(&expr, &[], &named_steps, &kernel, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "is_watertight with KernelHandle{{Manifold, 7}} must produce Some(Bool(true)); \
             kernel was keyed on .id (7), not .kernel",
        );
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for successful conformance resolution, got: {:?}",
            diagnostics
        );
    }

    /// Contract test (task 4142, Cluster B RED — kinematic leaf):
    /// `try_eval_kinematic_query` resolves solid names via `KernelHandle.id`,
    /// ignoring `KernelHandle.kernel`.
    ///
    /// Uses `KernelId::Manifold`/"base" and `KernelId::Fidget`/"hole" (both
    /// non-default) to prove the leaf at geometry_ops.rs:1909 keys only off
    /// `.id`.
    ///
    /// RED on current main: `try_eval_kinematic_query` still takes
    /// `&HashMap<String, GeometryHandleId>` → E0308 type mismatch.
    ///
    /// NOTE: Pins the leaf-projection contract only (`.kernel` unused in the current
    /// single-kernel-per-build design). When cross-kernel handle resolution lands,
    /// update to assert per-kernel dispatch rather than treating `.kernel` as ignored.
    #[test]
    fn try_eval_kinematic_query_resolves_via_kernel_handle_id() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let base_id = reify_ir::GeometryHandleId(10);
        let hole_id = reify_ir::GeometryHandleId(20);

        // Distance <= 0.0 → interference.
        let mut kernel = MockGeometryKernel::new().with_distance_result(
            base_id,
            hole_id,
            reify_ir::Value::Real(-1.0),
        );

        // Map solid names to KernelHandle with deliberately non-default kernels.
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "base".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Manifold, // non-default
                id: base_id,
            },
        );
        named_steps.insert(
            "hole".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Fidget, // non-default
                id: hole_id,
            },
        );

        // Build a Snapshot value: { kind: "snapshot", bodies: [{id:1, solid:"base"}, {id:2, solid:"hole"}] }
        let make_body = |id: i64, solid: &str| -> reify_ir::Value {
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                reify_ir::Value::String("id".to_string()),
                reify_ir::Value::Int(id),
            );
            m.insert(
                reify_ir::Value::String("solid".to_string()),
                reify_ir::Value::String(solid.to_string()),
            );
            reify_ir::Value::Map(m)
        };
        let mut snap_map = std::collections::BTreeMap::new();
        snap_map.insert(
            reify_ir::Value::String("kind".to_string()),
            reify_ir::Value::String("snapshot".to_string()),
        );
        snap_map.insert(
            reify_ir::Value::String("bodies".to_string()),
            reify_ir::Value::List(vec![make_body(1, "base"), make_body(2, "hole")]),
        );
        let snapshot = reify_ir::Value::Map(snap_map);

        let snap_cell = ValueCellId::new("Mech", "snap");
        let snap_arg = reify_ir::CompiledExpr::value_ref(snap_cell.clone(), Type::Geometry);
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("interferes"))
            .combine(snap_arg.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "interferes".to_string(),
                    qualified_name: "interferes".to_string(),
                },
                args: vec![snap_arg],
            },
            result_type: Type::List(Box::new(Type::Map(
                Box::new(Type::String),
                Box::new(Type::Int),
            ))),
            content_hash,
        };

        let mut values = reify_ir::ValueMap::new();
        values.insert(snap_cell, snapshot);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        // Distance(base_id=10, hole_id=20) = -1.0 ≤ 0.0 → the pair (1,2) interferes.
        // Result must be Some(List([{a:1, b:2}])).
        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "interferes with overlapping bodies must return Some(List([..])), \
                 got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "exactly one interfering pair expected, got {} entries: {:?}",
            list.len(),
            list
        );
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for successful kinematic resolution, got: {:?}",
            diagnostics
        );
    }

    /// Contract test (task 3906 T8, step-1 RED): `try_eval_kinematic_query` applies each
    /// body's `world_transform` via `kernel.execute(ApplyTransform{…})` before the pairwise
    /// `Distance` probe, so FK-posed geometry is what actually determines interference.
    ///
    /// Fixture: two bodies whose SOURCE handles are disjoint (distance 5.0 > 0) but whose
    /// FK-POSED handles interfere (distance -1.0 ≤ 0). Each body carries a NON-identity
    /// `world_transform` (pure translation: body_a +10mm, body_b +15mm along X).
    ///
    /// After step-2's impl:
    ///   (a) result is `Some(List([{a:1, b:2}]))` — posed geometry interferes.
    ///   (b) `kernel.operations()` has exactly 2 `ApplyTransform` records whose
    ///       `(target, rotation, translation)` match each body's source handle and
    ///       `decompose_transform_to_arrays` output.
    ///
    /// RED on main: no ApplyTransform ops emitted → probe uses source handles (10, 20)
    /// → distance 5.0 > 0 → empty list; both (a) and (b) fail.
    #[test]
    fn try_eval_kinematic_query_applies_world_transform_before_distance() {
        use reify_core::{Type, ValueCellId};
        use reify_ir::GeometryOp;
        use reify_test_support::mocks::MockGeometryKernel;

        let src_a = reify_ir::GeometryHandleId(10);
        let src_b = reify_ir::GeometryHandleId(20);
        // MockGeometryKernel::new() initialises next_id = 1 with no prior
        // operations (see mocks.rs: `next_id: 1`). Each execute() call
        // auto-increments: first call → GeometryHandleId(1), second → (2).
        // This test issues no other execute() calls before try_eval_kinematic_query,
        // so body A's ApplyTransform → id 1, body B's → id 2. If the mock ever
        // pre-allocates a handle or changes its seed, update these constants and
        // the with_distance_result fixture below to match.
        let posed_a = reify_ir::GeometryHandleId(1);
        let posed_b = reify_ir::GeometryHandleId(2);

        // Source handles disjoint; posed handles interfere.
        let mut kernel = MockGeometryKernel::new()
            .with_distance_result(src_a, src_b, reify_ir::Value::Real(5.0))
            .with_distance_result(posed_a, posed_b, reify_ir::Value::Real(-1.0));

        // Non-identity world_transforms: pure translations along X.
        // Both are non-identity (translation != [0,0,0]).
        let tx_a = 0.010_f64; // 10 mm
        let tx_b = 0.015_f64; // 15 mm

        let make_transform = |tx: f64| -> reify_ir::Value {
            reify_ir::Value::Transform {
                rotation: Box::new(reify_ir::Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                translation: Box::new(reify_ir::Value::Vector(vec![
                    reify_ir::Value::length(tx),
                    reify_ir::Value::length(0.0),
                    reify_ir::Value::length(0.0),
                ])),
            }
        };

        // Map solid names to KernelHandle.
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body_a".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_a,
            },
        );
        named_steps.insert(
            "body_b".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_b,
            },
        );

        // Snapshot with body records that carry a `world_transform` key.
        let make_body = |id: i64, solid: &str, wt: reify_ir::Value| -> reify_ir::Value {
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                reify_ir::Value::String("id".to_string()),
                reify_ir::Value::Int(id),
            );
            m.insert(
                reify_ir::Value::String("solid".to_string()),
                reify_ir::Value::String(solid.to_string()),
            );
            m.insert(reify_ir::Value::String("world_transform".to_string()), wt);
            reify_ir::Value::Map(m)
        };
        let mut snap_map = std::collections::BTreeMap::new();
        snap_map.insert(
            reify_ir::Value::String("kind".to_string()),
            reify_ir::Value::String("snapshot".to_string()),
        );
        snap_map.insert(
            reify_ir::Value::String("bodies".to_string()),
            reify_ir::Value::List(vec![
                make_body(1, "body_a", make_transform(tx_a)),
                make_body(2, "body_b", make_transform(tx_b)),
            ]),
        );
        let snapshot = reify_ir::Value::Map(snap_map);

        // Build interferes(s) call expr.
        let snap_cell = ValueCellId::new("Mech", "snap");
        let snap_arg = reify_ir::CompiledExpr::value_ref(snap_cell.clone(), Type::Geometry);
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("interferes"))
            .combine(snap_arg.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "interferes".to_string(),
                    qualified_name: "interferes".to_string(),
                },
                args: vec![snap_arg],
            },
            result_type: Type::List(Box::new(Type::Map(
                Box::new(Type::String),
                Box::new(Type::Int),
            ))),
            content_hash,
        };

        let mut values = reify_ir::ValueMap::new();
        values.insert(snap_cell, snapshot);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        // (a) Posed geometry interferes — exactly one pair {a:1, b:2} (body ids, not handle ids).
        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "FK-posed interfering bodies must return Some(List([..])), \
                 got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "exactly one interfering pair expected from FK-posed geometry, got {} entries: {:?}",
            list.len(),
            list
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            diagnostics
        );

        // (b) Exactly two ApplyTransform ops (one per non-identity body).
        let ops = kernel.operations();
        let apply_ops: Vec<_> = ops
            .iter()
            .filter(|rec| matches!(&rec.op, GeometryOp::ApplyTransform { .. }))
            .collect();
        assert_eq!(
            apply_ops.len(),
            2,
            "expected exactly 2 ApplyTransform ops (one per body), got {}: {:?}",
            apply_ops.len(),
            apply_ops
        );
        // body_a: first op targets src_a with its decomposed transform.
        match &apply_ops[0].op {
            GeometryOp::ApplyTransform {
                target,
                rotation,
                translation,
            } => {
                assert_eq!(
                    *target, src_a,
                    "first ApplyTransform must target body_a source handle"
                );
                assert_eq!(
                    *rotation,
                    [1.0_f64, 0.0, 0.0, 0.0],
                    "body_a rotation must be identity quaternion"
                );
                assert!(
                    (translation[0] - tx_a).abs() < 1e-12,
                    "body_a tx[0]: expected {tx_a}, got {}",
                    translation[0]
                );
                assert_eq!(translation[1], 0.0, "body_a tx[1] must be zero");
                assert_eq!(translation[2], 0.0, "body_a tx[2] must be zero");
            }
            other => panic!("expected ApplyTransform, got {:?}", other),
        }
        // body_b: second op targets src_b with its decomposed transform.
        match &apply_ops[1].op {
            GeometryOp::ApplyTransform {
                target,
                rotation,
                translation,
            } => {
                assert_eq!(
                    *target, src_b,
                    "second ApplyTransform must target body_b source handle"
                );
                assert_eq!(
                    *rotation,
                    [1.0_f64, 0.0, 0.0, 0.0],
                    "body_b rotation must be identity quaternion"
                );
                assert!(
                    (translation[0] - tx_b).abs() < 1e-12,
                    "body_b tx[0]: expected {tx_b}, got {}",
                    translation[0]
                );
                assert_eq!(translation[1], 0.0, "body_b tx[1] must be zero");
                assert_eq!(translation[2], 0.0, "body_b tx[2] must be zero");
            }
            other => panic!("expected ApplyTransform, got {:?}", other),
        }
    }

    /// Contract test (task 3906 T8, step-3 RED): bodies with an IDENTITY `world_transform`
    /// must NOT emit an `ApplyTransform` op — only bodies with a non-identity transform do.
    ///
    /// Fixture: body A has an identity world_transform (rotation [1,0,0,0], translation
    /// [0,0,0]); body B has a non-identity world_transform (translation +20mm along X).
    /// After step-4's identity short-circuit, only body B gets an ApplyTransform op.
    ///
    /// RED states:
    ///   - on main (before step-2): ZERO ApplyTransform ops (≠ 1)
    ///   - after step-2's apply-unconditionally impl: TWO ops (identity applied too, ≠ 1)
    ///
    /// Either way "exactly one" fails until the short-circuit lands in step-4.
    #[test]
    fn try_eval_kinematic_query_skips_identity_world_transform() {
        use reify_core::{Type, ValueCellId};
        use reify_ir::GeometryOp;
        use reify_test_support::mocks::MockGeometryKernel;

        let src_a = reify_ir::GeometryHandleId(100);
        let src_b = reify_ir::GeometryHandleId(200);
        // Only body B (non-identity) gets an ApplyTransform; body A (identity)
        // stays at its raw handle. MockGeometryKernel::new() initialises
        // next_id = 1, and no execute() calls precede try_eval_kinematic_query
        // in this test, so body B's single ApplyTransform → GeometryHandleId(1).
        // If the mock ever changes its seeding, update posed_b accordingly.
        let posed_b = reify_ir::GeometryHandleId(1);

        // Probe (src_a=100, posed_b=1) interferes; body A stays at raw handle.
        let mut kernel = MockGeometryKernel::new().with_distance_result(
            src_a,
            posed_b,
            reify_ir::Value::Real(-1.0),
        );

        let make_transform = |tx: f64, ty: f64, tz: f64| -> reify_ir::Value {
            reify_ir::Value::Transform {
                rotation: Box::new(reify_ir::Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                translation: Box::new(reify_ir::Value::Vector(vec![
                    reify_ir::Value::length(tx),
                    reify_ir::Value::length(ty),
                    reify_ir::Value::length(tz),
                ])),
            }
        };

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body_a".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_a,
            },
        );
        named_steps.insert(
            "body_b".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_b,
            },
        );

        let make_body = |id: i64, solid: &str, wt: reify_ir::Value| -> reify_ir::Value {
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                reify_ir::Value::String("id".to_string()),
                reify_ir::Value::Int(id),
            );
            m.insert(
                reify_ir::Value::String("solid".to_string()),
                reify_ir::Value::String(solid.to_string()),
            );
            m.insert(reify_ir::Value::String("world_transform".to_string()), wt);
            reify_ir::Value::Map(m)
        };
        let mut snap_map = std::collections::BTreeMap::new();
        snap_map.insert(
            reify_ir::Value::String("kind".to_string()),
            reify_ir::Value::String("snapshot".to_string()),
        );
        snap_map.insert(
            reify_ir::Value::String("bodies".to_string()),
            reify_ir::Value::List(vec![
                // body A: identity transform — must NOT emit ApplyTransform.
                make_body(100, "body_a", make_transform(0.0, 0.0, 0.0)),
                // body B: non-identity (20mm along X) — MUST emit ApplyTransform.
                make_body(200, "body_b", make_transform(0.020, 0.0, 0.0)),
            ]),
        );
        let snapshot = reify_ir::Value::Map(snap_map);

        let snap_cell = ValueCellId::new("Mech", "snap");
        let snap_arg = reify_ir::CompiledExpr::value_ref(snap_cell.clone(), Type::Geometry);
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("interferes"))
            .combine(snap_arg.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "interferes".to_string(),
                    qualified_name: "interferes".to_string(),
                },
                args: vec![snap_arg],
            },
            result_type: Type::List(Box::new(Type::Map(
                Box::new(Type::String),
                Box::new(Type::Int),
            ))),
            content_hash,
        };

        let mut values = reify_ir::ValueMap::new();
        values.insert(snap_cell, snapshot);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let _ = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        // Exactly ONE ApplyTransform op: only body B's non-identity transform.
        // Body A's identity transform must short-circuit to its raw handle.
        let ops = kernel.operations();
        let apply_ops: Vec<_> = ops
            .iter()
            .filter(|rec| matches!(&rec.op, GeometryOp::ApplyTransform { .. }))
            .collect();
        assert_eq!(
            apply_ops.len(),
            1,
            "expected exactly 1 ApplyTransform (body B only, not identity body A), \
             got {}: {:?}",
            apply_ops.len(),
            apply_ops
        );
        // Verify the single op targets body B's source handle.
        match &apply_ops[0].op {
            GeometryOp::ApplyTransform {
                target,
                translation,
                ..
            } => {
                assert_eq!(
                    *target, src_b,
                    "ApplyTransform must target body_b (non-identity)"
                );
                assert!(
                    (translation[0] - 0.020).abs() < 1e-12,
                    "body_b tx[0]: expected 0.020, got {}",
                    translation[0]
                );
            }
            other => panic!("expected ApplyTransform, got {:?}", other),
        }
    }

    /// Contract test (task 3844 KCC-ε): a `flat_map(snaps, |s| [center_of_mass(s)])`
    /// cell must return `None` from `try_eval_kinematic_query` so the pure-eval value
    /// (set by the regular eval pass) is preserved.
    ///
    /// The swept dispatch intercepts ALL `flat_map` calls in the kinematic post-process;
    /// this test locks the fall-through contract for non-kinematic inner functions,
    /// independent of OCCT availability.
    #[test]
    fn try_eval_swept_kinematic_query_non_kinematic_inner_falls_through_to_none() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        // snaps cell → a single-element list.  The snapshot content is never
        // accessed because the non-kinematic inner fn name triggers the
        // fall-through before any snapshot body processing.
        let snaps_cell = ValueCellId::new("Swept", "snaps");
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            snaps_cell.clone(),
            reify_ir::Value::List(vec![reify_ir::Value::Undef]),
        );

        // Lambda param cell.
        let s_param = ValueCellId::new("Swept", "s");

        // Inner: FunctionCall("center_of_mass", [ValueRef(s_param)])
        let s_ref = reify_ir::CompiledExpr::value_ref(s_param.clone(), Type::Geometry);
        let inner = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "center_of_mass".to_string(),
                    qualified_name: "center_of_mass".to_string(),
                },
                args: vec![s_ref],
            },
            result_type: Type::dimensionless_scalar(),
            content_hash: reify_core::ContentHash(0),
        };

        // Lambda body: ListLiteral([inner])
        let lambda_body = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::ListLiteral(vec![inner]),
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        // Lambda arg: Lambda { params, param_ids: [s_param], body, captures: [] }
        let lambda_arg = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::Lambda {
                params: vec![("s".to_string(), None)],
                param_ids: vec![s_param],
                body: Box::new(lambda_body),
                captures: vec![],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        // flat_map(snaps, lambda_arg)
        let snaps_ref =
            reify_ir::CompiledExpr::value_ref(snaps_cell, Type::List(Box::new(Type::Geometry)));
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "flat_map".to_string(),
                    qualified_name: "flat_map".to_string(),
                },
                args: vec![snaps_ref, lambda_arg],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();

        let result = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        assert!(
            result.is_none(),
            "flat_map with non-kinematic inner (center_of_mass) must return None \
             so the pure-eval value is preserved; got {result:?}"
        );
        assert!(
            diagnostics.is_empty(),
            "fall-through must emit no diagnostics; got {diagnostics:?}"
        );
    }

    /// Contract test (task 3844 KCC-ε): a well-formed
    /// `flat_map(snaps, |s| [min_clearance(s, id_a, id_b)])` over a 2-element
    /// snapshot list must return `Some(Value::List(len=2))` from
    /// `try_eval_kinematic_query`.
    ///
    /// Uses identity world_transforms (no `ApplyTransform` ops emitted) so the
    /// test can run without OCCT — only `MockGeometryKernel::with_distance_result`
    /// is required.  Locks the happy-path list-length contract independent of OCCT
    /// availability.
    #[test]
    fn try_eval_swept_kinematic_query_min_clearance_returns_list_of_correct_length() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let src_a = reify_ir::GeometryHandleId(10);
        let src_b = reify_ir::GeometryHandleId(20);

        // Identity world_transforms: no ApplyTransform ops, probe uses raw handles.
        let mut kernel = MockGeometryKernel::new().with_distance_result(
            src_a,
            src_b,
            reify_ir::Value::Real(0.050),
        ); // 50 mm

        let make_transform_identity = || -> reify_ir::Value {
            reify_ir::Value::Transform {
                rotation: Box::new(reify_ir::Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                translation: Box::new(reify_ir::Value::Vector(vec![
                    reify_ir::Value::length(0.0),
                    reify_ir::Value::length(0.0),
                    reify_ir::Value::length(0.0),
                ])),
            }
        };

        let make_snapshot =
            |transform_a: reify_ir::Value, transform_b: reify_ir::Value| -> reify_ir::Value {
                let make_body = |id: i64, solid: &str, wt: reify_ir::Value| -> reify_ir::Value {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert(
                        reify_ir::Value::String("id".to_string()),
                        reify_ir::Value::Int(id),
                    );
                    m.insert(
                        reify_ir::Value::String("solid".to_string()),
                        reify_ir::Value::String(solid.to_string()),
                    );
                    m.insert(reify_ir::Value::String("world_transform".to_string()), wt);
                    reify_ir::Value::Map(m)
                };
                let mut snap_map = std::collections::BTreeMap::new();
                snap_map.insert(
                    reify_ir::Value::String("kind".to_string()),
                    reify_ir::Value::String("snapshot".to_string()),
                );
                snap_map.insert(
                    reify_ir::Value::String("bodies".to_string()),
                    reify_ir::Value::List(vec![
                        make_body(1, "body_a", transform_a),
                        make_body(2, "body_b", transform_b),
                    ]),
                );
                reify_ir::Value::Map(snap_map)
            };

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body_a".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_a,
            },
        );
        named_steps.insert(
            "body_b".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_b,
            },
        );

        // Two snapshots, each with identity transforms.
        let snaps_cell = ValueCellId::new("Swept", "snaps");
        let id_a_cell = ValueCellId::new("Swept", "id_a");
        let id_b_cell = ValueCellId::new("Swept", "id_b");
        let s_param = ValueCellId::new("Swept", "s");

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            snaps_cell.clone(),
            reify_ir::Value::List(vec![
                make_snapshot(make_transform_identity(), make_transform_identity()),
                make_snapshot(make_transform_identity(), make_transform_identity()),
            ]),
        );
        values.insert(id_a_cell.clone(), reify_ir::Value::Int(1));
        values.insert(id_b_cell.clone(), reify_ir::Value::Int(2));

        // Build flat_map(snaps, |s| [min_clearance(s, id_a, id_b)])
        let s_ref = reify_ir::CompiledExpr::value_ref(s_param.clone(), Type::Geometry);
        let id_a_ref = reify_ir::CompiledExpr::value_ref(id_a_cell.clone(), Type::Int);
        let id_b_ref = reify_ir::CompiledExpr::value_ref(id_b_cell.clone(), Type::Int);

        let inner = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "min_clearance".to_string(),
                    qualified_name: "min_clearance".to_string(),
                },
                args: vec![s_ref, id_a_ref, id_b_ref],
            },
            result_type: Type::dimensionless_scalar(),
            content_hash: reify_core::ContentHash(0),
        };

        let lambda_body = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::ListLiteral(vec![inner]),
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        let lambda_arg = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::Lambda {
                params: vec![("s".to_string(), None)],
                param_ids: vec![s_param],
                body: Box::new(lambda_body),
                captures: vec![id_a_cell, id_b_cell],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        let snaps_ref =
            reify_ir::CompiledExpr::value_ref(snaps_cell, Type::List(Box::new(Type::Geometry)));
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "flat_map".to_string(),
                    qualified_name: "flat_map".to_string(),
                },
                args: vec![snaps_ref, lambda_arg],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        // Must return Some(Value::List) of length 2 (one result per snapshot).
        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "swept min_clearance flat_map over a 2-snapshot list must return \
                 Some(Value::List(len=2)), got {other:?}; diagnostics: {diagnostics:?}"
            ),
        };
        assert_eq!(
            list.len(),
            2,
            "list length must equal snapshot count (2), got {}: {list:?}",
            list.len()
        );
        // Each element must be a length Scalar with si_value ≈ 0.050 m.
        // Without this check, a regression that returns Value::Undef or dispatches
        // to the wrong helper would still pass the length-only assertion above.
        for (i, elem) in list.iter().enumerate() {
            match elem {
                reify_ir::Value::Scalar { si_value, .. } => {
                    let diff = (si_value - 0.050_f64).abs();
                    assert!(
                        diff < 1e-9,
                        "clearances[{i}] expected ≈ 0.050 m, got {si_value:.9} m (delta {diff:.2e})"
                    );
                }
                other => panic!(
                    "clearances[{i}] expected Value::Scalar (length ≈ 0.050 m), got {other:?}"
                ),
            }
        }
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
    }

    /// Swept kinematic query: per-snapshot failures (malformed snapshot missing
    /// `bodies`) become `Value::Undef` in the output list, while other snapshots
    /// still resolve.  List length is always equal to the snapshot count.
    ///
    /// Pins the most subtle invariant documented in `try_eval_swept_kinematic_query`:
    /// "Per-snapshot failures (None) become Value::Undef so the list length is
    /// always equal to the snapshot count."
    #[test]
    fn try_eval_swept_kinematic_query_malformed_snapshot_yields_undef_element() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let src_a = reify_ir::GeometryHandleId(10);
        let src_b = reify_ir::GeometryHandleId(20);

        let mut kernel = MockGeometryKernel::new().with_distance_result(
            src_a,
            src_b,
            reify_ir::Value::Real(0.030),
        ); // 30 mm

        // Well-formed snapshot with identity transforms.
        let make_body = |id: i64, solid: &str| -> reify_ir::Value {
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                reify_ir::Value::String("id".to_string()),
                reify_ir::Value::Int(id),
            );
            m.insert(
                reify_ir::Value::String("solid".to_string()),
                reify_ir::Value::String(solid.to_string()),
            );
            let identity_transform = reify_ir::Value::Transform {
                rotation: Box::new(reify_ir::Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }),
                translation: Box::new(reify_ir::Value::Vector(vec![
                    reify_ir::Value::length(0.0),
                    reify_ir::Value::length(0.0),
                    reify_ir::Value::length(0.0),
                ])),
            };
            m.insert(
                reify_ir::Value::String("world_transform".to_string()),
                identity_transform,
            );
            reify_ir::Value::Map(m)
        };

        // snapshot_good: valid Snapshot Map with a `bodies` list.
        let mut good_map = std::collections::BTreeMap::new();
        good_map.insert(
            reify_ir::Value::String("kind".to_string()),
            reify_ir::Value::String("snapshot".to_string()),
        );
        good_map.insert(
            reify_ir::Value::String("bodies".to_string()),
            reify_ir::Value::List(vec![make_body(1, "body_a"), make_body(2, "body_b")]),
        );
        let snapshot_good = reify_ir::Value::Map(good_map);

        // snapshot_malformed: Map has `kind="snapshot"` but is missing `bodies`.
        // `extract_snapshot_bodies` returns None → `eval_kinematic_on_snapshot`
        // returns Some(Value::Undef).
        let mut bad_map = std::collections::BTreeMap::new();
        bad_map.insert(
            reify_ir::Value::String("kind".to_string()),
            reify_ir::Value::String("snapshot".to_string()),
        );
        let snapshot_malformed = reify_ir::Value::Map(bad_map);

        let snaps_cell = ValueCellId::new("Swept", "snaps");
        let id_a_cell = ValueCellId::new("Swept", "id_a");
        let id_b_cell = ValueCellId::new("Swept", "id_b");
        let s_param = ValueCellId::new("Swept", "s");

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "body_a".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_a,
            },
        );
        named_steps.insert(
            "body_b".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Occt,
                id: src_b,
            },
        );

        // List: [snapshot_good, snapshot_malformed].
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            snaps_cell.clone(),
            reify_ir::Value::List(vec![snapshot_good, snapshot_malformed]),
        );
        values.insert(id_a_cell.clone(), reify_ir::Value::Int(1));
        values.insert(id_b_cell.clone(), reify_ir::Value::Int(2));

        // Build flat_map(snaps, |s| [min_clearance(s, id_a, id_b)])
        let s_ref = reify_ir::CompiledExpr::value_ref(s_param.clone(), Type::Geometry);
        let id_a_ref = reify_ir::CompiledExpr::value_ref(id_a_cell.clone(), Type::Int);
        let id_b_ref = reify_ir::CompiledExpr::value_ref(id_b_cell.clone(), Type::Int);

        let inner = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "min_clearance".to_string(),
                    qualified_name: "min_clearance".to_string(),
                },
                args: vec![s_ref, id_a_ref, id_b_ref],
            },
            result_type: Type::dimensionless_scalar(),
            content_hash: reify_core::ContentHash(0),
        };
        let lambda_body = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::ListLiteral(vec![inner]),
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };
        let lambda_arg = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::Lambda {
                params: vec![("s".to_string(), None)],
                param_ids: vec![s_param],
                body: Box::new(lambda_body),
                captures: vec![id_a_cell, id_b_cell],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };
        let snaps_ref =
            reify_ir::CompiledExpr::value_ref(snaps_cell, Type::List(Box::new(Type::Geometry)));
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "flat_map".to_string(),
                    qualified_name: "flat_map".to_string(),
                },
                args: vec![snaps_ref, lambda_arg],
            },
            result_type: Type::List(Box::new(Type::dimensionless_scalar())),
            content_hash: reify_core::ContentHash(0),
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::try_eval_kinematic_query(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
            &mut HashMap::new(),
        );

        // Must return Some(Value::List) of length 2 (one per snapshot, not one per resolution).
        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "malformed-snapshot test must return Some(Value::List(len=2)), \
                 got {other:?}; diagnostics: {diagnostics:?}"
            ),
        };
        assert_eq!(
            list.len(),
            2,
            "list length must equal snapshot count (2) even with a malformed element, \
             got {}: {list:?}",
            list.len()
        );

        // Element 0 (good snapshot): resolved to a length Scalar ≈ 0.030 m.
        match &list[0] {
            reify_ir::Value::Scalar { si_value, .. } => {
                let diff = (si_value - 0.030_f64).abs();
                assert!(
                    diff < 1e-9,
                    "list[0] (good snapshot) expected ≈ 0.030 m, got {si_value:.9} m"
                );
            }
            other => panic!("list[0] (good snapshot) expected Value::Scalar, got {other:?}"),
        }

        // Element 1 (malformed snapshot): must be Value::Undef.
        assert_eq!(
            list[1],
            reify_ir::Value::Undef,
            "list[1] (malformed snapshot missing 'bodies') must be Value::Undef, got {:?}",
            list[1]
        );
    }

    /// Contract test (task 4142, Cluster B RED — topology/resolve_geometry_handle_arg leaf):
    /// `try_eval_topology_selector` resolves the geometry handle via
    /// `KernelHandle.id`, ignoring `KernelHandle.kernel`.
    ///
    /// The `edges` path exercises the shared `resolve_geometry_handle_arg` leaf
    /// (geometry_ops.rs:3620), which is the single leaf covering ALL topology
    /// selectors AND the new ghr-zeta geometry-query path.  Proves `.kernel`
    /// is never consulted.
    ///
    /// RED on current main: `try_eval_topology_selector` still takes
    /// `&HashMap<String, GeometryHandleId>` → E0308 type mismatch.
    ///
    /// NOTE: Pins the leaf-projection contract only (`.kernel` unused in the current
    /// single-kernel-per-build design). When cross-kernel handle resolution lands,
    /// update to assert per-kernel dispatch rather than treating `.kernel` as ignored.
    #[test]
    fn try_eval_topology_selector_resolves_via_kernel_handle_id() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_id = reify_ir::GeometryHandleId(1);
        let edge_a = reify_ir::GeometryHandleId(2);
        let edge_b = reify_ir::GeometryHandleId(3);
        let parent_rr = RealizationNodeId::new("EdgeBody", 0);
        let parent_hash: [u8; 32] = [0x42; 32];

        let mut kernel =
            MockGeometryKernel::new().with_extracted_edges(parent_id, vec![edge_a, edge_b]);

        // Map "s" to a KernelHandle with deliberately non-default kernel.
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert(
            "s".to_string(),
            reify_ir::KernelHandle {
                kernel: reify_ir::KernelId::Manifold, // non-default: must be ignored
                id: parent_id,
            },
        );

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("EdgeBody", "s"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_id),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "edges",
            "EdgeBody",
            "s",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Task 4118 (γ): `edges` now constructs a kernel-FREE typed
        // `Value::Selector(Edge)` (All leaf) over the parent solid. The arm
        // resolves its target from `values` (the hydrated Value::GeometryHandle),
        // so `named_steps` — and thus `KernelHandle.kernel` — is not consulted at
        // all; the leaf target carries `parent_id` as its kernel_handle regardless
        // of the deliberately-non-default `KernelId::Manifold` staged in named_steps.
        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "edges with KernelHandle{{Manifold, 1}} must return Some(Value::Selector(..)), \
                 got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Edge,
            "edges → Edge kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, Some(parent_id),
                    "leaf target kernel_handle must be parent_id (resolved from values, \
                     KernelHandle.kernel ignored)"
                );
                assert_eq!(*query, reify_ir::value::LeafQuery::All, "edges → All leaf");
            }
            other => panic!("edges must be a Leaf selector node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for successful edge construction, got: {:?}",
            diagnostics
        );
    }

    // ── try_eval_topology_selector unit tests (task 2324) ────────────────────
    //
    // These tests pin the contract of `try_eval_topology_selector`, the
    // kernel-aware eval-time dispatch surface for the `closest_point`, `is_on`,
    // and `angle_between_surfaces` stdlib helpers. Sibling to the
    // `try_eval_conformance_query_*` and (integration-only) kinematic-query
    // tests above. The function lives in this module (rather than
    // `eval_expr`) because the build pipeline owns both the kernel and the
    // per-realization name → handle map (`named_steps`).

    /// Build a `CompiledExpr` for a stdlib call `helper(<entity>.<member>)` with
    /// a single `ValueRef` arg. Used for the `edges(b)` / `faces(b)` dispatch
    /// unit tests (task 3616 step-5).
    fn topology_selector_call_one_value_ref(
        helper_name: &str,
        entity: &str,
        member: &str,
        arg_type: reify_core::Type,
        result_type: reify_core::Type,
    ) -> reify_ir::CompiledExpr {
        let arg = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member),
            arg_type,
        );
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name))
            .combine(arg.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg],
            },
            result_type,
            content_hash,
        }
    }

    // ── step-5 (task 3616): edges/faces dispatch unit tests ─────────────────
    //
    // These tests verify that the arm emits Value::List(Value::GeometryHandle)
    // via dispatch_filtered_subhandles.

    /// `edges` dispatch returns `Value::List` of three `Value::GeometryHandle`
    /// elements when the mock kernel returns [GHId(2),GHId(3),GHId(4)] and the
    /// `values` map carries the parent `Value::GeometryHandle`. Each element
    /// must carry the parent's `realization_ref`, and the three
    /// `upstream_values_hash` fields must be pairwise distinct (PRD §4 iii).
    #[test]
    fn edges_dispatch_returns_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("BoxEdges", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new().with_extracted_edges(
            parent_handle,
            vec![
                GeometryHandleId(2),
                GeometryHandleId(3),
                GeometryHandleId(4),
            ],
        );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("BoxEdges", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "edges",
            "BoxEdges",
            "b",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Task 4118 (γ): construction is kernel-FREE — `edges(b)` builds a typed
        // `Value::Selector(Edge)` with an `All` leaf over the parent solid handle,
        // NOT an eagerly-extracted `Value::List` of sub-handles. The staged
        // `with_extracted_edges` data is intentionally unused (zero kernel queries
        // during construction, K2/BT7); the `Selector → List<Geometry>` resolution
        // (extraction + per-element canonical hashing) is the ResolveSelector
        // coercion node's job, covered by the try_eval_resolve_selector tests.
        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "expected Some(Value::Selector(..)), got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Edge,
            "edges(b) → Edge kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, Some(parent_handle),
                    "leaf target must be the parent solid handle"
                );
                assert_eq!(
                    *query,
                    reify_ir::value::LeafQuery::All,
                    "edges(b) → All leaf"
                );
            }
            other => panic!("edges(b) must be a Leaf selector node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "kernel-free construction must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// When the `values` map does not carry a `Value::GeometryHandle` for the
    /// arg cell, the `edges` arm must fall through to `None` (cell stays Undef)
    /// rather than partially constructing a sub-handle (PRD invariant #2).
    /// RED: current arm dispatches via `named_steps` regardless of `values` and
    /// returns `Some(Value::List(Value::Int))`.
    #[test]
    fn edges_dispatch_falls_through_to_none_when_parent_not_hydrated() {
        use reify_core::Type;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let mut kernel = MockGeometryKernel::new().with_extracted_edges(
            parent_handle,
            vec![GeometryHandleId(2), GeometryHandleId(3)],
        );

        // named_steps has the handle so the kernel could serve the call …
        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        // … but values has NO Value::GeometryHandle for the arg cell.
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_one_value_ref(
            "edges",
            "BoxEdges",
            "b",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "must fall through to None when parent is not a hydrated Value::GeometryHandle, \
             got {:?}",
            result
        );
    }

    /// Build a `CompiledExpr` for a stdlib call `helper(<entity>.<member_a>,
    /// <entity>.<member_b>)` with two `ValueRef` args resolving to let-bound
    /// cells. Mirrors `conformance_call` above.
    fn topology_selector_call_two_value_refs(
        helper_name: &str,
        entity: &str,
        member_a: &str,
        type_a: reify_core::Type,
        member_b: &str,
        type_b: reify_core::Type,
        result_type: reify_core::Type,
    ) -> reify_ir::CompiledExpr {
        let arg_a = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_a),
            type_a,
        );
        let arg_b = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_b),
            type_b,
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_a, arg_b],
            },
            result_type,
            content_hash,
        }
    }

    /// Build a `CompiledExpr` for `helper(<literal_real>, <literal_real>)` —
    /// used for the literal-arg fall-through defensive tests. Mirrors
    /// `conformance_call_literal_arg` above.
    fn topology_selector_call_literal_args(helper_name: &str) -> reify_ir::CompiledExpr {
        let arg_a = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(1.0),
            reify_core::Type::dimensionless_scalar(),
        );
        let arg_b = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(2.0),
            reify_core::Type::dimensionless_scalar(),
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_a, arg_b],
            },
            // result_type is unused on the dispatch path — set to a
            // representative value to keep the literal hand-built expression
            // structurally well-formed.
            result_type: reify_core::Type::Bool,
            content_hash,
        }
    }

    /// Build a Value::Point with three Length scalars, mirroring how a
    /// let-bound `point3(x_mm, y_mm, z_mm)` realises in the `values` map.
    fn point3_length_value(x_m: f64, y_m: f64, z_m: f64) -> reify_ir::Value {
        reify_ir::Value::Point(vec![
            reify_ir::Value::length(x_m),
            reify_ir::Value::length(y_m),
            reify_ir::Value::length(z_m),
        ])
    }

    /// Build a Value::Vector with three dimensionless Real components, mirroring
    /// how a let-bound `vec3(x, y, z)` realises in the `values` map.
    /// Analogous to `point3_length_value` above. Used by the `angle` dispatch
    /// unit tests (task 3614, KGQ-ε).
    fn vec3_value(x: f64, y: f64, z: f64) -> reify_ir::Value {
        reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(x),
            reify_ir::Value::Real(y),
            reify_ir::Value::Real(z),
        ])
    }

    #[test]
    fn try_eval_topology_selector_closest_point_kernel_reply_parses_to_point3_length() {
        use reify_test_support::mocks::MockGeometryKernel;
        let body_handle = reify_ir::GeometryHandleId(7);
        // The kernel reply mirrors the `OcctKernel::query()` arm for
        // `ClosestPointOnShape` (lib.rs JSON-Point3 encoding). The dispatcher
        // is expected to parse it and produce a `Value::Point(vec![length(...),
        // length(...), length(...)])`.
        let mut kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            body_handle,
            [10.0, 0.0, 0.0],
            reify_ir::Value::String("{\"x\":5.0,\"y\":0.0,\"z\":0.0}".to_string()),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Bracket", "p"),
            point3_length_value(10.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "closest_point",
            "Bracket",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::point3(reify_core::Type::length()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(point3_length_value(5.0, 0.0, 0.0)),
            "closest_point(p, body) with kernel JSON-Point3 reply must \
             produce Some(Value::Point(vec![length, length, length])) parsed \
             from the JSON; got {:?}",
            result
        );
    }

    #[test]
    fn try_eval_topology_selector_is_on_kernel_reply_returns_bool_with_default_tolerance() {
        use reify_test_support::mocks::MockGeometryKernel;
        let body_handle = reify_ir::GeometryHandleId(11);
        // The dispatcher must use `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` (≈ OCCT's
        // `Precision::Confusion()`, ~1e-7) for the 2-arg `is_on(point, geometry)`
        // form. Recording the mock under exactly this tolerance pins the contract —
        // if the dispatcher ever changes the default, the recorded reply would not
        // be served and the test would fail with `None`.
        let mut kernel = MockGeometryKernel::new().with_point_on_shape_result(
            body_handle,
            [5.0, 0.0, 0.0],
            reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            reify_ir::Value::Bool(true),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Bracket", "p"),
            point3_length_value(5.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "is_on",
            "Bracket",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "is_on(p, body) with kernel reply Bool(true) must produce \
             Some(Value::Bool(true)) (default tolerance DEFAULT_POINT_ON_SHAPE_TOLERANCE_M); got {:?}",
            result
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_returns_angle_scalar() {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_a = reify_ir::GeometryHandleId(31);
        let face_b = reify_ir::GeometryHandleId(37);
        // Kernel returns a raw f64 (radians) — the dispatcher is expected to
        // wrap as `Value::angle(rad)` to match the cell type
        // `Type::angle()`.
        let mut kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_ir::Value::Real(std::f64::consts::FRAC_PI_2),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face_a".to_string(), kh(face_a));
        named_steps.insert("face_b".to_string(), kh(face_b));

        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_core::Type::Geometry,
            "face_b",
            reify_core::Type::Geometry,
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::FRAC_PI_2)),
            "angle_between_surfaces(face_a, face_b) with kernel reply \
             Real(PI/2) must produce Some(Value::angle(PI/2)); got {:?}",
            result
        );
    }

    #[test]
    fn try_eval_topology_selector_closest_point_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        // `closest_point(<literal>, <literal>)` — literal args, no `let`
        // bindings to resolve. The dispatcher must return None *and* never
        // consult the kernel, mirroring `try_eval_conformance_query`'s
        // literal-arg-fall-through contract.
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("closest_point");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "closest_point(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_is_on_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("is_on");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "is_on(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("angle_between_surfaces");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "angle_between_surfaces(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_non_helper_name_returns_none_no_kernel_call() {
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(reify_ir::GeometryHandleId(7)));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Bracket", "p"),
            point3_length_value(0.0, 0.0, 0.0),
        );

        // `volume` is a real stdlib function name but NOT one of the three
        // recognised topology-selector helpers. The dispatch must return
        // None, mirroring the conformance-query contract.
        let expr = topology_selector_call_two_value_refs(
            "volume",
            "Bracket",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::dimensionless_scalar(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "non-helper name 'volume' must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-helper names"
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_scalar_resolves_identically_to_real()
     {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the dispatch's `Real | Scalar` leniency for `dispatch_surface_angle`:
        // a kernel reply of `Value::Scalar { dimension: ANGLE, si_value: PI/2 }`
        // must resolve to `Value::angle(PI/2)`, identically to the `Value::Real(PI/2)`
        // reply pinned by the sibling `..._returns_angle_scalar` test above. Mirrors
        // `kernel_distance`'s Real|Scalar leniency so a future kernel returning a
        // dimensioned Scalar does not regress silently.
        let face_a = reify_ir::GeometryHandleId(31);
        let face_b = reify_ir::GeometryHandleId(37);
        let mut kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_ir::Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_2,
                dimension: reify_core::DimensionVector::ANGLE,
            },
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face_a".to_string(), kh(face_a));
        named_steps.insert("face_b".to_string(), kh(face_b));

        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_core::Type::Geometry,
            "face_b",
            reify_core::Type::Geometry,
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::FRAC_PI_2)),
            "angle_between_surfaces with kernel Scalar(ANGLE, PI/2) reply must \
             resolve identically to a Real(PI/2) reply; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "Scalar reply on the happy path must NOT emit diagnostics, got: {:?}",
            diagnostics
        );
    }

    /// Pins the DIMENSIONLESS leniency documented at the `dispatch_surface_angle`
    /// Scalar arm (see comment block around line 1902). A mock kernel that returns
    /// `Value::Scalar { dimension: DIMENSIONLESS, si_value: x }` must be accepted
    /// alongside ANGLE without emitting any diagnostic, and must resolve to
    /// `Value::angle(x)`. Without this test, tightening the guard to ANGLE-only
    /// would not be caught by the existing ANGLE or Real fixtures.
    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_scalar_dimensionless_resolves_as_angle()
     {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_a = reify_ir::GeometryHandleId(31);
        let face_b = reify_ir::GeometryHandleId(37);
        let mut kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_ir::Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_2,
                dimension: reify_core::DimensionVector::DIMENSIONLESS,
            },
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face_a".to_string(), kh(face_a));
        named_steps.insert("face_b".to_string(), kh(face_b));

        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_core::Type::Geometry,
            "face_b",
            reify_core::Type::Geometry,
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::FRAC_PI_2)),
            "angle_between_surfaces with kernel Scalar(DIMENSIONLESS, PI/2) reply must \
             resolve to Some(Value::angle(PI/2)); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "DIMENSIONLESS Scalar reply must NOT emit diagnostics (intentional leniency), \
             got: {:?}",
            diagnostics
        );
    }

    /// Shared fixture for the two wrong-dim-Scalar tests below. Builds a
    /// `MockGeometryKernel` wired to return a LENGTH-dimensioned Scalar for the
    /// `angle_between_surfaces(face_a, face_b)` call, together with the
    /// `named_steps` map, empty `ValueMap`, and the compiled `expr`. Each test
    /// owns its own `diagnostics` Vec and call site, which is all that differs
    /// between the debug-panic and release-warn cases.
    fn wrong_dim_scalar_fixture() -> (
        reify_ir::CompiledExpr,
        HashMap<String, reify_ir::KernelHandle>,
        reify_ir::ValueMap,
        reify_test_support::mocks::MockGeometryKernel,
    ) {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_a = reify_ir::GeometryHandleId(31);
        let face_b = reify_ir::GeometryHandleId(37);
        // LENGTH is the real-world bug class: metres silently reinterpreted as
        // radians. Using LENGTH (not e.g. MASS) ties the fixture to the actual
        // failure mode described in the task analysis.
        let kernel = MockGeometryKernel::new().with_surface_angle_result(
            face_a,
            face_b,
            reify_ir::Value::Scalar {
                si_value: 1.0,
                dimension: reify_core::DimensionVector::LENGTH,
            },
        );
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face_a".to_string(), kh(face_a));
        named_steps.insert("face_b".to_string(), kh(face_b));
        let values = reify_ir::ValueMap::new();
        let expr = topology_selector_call_two_value_refs(
            "angle_between_surfaces",
            "Bracket",
            "face_a",
            reify_core::Type::Geometry,
            "face_b",
            reify_core::Type::Geometry,
            reify_core::Type::angle(),
        );
        (expr, named_steps, values, kernel)
    }

    /// Pins the defensive dim-check in `dispatch_surface_angle`'s Scalar arm —
    /// mirrors `resolve_point3_length_arg`'s tightened LENGTH check from commit
    /// 8c464177db. A LENGTH-dimensioned Scalar reply must NOT be silently
    /// reinterpreted as radians; the dispatcher must emit a Warning naming the
    /// helper and return Undef. Gated `#[cfg(not(debug_assertions))]` because in
    /// debug builds the same fixture trips the sibling
    /// `..._panics_in_debug_build` test's debug_assert.
    #[cfg(not(debug_assertions))]
    #[test]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_scalar_wrong_dimension_emits_warning_and_returns_undef()
     {
        let (expr, named_steps, values, mut kernel) = wrong_dim_scalar_fixture();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "angle_between_surfaces with LENGTH-dimensioned Scalar reply must yield \
             Some(Value::Undef), NOT Some(Value::angle(1.0)); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "wrong-dim Scalar reply must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("angle_between_surfaces"),
            "diagnostic must mention the helper name 'angle_between_surfaces', got: {}",
            diag.message
        );
        // DimensionVector::LENGTH displays as "m" (via its fmt::Display impl).
        // The format string is `"(dimension={}, si_value={})"`  so the rendered
        // fragment is `"dimension=m, si_value="`.  Anchoring past the trailing
        // comma prevents false positives from dimensions that also start with
        // "m" (e.g. m^2, m·s^-1, mol, …).
        assert!(
            diag.message.contains("dimension=m, si_value="),
            "diagnostic must mention the offending dimension anchored by the trailing \
             ', si_value=' (LENGTH displays as 'm'; bare 'dimension=m' would also \
             match m^2, m·… etc.); got: {}",
            diag.message
        );
    }

    /// Pins the debug-mode panic in `dispatch_surface_angle`'s Scalar arm.
    /// Uses the same LENGTH-dimensioned Scalar fixture as the sibling release
    /// test; in debug builds the `debug_assert!` panics before the if-fall-through
    /// runs, so the `#[should_panic]` attribute is the only assertion needed.
    ///
    /// Follows the dual-test pattern from `crates/reify-eval/src/kernel_registry.rs`
    /// (see `emit_kernel_selection_panics_when_total_is_zero` at line 665 and
    /// `warn_if_duplicate_op_repr_pairs_always_emits_warn_on_duplicate` at 685):
    /// pair a `#[cfg(debug_assertions)] #[should_panic]` test with a
    /// `#[cfg(not(debug_assertions))]` test for the release fall-through.
    /// The `#[cfg(debug_assertions)]` guard is required because `debug_assert!`
    /// compiles to a no-op in release builds — `#[should_panic]` would falsely
    /// "pass" in a release build where the panic never fires.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "expected ANGLE")]
    fn try_eval_topology_selector_angle_between_surfaces_kernel_reply_scalar_wrong_dimension_panics_in_debug_build()
     {
        let (expr, named_steps, values, mut kernel) = wrong_dim_scalar_fixture();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        // The debug_assert! in dispatch_surface_angle's Scalar arm must panic
        // with a message containing "expected ANGLE". No assert_eq! after this
        // call — the #[should_panic] attribute drives the assertion.
        super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );
    }

    #[test]
    fn try_eval_topology_selector_is_on_non_bool_kernel_reply_emits_warning_and_returns_undef() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the `Ok(other)` warning arm of `dispatch_point_on_shape`: a kernel
        // reply that is neither `Value::Bool(_)` nor an Err must produce
        // `Some(Value::Undef)` with a Warning diagnostic naming the helper. Defends
        // the contract against a future kernel that mistakenly returns the
        // wrong-typed Value.
        let body_handle = reify_ir::GeometryHandleId(11);
        let mut kernel = MockGeometryKernel::new().with_point_on_shape_result(
            body_handle,
            [5.0, 0.0, 0.0],
            reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            // Wrong type — should trigger the non-Bool warning arm.
            reify_ir::Value::Real(0.5),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Bracket", "p"),
            point3_length_value(5.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "is_on",
            "Bracket",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "is_on(...) with non-Bool kernel reply must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Bool reply must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("is_on"),
            "diagnostic must mention the helper name 'is_on', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("non-Bool"),
            "diagnostic must indicate the non-Bool reply, got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_closest_point_malformed_json_reply_emits_warning_and_returns_undef()
     {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the `Err(err)` parse-failure arm of `dispatch_point3_length_reply`: a
        // kernel reply whose `Value::String(_)` payload is not parseable as a
        // JSON-Point3 must produce `Some(Value::Undef)` with a Warning
        // diagnostic naming the helper. Defends the contract against a future
        // kernel that emits a malformed JSON string.
        let body_handle = reify_ir::GeometryHandleId(7);
        let mut kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            body_handle,
            [10.0, 0.0, 0.0],
            // Not a JSON-Point3 payload — should trigger the parse-failure
            // warning arm.
            reify_ir::Value::String("not a valid json point".to_string()),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("body".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Bracket", "p"),
            point3_length_value(10.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "closest_point",
            "Bracket",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::point3(reify_core::Type::length()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "closest_point with malformed JSON reply must yield \
             Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "malformed reply must emit exactly one Warning, got {} \
             diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("closest_point"),
            "diagnostic must mention the helper name 'closest_point', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("parse failed"),
            "diagnostic must indicate the parse failure, got: {}",
            diag.message
        );
    }

    // ── try_eval_topology_selector: angle dispatch (task 3614, KGQ-ε) ────────
    //
    // These tests pin the pure-math `angle(Vec3, Vec3) -> Angle` dispatch arm.
    // No kernel calls — acos(clamp(dot/(|a||b|), -1, 1)).  Modelled on the
    // `try_eval_topology_selector_angle_between_surfaces_*` tests above.

    #[test]
    fn try_eval_topology_selector_angle_two_vec3_value_refs_returns_angle_scalar() {
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "a"),
            vec3_value(1.0, 0.0, 0.0),
        );
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "b"),
            vec3_value(0.0, 1.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "angle",
            "AngleSmoke",
            "a",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "b",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::FRAC_PI_2)),
            "angle(vec3(1,0,0), vec3(0,1,0)) must return Some(Value::angle(PI/2)); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "perpendicular vectors must NOT emit diagnostics, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_parallel_vectors_returns_zero_angle() {
        // vec3(1,0,0) · vec3(2,0,0): cos=1.0 → clamp → acos(1.0) = 0.0.
        // Proves the acos-domain upper-bound clamp for parallel vectors.
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "a"),
            vec3_value(1.0, 0.0, 0.0),
        );
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "b"),
            vec3_value(2.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "angle",
            "AngleSmoke",
            "a",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "b",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(0.0)),
            "angle(vec3(1,0,0), vec3(2,0,0)) (parallel) must return Some(Value::angle(0.0)); \
             got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "parallel vectors must NOT emit diagnostics, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_antiparallel_vectors_returns_pi() {
        // vec3(1,0,0) · vec3(-1,0,0): cos=-1.0 → clamp → acos(-1.0) = π.
        // Proves the acos-domain lower-bound clamp for antiparallel vectors.
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "a"),
            vec3_value(1.0, 0.0, 0.0),
        );
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "b"),
            vec3_value(-1.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "angle",
            "AngleSmoke",
            "a",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "b",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::PI)),
            "angle(vec3(1,0,0), vec3(-1,0,0)) (antiparallel) must return \
             Some(Value::angle(PI)); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "antiparallel vectors must NOT emit diagnostics, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_nonvec3_scalar_literal_args_falls_through_to_none() {
        // angle(<literal_real>, <literal_real>) — scalar Real literals evaluate
        // (task ε) to Value::Real, which resolve_vec3_arg rejects as
        // defined-but-wrong: it pushes a Warning and returns None for args[0],
        // so the `?` short-circuits and the dispatcher returns None WITHOUT
        // consulting the kernel.  Note: an inline expr that EVALUATES to a
        // Value::Vector (e.g. a vec3(...) constructor or a Value::Vector literal)
        // DOES resolve and compute an angle — see
        // `try_eval_topology_selector_angle_literal_vec3_args_resolves_and_returns_angle`.
        // This test pins the non-Vec3 scalar literal case (result None; kernel
        // untouched).
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("angle");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "angle(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_literal_vec3_args_resolves_and_returns_angle() {
        // angle(Literal(vec3(1,0,0)), Literal(vec3(0,1,0))) — resolve_vec3_arg
        // EVALUATES the arg (task ε); a Literal(Value::Vector) evaluates to that
        // Value::Vector and is accepted, so literal vec3 args DO resolve and
        // produce an angle, unlike the scalar-literal case above.  Pins the
        // actually-distinct contract for literal-typed Vec3 args.
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new(); // empty — args come from literals

        // Build angle(Literal(vec3(1,0,0)), Literal(vec3(0,1,0))).
        let arg_a = reify_ir::CompiledExpr::literal(
            vec3_value(1.0, 0.0, 0.0),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let arg_b = reify_ir::CompiledExpr::literal(
            vec3_value(0.0, 1.0, 0.0),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let mut ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("angle"));
        ch = ch.combine(arg_a.content_hash).combine(arg_b.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "angle".to_string(),
                    qualified_name: "angle".to_string(),
                },
                args: vec![arg_a, arg_b],
            },
            result_type: reify_core::Type::angle(),
            content_hash: ch,
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::angle(std::f64::consts::FRAC_PI_2)),
            "angle(literal vec3(1,0,0), literal vec3(0,1,0)) must resolve to \
             Some(Value::angle(π/2)); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "orthogonal literal vec3 args must NOT emit diagnostics, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_zero_length_vector_returns_undef() {
        // Degenerate input: zero-length vec3(0,0,0) causes |a|=0, division by
        // zero → the dispatcher must emit exactly one Warning and return
        // Some(Value::Undef) rather than propagating NaN.
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "a"),
            vec3_value(0.0, 0.0, 0.0),
        );
        values.insert(
            reify_core::ValueCellId::new("AngleSmoke", "b"),
            vec3_value(0.0, 1.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "angle",
            "AngleSmoke",
            "a",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "b",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            reify_core::Type::angle(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "angle(vec3(0,0,0), ...) with zero-length input must return \
             Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "zero-length vector must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("angle"),
            "diagnostic must mention the helper name 'angle', got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_angle_nonfinite_vector_component_returns_undef() {
        // Degenerate input: a vector with a NaN component causes na = NaN
        // (NaN*NaN + 0 + 0 = NaN, sqrt(NaN) = NaN).  The primary guard
        // `!na.is_finite()` at the dispatch arm must catch this and return
        // Some(Value::Undef) with exactly one Warning — no panic, no NaN-poison.
        // Also tested: f64::INFINITY component → na = inf → same guard fires.
        use reify_test_support::mocks::MockGeometryKernel;

        for (label, ax, ay, az) in [
            ("NaN", f64::NAN, 0.0_f64, 0.0_f64),
            ("INFINITY", f64::INFINITY, 0.0, 0.0),
        ] {
            let mut kernel = MockGeometryKernel::new();
            let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
            let mut values = reify_ir::ValueMap::new();
            values.insert(
                reify_core::ValueCellId::new("T", "a"),
                vec3_value(ax, ay, az),
            );
            values.insert(
                reify_core::ValueCellId::new("T", "b"),
                vec3_value(0.0, 1.0, 0.0),
            );
            let expr = topology_selector_call_two_value_refs(
                "angle",
                "T",
                "a",
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
                "b",
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
                reify_core::Type::angle(),
            );
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            let result = super::try_eval_topology_selector(
                &expr,
                &named_steps,
                &values,
                &mut kernel,
                &mut diagnostics,
            );
            assert_eq!(
                result,
                Some(reify_ir::Value::Undef),
                "angle(vec3({label},...), ...) must return Some(Value::Undef); got {result:?}"
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "non-finite component must emit exactly one Warning \
                 (label={label}), got {} diagnostics: {diagnostics:?}",
                diagnostics.len()
            );
            assert_eq!(
                diagnostics[0].severity,
                reify_core::Severity::Warning,
                "diagnostic severity must be Warning (label={label}), got {:?}",
                diagnostics[0].severity
            );
        }
    }

    // ── try_eval_topology_selector `contains` unit tests (task 3611, KGQ-β) ────
    //
    // These tests pin the `contains(solid, point) -> Bool` dispatch contract
    // (PRD §9 KGQ-β). Arg order is solid-then-point (args[0]=geometry,
    // args[1]=point3<Length>), mirroring `is_on` with args swapped. The
    // dispatcher reuses `dispatch_point_on_shape` (Bool unwrapper) and
    // `DEFAULT_CONTAINS_TOLERANCE_M` per §5.2.
    //
    // Three contracts:
    //   (a) happy path: kernel Bool(true) reply → Some(Value::Bool(true))
    //   (b) literal-arg fall-through: non-ValueRef args → None, no kernel call
    //   (c) non-Bool kernel reply → Some(Value::Undef) + exactly-one Warning
    //       naming "contains"
    //
    // All three FAIL (RED) until step-6 wires the `contains` arm in
    // try_eval_topology_selector / TopologySelectorHelper.

    #[test]
    fn try_eval_topology_selector_contains_kernel_reply_returns_bool_with_default_tolerance() {
        use reify_test_support::mocks::MockGeometryKernel;
        let body_handle = reify_ir::GeometryHandleId(42);
        // Record the mock under DEFAULT_CONTAINS_TOLERANCE_M — pins that the
        // dispatcher uses this constant and not some ad-hoc value.
        let mut kernel = MockGeometryKernel::new().with_contains_result(
            body_handle,
            [0.0, 0.0, 0.0],
            reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
            reify_ir::Value::Bool(true),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // args[0] = solid → resolved via named_steps by member name "solid"
        named_steps.insert("solid".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[1] = point → resolved via values by ValueCellId
        values.insert(
            reify_core::ValueCellId::new("ContainsBox", "center"),
            point3_length_value(0.0, 0.0, 0.0),
        );

        // contains(solid, center): args[0]=solid (Geometry), args[1]=center (Point3<Length>)
        let expr = topology_selector_call_two_value_refs(
            "contains",
            "ContainsBox",
            "solid",
            reify_core::Type::Geometry,
            "center",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "contains(solid, center) with kernel reply Bool(true) must produce \
             Some(Value::Bool(true)) (default tolerance DEFAULT_CONTAINS_TOLERANCE_M); \
             got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path contains must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_contains_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        // `contains(<literal>, <literal>)` — non-ValueRef args, so both
        // resolve_geometry_handle_arg and resolve_point3_length_arg return None,
        // and the dispatcher must return None without consulting the kernel.
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("contains");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "contains(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_contains_non_bool_kernel_reply_emits_warning_and_returns_undef() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the `Ok(other)` warning arm of `dispatch_point_on_shape` (reused for
        // `contains`): a kernel reply that is not `Value::Bool(_)` must produce
        // `Some(Value::Undef)` with a Warning diagnostic naming "contains".
        let body_handle = reify_ir::GeometryHandleId(42);
        let mut kernel = MockGeometryKernel::new().with_contains_result(
            body_handle,
            [0.0, 0.0, 0.0],
            reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
            // Wrong type — should trigger the non-Bool warning arm.
            reify_ir::Value::Real(0.5),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("solid".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("ContainsBox", "center"),
            point3_length_value(0.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "contains",
            "ContainsBox",
            "solid",
            reify_core::Type::Geometry,
            "center",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "contains(...) with non-Bool kernel reply must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Bool reply must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("contains"),
            "diagnostic must mention the helper name 'contains', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("non-Bool"),
            "diagnostic must indicate the non-Bool reply, got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_contains_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        // No `with_contains_result` seeding — MockGeometryKernel.query() falls
        // through to the generic handle-only map which also has no entry for
        // this handle, so it returns `Err(QueryError::QueryFailed(...))`.
        // `dispatch_point_on_shape` must downgrade this to `Some(Value::Undef)`
        // and emit exactly one Warning diagnostic naming "contains" and
        // "kernel query failed". Pins the `Err(err)` arm of that helper.
        let body_handle = reify_ir::GeometryHandleId(42);
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("solid".to_string(), kh(body_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("ContainsBox", "center"),
            point3_length_value(0.0, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "contains",
            "ContainsBox",
            "solid",
            reify_core::Type::Geometry,
            "center",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "contains(...) with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("contains"),
            "diagnostic must mention the helper name 'contains', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("kernel query failed"),
            "diagnostic must indicate the kernel failure, got: {}",
            diag.message
        );
    }

    // ── distance unit tests (task 3610, KGQ-α) ──────────────────────────────
    //
    // Tests for `try_eval_topology_selector` with the `distance` helper (step-1
    // RED driver and step-3/5 contract pins).
    //
    // Step-1a (RED driver): Shape×Point happy path. Asserts that
    // `distance(shape_ref, point_ref)` with a canned ClosestPointOnShape
    // mock reply returns `Some(Value::Scalar{LENGTH, si_value ≈ 0.015})`.
    // RED before step-2 because `distance` is absent from the name-match
    // (the function returns `None` immediately at the `_ => return None` arm).

    #[test]
    fn try_eval_topology_selector_distance_shape_point_happy_path() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Box handle inserted into named_steps under member "b".
        let box_handle = reify_ir::GeometryHandleId(99);
        // Mock: ClosestPointOnShape query for (box_handle, point=(0.02,0,0))
        // replies with the closest surface point (0.005, 0.0, 0.0) as JSON.
        // Euclidean distance = |(0.02,0,0) - (0.005,0,0)| = 0.015 m.
        let mut kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            box_handle,
            [0.02, 0.0, 0.0],
            reify_ir::Value::String("{\"x\":0.005,\"y\":0.0,\"z\":0.0}".to_string()),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // args[0] = b (Shape) → named_steps by member name "b"
        named_steps.insert("b".to_string(), kh(box_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[1] = p (Point3<Length>) → values by ValueCellId
        // 20mm = 0.02m in SI
        values.insert(
            reify_core::ValueCellId::new("DistanceBoxPoint", "p"),
            point3_length_value(0.02, 0.0, 0.0),
        );

        // distance(b, p): args[0]=b (Geometry), args[1]=p (Point3<Length>)
        let expr = topology_selector_call_two_value_refs(
            "distance",
            "DistanceBoxPoint",
            "b",
            reify_core::Type::Geometry,
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Expected: Some(Value::Scalar{ dimension: LENGTH, si_value ≈ 0.015 })
        match result {
            Some(reify_ir::Value::Scalar {
                si_value,
                dimension,
            }) if dimension == reify_core::DimensionVector::LENGTH => {
                let expected = 0.015_f64;
                let epsilon = 1e-12;
                assert!(
                    (si_value - expected).abs() < epsilon,
                    "distance(box, point) si_value should be 0.015 (≈{expected:.15}), \
                     got {si_value:.15} (delta {delta:.3e})",
                    delta = (si_value - expected).abs()
                );
            }
            other => panic!(
                "distance(shape, point) with canned ClosestPointOnShape reply must return \
                 Some(Value::Scalar{{LENGTH, ≈0.015}}); got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "happy-path distance must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    // Step-3 RED tests: error-downgrade contract (invariant #3) and
    // non-ValueRef fall-through (invariant #1).
    //
    // (a) Invariant #3 (error downgrade): distance(shape_ref, point_ref) whose
    // ClosestPointOnShape query returns Err must produce Some(Value::Undef) AND
    // exactly one Severity::Warning diagnostic — NOT None.
    // RED against step-2's naive `.ok()? → None` path.
    //
    // (b) Invariant #1 (ValueRef contract): distance(<literal>, <literal>)
    // must return None without consulting the kernel.

    #[test]
    fn try_eval_topology_selector_distance_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        // No with_closest_point_on_shape_result seeding — the mock returns
        // Err(QueryError::QueryFailed(...)) for unregistered queries.
        // The step-2 naive `.ok()?` path returns None (RED); step-4 replaces
        // it with dispatch_point3_length_reply which downgrades to Undef+Warning.
        let box_handle = reify_ir::GeometryHandleId(55);
        let mut kernel = MockGeometryKernel::new(); // no canned reply for this handle+point

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("b".to_string(), kh(box_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("DistanceBoxPoint", "p"),
            point3_length_value(0.02, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "distance",
            "DistanceBoxPoint",
            "b",
            reify_core::Type::Geometry,
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "distance(shape, point) with kernel Err must yield Some(Value::Undef) \
             (not None); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("distance"),
            "diagnostic must mention the helper name 'distance'; got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_distance_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        // distance(Real(1.0), Real(2.0)) — each arg is a defined-but-unusable
        // value (not a shape, not a Point<Length>). The dispatcher returns None
        // without consulting the kernel, but under task ε (evaluate-then-accept)
        // it is no longer SILENT: the point probe emits one Severity::Warning per
        // arg naming `distance` (FLIP from the prior silent fall-through).
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("distance");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "distance(<literal>, <literal>) must return None; got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
        // FLIP (task ε): one Warning per defined-but-unusable arg, naming the
        // `distance` builtin. The result still degrades to None.
        assert_eq!(
            diagnostics.len(),
            2,
            "distance(<literal>, <literal>) must emit one Warning per arg \
             (FLIP from silent), got: {:?}",
            diagnostics
        );
        for d in &diagnostics {
            assert_eq!(d.severity, reify_core::Severity::Warning);
            assert!(
                d.message.to_lowercase().contains("distance"),
                "warning must name the distance builtin, got: {:?}",
                d.message
            );
        }
    }

    // Step-5 RED tests: remaining arg combinations (Shape×Shape, Point×Point).
    //
    // (a) Shape×Shape: both members in named_steps, kernel seeded with
    // with_distance_result(handle_a, handle_b, meters(0.04)) → must produce
    // Some(Value::Scalar{LENGTH, ≈0.04}). RED: step-4 only handles Shape×Point;
    // (Some(shapeA), None, Some(shapeB), None) hits `_ => None`.
    //
    // (b) Point×Point: both in values, no kernel call → pure Euclidean result.
    // (0,0,0) to (0.03,0.04,0.0) = 0.05 (3-4-5). RED: same placeholder.

    #[test]
    fn try_eval_topology_selector_distance_shape_shape_uses_kernel_distance() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_test_support::values::meters;
        let handle_a = reify_ir::GeometryHandleId(10);
        let handle_b = reify_ir::GeometryHandleId(11);
        // kernel_distance reads GeometryQuery::Distance{from, to} and accepts
        // Real or Scalar{LENGTH} reply. Use meters(0.04) (Value::Scalar{LENGTH}).
        let mut kernel =
            MockGeometryKernel::new().with_distance_result(handle_a, handle_b, meters(0.04));

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("a".to_string(), kh(handle_a));
        named_steps.insert("b".to_string(), kh(handle_b));

        let values = reify_ir::ValueMap::new();

        // distance(a, b): both args are Geometry (named_steps).
        let expr = topology_selector_call_two_value_refs(
            "distance",
            "ShapeShape",
            "a",
            reify_core::Type::Geometry,
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        match result {
            Some(reify_ir::Value::Scalar {
                si_value,
                dimension,
            }) if dimension == reify_core::DimensionVector::LENGTH => {
                let expected = 0.04_f64;
                let epsilon = 1e-12;
                assert!(
                    (si_value - expected).abs() < epsilon,
                    "distance(shapeA, shapeB) si_value should be 0.04; \
                     got {si_value:.15} (delta {delta:.3e})",
                    delta = (si_value - expected).abs()
                );
            }
            other => panic!(
                "distance(shapeA, shapeB) with kernel Distance reply must return \
                 Some(Value::Scalar{{LENGTH, ≈0.04}}); got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "happy-path Shape×Shape distance must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_distance_point_point_pure_euclidean() {
        use reify_test_support::mocks::CountingMockKernel;
        // Point×Point: both args resolved from values; pure Euclidean, no kernel.
        // (0,0,0) to (0.03,0.04,0) = 0.05 (3-4-5 right triangle).
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("PointPoint", "pa"),
            point3_length_value(0.0, 0.0, 0.0),
        );
        values.insert(
            reify_core::ValueCellId::new("PointPoint", "pb"),
            point3_length_value(0.03, 0.04, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "distance",
            "PointPoint",
            "pa",
            reify_core::Type::point3(reify_core::Type::length()),
            "pb",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        match result {
            Some(reify_ir::Value::Scalar {
                si_value,
                dimension,
            }) if dimension == reify_core::DimensionVector::LENGTH => {
                let expected = 0.05_f64; // |(0.03, 0.04, 0)| = 0.05 exactly
                let epsilon = 1e-12;
                assert!(
                    (si_value - expected).abs() < epsilon,
                    "distance(pointA, pointB) pure Euclidean should be 0.05; \
                     got {si_value:.15} (delta {delta:.3e})",
                    delta = (si_value - expected).abs()
                );
            }
            other => panic!(
                "distance(pointA, pointB) must return Some(Value::Scalar{{LENGTH, ≈0.05}}); \
                 got {:?}",
                other
            ),
        }
        assert_eq!(
            kernel.total_query_count(),
            0,
            "Point×Point distance must not consult the kernel; got {} queries",
            kernel.total_query_count()
        );
        assert!(
            diagnostics.is_empty(),
            "Point×Point distance must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    // ── Amendment tests for distance dispatch (reviewer suggestions) ─────────
    //
    // These tests were added as part of the code-review amendment pass to
    // address three reviewer observations:
    //
    //   1. Point×Shape happy-path (symmetric arm — could regress independently
    //      of Shape×Point after the deduplication refactor).
    //   2. Shape×Shape error-downgrade (invariant #3 for the S×S branch: kernel
    //      Err → Some(Value::Undef) + one Warning, not None).
    //   3. Invariant #4 (exactly one kernel query) for Shape×Point and Shape×Shape
    //      success paths (previously only the zero-query Point cases were pinned).

    #[test]
    fn try_eval_topology_selector_distance_point_shape_happy_path() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Point × Shape: args swapped versus the Shape×Point test; the
        // normalised dispatch should route to the same ClosestPointOnShape block.
        let box_handle = reify_ir::GeometryHandleId(99);
        let mut kernel = MockGeometryKernel::new().with_closest_point_on_shape_result(
            box_handle,
            [0.02, 0.0, 0.0],
            reify_ir::Value::String("{\"x\":0.005,\"y\":0.0,\"z\":0.0}".to_string()),
        );

        // arg0 = p (Point3<Length>) → values map
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("PointShape", "p"),
            point3_length_value(0.02, 0.0, 0.0),
        );
        // arg1 = b (Shape) → named_steps
        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("b".to_string(), kh(box_handle));

        // distance(p, b): args[0]=p (Point3), args[1]=b (Geometry)
        let expr = topology_selector_call_two_value_refs(
            "distance",
            "PointShape",
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        match result {
            Some(reify_ir::Value::Scalar {
                si_value,
                dimension,
            }) if dimension == reify_core::DimensionVector::LENGTH => {
                let expected = 0.015_f64;
                let epsilon = 1e-12;
                assert!(
                    (si_value - expected).abs() < epsilon,
                    "distance(point, shape) si_value should be 0.015 (≈{expected:.15}), \
                     got {si_value:.15} (delta {delta:.3e})",
                    delta = (si_value - expected).abs()
                );
            }
            other => panic!(
                "distance(point, shape) with canned ClosestPointOnShape reply must return \
                 Some(Value::Scalar{{LENGTH, ≈0.015}}); got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "happy-path Point×Shape distance must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_distance_shape_shape_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Shape × Shape with NO seeded Distance result → mock returns Err for
        // the unregistered (handle_a, handle_b) pair.  Invariant #3 contract:
        // kernel_distance maps Err → None → Some(Value::Undef) + one Warning.
        let handle_a = reify_ir::GeometryHandleId(10);
        let handle_b = reify_ir::GeometryHandleId(11);
        let mut kernel = MockGeometryKernel::new(); // no Distance reply seeded

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("a".to_string(), kh(handle_a));
        named_steps.insert("b".to_string(), kh(handle_b));
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_two_value_refs(
            "distance",
            "ShapeShape",
            "a",
            reify_core::Type::Geometry,
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "distance(shapeA, shapeB) with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "Shape×Shape kernel Err must emit exactly one Warning; got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("distance"),
            "diagnostic must mention the helper name 'distance'; got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_distance_shape_point_query_count_is_one() {
        use reify_test_support::mocks::{CountingMockKernel, MockGeometryKernel};
        // Invariant #4: Shape×Point success path issues exactly one kernel query.
        let box_handle = reify_ir::GeometryHandleId(99);
        let inner = MockGeometryKernel::new().with_closest_point_on_shape_result(
            box_handle,
            [0.02, 0.0, 0.0],
            reify_ir::Value::String("{\"x\":0.005,\"y\":0.0,\"z\":0.0}".to_string()),
        );
        let mut kernel = CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("b".to_string(), kh(box_handle));
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("DistanceBoxPoint", "p"),
            point3_length_value(0.02, 0.0, 0.0),
        );
        let expr = topology_selector_call_two_value_refs(
            "distance",
            "DistanceBoxPoint",
            "b",
            reify_core::Type::Geometry,
            "p",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );
        assert!(
            result.is_some(),
            "Shape×Point happy path must return Some; got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            1,
            "Shape×Point distance must issue exactly one kernel query (invariant #4); got {}",
            kernel.total_query_count()
        );
    }

    #[test]
    fn try_eval_topology_selector_distance_shape_shape_query_count_is_one() {
        use reify_test_support::mocks::{CountingMockKernel, MockGeometryKernel};
        use reify_test_support::values::meters;
        // Invariant #4: Shape×Shape success path issues exactly one kernel query.
        let handle_a = reify_ir::GeometryHandleId(10);
        let handle_b = reify_ir::GeometryHandleId(11);
        let inner =
            MockGeometryKernel::new().with_distance_result(handle_a, handle_b, meters(0.04));
        let mut kernel = CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("a".to_string(), kh(handle_a));
        named_steps.insert("b".to_string(), kh(handle_b));
        let values = reify_ir::ValueMap::new();
        let expr = topology_selector_call_two_value_refs(
            "distance",
            "ShapeShape",
            "a",
            reify_core::Type::Geometry,
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );
        assert!(
            result.is_some(),
            "Shape×Shape happy path must return Some; got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            1,
            "Shape×Shape distance must issue exactly one kernel query (invariant #4); got {}",
            kernel.total_query_count()
        );
    }

    // ── gate_query_capability unit tests (task 3623) ─────────────────────────
    //
    // These tests pin the §5.4 four-branch policy contract of
    // `gate_query_capability`. The function lives in this module (pub(crate))
    // and is tested here following the established in-module pattern for
    // `try_eval_conformance_query_*` / `try_eval_topology_selector_*`.
    //
    // Coverage map (PRD §8.1):
    //  branch-a: BRepOnly + BRep    → Occt,        zero diagnostics
    //  branch-b: BRepAndMesh + BRep → Occt,        zero diagnostics
    //  branch-c: BRepAndMesh + Mesh → Manifold,    zero diagnostics
    //  branch-d: BRepOnly + Mesh    → Unsupported, exactly-one Error
    //            with code QueryNotSupportedOnRepr, message contains
    //            helper name + repr token
    //  branch-e: any capability + Voxel/Sdf/VolumeMesh → Unsupported + diag
    //  branch-f: exhaustive no-panic loop over all 5 ReprKind values for both
    //            BRepOnly and BRepAndMesh; Unsupported ⟺ exactly-one-diagnostic

    #[test]
    fn gate_query_capability_brep_only_on_brep_routes_occt_no_diag() {
        // branch-a: BRepOnly + BRep → Occt
        let query = reify_ir::GeometryQuery::EdgeLength(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_ir::ReprKind::BRep,
            "edge_length",
            &mut diags,
        );
        assert_eq!(
            route,
            super::CapabilityRoute::Occt,
            "BRepOnly query on BRep repr must route to Occt"
        );
        assert!(
            diags.is_empty(),
            "BRepOnly on BRep must emit zero diagnostics; got: {:?}",
            diags
        );
    }

    #[test]
    fn gate_query_capability_brep_and_mesh_on_brep_routes_occt_no_diag() {
        // branch-b: BRepAndMesh + BRep → Occt
        let query = reify_ir::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route =
            super::gate_query_capability(&query, reify_ir::ReprKind::BRep, "distance", &mut diags);
        assert_eq!(
            route,
            super::CapabilityRoute::Occt,
            "BRepAndMesh query on BRep repr must route to Occt"
        );
        assert!(
            diags.is_empty(),
            "BRepAndMesh on BRep must emit zero diagnostics; got: {:?}",
            diags
        );
    }

    #[test]
    fn gate_query_capability_brep_and_mesh_on_mesh_routes_manifold_no_diag() {
        // branch-c: BRepAndMesh + Mesh → Manifold
        let query = reify_ir::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route =
            super::gate_query_capability(&query, reify_ir::ReprKind::Mesh, "distance", &mut diags);
        assert_eq!(
            route,
            super::CapabilityRoute::Manifold,
            "BRepAndMesh query on Mesh repr must route to Manifold"
        );
        assert!(
            diags.is_empty(),
            "BRepAndMesh on Mesh must emit zero diagnostics; got: {:?}",
            diags
        );
    }

    #[test]
    fn gate_query_capability_brep_only_on_mesh_fails_closed_with_diag() {
        // branch-d: BRepOnly + Mesh → Unsupported + exactly-one Error diag
        let query = reify_ir::GeometryQuery::EdgeLength(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route =
            super::gate_query_capability(&query, reify_ir::ReprKind::Mesh, "curvature", &mut diags);
        assert_eq!(
            route,
            super::CapabilityRoute::Unsupported,
            "BRepOnly query on Mesh repr must route to Unsupported (fail closed)"
        );
        assert_eq!(
            diags.len(),
            1,
            "BRepOnly on Mesh must emit exactly one diagnostic; got {} ({:?})",
            diags.len(),
            diags
        );
        let diag = &diags[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Error,
            "diagnostic severity must be Error, got {:?}",
            diag.severity
        );
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::QueryNotSupportedOnRepr),
            "diagnostic code must be QueryNotSupportedOnRepr, got {:?}",
            diag.code
        );
        assert!(
            diag.message.contains("curvature"),
            "diagnostic must contain the helper name 'curvature'; got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("Mesh"),
            "diagnostic must contain the repr token 'Mesh'; got: {}",
            diag.message
        );
    }

    #[test]
    fn gate_query_capability_any_query_on_voxel_fails_closed() {
        // branch-e (Voxel): BRepAndMesh query + Voxel → Unsupported + one diag
        // Message must say "BRep or Mesh" (not just "BRep") because Distance is BRepAndMesh.
        let query = reify_ir::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route =
            super::gate_query_capability(&query, reify_ir::ReprKind::Voxel, "distance", &mut diags);
        assert_eq!(route, super::CapabilityRoute::Unsupported);
        assert_eq!(diags.len(), 1, "Voxel repr must emit one diag: {:?}", diags);
        assert_eq!(
            diags[0].code,
            Some(reify_core::DiagnosticCode::QueryNotSupportedOnRepr)
        );
        assert!(
            diags[0].message.contains("BRep or Mesh"),
            "BRepAndMesh query on Voxel must say 'BRep or Mesh', not just 'BRep'; \
             got: {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("Voxel"),
            "diagnostic must contain repr token 'Voxel'; got: {}",
            diags[0].message
        );
    }

    #[test]
    fn gate_query_capability_any_query_on_sdf_fails_closed() {
        // branch-e (Sdf): BRepAndMesh query + Sdf → Unsupported + one diag
        // Message must say "BRep or Mesh" because Volume is BRepAndMesh.
        let query = reify_ir::GeometryQuery::Volume(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route =
            super::gate_query_capability(&query, reify_ir::ReprKind::Sdf, "volume", &mut diags);
        assert_eq!(route, super::CapabilityRoute::Unsupported);
        assert_eq!(diags.len(), 1, "Sdf repr must emit one diag: {:?}", diags);
        assert_eq!(
            diags[0].code,
            Some(reify_core::DiagnosticCode::QueryNotSupportedOnRepr)
        );
        assert!(
            diags[0].message.contains("BRep or Mesh"),
            "BRepAndMesh query on Sdf must say 'BRep or Mesh'; got: {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("Sdf"),
            "diagnostic must contain repr token 'Sdf'; got: {}",
            diags[0].message
        );
    }

    #[test]
    fn gate_query_capability_any_query_on_volume_mesh_fails_closed() {
        // branch-e (VolumeMesh): BRepAndMesh query + VolumeMesh → Unsupported + one diag
        // Message must say "BRep or Mesh" because BoundingBox is BRepAndMesh.
        let query = reify_ir::GeometryQuery::BoundingBox(GeometryHandleId(1));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let route = super::gate_query_capability(
            &query,
            reify_ir::ReprKind::VolumeMesh,
            "bounding_box",
            &mut diags,
        );
        assert_eq!(route, super::CapabilityRoute::Unsupported);
        assert_eq!(
            diags.len(),
            1,
            "VolumeMesh repr must emit one diag: {:?}",
            diags
        );
        assert_eq!(
            diags[0].code,
            Some(reify_core::DiagnosticCode::QueryNotSupportedOnRepr)
        );
        assert!(
            diags[0].message.contains("BRep or Mesh"),
            "BRepAndMesh query on VolumeMesh must say 'BRep or Mesh'; got: {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("VolumeMesh"),
            "diagnostic must contain repr token 'VolumeMesh'; got: {}",
            diags[0].message
        );
    }

    #[test]
    fn gate_query_capability_exhaustive_no_panic_unsupported_iff_one_diag() {
        // branch-f: no-panic loop over all 5 ReprKind values for two queries
        // (one BRepOnly, one BRepAndMesh); invariant: Unsupported ⟺ exactly
        // one diagnostic with code QueryNotSupportedOnRepr.
        let all_reprs = [
            reify_ir::ReprKind::BRep,
            reify_ir::ReprKind::Mesh,
            reify_ir::ReprKind::Sdf,
            reify_ir::ReprKind::Voxel,
            reify_ir::ReprKind::VolumeMesh,
        ];
        let brep_only_query = reify_ir::GeometryQuery::EdgeLength(GeometryHandleId(1));
        let brep_and_mesh_query = reify_ir::GeometryQuery::Distance {
            from: GeometryHandleId(1),
            to: GeometryHandleId(2),
        };
        for repr in all_reprs {
            for (query, label) in [
                (&brep_only_query, "edge_length"),
                (&brep_and_mesh_query, "distance"),
            ] {
                let mut diags: Vec<Diagnostic> = Vec::new();
                let route = super::gate_query_capability(query, repr, label, &mut diags);
                if matches!(route, super::CapabilityRoute::Unsupported) {
                    assert_eq!(
                        diags.len(),
                        1,
                        "Unsupported route for {label}/{repr:?} must emit exactly one diag; got {} ({:?})",
                        diags.len(),
                        diags
                    );
                    assert_eq!(
                        diags[0].code,
                        Some(reify_core::DiagnosticCode::QueryNotSupportedOnRepr),
                        "Unsupported diag must carry QueryNotSupportedOnRepr code"
                    );
                } else {
                    assert!(
                        diags.is_empty(),
                        "non-Unsupported route for {label}/{repr:?} must emit zero diagnostics; got: {:?}",
                        diags
                    );
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Unit tests for `quaternion_from_z_to_axis` (task 3463 amend pass)
    //
    // Covers the four canonical axis directions and the degenerate (-Z) fallback.
    // Each test verifies unit norm AND correct component values.
    // ─────────────────────────────────────────────────────────────────────────

    /// Helper: apply quaternion `(w, qx, qy, qz)` to a pure vector `(vx, vy, vz)`.
    /// Returns the rotated vector `[rx, ry, rz]`.
    ///
    /// Uses the Rodrigues-style formula:
    ///   `v' = v + 2w*(q_vec × v) + 2*(q_vec × (q_vec × v))`
    fn quat_rotate(w: f64, qx: f64, qy: f64, qz: f64, vx: f64, vy: f64, vz: f64) -> [f64; 3] {
        let cx = qy * vz - qz * vy;
        let cy = qz * vx - qx * vz;
        let cz = qx * vy - qy * vx;
        let dx = qy * cz - qz * cy;
        let dy = qz * cx - qx * cz;
        let dz = qx * cy - qy * cx;
        [
            vx + 2.0 * w * cx + 2.0 * dx,
            vy + 2.0 * w * cy + 2.0 * dy,
            vz + 2.0 * w * cz + 2.0 * dz,
        ]
    }

    /// Helper: extract `(w, x, y, z)` from a `Value::Orientation`.
    fn orientation_components(v: reify_ir::Value) -> (f64, f64, f64, f64) {
        match v {
            reify_ir::Value::Orientation { w, x, y, z } => (w, x, y, z),
            other => panic!("expected Value::Orientation, got {other:?}"),
        }
    }

    /// `+Z → +Z`: shortest arc is zero rotation → identity quaternion.
    ///
    /// All arithmetic is exact (w_unnorm = 2.0, len = 2.0, components are
    /// integer multiples of 0.5), so `assert_eq!` with bit-exact comparison
    /// is appropriate here.
    #[test]
    fn quaternion_from_z_to_axis_z_plus_is_identity() {
        let q = super::quaternion_from_z_to_axis(0.0, 0.0, 1.0);
        assert_eq!(
            q,
            reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            "+Z → +Z should yield identity quaternion"
        );
    }

    /// `(0, 0, -1)` is the degenerate case (anti-parallel). The function
    /// falls back to a 180° rotation around +X: `{w:0, x:1, y:0, z:0}`.
    #[test]
    fn quaternion_from_z_to_axis_z_minus_gives_180_around_x() {
        let q = super::quaternion_from_z_to_axis(0.0, 0.0, -1.0);
        assert_eq!(
            q,
            reify_ir::Value::Orientation {
                w: 0.0,
                x: 1.0,
                y: 0.0,
                z: 0.0
            },
            "-Z degenerate case should fall back to 180° around +X"
        );
        // Round-trip: applying the quaternion to (0,0,1) should give (0,0,-1).
        let (w, x, y, z) = orientation_components(q);
        let rotated = quat_rotate(w, x, y, z, 0.0, 0.0, 1.0);
        assert!(
            rotated[0].abs() < 1e-12
                && rotated[1].abs() < 1e-12
                && (rotated[2] + 1.0).abs() < 1e-12,
            "180°/+X applied to +Z should give -Z, got {rotated:?}"
        );
    }

    /// `+X axis`: shortest arc from +Z to +X is 90° around +Y.
    /// `quaternion_from_z_to_axis(1,0,0)` → `{w: 1/√2, x: 0, y: 1/√2, z: 0}`.
    #[test]
    fn quaternion_from_z_to_axis_x_plus_unit_norm_and_round_trip() {
        let q = super::quaternion_from_z_to_axis(1.0, 0.0, 0.0);
        let (w, x, y, z) = orientation_components(q);
        let norm = (w * w + x * x + y * y + z * z).sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-12,
            "+X axis: quaternion should be unit-norm; norm={norm}"
        );
        let sqrt2_inv = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (w - sqrt2_inv).abs() < 1e-12,
            "+X axis: w should be 1/√2; got {w}"
        );
        assert!(x.abs() < 1e-12, "+X axis: x should be 0; got {x}");
        assert!(
            (y - sqrt2_inv).abs() < 1e-12,
            "+X axis: y should be 1/√2; got {y}"
        );
        assert!(z.abs() < 1e-12, "+X axis: z should be 0; got {z}");
        // Round-trip: q applied to (0,0,1) should give (1,0,0).
        let rotated = quat_rotate(w, x, y, z, 0.0, 0.0, 1.0);
        assert!(
            (rotated[0] - 1.0).abs() < 1e-12
                && rotated[1].abs() < 1e-12
                && rotated[2].abs() < 1e-12,
            "+X round-trip: expected (1,0,0), got {rotated:?}"
        );
    }

    /// `+Y axis`: shortest arc from +Z to +Y is 90° around -X.
    /// `quaternion_from_z_to_axis(0,1,0)` → `{w: 1/√2, x: -1/√2, y: 0, z: 0}`.
    #[test]
    fn quaternion_from_z_to_axis_y_plus_unit_norm_and_round_trip() {
        let q = super::quaternion_from_z_to_axis(0.0, 1.0, 0.0);
        let (w, x, y, z) = orientation_components(q);
        let norm = (w * w + x * x + y * y + z * z).sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-12,
            "+Y axis: quaternion should be unit-norm; norm={norm}"
        );
        let sqrt2_inv = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (w - sqrt2_inv).abs() < 1e-12,
            "+Y axis: w should be 1/√2; got {w}"
        );
        assert!(
            (x + sqrt2_inv).abs() < 1e-12,
            "+Y axis: x should be -1/√2; got {x}"
        );
        assert!(y.abs() < 1e-12, "+Y axis: y should be 0; got {y}");
        assert!(z.abs() < 1e-12, "+Y axis: z should be 0; got {z}");
        // Round-trip: q applied to (0,0,1) should give (0,1,0).
        let rotated = quat_rotate(w, x, y, z, 0.0, 0.0, 1.0);
        assert!(
            rotated[0].abs() < 1e-12
                && (rotated[1] - 1.0).abs() < 1e-12
                && rotated[2].abs() < 1e-12,
            "+Y round-trip: expected (0,1,0), got {rotated:?}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // FrameSubShapeKind::from_selector_kind conversion contract
    //
    // These tests pin the Face→Some(Face), Edge→Some(Edge), Point→None contract
    // for the narrowed enum that eliminates `unreachable!()` arms in the
    // kernel-aware dispatch path.  Face and Edge are the only sub-shape kinds
    // that reach `construct_frame_from_kernel`; Point is filtered to `None` so
    // the dispatcher's `?` early-returns without ever reaching kernel queries.
    // ─────────────────────────────────────────────────────────────────────────

    /// Pins the full `FrameSubShapeKind::from_selector_kind` conversion contract:
    /// - `Face`  → `Some(Face)` — kernel path handles @face via FaceNormal query
    /// - `Edge`  → `Some(Edge)` — kernel path handles @edge via EdgeTangent query
    /// - `Point` → `None`       — @point is resolved by Layer-1; `?` propagates
    ///   None without ever reaching kernel dispatch
    #[test]
    fn frame_sub_shape_kind_from_selector_kind_contract() {
        assert_eq!(
            super::FrameSubShapeKind::from_selector_kind(&reify_ir::SelectorKind::Face),
            Some(super::FrameSubShapeKind::Face),
            "SelectorKind::Face should convert to Some(FrameSubShapeKind::Face)"
        );
        assert_eq!(
            super::FrameSubShapeKind::from_selector_kind(&reify_ir::SelectorKind::Edge),
            Some(super::FrameSubShapeKind::Edge),
            "SelectorKind::Edge should convert to Some(FrameSubShapeKind::Edge)"
        );
        assert!(
            super::FrameSubShapeKind::from_selector_kind(&reify_ir::SelectorKind::Point).is_none(),
            "SelectorKind::Point should convert to None — point selectors are \
             handled by Layer-1 eval_expr and must not reach kernel dispatch"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // construct_frame_from_kernel with narrowed FrameSubShapeKind signature
    //
    // These tests exercise the two match arms inside construct_frame_from_kernel
    // directly, locking the Face↔FaceNormal and Edge↔EdgeTangent dispatch.
    // They are RED until step 4 changes the function signature from
    // `selector_kind: &SelectorKind` to `sub_shape_kind: FrameSubShapeKind`.
    // ─────────────────────────────────────────────────────────────────────────

    /// `construct_frame_from_kernel` with `FrameSubShapeKind::Face` must query
    /// `GeometryQuery::FaceNormal` for the basis and return a `Value::Frame`
    /// whose origin is the centroid and whose basis is the identity quaternion
    /// when the face normal is +Z (the CAD standard orientation for a top cap).
    ///
    /// Pins the Face↔FaceNormal dispatch: the Face arm must use FaceNormal, not
    /// EdgeTangent.  With centroid (0, 0, 0.01) and normal (0, 0, 1) (both +Z),
    /// `quaternion_from_z_to_axis(0, 0, 1)` produces the identity quaternion
    /// (w=1, x=0, y=0, z=0) — exact IEEE 754.
    #[test]
    fn construct_frame_from_kernel_face_returns_frame_from_centroid_and_face_normal() {
        use reify_test_support::mocks::MockGeometryKernel;

        let target = reify_ir::GeometryHandleId(10);
        let centroid_json = reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":0.01}"#.to_string());
        let normal_json = reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
        let mut kernel = MockGeometryKernel::new()
            .with_centroid_result(target, centroid_json)
            .with_face_normal_result(target, normal_json);
        let mut diagnostics = Vec::new();

        let result = super::construct_frame_from_kernel(
            target,
            super::FrameSubShapeKind::Face,
            &mut kernel,
            &mut diagnostics,
        );

        let Some(reify_ir::Value::Frame {
            ref origin,
            ref basis,
        }) = result
        else {
            panic!(
                "construct_frame_from_kernel(Face) should return Some(Value::Frame {{ .. }}); got {:?}",
                result
            );
        };
        assert_eq!(
            **origin,
            reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.01),
            ]),
            "Face: origin should be centroid (0m, 0m, 0.01m)"
        );
        assert_eq!(
            **basis,
            reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            "Face: basis should be identity (FaceNormal +Z → +Z = zero rotation)"
        );
        assert!(
            diagnostics.is_empty(),
            "Face: no diagnostics expected on clean kernel results; got {:?}",
            diagnostics
        );
    }

    /// `construct_frame_from_kernel` with `FrameSubShapeKind::Edge` must query
    /// `GeometryQuery::EdgeTangent` for the basis and return a `Value::Frame`
    /// whose origin is the centroid and whose basis is the identity quaternion
    /// when the edge tangent is +Z.
    ///
    /// Pins the Edge↔EdgeTangent dispatch: the Edge arm must use EdgeTangent,
    /// not FaceNormal.  With centroid (0, 0, 0.005) and tangent (0, 0, 1),
    /// `quaternion_from_z_to_axis(0, 0, 1)` produces identity — exact IEEE 754.
    #[test]
    fn construct_frame_from_kernel_edge_returns_frame_from_centroid_and_edge_tangent() {
        use reify_test_support::mocks::MockGeometryKernel;

        let target = reify_ir::GeometryHandleId(20);
        let centroid_json = reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":0.005}"#.to_string());
        let tangent_json = reify_ir::Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
        let mut kernel = MockGeometryKernel::new()
            .with_centroid_result(target, centroid_json)
            .with_edge_tangent_result(target, tangent_json);
        let mut diagnostics = Vec::new();

        let result = super::construct_frame_from_kernel(
            target,
            super::FrameSubShapeKind::Edge,
            &mut kernel,
            &mut diagnostics,
        );

        let Some(reify_ir::Value::Frame {
            ref origin,
            ref basis,
        }) = result
        else {
            panic!(
                "construct_frame_from_kernel(Edge) should return Some(Value::Frame {{ .. }}); got {:?}",
                result
            );
        };
        assert_eq!(
            **origin,
            reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.005),
            ]),
            "Edge: origin should be centroid (0m, 0m, 0.005m)"
        );
        assert_eq!(
            **basis,
            reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            "Edge: basis should be identity (EdgeTangent +Z → +Z = zero rotation)"
        );
        assert!(
            diagnostics.is_empty(),
            "Edge: no diagnostics expected on clean kernel results; got {:?}",
            diagnostics
        );
    }

    #[test]
    fn cap_kind_translation_maps_all_canonical_labels_and_returns_none_for_unknown() {
        use reify_ir::{CapKind, Role};
        let cases: &[(&str, Option<(Role, u32)>)] = &[
            ("top", Some((Role::Cap(CapKind::Top), 0))),
            ("bottom", Some((Role::Cap(CapKind::Bottom), 0))),
            ("start", Some((Role::Cap(CapKind::Start), 0))),
            ("end", Some((Role::Cap(CapKind::End), 0))),
            ("side", Some((Role::Side, 0))),
            ("nonexistent", None),
        ];
        for (label, expected) in cases {
            assert_eq!(
                cap_kind_translation(label),
                *expected,
                "label {:?} should map to {:?}",
                label,
                expected
            );
        }
    }

    // ── try_eval_topology_selector `geo_equiv` unit tests (task 3613, KGQ-δ) ──
    //
    // These tests pin the `geo_equiv(left, right, tol) -> Bool` dispatch
    // contract (PRD §9 KGQ-δ). Args[0]/args[1] are Geometry ValueRefs resolved
    // via named_steps; args[2] is a Length scalar ValueRef resolved via values
    // to SI metres. The dispatcher reuses `dispatch_point_on_shape`
    // (Bool unwrapper), threading `DEFAULT_GEO_EQUIV_SAMPLE_COUNT` to the FFI.
    //
    // Four contracts (mirror the four `try_eval_topology_selector_contains_*`):
    //   (a) happy path: kernel Bool(true) reply → Some(Value::Bool(true)), no diags
    //   (b) literal-arg fall-through: 3 literal args → None, zero kernel calls
    //   (c) non-Bool kernel reply → Some(Value::Undef) + exactly-one Warning
    //       naming "geo_equiv" and "non-Bool"
    //   (d) kernel-Err: no seeding → Some(Value::Undef) + exactly-one Warning
    //       naming "geo_equiv" and "kernel query failed"
    //
    // All four FAIL (RED) until step-6 wires the `geo_equiv` arm in
    // try_eval_topology_selector / TopologySelectorHelper.

    /// Build a `CompiledExpr` for `helper(member_a, member_b, member_c)` where
    /// all three args are ValueRefs. Mirrors `topology_selector_call_two_value_refs`
    /// but with a third arg — used by the geo_equiv 3-arg dispatch tests.
    #[allow(clippy::too_many_arguments)]
    fn topology_selector_call_three_value_refs(
        helper_name: &str,
        entity: &str,
        member_a: &str,
        type_a: reify_core::Type,
        member_b: &str,
        type_b: reify_core::Type,
        member_c: &str,
        type_c: reify_core::Type,
        result_type: reify_core::Type,
    ) -> reify_ir::CompiledExpr {
        let arg_a = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_a),
            type_a,
        );
        let arg_b = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_b),
            type_b,
        );
        let arg_c = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_c),
            type_c,
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        content_hash = content_hash.combine(arg_c.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_a, arg_b, arg_c],
            },
            result_type,
            content_hash,
        }
    }

    /// Build a `CompiledExpr` for `helper(<literal>, <literal>, <literal>)` —
    /// used for 3-arg literal fall-through defensive tests. Mirrors
    /// `topology_selector_call_literal_args` but with three args so the arity
    /// gate for arity-3 helpers (like `geo_equiv`) passes.
    fn topology_selector_call_three_literal_args(helper_name: &str) -> reify_ir::CompiledExpr {
        let arg_a = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(1.0),
            reify_core::Type::dimensionless_scalar(),
        );
        let arg_b = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(2.0),
            reify_core::Type::dimensionless_scalar(),
        );
        let arg_c = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(3.0),
            reify_core::Type::dimensionless_scalar(),
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        content_hash = content_hash.combine(arg_c.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_a, arg_b, arg_c],
            },
            result_type: reify_core::Type::Bool,
            content_hash,
        }
    }

    #[test]
    fn try_eval_topology_selector_geo_equiv_kernel_reply_returns_bool() {
        use reify_test_support::mocks::MockGeometryKernel;
        let left_handle = reify_ir::GeometryHandleId(41);
        let right_handle = reify_ir::GeometryHandleId(42);
        let tol = 1e-6_f64;
        // Record the mock using `with_geo_equiv_result` — pins that the
        // dispatcher builds `GeometryQuery::GeoEquiv { left, right, tolerance }`.
        let mut kernel = MockGeometryKernel::new().with_geo_equiv_result(
            left_handle,
            right_handle,
            tol,
            reify_ir::Value::Bool(true),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // args[0] = left geometry → resolved via named_steps by member "left"
        named_steps.insert("left".to_string(), kh(left_handle));
        // args[1] = right geometry → resolved via named_steps by member "right"
        named_steps.insert("right".to_string(), kh(right_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[2] = tolerance → resolved via values by ValueCellId
        values.insert(
            reify_core::ValueCellId::new("GeoEquivSmoke", "tol"),
            reify_ir::Value::length(tol),
        );

        // geo_equiv(left, right, tol): args[0]=left (Geometry), args[1]=right (Geometry),
        //                              args[2]=tol (Scalar<Length>)
        let expr = topology_selector_call_three_value_refs(
            "geo_equiv",
            "GeoEquivSmoke",
            "left",
            reify_core::Type::Geometry,
            "right",
            reify_core::Type::Geometry,
            "tol",
            reify_core::Type::length(),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Bool(true)),
            "geo_equiv(left, right, tol) with kernel reply Bool(true) must produce \
             Some(Value::Bool(true)); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path geo_equiv must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_geo_equiv_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        // `geo_equiv(<literal>, <literal>, <literal>)` — non-ValueRef args, so
        // resolve_geometry_handle_arg returns None on args[0], and the dispatcher
        // must return None without consulting the kernel.
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_three_literal_args("geo_equiv");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "geo_equiv(<literal>, <literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    #[test]
    fn try_eval_topology_selector_geo_equiv_non_bool_kernel_reply_emits_warning_and_returns_undef()
    {
        use reify_test_support::mocks::MockGeometryKernel;
        // Pin the `Ok(other)` warning arm of `dispatch_point_on_shape` (reused
        // for `geo_equiv`): a kernel reply that is not `Value::Bool(_)` must
        // produce `Some(Value::Undef)` with a Warning diagnostic naming
        // "geo_equiv" and "non-Bool".
        let left_handle = reify_ir::GeometryHandleId(41);
        let right_handle = reify_ir::GeometryHandleId(42);
        let tol = 1e-6_f64;
        let mut kernel = MockGeometryKernel::new().with_geo_equiv_result(
            left_handle,
            right_handle,
            tol,
            reify_ir::Value::Real(0.5), // Wrong type — triggers non-Bool warning arm
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("left".to_string(), kh(left_handle));
        named_steps.insert("right".to_string(), kh(right_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("GeoEquivSmoke", "tol"),
            reify_ir::Value::length(tol),
        );

        let expr = topology_selector_call_three_value_refs(
            "geo_equiv",
            "GeoEquivSmoke",
            "left",
            reify_core::Type::Geometry,
            "right",
            reify_core::Type::Geometry,
            "tol",
            reify_core::Type::length(),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "geo_equiv(...) with non-Bool kernel reply must yield Some(Value::Undef); \
             got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Bool reply must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("geo_equiv"),
            "diagnostic must mention the helper name 'geo_equiv', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("non-Bool"),
            "diagnostic must indicate the non-Bool reply, got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_geo_equiv_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        // No `with_geo_equiv_result` seeding — MockGeometryKernel.query() falls
        // through to the generic handle-only map which also has no entry for
        // this handle, so it returns `Err(QueryError::QueryFailed(...))`.
        // `dispatch_point_on_shape` must downgrade this to `Some(Value::Undef)`
        // and emit exactly one Warning diagnostic naming "geo_equiv" and
        // "kernel query failed". Pins the `Err(err)` arm of that helper.
        let left_handle = reify_ir::GeometryHandleId(41);
        let right_handle = reify_ir::GeometryHandleId(42);
        let tol = 1e-6_f64;
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("left".to_string(), kh(left_handle));
        named_steps.insert("right".to_string(), kh(right_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("GeoEquivSmoke", "tol"),
            reify_ir::Value::length(tol),
        );

        let expr = topology_selector_call_three_value_refs(
            "geo_equiv",
            "GeoEquivSmoke",
            "left",
            reify_core::Type::Geometry,
            "right",
            reify_core::Type::Geometry,
            "tol",
            reify_core::Type::length(),
            reify_core::Type::Bool,
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "geo_equiv(...) with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("geo_equiv"),
            "diagnostic must mention the helper name 'geo_equiv', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("kernel query failed"),
            "diagnostic must indicate the kernel failure, got: {}",
            diag.message
        );
    }

    // ── try_eval_topology_selector — `normal` dispatch unit tests (task 3615, KGQ-ζ) ─────────
    //
    // Four contracts (mirrors the four `try_eval_topology_selector_contains_*` tests):
    //   (a) HAPPY        — kernel reply `Value::String("{\"x\":0,\"y\":0,\"z\":1}")` → `Some(Value::Vector([Real(0), Real(0), Real(1)]))`
    //   (b) FALL-THROUGH — non-ValueRef literal args → `None` with zero kernel calls
    //   (c) ERROR        — kernel `Err` → `Some(Value::Undef)` + exactly one Warning mentioning "normal" + "kernel query failed"
    //   (d) MALFORMED    — non-`Value::String` kernel reply → `Some(Value::Undef)` + exactly one Warning
    //
    // Arg order: Surface = args[0] (resolved via named_steps["surface"]),
    //            Point3  = args[1] (resolved via values[(entity, "pt")]).
    // This mirrors the `contains(solid, point)` precedent (KGQ-β), NOT closest_point.
    //
    // Depends on `MockGeometryKernel::with_face_normal_at_result` (pre-1, task 3615).

    #[test]
    fn try_eval_topology_selector_normal_kernel_reply_returns_vec3_real() {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_handle = reify_ir::GeometryHandleId(55);
        // Stage the mock: point (0m, 0m, 0.005m) ≈ (0, 0, 5mm) in SI.
        // The kernel wire format for FaceNormalAt is the same JSON-Point3 encoding
        // as FaceNormal / surface_normal_at: {"x":_,"y":_,"z":_}.
        let mut kernel = MockGeometryKernel::new().with_face_normal_at_result(
            face_handle,
            [0.0, 0.0, 0.005],
            reify_ir::Value::String("{\"x\":0,\"y\":0,\"z\":1}".to_string()),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // args[0] = surface → resolved via named_steps by member name "surface"
        named_steps.insert("surface".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[1] = point3 → resolved via values by ValueCellId
        values.insert(
            reify_core::ValueCellId::new("NormalSmoke", "pt"),
            point3_length_value(0.0, 0.0, 0.005),
        );

        // normal(surface_ref, point3_ref) — Surface=args[0], Point3=args[1]
        let expr = topology_selector_call_two_value_refs(
            "normal",
            "NormalSmoke",
            "surface",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ])),
            "normal(surface, point3) with kernel reply {{x:0,y:0,z:1}} must produce \
             Some(Value::Vector([Real(0),Real(0),Real(1)])); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path normal must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn try_eval_topology_selector_normal_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        // `normal(<literal>, <literal>)` — non-ValueRef args: both
        // resolve_geometry_handle_arg (args[0]) and resolve_point3_length_arg (args[1])
        // return None, so the dispatcher must return None without consulting the kernel.
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("normal");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "normal(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args"
        );
    }

    /// Guard the point-arg LENGTH-qualification contract end-to-end through the
    /// dispatcher.  Even when args[0] (surface) resolves successfully via
    /// `named_steps`, a non-LENGTH-qualified point arg[1] must cause
    /// `resolve_point3_length_arg` to return `None`, which propagates as a
    /// silent fall-through (`None`) — zero kernel calls, zero diagnostics.
    ///
    /// Complements `try_eval_topology_selector_normal_literal_args_falls_through_to_none`
    /// (which tests BOTH args failing at the arg-shape level) by exercising the
    /// case where the SURFACE resolves but the POINT fails its unit-qualification
    /// check.  Locks the split-arg fall-through path: the result still degrades
    /// to None, but under task ε the point failure now emits exactly one
    /// Severity::Warning (no longer silent).
    #[test]
    fn try_eval_topology_selector_normal_dimensionless_point_falls_through_to_none() {
        use reify_test_support::mocks::{CountingMockKernel, MockGeometryKernel};
        let face_handle = reify_ir::GeometryHandleId(55);

        // Wrap in a counting mock so we can assert zero kernel queries.
        let inner = MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        // args[0] = surface → present in named_steps, so resolve_geometry_handle_arg
        // returns Some(face_handle).
        named_steps.insert("surface".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[1] = point → bare Value::Real components, NOT Value::Scalar with
        // DimensionVector::LENGTH.  resolve_point3_length_arg returns None for
        // this shape (the `_ => return None` arm on the component match) AND, under
        // task ε (evaluate-then-accept), pushes exactly one Severity::Warning
        // naming the builtin / arg / expected Point<Length> — the surface resolves,
        // so the point probe IS reached and the defined-but-wrong value is no
        // longer silent.
        values.insert(
            reify_core::ValueCellId::new("NormalSmoke", "pt"),
            reify_ir::Value::Point(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(5.0),
            ]),
        );

        // normal(surface_ref, pt_ref) — same arg shape as the happy-path test,
        // but with a dimensionless-Real point value.
        let expr = topology_selector_call_two_value_refs(
            "normal",
            "NormalSmoke",
            "surface",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "normal(surface, dimensionless_point) must return None (fall-through); \
             got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted when point arg is not LENGTH-qualified; \
             got {} query calls",
            kernel.total_query_count()
        );
        // FLIP (task ε): the defined-but-wrong point (bare-Real components) is no
        // longer a silent fall-through — the point probe pushes exactly one
        // Severity::Warning naming the `normal` builtin and the expected
        // Point<Length>. The result still degrades to None with no kernel call.
        assert_eq!(
            diagnostics.len(),
            1,
            "dimensionless-point fall-through must emit exactly 1 Warning (FLIP \
             from silent); got: {:?}",
            diagnostics
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);
        let msg = diagnostics[0].message.to_lowercase();
        assert!(
            msg.contains("normal"),
            "warning must name the normal builtin; got: {:?}",
            diagnostics[0].message
        );
        assert!(
            msg.contains("point<length>"),
            "warning must name expected Point<Length>; got: {:?}",
            diagnostics[0].message
        );
    }

    #[test]
    fn try_eval_topology_selector_normal_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        // No `with_face_normal_at_result` staging — MockGeometryKernel.query() falls
        // through to the generic handle-only map which also has no entry for this
        // handle, so it returns `Err(QueryError::QueryFailed(...))`.
        // `dispatch_normal_vector3` must downgrade this to `Some(Value::Undef)`
        // and emit exactly one Warning diagnostic naming "normal" + "kernel query failed".
        let face_handle = reify_ir::GeometryHandleId(55);
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("surface".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("NormalSmoke", "pt"),
            point3_length_value(0.0, 0.0, 0.005),
        );

        let expr = topology_selector_call_two_value_refs(
            "normal",
            "NormalSmoke",
            "surface",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "normal(...) with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("normal"),
            "diagnostic must mention the helper name 'normal', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("kernel query failed"),
            "diagnostic must indicate the kernel failure, got: {}",
            diag.message
        );
    }

    #[test]
    fn try_eval_topology_selector_normal_malformed_kernel_reply_emits_warning_and_returns_undef() {
        use reify_test_support::mocks::MockGeometryKernel;
        // Stage a non-`Value::String` reply (Value::Real) — parse_xyz_value rejects
        // non-String replies, so dispatch_normal_vector3 must produce
        // `Some(Value::Undef)` with a Warning diagnostic naming "normal".
        let face_handle = reify_ir::GeometryHandleId(55);
        let mut kernel = MockGeometryKernel::new().with_face_normal_at_result(
            face_handle,
            [0.0, 0.0, 0.005],
            // Wrong type: a Real, not the expected JSON String.
            reify_ir::Value::Real(42.0),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("surface".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("NormalSmoke", "pt"),
            point3_length_value(0.0, 0.0, 0.005),
        );

        let expr = topology_selector_call_two_value_refs(
            "normal",
            "NormalSmoke",
            "surface",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "normal(...) with malformed kernel reply must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "malformed reply must emit exactly one Warning, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("normal"),
            "diagnostic must mention the helper name 'normal', got: {}",
            diag.message
        );
    }

    // ── step-5 (task 4118 γ): ResolveSelector kernel-bearing eval tests ──────
    //
    // These pin `try_eval_resolve_selector`, the kernel-bearing dispatch for the
    // compiler-inserted `ResolveSelector` coercion node (and `IndexAccess` over a
    // selector). It reconstructs the inner `Value::Selector` INLINE from the
    // nested selector FunctionCall (sidestepping value-cell ordering), calls the
    // single `topology_selectors::resolve` executor, and wraps the resulting
    // canonical-order handle ids as `Value::List(Value::GeometryHandle)`
    // sub-handles via `make_sub_handle`. RED until step-6 adds the function.

    /// `ResolveSelector { faces(b) }` resolves the All-face leaf via the kernel
    /// and yields a `Value::List` of three `Value::GeometryHandle` sub-handles,
    /// matching a direct `resolve()` + `make_sub_handle`: canonical TopExp order,
    /// per-element hashing, parent realization_ref inherited.
    #[test]
    fn resolve_selector_faces_all_yields_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("BoxFaces", 0);
        let parent_hash: [u8; 32] = [0x55; 32];

        let mut kernel = MockGeometryKernel::new().with_extracted_faces(
            parent_handle,
            vec![
                GeometryHandleId(2),
                GeometryHandleId(3),
                GeometryHandleId(4),
            ],
        );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("BoxFaces", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );

        // ResolveSelector { faces(b) } — inner selector is a nested FunctionCall,
        // reconstructed inline (no value-cell ordering dependency).
        let inner = topology_selector_call_one_value_ref(
            "faces",
            "BoxFaces",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let expr = reify_ir::CompiledExpr::resolve_selector(inner);

        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &HashMap::new(),
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "ResolveSelector{{faces(b)}} must yield Some(Value::List(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(list.len(), 3, "expected 3 resolved face sub-handles");

        let expected_ids = [
            GeometryHandleId(2),
            GeometryHandleId(3),
            GeometryHandleId(4),
        ];
        for (i, (elem, expected_id)) in list.iter().zip(&expected_ids).enumerate() {
            let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
                &parent_hash,
                crate::topology_selectors::SubKind::Face,
                i as u32,
            );
            match elem {
                reify_ir::Value::GeometryHandle {
                    realization_ref,
                    upstream_values_hash,
                    kernel_handle,
                } => {
                    assert_eq!(
                        realization_ref, &parent_rr,
                        "elem[{i}] realization_ref must inherit parent"
                    );
                    assert_eq!(kernel_handle, &Some(*expected_id), "elem[{i}] kernel_handle");
                    assert_eq!(
                        upstream_values_hash, &expected_hash,
                        "elem[{i}] hash must be compose_sub_handle_hash(parent, Face, {i})"
                    );
                }
                other => panic!("elem[{i}] must be Value::GeometryHandle, got {:?}", other),
            }
        }
        assert!(
            diagnostics.is_empty(),
            "successful resolve must emit zero diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `IndexAccess { object: ResolveSelector { faces(b) }, index: 0 }` recomputes
    /// to the indexed sub-handle (the curvature_smoke `faces(s)[0]` shape): resolve
    /// the selector to its list then index — element 0 is the canonical first face.
    #[test]
    fn resolve_selector_index_access_returns_indexed_handle() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("BoxFaces", 0);
        let parent_hash: [u8; 32] = [0x55; 32];

        let mut kernel = MockGeometryKernel::new().with_extracted_faces(
            parent_handle,
            vec![
                GeometryHandleId(2),
                GeometryHandleId(3),
                GeometryHandleId(4),
            ],
        );
        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("BoxFaces", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );

        let inner = topology_selector_call_one_value_ref(
            "faces",
            "BoxFaces",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let object = reify_ir::CompiledExpr::resolve_selector(inner);
        let index = reify_ir::CompiledExpr::literal(reify_ir::Value::Int(0), Type::Int);
        let expr = reify_ir::CompiledExpr::index_access(object, index, Type::Geometry);

        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &HashMap::new(),
            &mut diagnostics,
        );

        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Face,
            0,
        );
        match result {
            Some(reify_ir::Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            }) => {
                assert_eq!(realization_ref, parent_rr, "indexed handle realization_ref");
                assert_eq!(
                    kernel_handle,
                    Some(GeometryHandleId(2)),
                    "faces(b)[0] → canonical first face GHId(2)"
                );
                assert_eq!(
                    upstream_values_hash, expected_hash,
                    "indexed handle hash == compose_sub_handle_hash(parent, Face, 0)"
                );
            }
            other => panic!(
                "faces(b)[0] must yield Some(Value::GeometryHandle(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "successful index must emit zero diagnostics; got {:?}",
            diagnostics
        );
    }

    // ── step-5 (task 4119 δ): composition-algebra eval + resolve tests ─────────
    //
    // These tests pin `try_eval_topology_selector` for the three combinator names
    // (union/intersect/difference) and `topology_selectors::resolve` for their
    // K3 set semantics. RED until step-6 adds the composition arms to
    // try_eval_topology_selector; the resolve-semantics assertions are only
    // reachable once the eval arm returns Some(Value::Selector(..)).
    //
    // BT2: union = canonical-order set-union of children (no duplicates).
    // BT3: intersect of disjoint children = []; difference = a minus b.

    /// Build a two-arg composition FunctionCall (`union(arg_a, arg_b)` etc.)
    /// from two pre-compiled selector exprs.
    fn topology_selector_composition_call(
        combinator: &str,
        arg_a: reify_ir::CompiledExpr,
        arg_b: reify_ir::CompiledExpr,
    ) -> reify_ir::CompiledExpr {
        // Result type mirrors arg_a (same kind for valid same-kind composition).
        let result_type = arg_a.result_type.clone();
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(combinator))
            .combine(arg_a.content_hash)
            .combine(arg_b.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: combinator.to_string(),
                    qualified_name: combinator.to_string(),
                },
                args: vec![arg_a, arg_b],
            },
            result_type,
            content_hash,
        }
    }

    /// `union(faces(b), faces(c))` evaluates to `Value::Selector(Union([sv_b, sv_c]))`
    /// of kind Face, and resolving via `topology_selectors::resolve` yields the
    /// canonical-order set-union of all face handles. BT2.
    ///
    /// RED until step-6 adds the `union` arm to `try_eval_topology_selector`.
    #[test]
    fn union_eval_produces_union_selector_and_resolve_yields_set_union() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let handle_c = GeometryHandleId(2);
        let rr = RealizationNodeId::new("UnionTest", 0);
        let hash_b: [u8; 32] = [0x11; 32];
        let hash_c: [u8; 32] = [0x22; 32];

        // faces(b) = [GHId(10), GHId(11)]; faces(c) = [GHId(12), GHId(13)].
        // Union = [GHId(10), GHId(11), GHId(12), GHId(13)] (first-seen order, no dups).
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(handle_b, vec![GeometryHandleId(10), GeometryHandleId(11)])
            .with_extracted_faces(handle_c, vec![GeometryHandleId(12), GeometryHandleId(13)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(handle_b));
        named_steps.insert("c".to_string(), kh(handle_c));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("UnionTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );
        values.insert(
            ValueCellId::new("UnionTest", "c"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_c,
                kernel_handle: Some(handle_c),
            },
        );

        let faces_b = topology_selector_call_one_value_ref(
            "faces",
            "UnionTest",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_c = topology_selector_call_one_value_ref(
            "faces",
            "UnionTest",
            "c",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let union_expr = topology_selector_composition_call("union", faces_b, faces_c);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &union_expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // RED until step-6 adds the union arm.
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "union(faces(b), faces(c)) must yield Some(Value::Selector(..)), \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "union of face selectors → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Union(children) => {
                assert_eq!(children.len(), 2, "union of 2 operands → 2 children");
            }
            other => panic!("expected Union node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean composition must emit no diagnostics; got: {:?}",
            diagnostics
        );

        // Resolve set semantics: union = set-union of children (BT2).
        let resolved = crate::topology_selectors::resolve(&sv, &mut kernel, &mut diagnostics)
            .expect("union resolve must not error");
        assert_eq!(
            resolved,
            vec![
                GeometryHandleId(10),
                GeometryHandleId(11),
                GeometryHandleId(12),
                GeometryHandleId(13),
            ],
            "union resolves to canonical-order set-union of all child handles"
        );
    }

    /// `intersect(faces(b), faces(c))` where b and c are disjoint evaluates to
    /// `Value::Selector(Intersect([sv_b, sv_c]))` of kind Face, and resolving
    /// yields [] (empty intersection of disjoint sets). BT3.
    ///
    /// RED until step-6 adds the `intersect` arm to `try_eval_topology_selector`.
    #[test]
    fn intersect_eval_produces_intersect_selector_and_disjoint_resolves_empty() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let handle_c = GeometryHandleId(2);
        let rr = RealizationNodeId::new("IntersectTest", 0);
        let hash_b: [u8; 32] = [0x33; 32];
        let hash_c: [u8; 32] = [0x44; 32];

        // faces(b) = [GHId(10), GHId(11)]; faces(c) = [GHId(12), GHId(13)] (disjoint).
        // Intersect of disjoint sets = [].
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(handle_b, vec![GeometryHandleId(10), GeometryHandleId(11)])
            .with_extracted_faces(handle_c, vec![GeometryHandleId(12), GeometryHandleId(13)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(handle_b));
        named_steps.insert("c".to_string(), kh(handle_c));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("IntersectTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );
        values.insert(
            ValueCellId::new("IntersectTest", "c"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_c,
                kernel_handle: Some(handle_c),
            },
        );

        let faces_b = topology_selector_call_one_value_ref(
            "faces",
            "IntersectTest",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_c = topology_selector_call_one_value_ref(
            "faces",
            "IntersectTest",
            "c",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let intersect_expr = topology_selector_composition_call("intersect", faces_b, faces_c);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &intersect_expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // RED until step-6 adds the intersect arm.
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "intersect(faces(b), faces(c)) must yield Some(Value::Selector(..)), \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "intersect of face selectors → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Intersect(children) => {
                assert_eq!(children.len(), 2, "intersect of 2 operands → 2 children");
            }
            other => panic!("expected Intersect node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean intersect composition must emit no diagnostics; got: {:?}",
            diagnostics
        );

        // Resolve set semantics: intersect of disjoint sets = [] (BT3).
        let resolved = crate::topology_selectors::resolve(&sv, &mut kernel, &mut diagnostics)
            .expect("intersect resolve must not error");
        assert!(
            resolved.is_empty(),
            "intersect of disjoint face sets must resolve to []; got {:?}",
            resolved
        );
    }

    /// `difference(faces(b), faces(c))` evaluates to `Value::Selector(Difference(sv_b, sv_c))`
    /// of kind Face, and resolving yields faces in b but not in c. BT3.
    ///
    /// RED until step-6 adds the `difference` arm to `try_eval_topology_selector`.
    #[test]
    fn difference_eval_produces_difference_selector_and_resolve_yields_set_difference() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let handle_c = GeometryHandleId(2);
        let rr = RealizationNodeId::new("DiffTest", 0);
        let hash_b: [u8; 32] = [0x55; 32];
        let hash_c: [u8; 32] = [0x66; 32];

        // faces(b) = [GHId(10), GHId(11), GHId(12)]; faces(c) = [GHId(11)].
        // Difference = b \ c = [GHId(10), GHId(12)] (GHId(11) excluded).
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(
                handle_b,
                vec![
                    GeometryHandleId(10),
                    GeometryHandleId(11),
                    GeometryHandleId(12),
                ],
            )
            .with_extracted_faces(handle_c, vec![GeometryHandleId(11)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(handle_b));
        named_steps.insert("c".to_string(), kh(handle_c));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("DiffTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );
        values.insert(
            ValueCellId::new("DiffTest", "c"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_c,
                kernel_handle: Some(handle_c),
            },
        );

        let faces_b = topology_selector_call_one_value_ref(
            "faces",
            "DiffTest",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_c = topology_selector_call_one_value_ref(
            "faces",
            "DiffTest",
            "c",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let diff_expr = topology_selector_composition_call("difference", faces_b, faces_c);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &diff_expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // RED until step-6 adds the difference arm.
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "difference(faces(b), faces(c)) must yield Some(Value::Selector(..)), \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "difference of face selectors → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Difference(a, _b) => {
                assert_eq!(
                    a.kind,
                    reify_core::ty::SelectorKind::Face,
                    "difference left operand → Face kind"
                );
            }
            other => panic!("expected Difference node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean difference composition must emit no diagnostics; got: {:?}",
            diagnostics
        );

        // Resolve set semantics: difference = a \ b (BT3).
        let resolved = crate::topology_selectors::resolve(&sv, &mut kernel, &mut diagnostics)
            .expect("difference resolve must not error");
        assert_eq!(
            resolved,
            vec![GeometryHandleId(10), GeometryHandleId(12)],
            "difference(faces(b), faces(c)) resolves to b \\ c = [GHId(10), GHId(12)]"
        );
    }

    // ── eval-side composition coverage (task 4119 δ amendment) ─────────────
    //
    // Tests added in the amendment pass to cover paths not exercised by the
    // compile-time tests in selector_composition_tests.rs:
    //   1. Variadic 3-arg union at eval level.
    //   2. SelectorError::KindMismatch → Warning + Value::Undef backstop in
    //      eval_variadic_composition (defensive path; compile-time
    //      E_SELECTOR_KIND_MISMATCH should fire first in normal use).

    /// `union(faces(b), faces(c), faces(d))` — 3 operands, all Face — evaluates
    /// to `Value::Selector(Union([sv_b, sv_c, sv_d]))` of kind Face.
    /// Covers the variadic path in `eval_variadic_composition`.
    #[test]
    fn union_three_operands_eval_produces_union_with_three_children() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let handle_c = GeometryHandleId(2);
        let handle_d = GeometryHandleId(3);
        let rr = RealizationNodeId::new("Union3Test", 0);
        let hash_b: [u8; 32] = [0x11; 32];
        let hash_c: [u8; 32] = [0x22; 32];
        let hash_d: [u8; 32] = [0x33; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(handle_b, vec![GeometryHandleId(10)])
            .with_extracted_faces(handle_c, vec![GeometryHandleId(11)])
            .with_extracted_faces(handle_d, vec![GeometryHandleId(12)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(handle_b));
        named_steps.insert("c".to_string(), kh(handle_c));
        named_steps.insert("d".to_string(), kh(handle_d));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Union3Test", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );
        values.insert(
            ValueCellId::new("Union3Test", "c"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_c,
                kernel_handle: Some(handle_c),
            },
        );
        values.insert(
            ValueCellId::new("Union3Test", "d"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_d,
                kernel_handle: Some(handle_d),
            },
        );

        let faces_b = topology_selector_call_one_value_ref(
            "faces",
            "Union3Test",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_c = topology_selector_call_one_value_ref(
            "faces",
            "Union3Test",
            "c",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_d = topology_selector_call_one_value_ref(
            "faces",
            "Union3Test",
            "d",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );

        // Build union(faces_b, faces_c, faces_d) — three-arg FunctionCall.
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("union"))
            .combine(faces_b.content_hash)
            .combine(faces_c.content_hash)
            .combine(faces_d.content_hash);
        let union3_expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "union".to_string(),
                    qualified_name: "union".to_string(),
                },
                args: vec![faces_b, faces_c, faces_d],
            },
            result_type: Type::Selector(reify_core::ty::SelectorKind::Face),
            content_hash,
        };

        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &union3_expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "union(faces(b), faces(c), faces(d)) must yield Some(Value::Selector(..)), \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "3-arg union → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Union(children) => {
                assert_eq!(children.len(), 3, "3-arg union → 3 children in Union node");
            }
            other => panic!("expected Union node with 3 children, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean 3-arg union must emit no diagnostics; got: {:?}",
            diagnostics
        );

        // Resolve: union of 3 disjoint single-face sets = all three handles.
        let resolved = crate::topology_selectors::resolve(&sv, &mut kernel, &mut diagnostics)
            .expect("3-arg union resolve must not error");
        assert_eq!(
            resolved,
            vec![
                GeometryHandleId(10),
                GeometryHandleId(11),
                GeometryHandleId(12)
            ],
            "3-arg union resolves to set-union of all three child face sets"
        );
    }

    /// Defensive backstop: when `eval_variadic_composition` receives children of
    /// mismatched `SelectorKind` (bypassing the compile-time
    /// `E_SELECTOR_KIND_MISMATCH`), `SelectorValue::union` returns
    /// `SelectorError::KindMismatch` and the result is `Some(Value::Undef)` with
    /// exactly one Warning diagnostic.
    ///
    /// This path is not reachable from valid .ri source (the compiler catches it
    /// first) but is reachable from hand-crafted IR, so we pin the defensive
    /// behaviour here.
    #[test]
    fn eval_variadic_composition_kind_mismatch_yields_undef_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let handle_c = GeometryHandleId(2);
        let rr = RealizationNodeId::new("KindMismatchTest", 0);
        let hash_b: [u8; 32] = [0xAA; 32];
        let hash_c: [u8; 32] = [0xBB; 32];

        // Kernel needs no mock data: `faces`/`edges` construction is kernel-free
        // (LeafQuery::All build via build_leaf_selector, no extract_* calls).
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(handle_b));
        named_steps.insert("c".to_string(), kh(handle_c));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("KindMismatchTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );
        values.insert(
            ValueCellId::new("KindMismatchTest", "c"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_c,
                kernel_handle: Some(handle_c),
            },
        );

        // Build union(faces(b), edges(c)) at IR level — mixed kinds, bypasses
        // the compiler's kind-mismatch check.  result_type is deliberately Face
        // (as if the compiler did anti-cascade), so the FunctionCall arm in
        // try_eval_topology_selector routes to the Union handler.
        let faces_b = topology_selector_call_one_value_ref(
            "faces",
            "KindMismatchTest",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let edges_c = topology_selector_call_one_value_ref(
            "edges",
            "KindMismatchTest",
            "c",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Edge),
        );
        let union_mixed = topology_selector_composition_call("union", faces_b, edges_c);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &union_mixed,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Defensive backstop: kind-mismatch at eval level → Some(Undef) + Warning.
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "kind-mismatch union at eval level must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kind-mismatch must emit exactly 1 Warning diagnostic; got {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "backstop diagnostic must be Warning severity"
        );
    }

    // ── step-9 (task 4119 δ): Named-leaf constructor eval tests ─────────────
    //
    // These tests pin `try_eval_topology_selector` for the three named-leaf
    // constructors (face/edge/solid_body) and the BT8 resolve-to-empty path
    // for an unresolvable name.  RED until step-10 adds the face/edge/solid_body
    // arms to try_eval_topology_selector.
    //
    // BT8: resolving a face(b,"nope") Named leaf (no matching tag) returns []
    // and pushes EXACTLY ONE DiagnosticCode::TopologyTagStale — exercising the
    // already-landed resolve_leaf Named interim now reachable from the .ri surface.

    /// Build a two-arg `face`/`edge`/`solid_body` FunctionCall: arg[0] is a
    /// ValueRef to the parent geometry cell, arg[1] is a string Literal.
    fn named_selector_call(
        helper_name: &str,
        entity: &str,
        member: &str,
        result_kind: reify_core::ty::SelectorKind,
        name_str: &str,
    ) -> reify_ir::CompiledExpr {
        let arg_geom = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member),
            reify_core::Type::Geometry,
        );
        let arg_name = reify_ir::CompiledExpr::literal(
            reify_ir::Value::String(name_str.to_string()),
            reify_core::Type::String,
        );
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name))
            .combine(arg_geom.content_hash)
            .combine(arg_name.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_geom, arg_name],
            },
            result_type: reify_core::Type::Selector(result_kind),
            content_hash,
        }
    }

    /// Build a two-arg `face`/`edge`/`solid_body` FunctionCall where arg[0] is a
    /// `ValueRef` to a `Value::Selector` cell (typed `Type::Selector(arg0_kind)`),
    /// and arg[1] is a string `Literal` for the name. Used by the
    /// face/edge/solid_body-over-selector tests (task #4583).
    fn named_selector_call_over_selector(
        helper_name: &str,
        entity: &str,
        member: &str,
        arg0_selector_kind: reify_core::ty::SelectorKind,
        name_str: &str,
        result_kind: reify_core::ty::SelectorKind,
    ) -> reify_ir::CompiledExpr {
        let arg_selector = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member),
            reify_core::Type::Selector(arg0_selector_kind),
        );
        let arg_name = reify_ir::CompiledExpr::literal(
            reify_ir::Value::String(name_str.to_string()),
            reify_core::Type::String,
        );
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name))
            .combine(arg_selector.content_hash)
            .combine(arg_name.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_selector, arg_name],
            },
            result_type: reify_core::Type::Selector(result_kind),
            content_hash,
        }
    }

    /// `face(b, "top")` evaluates to `Value::Selector(Face)` with a
    /// `SelectorNode::Leaf { query: LeafQuery::Named("top") }`. Zero kernel
    /// queries at construction time (K2/BT7). RED until step-10.
    #[test]
    fn face_named_ctor_yields_named_leaf_selector_of_face_kind() {
        use reify_core::ValueCellId;
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("NamedFaceCtorTest", 0);
        let hash_b: [u8; 32] = [0xAA; 32];

        let named_steps = HashMap::new(); // no kernel queries at construction
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("NamedFaceCtorTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );

        let expr = named_selector_call(
            "face",
            "NamedFaceCtorTest",
            "b",
            reify_core::ty::SelectorKind::Face,
            "top",
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "face(b, \"top\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "face() → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                ..
            } => {
                assert_eq!(n, "top", "face(b, \"top\") → Named(\"top\") leaf");
            }
            other => panic!("expected Leaf{{ Named }}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    // ── step-1 (task #4583): face/edge/solid_body over a Value::Selector arg0 ─
    //
    // These tests pin `try_eval_topology_selector` for the chained selector
    // form `selector.face("name")` where arg0 is a hydrated `Value::Selector`
    // cell. RED on base: `eval_named_leaf_selector_ctor` resolves arg0 via
    // `resolve_selector_target`, which returns None for a Value::Selector cell
    // (GeometryHandleRef::from_geometry_handle matches only Value::GeometryHandle).

    /// `face(mid_surface(body), "region_0")` — arg0 is a hydrated
    /// `Value::Selector(Face, ByRole(MidSurfaceFace))` — must evaluate to
    /// `Value::Selector(Face, Named("region_0"))` with the input selector's
    /// target `GeometryHandleRef` preserved. Models `body.mid_surface().face("region_0")`.
    /// RED on base: eval_named_leaf_selector_ctor returns None for a Selector arg0.
    #[test]
    fn face_over_selector_first_arg_builds_named_leaf() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ValueCellId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(7);
        let rr = RealizationNodeId::new("FaceOverSelectorTest", 0);
        let hash_b: [u8; 32] = [0xBB; 32];

        // Input selector: mid_surface(body) → Face ByRole(MidSurfaceFace) leaf.
        let target_ghr = reify_ir::value::GeometryHandleRef {
            realization_ref: rr.clone(),
            upstream_values_hash: hash_b,
            kernel_handle: Some(handle_b),
        };
        let input_sv = reify_ir::value::SelectorValue::leaf(
            reify_core::ty::SelectorKind::Face,
            target_ghr,
            reify_ir::value::LeafQuery::ByRole(reify_ir::Role::MidSurfaceFace),
        )
        .expect("kind-closure: Face/ByRole(MidSurfaceFace) is valid");

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("FaceOverSelectorTest", "sel"),
            reify_ir::Value::Selector(input_sv),
        );

        // face(sel, "region_0") with arg0 typed as Selector(Face).
        let expr = named_selector_call_over_selector(
            "face",
            "FaceOverSelectorTest",
            "sel",
            reify_core::ty::SelectorKind::Face,
            "region_0",
            reify_core::ty::SelectorKind::Face,
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "face(sel, \"region_0\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Face, "face() → Face kind");
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                target,
            } => {
                assert_eq!(n, "region_0", "face(sel, \"region_0\") → Named(\"region_0\") leaf");
                assert_eq!(
                    target.kernel_handle,
                    Some(handle_b),
                    "Named leaf target kernel_handle preserved from input selector"
                );
                assert_eq!(
                    target.realization_ref, rr,
                    "Named leaf target realization_ref preserved from input selector"
                );
            }
            other => panic!("expected Leaf{{Named(\"region_0\")}}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `edge(edges(body), "region_0")` — arg0 is a hydrated
    /// `Value::Selector(Edge, All)` — must evaluate to
    /// `Value::Selector(Edge, Named("region_0"))` with the input selector's
    /// target `GeometryHandleRef` preserved.
    /// RED on base: eval_named_leaf_selector_ctor returns None for a Selector arg0.
    #[test]
    fn edge_over_selector_first_arg_builds_named_leaf() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ValueCellId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(8);
        let rr = RealizationNodeId::new("EdgeOverSelectorTest", 0);
        let hash_b: [u8; 32] = [0xCC; 32];

        let target_ghr = reify_ir::value::GeometryHandleRef {
            realization_ref: rr.clone(),
            upstream_values_hash: hash_b,
            kernel_handle: Some(handle_b),
        };
        let input_sv = reify_ir::value::SelectorValue::leaf(
            reify_core::ty::SelectorKind::Edge,
            target_ghr,
            reify_ir::value::LeafQuery::All,
        )
        .expect("kind-closure: Edge/All is valid");

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("EdgeOverSelectorTest", "sel"),
            reify_ir::Value::Selector(input_sv),
        );

        let expr = named_selector_call_over_selector(
            "edge",
            "EdgeOverSelectorTest",
            "sel",
            reify_core::ty::SelectorKind::Edge,
            "region_0",
            reify_core::ty::SelectorKind::Edge,
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "edge(sel, \"region_0\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Edge, "edge() → Edge kind");
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                target,
            } => {
                assert_eq!(n, "region_0", "edge(sel, \"region_0\") → Named(\"region_0\") leaf");
                assert_eq!(
                    target.kernel_handle,
                    Some(handle_b),
                    "Named leaf target kernel_handle preserved from input selector"
                );
                assert_eq!(
                    target.realization_ref, rr,
                    "Named leaf target realization_ref preserved from input selector"
                );
            }
            other => panic!("expected Leaf{{Named(\"region_0\")}}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `solid_body(faces(body), "region_0")` — arg0 is a hydrated
    /// `Value::Selector(Body, All)` — must evaluate to
    /// `Value::Selector(Body, Named("region_0"))` with the input selector's
    /// target `GeometryHandleRef` preserved.
    /// RED on base: eval_named_leaf_selector_ctor returns None for a Selector arg0.
    #[test]
    fn solid_body_over_selector_first_arg_builds_named_leaf() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ValueCellId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(9);
        let rr = RealizationNodeId::new("SolidBodyOverSelectorTest", 0);
        let hash_b: [u8; 32] = [0xDD; 32];

        let target_ghr = reify_ir::value::GeometryHandleRef {
            realization_ref: rr.clone(),
            upstream_values_hash: hash_b,
            kernel_handle: Some(handle_b),
        };
        let input_sv = reify_ir::value::SelectorValue::leaf(
            reify_core::ty::SelectorKind::Body,
            target_ghr,
            reify_ir::value::LeafQuery::All,
        )
        .expect("kind-closure: Body/All is valid");

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("SolidBodyOverSelectorTest", "sel"),
            reify_ir::Value::Selector(input_sv),
        );

        let expr = named_selector_call_over_selector(
            "solid_body",
            "SolidBodyOverSelectorTest",
            "sel",
            reify_core::ty::SelectorKind::Body,
            "region_0",
            reify_core::ty::SelectorKind::Body,
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "solid_body(sel, \"region_0\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Body, "solid_body() → Body kind");
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                target,
            } => {
                assert_eq!(
                    n, "region_0",
                    "solid_body(sel, \"region_0\") → Named(\"region_0\") leaf"
                );
                assert_eq!(
                    target.kernel_handle,
                    Some(handle_b),
                    "Named leaf target kernel_handle preserved from input selector"
                );
                assert_eq!(
                    target.realization_ref, rr,
                    "Named leaf target realization_ref preserved from input selector"
                );
            }
            other => panic!("expected Leaf{{Named(\"region_0\")}}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `vertex(vertices(body), "tip")` — arg0 is a hydrated
    /// `Value::Selector(Vertex, All)` — must evaluate to
    /// `Value::Selector(Vertex, Named("tip"))` with the input selector's
    /// target `GeometryHandleRef` preserved. Vertex gains the Selector-arg0
    /// capability for free via the shared `eval_named_leaf_selector_ctor` (task #4583).
    #[test]
    fn vertex_over_selector_first_arg_builds_named_leaf() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ValueCellId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(11);
        let rr = RealizationNodeId::new("VertexOverSelectorTest", 0);
        let hash_b: [u8; 32] = [0xEE; 32];

        let target_ghr = reify_ir::value::GeometryHandleRef {
            realization_ref: rr.clone(),
            upstream_values_hash: hash_b,
            kernel_handle: Some(handle_b),
        };
        // Input selector: vertices(body) → Vertex All leaf.
        let input_sv = reify_ir::value::SelectorValue::leaf(
            reify_core::ty::SelectorKind::Vertex,
            target_ghr,
            reify_ir::value::LeafQuery::All,
        )
        .expect("kind-closure: Vertex/All is valid");

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("VertexOverSelectorTest", "sel"),
            reify_ir::Value::Selector(input_sv),
        );

        // vertex(sel, "tip") with arg0 typed as Selector(Vertex).
        let expr = named_selector_call_over_selector(
            "vertex",
            "VertexOverSelectorTest",
            "sel",
            reify_core::ty::SelectorKind::Vertex,
            "tip",
            reify_core::ty::SelectorKind::Vertex,
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "vertex(sel, \"tip\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Vertex,
            "vertex() → Vertex kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                target,
            } => {
                assert_eq!(n, "tip", "vertex(sel, \"tip\") → Named(\"tip\") leaf");
                assert_eq!(
                    target.kernel_handle,
                    Some(handle_b),
                    "Named leaf target kernel_handle preserved from input selector"
                );
                assert_eq!(
                    target.realization_ref, rr,
                    "Named leaf target realization_ref preserved from input selector"
                );
            }
            other => panic!("expected Leaf{{Named(\"tip\")}}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// A `face(union_sel, "region_0")` call where arg0 is a
    /// `Value::Selector(Face, Union([leaf_a, leaf_b]))` with two leaves rooted at
    /// *distinct* `GeometryHandleRef` targets must produce a `Named("region_0")`
    /// leaf rooted at the **first** (left-most) leaf's target (`leaf_a.target`).
    ///
    /// This pins the documented v1 narrowing: `resolve_named_leaf_target` calls
    /// `first_leaf_target` which does a left-most walk of Union/Intersect children,
    /// so a multi-leaf composition narrows to the first leaf's parent geometry.
    /// A future v2 could error instead; this test documents the current contract.
    #[test]
    fn named_leaf_over_union_selector_narrows_to_first_leaf_target() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ValueCellId;
        use reify_test_support::mocks::MockGeometryKernel;

        // Two distinct targets — only the FIRST should survive as the Named leaf's target.
        let handle_a = GeometryHandleId(21);
        let handle_b = GeometryHandleId(22);
        let rr_a = RealizationNodeId::new("UnionNarrowTest", 0);
        let rr_b = RealizationNodeId::new("UnionNarrowTest", 1);
        let hash_a: [u8; 32] = [0xA1; 32];
        let hash_b: [u8; 32] = [0xB2; 32];

        let ghr_a = reify_ir::value::GeometryHandleRef {
            realization_ref: rr_a.clone(),
            upstream_values_hash: hash_a,
            kernel_handle: Some(handle_a),
        };
        let ghr_b = reify_ir::value::GeometryHandleRef {
            realization_ref: rr_b.clone(),
            upstream_values_hash: hash_b,
            kernel_handle: Some(handle_b),
        };

        // Build Face All leaves over distinct targets, then union them.
        let sv_a = reify_ir::value::SelectorValue::leaf(
            reify_core::ty::SelectorKind::Face,
            ghr_a,
            reify_ir::value::LeafQuery::All,
        )
        .expect("Face/All valid");
        let sv_b = reify_ir::value::SelectorValue::leaf(
            reify_core::ty::SelectorKind::Face,
            ghr_b,
            reify_ir::value::LeafQuery::All,
        )
        .expect("Face/All valid");
        let union_sv = reify_ir::value::SelectorValue::union(vec![sv_a, sv_b])
            .expect("same-kind union valid");

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("UnionNarrowTest", "sel"),
            reify_ir::Value::Selector(union_sv),
        );

        // face(union_sel, "region_0") — arg0 typed as Selector(Face).
        let expr = named_selector_call_over_selector(
            "face",
            "UnionNarrowTest",
            "sel",
            reify_core::ty::SelectorKind::Face,
            "region_0",
            reify_core::ty::SelectorKind::Face,
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "face(union_sel, \"region_0\"): expected Some(Value::Selector(..)); \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Face, "face() → Face kind");
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                target,
            } => {
                assert_eq!(n, "region_0", "Named(\"region_0\") leaf");
                // v1 narrowing: Named leaf is rooted at the FIRST (left-most) union child's target.
                assert_eq!(
                    target.kernel_handle,
                    Some(handle_a),
                    "Named leaf target must be first leaf's kernel_handle (v1 first_leaf_target narrowing)"
                );
                assert_eq!(
                    target.realization_ref, rr_a,
                    "Named leaf target must be first leaf's realization_ref (v1 first_leaf_target narrowing)"
                );
                // Confirm the second leaf's target is NOT used.
                assert_ne!(
                    target.kernel_handle,
                    Some(handle_b),
                    "Named leaf must NOT root at the second union leaf's target"
                );
            }
            other => panic!("expected Leaf{{Named(\"region_0\")}}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `mid_surface(body)` (task 4536) evaluates to `Value::Selector(Face)` with
    /// a `SelectorNode::Leaf { query: LeafQuery::ByRole(Role::MidSurfaceFace) }`.
    /// Mirrors the `faces(b)` All-leaf ctor, differing only in the leaf query —
    /// the role-addressed `ByRole` leaf composes with 4119's union/intersect as a
    /// first-class kind-typed leaf. Zero kernel queries at construction time
    /// (K2/BT7): the `TopologyAttributeTable` filter is deferred to the
    /// `ResolveSelector` coercion path. RED until step-10 adds the `MidSurface`
    /// helper variant + build arm.
    #[test]
    fn mid_surface_ctor_yields_byrole_leaf_selector_of_face_kind() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("MidSurfaceCtorTest", 0);
        let hash_b: [u8; 32] = [0xCD; 32];

        let named_steps = HashMap::new(); // no kernel queries at construction
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("MidSurfaceCtorTest", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "mid_surface",
            "MidSurfaceCtorTest",
            "body",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "mid_surface(body): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "mid_surface() → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::ByRole(role),
                ..
            } => {
                assert_eq!(
                    *role,
                    reify_ir::Role::MidSurfaceFace,
                    "mid_surface(body) → ByRole(MidSurfaceFace) leaf"
                );
            }
            other => panic!("expected Leaf{{ ByRole(MidSurfaceFace) }}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// Integration via the `ResolveSelector` coercion path (task 4536, step-11).
    ///
    /// A `ResolveSelector { mid_surface(body) }` whose realized body carries
    /// `Role::MidSurfaceFace` entries in the `TopologyAttributeTable` resolves to
    /// the list of those mid-surface sub-handles — ordered by `(local_index, id)`,
    /// each a `Value::GeometryHandle` whose `kernel_handle` is the seeded id and
    /// whose `upstream_values_hash` is `compose_sub_handle_hash(parent, Face, i)`.
    /// The kernel records NO `extract_faces` call: the synthetic mid-surface ids
    /// are not kernel-enumerable, so resolution is a pure `table` filter.
    #[test]
    fn resolve_mid_surface_seeded_table_yields_subhandle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("MidSurfaceResolve", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // Seed two MidSurfaceFace entries (recorded in REVERSE local_index order
        // so the (local_index, id) sort — not insertion order — governs output),
        // plus one unrelated `Side` role that the ByRole filter must exclude.
        let face_a = GeometryHandleId(7001); // local_index 0
        let face_b = GeometryHandleId(7002); // local_index 1
        let other = GeometryHandleId(7003);
        let attr = |role: reify_ir::Role, local_index: u32| reify_ir::TopologyAttribute {
            feature_id: reify_ir::FeatureId::realization("body", 0),
            role,
            local_index,
            user_label: None,
            mod_history: vec![],
        };
        let mut table = reify_ir::TopologyAttributeTable::default();
        table.record(KernelHandle { kernel: KernelId::Occt, id: face_b }, attr(reify_ir::Role::MidSurfaceFace, 1));
        table.record(KernelHandle { kernel: KernelId::Occt, id: face_a }, attr(reify_ir::Role::MidSurfaceFace, 0));
        table.record(KernelHandle { kernel: KernelId::Occt, id: other }, attr(reify_ir::Role::Side, 0));

        let mut named_steps = HashMap::new();
        named_steps.insert("body".to_string(), kh(parent_handle));
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("MidSurfaceResolve", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );

        let inner = topology_selector_call_one_value_ref(
            "mid_surface",
            "MidSurfaceResolve",
            "body",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let expr = reify_ir::CompiledExpr::resolve_selector(inner);

        // No extract_faces stubbing — a ByRole resolve must never reach the kernel.
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &HashMap::new(),
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "ResolveSelector{{mid_surface(body)}} with a seeded table must yield \
                 Some(Value::List(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(list.len(), 2, "expected 2 mid-surface face sub-handles");

        // Ordered by (local_index, id): face_a (li 0) then face_b (li 1).
        let expected_ids = [face_a, face_b];
        for (i, (elem, expected_id)) in list.iter().zip(&expected_ids).enumerate() {
            let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
                &parent_hash,
                crate::topology_selectors::SubKind::Face,
                i as u32,
            );
            match elem {
                reify_ir::Value::GeometryHandle {
                    realization_ref,
                    upstream_values_hash,
                    kernel_handle,
                } => {
                    assert_eq!(
                        realization_ref, &parent_rr,
                        "elem[{i}] realization_ref must inherit parent"
                    );
                    assert_eq!(
                        kernel_handle, &Some(*expected_id),
                        "elem[{i}] kernel_handle == seeded mid-surface id (local_index order)"
                    );
                    assert_eq!(
                        upstream_values_hash, &expected_hash,
                        "elem[{i}] hash must be compose_sub_handle_hash(parent, Face, {i})"
                    );
                }
                other => panic!("elem[{i}] must be Value::GeometryHandle, got {:?}", other),
            }
        }
        assert!(
            diagnostics.is_empty(),
            "a successful mid-surface resolve emits zero diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `mid_surface(body)` over a body WITHOUT any mid-surface attribute (a
    /// non-shell body) must resolve to `Value::Undef` + a Warning naming the
    /// missing mid-surface / role — NOT a silent empty `Value::List`. Covers both
    /// an empty table and a table carrying only an unrelated role. RED until
    /// step-12 adds the empty-ByRole→Undef branch to `resolve_selector_to_list`.
    #[test]
    fn resolve_mid_surface_no_attribute_yields_undef_and_diagnostic() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("MidSurfaceNoAttr", 0);
        let parent_hash: [u8; 32] = [0x99; 32];

        let mut named_steps = HashMap::new();
        named_steps.insert("body".to_string(), kh(parent_handle));
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("MidSurfaceNoAttr", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );

        // (1) empty table; (2) only an unrelated `Side` role — both are
        // "non-shell body" fixtures that yield zero MidSurfaceFace matches.
        let empty = reify_ir::TopologyAttributeTable::default();
        let mut other_only = reify_ir::TopologyAttributeTable::default();
        other_only.record(
            KernelHandle { kernel: KernelId::Occt, id: GeometryHandleId(8001) },
            reify_ir::TopologyAttribute {
                feature_id: reify_ir::FeatureId::realization("body", 0),
                role: reify_ir::Role::Side,
                local_index: 0,
                user_label: None,
                mod_history: vec![],
            },
        );

        for (label, table) in [("empty", &empty), ("other-role-only", &other_only)] {
            let inner = topology_selector_call_one_value_ref(
                "mid_surface",
                "MidSurfaceNoAttr",
                "body",
                Type::Geometry,
                Type::Selector(reify_core::ty::SelectorKind::Face),
            );
            let expr = reify_ir::CompiledExpr::resolve_selector(inner);
            let mut kernel = MockGeometryKernel::new();
            let mut diagnostics = Vec::new();
            let result = super::try_eval_resolve_selector(
                &expr,
                &named_steps,
                &values,
                &mut kernel,
                table,
                &HashMap::new(),
                &mut diagnostics,
            );
            assert!(
                matches!(result, Some(reify_ir::Value::Undef)),
                "[{label}] mid_surface over a non-shell body must yield \
                 Some(Value::Undef); got {:?}; diags: {:?}",
                result,
                diagnostics
            );
            assert!(
                diagnostics.iter().any(|d| {
                    let m = d.message.to_lowercase();
                    m.contains("mid") || m.contains("midsurfaceface") || m.contains("role")
                }),
                "[{label}] expected a diagnostic naming the missing mid-surface / role; got {:?}",
                diagnostics
            );
        }
    }

    /// D3 fail-closed sub-case (b) (task 4831, P3β / PRD §3 D3): a BRep body
    /// with NO recorded provenance for the queried feature (imported
    /// geometry, or simply a feature that created/split nothing in THIS
    /// design) must resolve `created_by_feature`/`split_by_feature` to
    /// `Value::Undef` + a Warning — NOT a silent empty `Value::List`. Mirrors
    /// `resolve_mid_surface_no_attribute_yields_undef_and_diagnostic`. Covers
    /// both an empty table and a table carrying only an unrelated feature's
    /// entries. RED until step-8 adds selector_is_provenance_leaf + the
    /// parallel empty→Undef branch to `resolve_selector_to_list`.
    #[test]
    fn resolve_provenance_leaves_no_matching_feature_yields_undef_and_diagnostic() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let entity = "ProvenanceNoAttr";
        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new(entity, 0);
        let parent_hash: [u8; 32] = [0x99; 32];
        let fid = reify_ir::FeatureId::realization(entity, 0);
        let other_fid = reify_ir::FeatureId::realization("other", 0);

        let mut named_steps = HashMap::new();
        named_steps.insert("body".to_string(), kh(parent_handle));
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new(entity, "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        values.insert(ValueCellId::new(entity, "f"), reify_ir::Value::Feature(fid.clone()));

        // (1) empty table; (2) only an entry for a DIFFERENT feature — both
        // model "no recorded provenance for the queried feature" (e.g.
        // imported geometry).
        let empty = reify_ir::TopologyAttributeTable::default();
        let mut other_only = reify_ir::TopologyAttributeTable::default();
        other_only.record(
            KernelHandle { kernel: KernelId::Occt, id: GeometryHandleId(8001) },
            reify_ir::TopologyAttribute {
                feature_id: other_fid,
                role: reify_ir::Role::Side,
                local_index: 0,
                user_label: None,
                mod_history: vec![],
            },
        );

        for name in ["created_by_feature", "split_by_feature"] {
            for (label, table) in [("empty", &empty), ("other-feature-only", &other_only)] {
                let inner = topology_selector_call_two_value_refs(
                    name,
                    entity,
                    "body",
                    Type::Geometry,
                    "f",
                    Type::Feature,
                    Type::Selector(reify_core::ty::SelectorKind::Face),
                );
                let expr = reify_ir::CompiledExpr::resolve_selector(inner);
                let mut kernel = MockGeometryKernel::new();
                let mut diagnostics = Vec::new();
                let result = super::try_eval_resolve_selector(
                    &expr,
                    &named_steps,
                    &values,
                    &mut kernel,
                    table,
                    &HashMap::new(),
                    &mut diagnostics,
                );
                assert!(
                    matches!(result, Some(reify_ir::Value::Undef)),
                    "[{name}/{label}] must yield Some(Value::Undef); got {:?}; diags: {:?}",
                    result,
                    diagnostics
                );
                assert_eq!(
                    diagnostics.len(),
                    1,
                    "[{name}/{label}] exactly ONE diagnostic must fire; got {:?}",
                    diagnostics
                );
            }
        }
    }

    /// Non-collapsing discipline, end-to-end (reviewer suggestion, task 4831
    /// amendment pass): a provenance leaf that matches ZERO faces, nested
    /// inside a task-4119 `Union` composite alongside an operand that DOES
    /// match, must NOT collapse the whole composition to `Value::Undef` — the
    /// empty→Undef treatment in `resolve_selector_to_list` only fires for a
    /// *bare* provenance leaf (`selector_is_provenance_leaf` returns `None`
    /// for composites, per its doc comment). The composite instead follows
    /// the generic non-empty set-union path and resolves to a `Value::List`
    /// with no provenance-empty warning. Mirrors how
    /// `resolve_mid_surface_multi_body_returns_union_documenting_single_shell_limitation`
    /// pins the analogous non-collapsing concern for the `ByRole` sibling.
    #[test]
    fn resolve_provenance_leaf_nested_in_union_does_not_collapse_to_undef() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let entity = "ProvenanceUnionNoCollapse";
        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new(entity, 0);
        let parent_hash: [u8; 32] = [0x99; 32];

        // The queried feature has NO recorded provenance in the table (so a
        // BARE `created_by_feature(body, f)` leaf would resolve empty), while
        // a DIFFERENT feature's `MidSurfaceFace` entry makes `mid_surface(body)`
        // match non-empty.
        let fid_no_match = reify_ir::FeatureId::realization(entity, 0);
        let fid_other = reify_ir::FeatureId::realization("other", 0);

        let mut table = reify_ir::TopologyAttributeTable::default();
        let matching_face = GeometryHandleId(9001);
        table.record(
            KernelHandle { kernel: KernelId::Occt, id: matching_face },
            reify_ir::TopologyAttribute {
                feature_id: fid_other,
                role: reify_ir::Role::MidSurfaceFace,
                local_index: 0,
                user_label: None,
                mod_history: vec![],
            },
        );

        let mut named_steps = HashMap::new();
        named_steps.insert("body".to_string(), kh(parent_handle));
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new(entity, "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        values.insert(
            ValueCellId::new(entity, "f"),
            reify_ir::Value::Feature(fid_no_match),
        );

        let created_by_feature_call = topology_selector_call_two_value_refs(
            "created_by_feature",
            entity,
            "body",
            Type::Geometry,
            "f",
            Type::Feature,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let mid_surface_call = topology_selector_call_one_value_ref(
            "mid_surface",
            entity,
            "body",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let union_call = topology_selector_composition_call(
            "union",
            created_by_feature_call,
            mid_surface_call,
        );
        let expr = reify_ir::CompiledExpr::resolve_selector(union_call);

        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &HashMap::new(),
            &mut diagnostics,
        );

        let elements = match result {
            Some(reify_ir::Value::List(elements)) => elements,
            other => panic!(
                "union(created_by_feature(body,f)[empty], mid_surface(body)[non-empty]) \
                 must resolve to Some(Value::List(..)), NOT collapse to Undef; got {:?}; \
                 diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            elements.len(),
            1,
            "union must carry the one non-empty (mid_surface) match through; got {:?}",
            elements
        );
        assert!(
            diagnostics.is_empty(),
            "a provenance leaf nested in a Union that overall resolves non-empty must \
             emit NO provenance-empty warning (only a BARE provenance leaf gets that \
             treatment); got {:?}",
            diagnostics
        );
    }

    /// `selector_is_provenance_leaf` classifies a single CreatedByFeature/
    /// SplitByFeature leaf as `Some`, and every other leaf/composite as
    /// `None` — sibling to `selector_is_attribute_role_leaf` (task 4831, P3β).
    #[test]
    fn selector_is_provenance_leaf_classifies_leaf_kinds() {
        use reify_core::identity::RealizationNodeId;
        use reify_ir::value::{LeafQuery, SelectorValue};

        let target = reify_ir::value::GeometryHandleRef {
            realization_ref: RealizationNodeId::new("W", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(GeometryHandleId(1)),
        };
        let fid = reify_ir::FeatureId::realization("W", 0);

        let created = SelectorValue::leaf(
            reify_core::ty::SelectorKind::Face,
            target.clone(),
            LeafQuery::CreatedByFeature(fid.clone()),
        )
        .expect("leaf");
        assert!(
            super::selector_is_provenance_leaf(&created).is_some(),
            "a single CreatedByFeature leaf must classify as a provenance leaf"
        );

        let split = SelectorValue::leaf(
            reify_core::ty::SelectorKind::Face,
            target.clone(),
            LeafQuery::SplitByFeature(fid),
        )
        .expect("leaf");
        assert!(
            super::selector_is_provenance_leaf(&split).is_some(),
            "a single SplitByFeature leaf must classify as a provenance leaf"
        );

        let by_role = SelectorValue::leaf(
            reify_core::ty::SelectorKind::Face,
            target.clone(),
            LeafQuery::ByRole(reify_ir::Role::MidSurfaceFace),
        )
        .expect("leaf");
        assert!(
            super::selector_is_provenance_leaf(&by_role).is_none(),
            "a ByRole leaf must NOT classify as a provenance leaf"
        );

        let all =
            SelectorValue::leaf(reify_core::ty::SelectorKind::Face, target.clone(), LeafQuery::All)
                .expect("leaf");
        assert!(
            super::selector_is_provenance_leaf(&all).is_none(),
            "an All leaf must NOT classify as a provenance leaf"
        );

        let created2 = SelectorValue::leaf(
            reify_core::ty::SelectorKind::Face,
            target,
            LeafQuery::CreatedByFeature(reify_ir::FeatureId::realization("W", 0)),
        )
        .expect("leaf");
        let composite = SelectorValue::union(vec![created, created2]).expect("union");
        assert!(
            super::selector_is_provenance_leaf(&composite).is_none(),
            "a composite (Union) selector must NOT classify as a single provenance leaf"
        );
    }

    /// Multi-body fixture documenting the single-shell-per-design LIMITATION
    /// (design decision #4, reviewer suggestion 2, task 4536).
    ///
    /// `ByRole` resolution matches by ROLE only over the BUILD-GLOBAL
    /// `TopologyAttributeTable`; it does NOT correlate `attr.feature_id` to the
    /// target body handle. So when two shell-extracted bodies both record
    /// `MidSurfaceFace`, `mid_surface(body_a)` returns the UNION of BOTH bodies'
    /// mid-surface faces, and a target that itself has no mid-surface does NOT
    /// collapse to `Undef` while another body has entries. This test LOCKS that
    /// current (leaky) behavior so a future per-body-scoping task
    /// (persistent-naming-v2, 2570/2302) must consciously update it — it is the
    /// documented limitation, NOT the desired end state.
    #[test]
    fn resolve_mid_surface_multi_body_returns_union_documenting_single_shell_limitation() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        // Two distinct bodies, each with two MidSurfaceFace patches, recorded
        // under distinct feature_ids. Ids/local_index chosen so the canonical
        // (local_index, id) sort interleaves the two bodies.
        let face_a0 = GeometryHandleId(7101); // body_a, local_index 0
        let face_a1 = GeometryHandleId(7102); // body_a, local_index 1
        let face_b0 = GeometryHandleId(7201); // body_b, local_index 0
        let face_b1 = GeometryHandleId(7202); // body_b, local_index 1
        let attr = |feature: &str, local_index: u32| reify_ir::TopologyAttribute {
            feature_id: reify_ir::FeatureId::realization(feature, 0),
            role: reify_ir::Role::MidSurfaceFace,
            local_index,
            user_label: None,
            mod_history: vec![],
        };
        let mut table = reify_ir::TopologyAttributeTable::default();
        table.record(KernelHandle { kernel: KernelId::Occt, id: face_a0 }, attr("body_a", 0));
        table.record(KernelHandle { kernel: KernelId::Occt, id: face_a1 }, attr("body_a", 1));
        table.record(KernelHandle { kernel: KernelId::Occt, id: face_b0 }, attr("body_b", 0));
        table.record(KernelHandle { kernel: KernelId::Occt, id: face_b1 }, attr("body_b", 1));

        // Two target cells: a real shell body ("body_a") and a body with no
        // mid-surface entry of its own ("non_shell"). Resolution ignores the
        // target, so BOTH must yield the same cross-body UNION.
        let body_a_handle = GeometryHandleId(1);
        let non_shell_handle = GeometryHandleId(2);
        let mut named_steps = HashMap::new();
        named_steps.insert("body_a".to_string(), kh(body_a_handle));
        named_steps.insert("non_shell".to_string(), kh(non_shell_handle));
        let mut values = reify_ir::ValueMap::new();
        for (name, handle) in [("body_a", body_a_handle), ("non_shell", non_shell_handle)] {
            values.insert(
                ValueCellId::new("MidSurfaceMultiBody", name),
                reify_ir::Value::GeometryHandle {
                    realization_ref: RealizationNodeId::new("MidSurfaceMultiBody", 0),
                    upstream_values_hash: [0x55; 32],
                    kernel_handle: Some(handle),
                },
            );
        }

        // Canonical (local_index, id) order across BOTH bodies.
        let expected_ids = [face_a0, face_b0, face_a1, face_b1];

        for target_cell in ["body_a", "non_shell"] {
            let inner = topology_selector_call_one_value_ref(
                "mid_surface",
                "MidSurfaceMultiBody",
                target_cell,
                Type::Geometry,
                Type::Selector(reify_core::ty::SelectorKind::Face),
            );
            let expr = reify_ir::CompiledExpr::resolve_selector(inner);
            let mut kernel = MockGeometryKernel::new();
            let mut diagnostics = Vec::new();
            let result = super::try_eval_resolve_selector(
                &expr,
                &named_steps,
                &values,
                &mut kernel,
                &table,
                &HashMap::new(),
                &mut diagnostics,
            );

            let list = match result {
                Some(reify_ir::Value::List(ref elems)) => elems.clone(),
                other => panic!(
                    "[target={target_cell}] build-global ByRole resolution must yield the \
                     cross-body UNION as Some(Value::List(..)); got {:?}; diags: {:?}",
                    other, diagnostics
                ),
            };
            // Cross-body leak: 4 faces (both bodies), NOT just the target's 2,
            // and NOT Undef for the `non_shell` target.
            assert_eq!(
                list.len(),
                4,
                "[target={target_cell}] expected the UNION of both bodies' mid-surface \
                 faces (documented single-shell limitation), got {} elems",
                list.len()
            );
            for (i, (elem, expected_id)) in list.iter().zip(&expected_ids).enumerate() {
                match elem {
                    reify_ir::Value::GeometryHandle { kernel_handle, .. } => assert_eq!(
                        kernel_handle, &Some(*expected_id),
                        "[target={target_cell}] elem[{i}] kernel_handle in (local_index, id) order"
                    ),
                    other => panic!(
                        "[target={target_cell}] elem[{i}] must be Value::GeometryHandle, got {:?}",
                        other
                    ),
                }
            }
            assert!(
                diagnostics.is_empty(),
                "[target={target_cell}] a non-empty (leaky) resolve emits no Undef diagnostic; \
                 got {:?}",
                diagnostics
            );
        }
    }

    /// `edge(b, "rim")` evaluates to `Value::Selector(Edge)` with
    /// `LeafQuery::Named("rim")`. RED until step-10.
    #[test]
    fn edge_named_ctor_yields_named_leaf_selector_of_edge_kind() {
        use reify_core::ValueCellId;
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("NamedEdgeCtorTest", 0);
        let hash_b: [u8; 32] = [0xBB; 32];

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("NamedEdgeCtorTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );

        let expr = named_selector_call(
            "edge",
            "NamedEdgeCtorTest",
            "b",
            reify_core::ty::SelectorKind::Edge,
            "rim",
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "edge(b, \"rim\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Edge,
            "edge() → Edge kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                ..
            } => {
                assert_eq!(n, "rim", "edge(b, \"rim\") → Named(\"rim\") leaf");
            }
            other => panic!("expected Leaf{{ Named }}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `solid_body(b, "core")` evaluates to `Value::Selector(Body)` with
    /// `LeafQuery::Named("core")`. RED until step-10.
    #[test]
    fn solid_body_named_ctor_yields_named_leaf_selector_of_body_kind() {
        use reify_core::ValueCellId;
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("NamedBodyCtorTest", 0);
        let hash_b: [u8; 32] = [0xCC; 32];

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("NamedBodyCtorTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );

        let expr = named_selector_call(
            "solid_body",
            "NamedBodyCtorTest",
            "b",
            reify_core::ty::SelectorKind::Body,
            "core",
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "solid_body(b, \"core\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Body,
            "solid_body() → Body kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                ..
            } => {
                assert_eq!(n, "core", "solid_body(b, \"core\") → Named(\"core\") leaf");
            }
            other => panic!("expected Leaf{{ Named }}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// BT8: resolving `face(b, "nope")` (unknown name) returns the empty list
    /// and pushes exactly ONE `DiagnosticCode::TopologyTagStale` warning — the
    /// already-landed resolve_leaf Named interim, now reachable from .ri surface.
    /// RED until step-10 wires the face arm; the resolve assertion is only
    /// reachable once construction succeeds.
    #[test]
    fn face_named_ctor_resolve_unknown_name_yields_empty_and_topology_tag_stale() {
        use reify_core::ValueCellId;
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("NamedBT8Test", 0);
        let hash_b: [u8; 32] = [0xDD; 32];

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("NamedBT8Test", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );

        let expr = named_selector_call(
            "face",
            "NamedBT8Test",
            "b",
            reify_core::ty::SelectorKind::Face,
            "nope",
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();

        // Step 1: construction produces Value::Selector with no diagnostics.
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "face(b, \"nope\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );

        // Step 2: resolve the Named leaf — resolves to [] + exactly one TopologyTagStale.
        let resolved = crate::topology_selectors::resolve(&sv, &mut kernel, &mut diagnostics)
            .expect("resolve must not return QueryError for Named leaf");
        assert_eq!(
            resolved,
            vec![],
            "Named(\"nope\"): resolve must return empty list (D8 interim)"
        );
        let stale_count = diagnostics
            .iter()
            .filter(|d| d.code == Some(reify_core::DiagnosticCode::TopologyTagStale))
            .count();
        assert_eq!(
            stale_count, 1,
            "resolve of Named leaf with no matching tag must emit exactly ONE \
             W_TOPOLOGY_TAG_STALE; got {stale_count}: {:?}",
            diagnostics
        );
    }

    // ── try_eval_topology_selector directional-selector dispatch unit tests ───
    // (task 3618, KGQ-ι: faces_by_normal + edges_parallel_to)
    //
    // These tests pin that the rewired arms resolve arg[0] via `values` (not
    // `named_steps`), run the filter helper, recover canonical TopExp indices,
    // and emit Value::List([Value::GeometryHandle]) — not Value::Int.
    //
    // The canonical-index correctness requirement: faces_by_normal(box,+z,1°)[0]
    // must hash identically to faces(box)[k] for the same physical face.
    // Each test places the retained face/edge at a NON-ZERO canonical index to
    // exercise the index recovery (if the arm hardcoded index 0 it would pass a
    // trivial index-0-only case but fail these).

    /// `faces_by_normal` dispatch emits `Value::List([Value::GeometryHandle])`
    /// with the retained face's canonical TopExp index (not filtered position).
    ///
    /// Setup: canonical list [GHId(2), GHId(3), GHId(4)]; only GHId(3) (index 1)
    /// has a +z normal within 1°; GHId(2) (+x) and GHId(4) (−z, sign-sensitive)
    /// are rejected. The result must carry canonical index 1, not position 0.
    #[test]
    fn faces_by_normal_dispatch_returns_geometry_handle_sub_handles() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Directional", 0);
        let parent_hash: [u8; 32] = [0xAA; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(
                parent_handle,
                vec![
                    GeometryHandleId(2),
                    GeometryHandleId(3),
                    GeometryHandleId(4),
                ],
            )
            .with_face_normal_result(
                GeometryHandleId(2),
                reify_ir::Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".to_string()),
            )
            .with_face_normal_result(
                GeometryHandleId(3),
                reify_ir::Value::String("{\"x\":0.0,\"y\":0.0,\"z\":1.0}".to_string()),
            )
            .with_face_normal_result(
                GeometryHandleId(4),
                reify_ir::Value::String("{\"x\":0.0,\"y\":0.0,\"z\":-1.0}".to_string()),
            );

        // named_steps carries a different handle id to prove the arm reads from
        // values, not named_steps.
        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(GeometryHandleId(99)));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Directional", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        values.insert(
            ValueCellId::new("Directional", "dir"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        let tol_rad = std::f64::consts::PI / 180.0; // 1°
        values.insert(
            ValueCellId::new("Directional", "tol"),
            reify_ir::Value::Scalar {
                si_value: tol_rad,
                dimension: DimensionVector::ANGLE,
            },
        );

        let expr = topology_selector_call_three_value_refs(
            "faces_by_normal",
            "Directional",
            "b",
            Type::Geometry,
            "dir",
            Type::vec3(Type::dimensionless_scalar()),
            "tol",
            Type::angle(),
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Task 4118 (γ): construction is kernel-FREE — `faces_by_normal(b, dir, tol)`
        // builds a typed `Value::Selector(Face)` with a `ByNormal` leaf carrying the
        // direction + angular tolerance, NOT an eagerly-filtered `Value::List`. The
        // staged extract_faces / face-normal kernel data is intentionally unused
        // (zero kernel queries during construction, K2/BT7); the predicate filter and
        // canonical sub-handle indexing now run on the ResolveSelector / resolve()
        // path (see the try_eval_resolve_selector tests).
        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "faces_by_normal(..) must yield Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "faces_by_normal → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, Some(parent_handle),
                    "leaf target must be the parent solid handle"
                );
                assert_eq!(
                    *query,
                    reify_ir::value::LeafQuery::ByNormal {
                        dir: [0.0, 0.0, 1.0],
                        tol_rad
                    },
                    "faces_by_normal → ByNormal leaf (dir +z, tol 1°)"
                );
            }
            other => panic!(
                "faces_by_normal must be a Leaf selector node, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "kernel-free construction must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `faces_by_normal` falls through to `None` when the parent arg is not a
    /// hydrated `Value::GeometryHandle` in `values` (PRD §4 invariant #2).
    #[test]
    fn faces_by_normal_dispatch_falls_through_when_parent_not_hydrated() {
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let mut kernel = MockGeometryKernel::new();
        let named_steps = HashMap::new();

        // values has NO Value::GeometryHandle for the arg cell.
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Directional", "dir"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        values.insert(
            ValueCellId::new("Directional", "tol"),
            reify_ir::Value::Scalar {
                si_value: std::f64::consts::PI / 180.0,
                dimension: DimensionVector::ANGLE,
            },
        );

        let expr = topology_selector_call_three_value_refs(
            "faces_by_normal",
            "Directional",
            "b",
            Type::Geometry,
            "dir",
            Type::vec3(Type::dimensionless_scalar()),
            "tol",
            Type::angle(),
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "must fall through to None when parent is not a hydrated Value::GeometryHandle; \
             got {:?}",
            result
        );
    }

    /// `edges_parallel_to` dispatch emits `Value::List([Value::GeometryHandle])`
    /// with the retained edge's canonical TopExp index (not filtered position).
    ///
    /// Setup: canonical list [GHId(2), GHId(3), GHId(4)]; only GHId(4) (index 2)
    /// is (anti-)parallel to +z within 1° (tangent = −z; sign-tolerant predicate).
    /// GHId(2) and GHId(3) have +x tangents and are rejected. The result must
    /// carry canonical index 2, not position 0.
    #[test]
    fn edges_parallel_to_dispatch_returns_geometry_handle_sub_handles() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{DimensionVector, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Directional", 0);
        let parent_hash: [u8; 32] = [0xBB; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(
                parent_handle,
                vec![
                    GeometryHandleId(2),
                    GeometryHandleId(3),
                    GeometryHandleId(4),
                ],
            )
            .with_edge_tangent_result(
                GeometryHandleId(2),
                reify_ir::Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".to_string()),
            )
            .with_edge_tangent_result(
                GeometryHandleId(3),
                reify_ir::Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".to_string()),
            )
            // GHId(4) has tangent −z: sign-tolerant |dot(−z, +z)| = 1 ≥ cos(1°), retained.
            .with_edge_tangent_result(
                GeometryHandleId(4),
                reify_ir::Value::String("{\"x\":0.0,\"y\":0.0,\"z\":-1.0}".to_string()),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(GeometryHandleId(99)));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Directional", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        values.insert(
            ValueCellId::new("Directional", "axis"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        let tol_rad = std::f64::consts::PI / 180.0; // 1°
        values.insert(
            ValueCellId::new("Directional", "tol"),
            reify_ir::Value::Scalar {
                si_value: tol_rad,
                dimension: DimensionVector::ANGLE,
            },
        );

        let expr = topology_selector_call_three_value_refs(
            "edges_parallel_to",
            "Directional",
            "b",
            Type::Geometry,
            "axis",
            Type::vec3(Type::dimensionless_scalar()),
            "tol",
            Type::angle(),
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Task 4118 (γ): construction is kernel-FREE — `edges_parallel_to(b, axis, tol)`
        // builds a typed `Value::Selector(Edge)` with a `ByParallel` leaf carrying the
        // axis + angular tolerance, NOT an eagerly-filtered `Value::List`. The staged
        // extract_edges / edge-tangent kernel data is intentionally unused (zero kernel
        // queries during construction, K2/BT7); the predicate filter and canonical
        // sub-handle indexing now run on the ResolveSelector / resolve() path.
        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "edges_parallel_to(..) must yield Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Edge,
            "edges_parallel_to → Edge kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, Some(parent_handle),
                    "leaf target must be the parent solid handle"
                );
                assert_eq!(
                    *query,
                    reify_ir::value::LeafQuery::ByParallel {
                        axis: [0.0, 0.0, 1.0],
                        tol_rad
                    },
                    "edges_parallel_to → ByParallel leaf (axis +z, tol 1°)"
                );
            }
            other => panic!(
                "edges_parallel_to must be a Leaf selector node, got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "kernel-free construction must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    // --- dispatch_filtered_subhandles defensive-branch tests ---

    /// Branch (a): filter_result is Err → dispatch emits a Warning and returns Value::Undef.
    #[test]
    fn dispatch_filtered_subhandles_filter_error_yields_undef_and_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_rr = RealizationNodeId::new("Def", 0);
        let parent_hash: [u8; 32] = [0x01; 32];
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();

        let result = super::dispatch_filtered_subhandles(
            &mut kernel,
            GeometryHandleId(1),
            crate::topology_selectors::SubKind::Face,
            &parent_rr,
            &parent_hash,
            Err(reify_ir::QueryError::QueryFailed(
                "mock filter failure".to_string(),
            )),
            "faces_by_normal",
            &mut diagnostics,
        );

        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "filter Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(diagnostics.len(), 1, "must emit exactly one warning");
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "diagnostic must be Warning severity, got {:?}",
            diagnostics[0]
        );
    }

    /// Branch (b): filter_result is Ok but canonical re-extract fails → Warning + Value::Undef.
    #[test]
    fn dispatch_filtered_subhandles_canonical_reextract_error_yields_undef_and_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_rr = RealizationNodeId::new("Def", 0);
        let parent_hash: [u8; 32] = [0x02; 32];
        // Kernel has no extract_faces entry for the parent → extract_faces returns QueryError.
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();

        // Filter returned Ok with a retained id, but the re-extract below will fail.
        let result = super::dispatch_filtered_subhandles(
            &mut kernel,
            GeometryHandleId(1),
            crate::topology_selectors::SubKind::Face,
            &parent_rr,
            &parent_hash,
            Ok(vec![GeometryHandleId(2)]),
            "faces_by_normal",
            &mut diagnostics,
        );

        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "canonical re-extract Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(diagnostics.len(), 1, "must emit exactly one warning");
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "diagnostic must be Warning severity, got {:?}",
            diagnostics[0]
        );
    }

    /// Branch (c): a retained id is absent from the canonical list → that element is silently
    /// skipped (list is shorter than retained), and a Warning is emitted for the missing id.
    #[test]
    fn dispatch_filtered_subhandles_absent_retained_id_is_skipped_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_rr = RealizationNodeId::new("Def", 0);
        let parent_hash: [u8; 32] = [0x03; 32];
        let parent_handle = GeometryHandleId(1);
        // Canonical list: [GHId(2), GHId(3)] — GHId(99) is NOT present.
        let mut kernel = MockGeometryKernel::new().with_extracted_faces(
            parent_handle,
            vec![GeometryHandleId(2), GeometryHandleId(3)],
        );
        let mut diagnostics = Vec::new();

        // Retained contains one present id (GHId(2)) and one absent id (GHId(99)).
        let result = super::dispatch_filtered_subhandles(
            &mut kernel,
            parent_handle,
            crate::topology_selectors::SubKind::Face,
            &parent_rr,
            &parent_hash,
            Ok(vec![GeometryHandleId(2), GeometryHandleId(99)]),
            "faces_by_normal",
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "must yield Some(Value::List(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        // GHId(2) at canonical index 0 is included; GHId(99) is absent → skipped.
        assert_eq!(
            list.len(),
            1,
            "absent retained id must be skipped; expected 1 element, got {}; diags: {:?}",
            list.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "must emit one warning for the absent id"
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "diagnostic must be Warning severity, got {:?}",
            diagnostics[0]
        );
        assert!(
            diagnostics[0]
                .message
                .contains("absent from canonical list"),
            "warning must mention 'absent from canonical list'; got: {}",
            diagnostics[0].message
        );
    }

    // ── GHR-ζ (task 3608): whole-handle geometry-query defensive-downgrade ──
    //
    // The volume/area/centroid/bounding_box dispatch helpers each promise the
    // PRD §4 defensive-downgrade contract: on a kernel error OR an unexpected/
    // malformed reply, return `Some(Value::Undef)` and push EXACTLY ONE Warning
    // (never `None`, never a panic). These unit tests drive each arm with a
    // `MockGeometryKernel` that (a) has no registered result → `query` returns
    // `Err`, and (b) returns a wrong-typed reply, asserting the Undef + single-
    // Warning contract. They live here, not in the OCCT integration test,
    // because the dispatch helpers are crate-private and a real kernel cannot be
    // coerced into erroring for a valid primitive.

    /// `volume` arm (`dispatch_scalar_query`, VOLUME): kernel `Err` and an
    /// unexpected (non-Real/Scalar) reply each yield `Some(Value::Undef)` + one
    /// Warning.
    #[test]
    fn dispatch_volume_query_error_and_unexpected_reply_yield_undef_and_one_warning() {
        use reify_test_support::mocks::MockGeometryKernel;

        // (a) kernel Err — no registered result.
        let kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::Volume(GeometryHandleId(1)),
            reify_core::DimensionVector::VOLUME,
            "volume",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "volume (kernel Err) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "volume (kernel Err) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);

        // (b) unexpected reply type — Bool is neither Real nor Scalar.
        let kernel = MockGeometryKernel::new()
            .with_volume_result(GeometryHandleId(1), reify_ir::Value::Bool(true));
        let mut diagnostics = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::Volume(GeometryHandleId(1)),
            reify_core::DimensionVector::VOLUME,
            "volume",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "volume (unexpected reply) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "volume (unexpected reply) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);
    }

    /// `area` arm (`dispatch_scalar_query`, AREA): kernel `Err` and an
    /// unexpected reply each yield `Some(Value::Undef)` + one Warning.
    #[test]
    fn dispatch_area_query_error_and_unexpected_reply_yield_undef_and_one_warning() {
        use reify_test_support::mocks::MockGeometryKernel;

        // (a) kernel Err.
        let kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::SurfaceArea(GeometryHandleId(1)),
            reify_core::DimensionVector::AREA,
            "area",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "area (kernel Err) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "area (kernel Err) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);

        // (b) unexpected reply type.
        let kernel = MockGeometryKernel::new()
            .with_surface_area_result(GeometryHandleId(1), reify_ir::Value::Bool(true));
        let mut diagnostics = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::SurfaceArea(GeometryHandleId(1)),
            reify_core::DimensionVector::AREA,
            "area",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "area (unexpected reply) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "area (unexpected reply) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);
    }

    /// `centroid` arm (`dispatch_point3_length_reply`): kernel `Err` and a
    /// malformed (non-String) reply each yield `Some(Value::Undef)` + one
    /// Warning.
    #[test]
    fn dispatch_centroid_query_error_and_malformed_reply_yield_undef_and_one_warning() {
        use reify_test_support::mocks::MockGeometryKernel;

        // (a) kernel Err.
        let kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::dispatch_point3_length_reply(
            &kernel,
            &reify_ir::GeometryQuery::Centroid(GeometryHandleId(1)),
            "centroid",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "centroid (kernel Err) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "centroid (kernel Err) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);

        // (b) malformed reply — a non-String value fails parse_xyz_value.
        let kernel = MockGeometryKernel::new()
            .with_centroid_result(GeometryHandleId(1), reify_ir::Value::Bool(true));
        let mut diagnostics = Vec::new();
        let result = super::dispatch_point3_length_reply(
            &kernel,
            &reify_ir::GeometryQuery::Centroid(GeometryHandleId(1)),
            "centroid",
            &mut diagnostics,
        );
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "centroid (malformed reply) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "centroid (malformed reply) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);
    }

    /// `bounding_box` arm (`dispatch_bounding_box`): kernel `Err` and a
    /// malformed (non-String) reply each yield `Some(Value::Undef)` + one
    /// Warning.
    #[test]
    fn dispatch_bounding_box_query_error_and_malformed_reply_yield_undef_and_one_warning() {
        use reify_test_support::mocks::MockGeometryKernel;

        // (a) kernel Err.
        let kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::dispatch_bounding_box(&kernel, GeometryHandleId(1), &mut diagnostics);
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "bounding_box (kernel Err) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "bounding_box (kernel Err) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);

        // (b) malformed reply — a non-String value fails parse_bbox_axis_extents.
        let kernel = MockGeometryKernel::new()
            .with_bbox_result(GeometryHandleId(1), reify_ir::Value::Bool(true));
        let mut diagnostics = Vec::new();
        let result = super::dispatch_bounding_box(&kernel, GeometryHandleId(1), &mut diagnostics);
        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "bounding_box (malformed reply) must yield Some(Value::Undef); got {result:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "bounding_box (malformed reply) must emit exactly one warning; got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].severity, reify_core::Severity::Warning);
    }

    // ── step-1 (task 3619): adjacent_faces dispatch unit tests ──────────────
    //
    // These tests verify that the arm emits Value::List(Value::GeometryHandle)
    // via dispatch_filtered_subhandles.

    /// `adjacent_faces` dispatch returns `Value::List` of one
    /// `Value::GeometryHandle` when the mock kernel returns the adjacent face
    /// at index 0. The element must carry the parent's `realization_ref` and
    /// an `upstream_values_hash` equal to
    /// `compose_sub_handle_hash(parent_hash, SubKind::Face, 0)`.
    #[test]
    fn adjacent_faces_dispatch_returns_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // args[0]: parent solid; args[1]: face sub-handle (same handle in mock)
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent_handle, vec![GeometryHandleId(1)])
            .with_adjacent_faces_result(
                parent_handle,
                0,
                reify_ir::Value::List(vec![reify_ir::Value::Int(0)]),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        // Seed parent solid (args[0])
        values.insert(
            ValueCellId::new("Solid", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        // Seed face arg (args[1]) — same kernel handle for the mock
        values.insert(
            ValueCellId::new("Solid", "face"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );

        let expr = topology_selector_call_two_value_refs(
            "adjacent_faces",
            "Solid",
            "b",
            Type::Geometry,
            "face",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "expected Some(Value::List(..)), got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "expected 1 adjacent face sub-handle; diags: {:?}",
            diagnostics
        );

        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Face,
            0,
        );
        match &list[0] {
            reify_ir::Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            } => {
                assert_eq!(
                    realization_ref.entity, parent_rr.entity,
                    "realization_ref.entity must match parent"
                );
                assert_eq!(
                    realization_ref.index, parent_rr.index,
                    "realization_ref.index must match parent"
                );
                assert_eq!(
                    *kernel_handle,
                    Some(GeometryHandleId(1)),
                    "kernel_handle must be GHId(1)"
                );
                assert_eq!(
                    *upstream_values_hash, expected_hash,
                    "upstream_values_hash must be compose_sub_handle_hash(parent_hash, Face, 0)"
                );
            }
            other => panic!("elem[0] is not Value::GeometryHandle: {:?}", other),
        }
    }

    /// When args[1]'s cell is absent from `values`, the `adjacent_faces` arm
    /// must fall through to `None` (PRD invariant #2: never partial-construct).
    #[test]
    fn adjacent_faces_dispatch_falls_through_when_face_arg_not_hydrated() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent_handle, vec![GeometryHandleId(1)]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        // Only the parent is seeded; the face cell is absent
        values.insert(
            ValueCellId::new("Solid", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        // "face" cell intentionally absent from values

        let expr = topology_selector_call_two_value_refs(
            "adjacent_faces",
            "Solid",
            "b",
            Type::Geometry,
            "face",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "must fall through to None when face arg is not a hydrated Value::GeometryHandle, \
             got {:?}",
            result
        );
    }

    // ── step-3 (task 3619): shared_edges dispatch unit tests ─────────────────
    //
    // These tests verify that the arm emits Value::List(Value::GeometryHandle)
    // via dispatch_filtered_subhandles.

    /// `shared_edges` dispatch returns `Value::List` of one
    /// `Value::GeometryHandle` (kernel_handle GHId(4)) when the mock kernel
    /// stages two faces (GHId(2), GHId(3)) sharing one edge (GHId(4)).
    /// The element must carry the parent solid's `realization_ref` and an
    /// `upstream_values_hash` equal to
    /// `compose_sub_handle_hash(parent_hash, SubKind::Edge, 0)`.
    #[test]
    fn shared_edges_dispatch_returns_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_a_handle = GeometryHandleId(2);
        let face_b_handle = GeometryHandleId(3);
        let edge_handle = GeometryHandleId(4);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_owner_body_result(face_a_handle, parent_handle)
            .with_owner_body_result(face_b_handle, parent_handle)
            .with_extracted_faces(parent_handle, vec![face_a_handle, face_b_handle])
            .with_extracted_edges(parent_handle, vec![edge_handle])
            .with_shared_edges_result(
                parent_handle,
                0,
                1,
                reify_ir::Value::List(vec![reify_ir::Value::Int(0)]),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("fa".to_string(), kh(face_a_handle));
        named_steps.insert("fb".to_string(), kh(face_b_handle));

        let mut values = reify_ir::ValueMap::new();
        // Parent solid — found by resolve_owner_solid_handle scanning values
        values.insert(
            ValueCellId::new("Solid", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        // Face args — resolved by resolve_parent_geometry_handle_arg for the arm
        values.insert(
            ValueCellId::new("Solid", "fa"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_a_handle),
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fb"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_b_handle),
            },
        );

        let expr = topology_selector_call_two_value_refs(
            "shared_edges",
            "Solid",
            "fa",
            Type::Geometry,
            "fb",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "expected Some(Value::List(..)), got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "expected 1 shared edge sub-handle; diags: {:?}",
            diagnostics
        );

        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Edge,
            0,
        );
        match &list[0] {
            reify_ir::Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            } => {
                assert_eq!(
                    realization_ref.entity, parent_rr.entity,
                    "realization_ref.entity must match parent solid"
                );
                assert_eq!(
                    realization_ref.index, parent_rr.index,
                    "realization_ref.index must match parent solid"
                );
                assert_eq!(
                    *kernel_handle, Some(edge_handle),
                    "kernel_handle must be the edge GHId(4)"
                );
                assert_eq!(
                    *upstream_values_hash, expected_hash,
                    "upstream_values_hash must be compose_sub_handle_hash(parent_hash, Edge, 0)"
                );
            }
            other => panic!("elem[0] is not Value::GeometryHandle: {:?}", other),
        }
    }

    /// When the parent solid is not hydrated in `values`, the `shared_edges`
    /// arm must fall through to `None` (PRD invariant #2: never partial-construct).
    #[test]
    fn shared_edges_dispatch_falls_through_when_parent_not_hydrated() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_a_handle = GeometryHandleId(2);
        let face_b_handle = GeometryHandleId(3);
        let edge_handle = GeometryHandleId(4);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_owner_body_result(face_a_handle, parent_handle)
            .with_owner_body_result(face_b_handle, parent_handle)
            .with_extracted_faces(parent_handle, vec![face_a_handle, face_b_handle])
            .with_extracted_edges(parent_handle, vec![edge_handle])
            .with_shared_edges_result(
                parent_handle,
                0,
                1,
                reify_ir::Value::List(vec![reify_ir::Value::Int(0)]),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("fa".to_string(), kh(face_a_handle));
        named_steps.insert("fb".to_string(), kh(face_b_handle));

        let mut values = reify_ir::ValueMap::new();
        // Face args present — arm resolves them, then hits dispatch_shared_edges
        values.insert(
            ValueCellId::new("Solid", "fa"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_a_handle),
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fb"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_b_handle),
            },
        );
        // Parent solid (kernel_handle=GHId(1)) intentionally absent from values
        // so resolve_owner_solid_handle returns None → arm falls through.

        let expr = topology_selector_call_two_value_refs(
            "shared_edges",
            "Solid",
            "fa",
            Type::Geometry,
            "fb",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "must fall through to None when parent solid is not hydrated in values, got {:?}",
            result
        );
    }

    // ── error-path coverage for dispatch_filtered_subhandles (suggestion 3) ──

    /// When the `AdjacentFaces` kernel query is not staged, `adjacent_to_face`
    /// returns `Err`; `dispatch_filtered_subhandles` receives `filter_result = Err`
    /// and must return `Some(Value::Undef)` with a Warning diagnostic.
    #[test]
    fn adjacent_faces_dispatch_emits_warning_and_undef_on_kernel_query_failure() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // Stage extract_faces so adjacent_to_face can find the face index (0),
        // but omit the AdjacentFaces query result → kernel.query(...) returns Err
        // → adjacent_to_face propagates Err → filter_result = Err in
        // dispatch_filtered_subhandles → Warning + Value::Undef.
        let mut kernel =
            MockGeometryKernel::new().with_extracted_faces(parent_handle, vec![face_handle]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Solid", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        values.insert(
            ValueCellId::new("Solid", "face"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_handle),
            },
        );

        let expr = topology_selector_call_two_value_refs(
            "adjacent_faces",
            "Solid",
            "b",
            Type::Geometry,
            "face",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "kernel query failure must yield Value::Undef; got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            !diagnostics.is_empty(),
            "a Warning diagnostic must be emitted on kernel query failure"
        );
    }

    /// When the `SharedEdges` kernel query is not staged, `dispatch_shared_edges`
    /// hits `Err` at step 4 (SharedEdges query) and must return
    /// `Some(Value::Undef)` with a Warning diagnostic.
    #[test]
    fn shared_edges_dispatch_emits_warning_and_undef_on_shared_edges_query_failure() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_a_handle = GeometryHandleId(2);
        let face_b_handle = GeometryHandleId(3);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // Stage OwnerBody + extract_faces so the arm passes the cross-solid guard
        // and face-index recovery, but omit the SharedEdges query result →
        // kernel.query(SharedEdges { ... }) returns Err → Warning + Value::Undef.
        let mut kernel = MockGeometryKernel::new()
            .with_owner_body_result(face_a_handle, parent_handle)
            .with_owner_body_result(face_b_handle, parent_handle)
            .with_extracted_faces(parent_handle, vec![face_a_handle, face_b_handle]);

        let mut named_steps = HashMap::new();
        named_steps.insert("fa".to_string(), kh(face_a_handle));
        named_steps.insert("fb".to_string(), kh(face_b_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Solid", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fa"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_a_handle),
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fb"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_b_handle),
            },
        );

        let expr = topology_selector_call_two_value_refs(
            "shared_edges",
            "Solid",
            "fa",
            Type::Geometry,
            "fb",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "SharedEdges query failure must yield Value::Undef; got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            !diagnostics.is_empty(),
            "a Warning diagnostic must be emitted on SharedEdges query failure"
        );
    }

    // ── step-3 (task #4759): siblings_of_face + ancestor_faces_of_edge dispatch ─
    //
    // Both selectors mirror adjacent_faces: parent solid (args[0]) + target
    // sub-handle (args[1]), returning List<GeometryHandle> FACE sub-handles.
    // RED until step-4 adds the name→enum arms and dispatch arms.

    /// `siblings_of_face` dispatch returns `Value::List` of one
    /// `Value::GeometryHandle` (the non-queried face) when the mock kernel
    /// stages two faces GHId(1) and GHId(2), with face GHId(1) as the target.
    /// The returned sub-handle element must carry the parent solid's
    /// `realization_ref` and an `upstream_values_hash` equal to
    /// `compose_sub_handle_hash(parent_hash, SubKind::Face, 1)` (canonical
    /// index 1 for GHId(2) in extract_faces = [GHId(1), GHId(2)];
    /// dispatch_filtered_subhandles uses the CANONICAL index from extract_faces,
    /// NOT the position in the filtered output — this ensures hash-stability
    /// so `faces_by_normal(box,+z)[0]` hashes identically to `faces(box)[k]`
    /// for the same physical face).
    #[test]
    fn siblings_of_face_dispatch_returns_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_handle = GeometryHandleId(1);   // queried face
        let sibling_handle = GeometryHandleId(2); // the one sibling
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // Stage extract_faces with [face_handle, sibling_handle] so
        // siblings_of_face can find face_handle at index 0 and return [sibling_handle].
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent_handle, vec![face_handle, sibling_handle]);

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        // Seed parent solid (args[0])
        values.insert(
            ValueCellId::new("Solid", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        // Seed face arg (args[1]) — the target face sub-handle
        values.insert(
            ValueCellId::new("Solid", "top"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_handle),
            },
        );

        let expr = topology_selector_call_two_value_refs(
            "siblings_of_face",
            "Solid",
            "b",
            Type::Geometry,
            "top",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "expected Some(Value::List(..)), got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            1,
            "expected 1 sibling face sub-handle; diags: {:?}",
            diagnostics
        );

        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Face,
            1, // GHId(2) is at canonical index 1 in extract_faces = [GHId(1), GHId(2)]
        );
        match &list[0] {
            reify_ir::Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            } => {
                assert_eq!(
                    realization_ref.entity, parent_rr.entity,
                    "realization_ref.entity must match parent"
                );
                assert_eq!(
                    realization_ref.index, parent_rr.index,
                    "realization_ref.index must match parent"
                );
                assert_eq!(
                    *kernel_handle,
                    Some(sibling_handle),
                    "kernel_handle must be the sibling GHId(2)"
                );
                assert_eq!(
                    *upstream_values_hash, expected_hash,
                    "upstream_values_hash must be compose_sub_handle_hash(parent_hash, Face, 1)"
                );
            }
            other => panic!("elem[0] is not Value::GeometryHandle: {:?}", other),
        }
    }

    /// `ancestor_faces_of_edge` dispatch returns `Value::List` of two
    /// `Value::GeometryHandle` FACE sub-handles when the mock kernel stages
    /// two faces (GHId(2), GHId(3)) owning edge GHId(4) (face indices 0 and 1).
    #[test]
    fn ancestor_faces_of_edge_dispatch_returns_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_a_handle = GeometryHandleId(2);
        let face_b_handle = GeometryHandleId(3);
        let edge_handle = GeometryHandleId(4);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // Stage extract_edges so ancestor_faces_of_edge can find the edge index,
        // extract_faces for the face-index→handle mapping, and
        // AncestorFacesOfEdge returning face indices [0, 1].
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(parent_handle, vec![edge_handle])
            .with_extracted_faces(parent_handle, vec![face_a_handle, face_b_handle])
            .with_ancestor_faces_result(
                parent_handle,
                0,
                reify_ir::Value::List(vec![
                    reify_ir::Value::Int(0),
                    reify_ir::Value::Int(1),
                ]),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        // Seed parent solid (args[0])
        values.insert(
            ValueCellId::new("Solid", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        // Seed edge arg (args[1]) — the target edge sub-handle
        values.insert(
            ValueCellId::new("Solid", "an_edge"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(edge_handle),
            },
        );

        let expr = topology_selector_call_two_value_refs(
            "ancestor_faces_of_edge",
            "Solid",
            "b",
            Type::Geometry,
            "an_edge",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "expected Some(Value::List(..)), got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            list.len(),
            2,
            "expected 2 ancestor face sub-handles; diags: {:?}",
            diagnostics
        );

        // Pre-compute expected upstream_values_hash for each canonical index.
        // face_a (GHId(2)) is at canonical index 0 and face_b (GHId(3)) at index 1
        // in extract_faces = [face_a_handle, face_b_handle].
        let expected_hash_0 = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Face,
            0,
        );
        let expected_hash_1 = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Face,
            1,
        );

        // Both returned handles must be face sub-handles carrying the parent rr.
        for (i, elem) in list.iter().enumerate() {
            match elem {
                reify_ir::Value::GeometryHandle {
                    realization_ref,
                    kernel_handle,
                    upstream_values_hash,
                } => {
                    assert_eq!(
                        realization_ref.entity, parent_rr.entity,
                        "elem[{i}] realization_ref.entity must match parent"
                    );
                    let expected_kh = if i == 0 {
                        Some(face_a_handle)
                    } else {
                        Some(face_b_handle)
                    };
                    assert_eq!(
                        *kernel_handle, expected_kh,
                        "elem[{i}] kernel_handle must be the face GHId"
                    );
                    let expected_hash = if i == 0 {
                        &expected_hash_0
                    } else {
                        &expected_hash_1
                    };
                    assert_eq!(
                        upstream_values_hash, expected_hash,
                        "elem[{i}] upstream_values_hash must be \
                         compose_sub_handle_hash(parent_hash, Face, {i})"
                    );
                }
                other => panic!("elem[{i}] is not Value::GeometryHandle: {:?}", other),
            }
        }
    }

    // ── single(relational) unwrap tests (task #4873) ─────────────────────────
    //
    // These pin the new single() fallback in try_eval_resolve_selector: when
    // resolve_selector_to_list returns None (relational selectors yield
    // Value::List, not Value::Selector, so reconstruct_selector_value returns
    // None), the arm must fall back to try_eval_topology_selector and unwrap
    // the unique element.
    //
    // RED on main: the single() arm ends with `resolve_selector_to_list(...)? {`
    // — the `?` propagates None for relational selectors, so the arm returns
    // None and the cell is never unwrapped. GREEN after step-2 wires the fallback.

    /// `single(shared_edges(fa, fb))` via `try_eval_resolve_selector` must unwrap
    /// the unique shared edge handle when the relational selector returns exactly 1
    /// edge. This is RED on main because the single() arm routes through
    /// `resolve_selector_to_list → reconstruct_selector_value`, which returns `None`
    /// for relational selectors (they yield `Value::List`, not `Value::Selector`),
    /// so the `?` propagates `None` and the cell is never unwrapped.
    #[test]
    fn single_of_relational_shared_edges_unwraps_to_single_handle() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_a_handle = GeometryHandleId(2);
        let face_b_handle = GeometryHandleId(3);
        let edge_handle = GeometryHandleId(4);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        let mut kernel = MockGeometryKernel::new()
            .with_owner_body_result(face_a_handle, parent_handle)
            .with_owner_body_result(face_b_handle, parent_handle)
            .with_extracted_faces(parent_handle, vec![face_a_handle, face_b_handle])
            .with_extracted_edges(parent_handle, vec![edge_handle])
            .with_shared_edges_result(
                parent_handle,
                0,
                1,
                reify_ir::Value::List(vec![reify_ir::Value::Int(0)]),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("fa".to_string(), kh(face_a_handle));
        named_steps.insert("fb".to_string(), kh(face_b_handle));

        let mut values = reify_ir::ValueMap::new();
        // Parent solid — found by resolve_owner_solid_handle scanning values.
        values.insert(
            ValueCellId::new("Solid", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        // Face args — resolved by the shared_edges arm.
        values.insert(
            ValueCellId::new("Solid", "fa"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_a_handle),
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fb"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_b_handle),
            },
        );

        // Build: single(shared_edges(fa, fb))
        // Inner: the shared_edges(fa, fb) FunctionCall.
        let shared_edges_expr = topology_selector_call_two_value_refs(
            "shared_edges",
            "Solid",
            "fa",
            Type::Geometry,
            "fb",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        // Outer: FunctionCall { "single", [shared_edges_expr] }
        let mut ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("single"));
        ch = ch.combine(shared_edges_expr.content_hash);
        let single_expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "single".to_string(),
                    qualified_name: "single".to_string(),
                },
                args: vec![shared_edges_expr],
            },
            result_type: Type::Geometry,
            content_hash: ch,
        };

        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_resolve_selector(
            &single_expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &HashMap::new(),
            &mut diagnostics,
        );

        let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
            &parent_hash,
            crate::topology_selectors::SubKind::Edge,
            0,
        );
        match result {
            Some(reify_ir::Value::GeometryHandle {
                realization_ref,
                upstream_values_hash,
                kernel_handle,
            }) => {
                assert_eq!(
                    realization_ref.entity, parent_rr.entity,
                    "single(shared_edges) realization_ref.entity must match parent"
                );
                assert_eq!(
                    realization_ref.index, parent_rr.index,
                    "single(shared_edges) realization_ref.index must match parent"
                );
                assert_eq!(
                    kernel_handle,
                    Some(edge_handle),
                    "single(shared_edges) kernel_handle must be the edge GHId(4)"
                );
                assert_eq!(
                    upstream_values_hash, expected_hash,
                    "single(shared_edges) upstream_values_hash must be \
                     compose_sub_handle_hash(parent_hash, Edge, 0)"
                );
            }
            other => panic!(
                "single(shared_edges(fa,fb)) with 1 shared edge must yield \
                 Some(Value::GeometryHandle{{..}}), got {:?}; diagnostics: {:?}",
                other, diagnostics
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "successful single(relational) must emit zero diagnostics; got {:?}",
            diagnostics
        );
    }

    /// Cardinality guard: when `shared_edges(fa, fb)` returns 2 edges,
    /// `single(shared_edges(fa, fb))` must yield `Some(Value::Undef)` with a
    /// Warning diagnostic (mirrors the existing `single(selector)` >1 guard in
    /// `try_eval_resolve_selector`).
    #[test]
    fn single_of_relational_shared_edges_multi_result_yields_undef() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let face_a_handle = GeometryHandleId(2);
        let face_b_handle = GeometryHandleId(3);
        let edge_a_handle = GeometryHandleId(4);
        let edge_b_handle = GeometryHandleId(5);
        let parent_rr = RealizationNodeId::new("Solid", 0);
        let parent_hash: [u8; 32] = [0x77; 32];

        // Stage 2 shared edges — single(shared_edges(...)) must yield Undef +
        // a Warning diagnostic.
        let mut kernel = MockGeometryKernel::new()
            .with_owner_body_result(face_a_handle, parent_handle)
            .with_owner_body_result(face_b_handle, parent_handle)
            .with_extracted_faces(parent_handle, vec![face_a_handle, face_b_handle])
            .with_extracted_edges(parent_handle, vec![edge_a_handle, edge_b_handle])
            .with_shared_edges_result(
                parent_handle,
                0,
                1,
                reify_ir::Value::List(vec![
                    reify_ir::Value::Int(0),
                    reify_ir::Value::Int(1),
                ]),
            );

        let mut named_steps = HashMap::new();
        named_steps.insert("fa".to_string(), kh(face_a_handle));
        named_steps.insert("fb".to_string(), kh(face_b_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Solid", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fa"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_a_handle),
            },
        );
        values.insert(
            ValueCellId::new("Solid", "fb"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_b_handle),
            },
        );

        // Build: single(shared_edges(fa, fb))
        let shared_edges_expr = topology_selector_call_two_value_refs(
            "shared_edges",
            "Solid",
            "fa",
            Type::Geometry,
            "fb",
            Type::Geometry,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("single"));
        ch = ch.combine(shared_edges_expr.content_hash);
        let single_expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "single".to_string(),
                    qualified_name: "single".to_string(),
                },
                args: vec![shared_edges_expr],
            },
            result_type: Type::Geometry,
            content_hash: ch,
        };

        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_resolve_selector(
            &single_expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "single(shared_edges) with 2 shared edges must yield Some(Undef), got {:?}",
            result
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.severity == reify_core::Severity::Warning),
            "single(shared_edges) with 2 results must emit a Warning diagnostic; got {:?}",
            diagnostics
        );
    }

    /// `single(unrecognized_fn(x, y))` must fall through to `None` so the cell
    /// is left to the pure eval path.  This pins the `None => match
    /// try_eval_topology_selector { other => other }` catch-all arm added in
    /// step-2 (#4873): when the inner arg is neither a reconstructable selector
    /// nor a recognised relational helper, both `resolve_selector_to_list` and
    /// `try_eval_topology_selector` return `None`, so `try_eval_resolve_selector`
    /// must return `None` (not accidentally capture the cell or panic).
    /// The kernel must NOT be consulted (#4873 amendment — reviewer suggestion 2).
    #[test]
    fn single_of_unrecognized_helper_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_core::Type;

        let inner = MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        // Inner: volume(fa, fb) — "volume" is a real stdlib name but NOT a
        // recognised selector or relational helper, so both
        // `resolve_selector_to_list` (via `reconstruct_selector_value`) and
        // `try_eval_topology_selector` return None for it.
        let inner_expr = topology_selector_call_two_value_refs(
            "volume",
            "Solid",
            "fa",
            Type::Geometry,
            "fb",
            Type::Geometry,
            Type::dimensionless_scalar(),
        );

        // Outer: single(volume(fa, fb))
        let mut ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("single"));
        ch = ch.combine(inner_expr.content_hash);
        let single_expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "single".to_string(),
                    qualified_name: "single".to_string(),
                },
                args: vec![inner_expr],
            },
            result_type: Type::Geometry,
            content_hash: ch,
        };

        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_resolve_selector(
            &single_expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "single(unrecognized_fn) must return None so the cell falls through \
             to pure eval; got {:?}; diagnostics: {:?}",
            result,
            diagnostics
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for an unrecognized inner helper; \
             got {} query calls",
            kernel.total_query_count()
        );
    }

    // ── try_eval_topology_selector curvature dispatch unit tests ─────────────
    // (task 3621, KGQ-μ: curvature(Curve) + curvature(Surface))
    //
    // Step-5 RED: these tests compile but FAIL until step-6 adds the
    // "curvature" → TopologySelectorHelper::Curvature arm to the dispatcher.
    // Modelled on the `normal` tests above (lines ~10006-10319).

    // DimensionVector for curvature = 1/Length = Length^-1.
    // Constructed directly (from_exps is private); index-0 is the LENGTH basis.
    const CURVATURE_DIM: reify_core::dimension::DimensionVector = {
        let mut d = [reify_core::dimension::Rational::ZERO; 10];
        d[0] = reify_core::dimension::Rational::new(-1, 1);
        reify_core::dimension::DimensionVector(d)
    };

    /// `curvature(surface, point)` with a fake kernel staging a 2×2 nested-List
    /// [[kappa_max, 0], [0, kappa_min]] must yield `Some(Value::Matrix(...))` where
    /// every cell is a `Value::Scalar` with dimension = 1/Length (Curvature), and
    /// the matrix diagonal mean (trace/2) equals the expected curvature.
    #[test]
    fn try_eval_topology_selector_curvature_surface_returns_matrix() {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_handle = reify_ir::GeometryHandleId(55);
        let kappa = 200.0_f64; // 1/(0.005 m) — sphere radius 5 mm
        // Kernel wire: [[kappa, 0.0], [0.0, kappa]] (diagonal: kappa_max == kappa_min for sphere).
        let row0 = reify_ir::Value::List(vec![
            reify_ir::Value::Real(kappa),
            reify_ir::Value::Real(0.0),
        ]);
        let row1 = reify_ir::Value::List(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(kappa),
        ]);
        // u = px = 0.005 m, v = py = 0.0 m  (eval maps DSL point3 coords → (u,v))
        let mut kernel = MockGeometryKernel::new().with_surface_curvature_at_result(
            face_handle,
            [0.005, 0.0],
            reify_ir::Value::List(vec![row0, row1]),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("CurvatureSmoke", "pt"),
            point3_length_value(0.005, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "curvature",
            "CurvatureSmoke",
            "face",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::dimensionless_scalar(), // placeholder result type — unused on dispatch path
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        // Expect a 2×2 Value::Matrix of curvature-dimensioned scalars.
        let expected_cell = reify_ir::Value::Scalar {
            si_value: kappa,
            dimension: CURVATURE_DIM,
        };
        let expected_zero = reify_ir::Value::Scalar {
            si_value: 0.0,
            dimension: CURVATURE_DIM,
        };
        let expected = Some(reify_ir::Value::Matrix(vec![
            vec![expected_cell.clone(), expected_zero.clone()],
            vec![expected_zero, expected_cell],
        ]));
        assert_eq!(
            result, expected,
            "curvature(surface, point) must return Some(Value::Matrix([[κ,0],[0,κ]])); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path surface curvature must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `curvature(curve, point)` with a fake kernel staging `Value::Real(κ)` must
    /// yield `Some(Value::Scalar{ si_value: κ, dimension: 1/Length })`.
    #[test]
    fn try_eval_topology_selector_curvature_curve_returns_scalar() {
        use reify_test_support::mocks::MockGeometryKernel;
        let edge_handle = reify_ir::GeometryHandleId(77);
        let kappa = 100.0_f64; // 1/(0.01 m) — circle radius 10 mm
        // Kernel wire: Value::Real(κ).  Staged for CurveCurvatureAt at point (0.01, 0.0, 0.0).
        let mut kernel = MockGeometryKernel::new().with_curve_curvature_at_result(
            edge_handle,
            [0.01, 0.0, 0.0],
            reify_ir::Value::Real(kappa),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("edge".to_string(), kh(edge_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("CurvatureSmoke", "pt"),
            point3_length_value(0.01, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "curvature",
            "CurvatureSmoke",
            "edge",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::dimensionless_scalar(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let expected = Some(reify_ir::Value::Scalar {
            si_value: kappa,
            dimension: CURVATURE_DIM,
        });
        assert_eq!(
            result, expected,
            "curvature(curve, point) must return Some(Value::Scalar{{κ, 1/m}}); got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path curve curvature must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `curvature(surface, point)` with no staged kernel result must yield
    /// `Some(Value::Undef)` + exactly one Warning diagnostic naming "curvature".
    #[test]
    fn try_eval_topology_selector_curvature_kernel_err_returns_undef_with_warning() {
        use reify_test_support::mocks::MockGeometryKernel;
        let face_handle = reify_ir::GeometryHandleId(55);
        // No staging — both SurfaceCurvatureAt and CurveCurvatureAt fall through to
        // the generic no-match error in the mock kernel, yielding QueryFailed.
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("face".to_string(), kh(face_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("CurvatureSmoke", "pt"),
            point3_length_value(0.005, 0.0, 0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "curvature",
            "CurvatureSmoke",
            "face",
            reify_core::Type::Geometry,
            "pt",
            reify_core::Type::point3(reify_core::Type::length()),
            reify_core::Type::dimensionless_scalar(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "curvature(...) with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning; got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning, got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("curvature"),
            "diagnostic must mention the helper name 'curvature', got: {}",
            diag.message
        );
    }

    /// `curvature(<literal>, <literal>)` must fall through to `None` without
    /// consulting the kernel — both arg-shape guards reject non-ValueRef args.
    #[test]
    fn try_eval_topology_selector_curvature_literal_args_falls_through_to_none() {
        use reify_test_support::mocks::CountingMockKernel;
        let inner = reify_test_support::mocks::MockGeometryKernel::new();
        let mut kernel = CountingMockKernel::new(inner);

        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_literal_args("curvature");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "curvature(<literal>, <literal>) must return None, got {:?}",
            result
        );
        assert_eq!(
            kernel.total_query_count(),
            0,
            "kernel must NOT be consulted for non-ValueRef args; got {} queries",
            kernel.total_query_count()
        );
        assert!(
            diagnostics.is_empty(),
            "literal-arg fall-through must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    // ── try_eval_topology_selector length dispatch unit tests ──────────────
    // (task 3622, KGQ-ν: length(Curve) + perimeter(Surface))
    //
    // Step-1 RED: tests compile but FAIL until step-2 wires the
    // "length" → TopologySelectorHelper::Length arm to the dispatcher.

    /// Build a `CompiledExpr` for `helper(<literal_real>)` with a single
    /// literal arg. Used for 1-arg literal fall-through tests.
    fn topology_selector_call_one_literal_arg(helper_name: &str) -> reify_ir::CompiledExpr {
        let arg = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(1.0),
            reify_core::Type::dimensionless_scalar(),
        );
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name))
            .combine(arg.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg],
            },
            result_type: reify_core::Type::dimensionless_scalar(),
            content_hash,
        }
    }

    /// `length(edge_sub_handle)` with a staged `Value::Real(0.02)` EdgeLength
    /// result must yield `Some(Value::length(0.02))` and zero diagnostics.
    ///
    /// PRIMARY RED assertion — pre-impl `length` hits the `_ => return None` arm.
    #[test]
    fn try_eval_topology_selector_length_edge_subhandle_returns_scalar_length() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let edge_kh = reify_ir::GeometryHandleId(10);
        let parent_rr = RealizationNodeId::new("LengthTest", 0);
        let parent_hash: [u8; 32] = [0x42; 32];
        let mut kernel =
            MockGeometryKernel::new().with_edge_length_result(edge_kh, reify_ir::Value::Real(0.02));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("LengthTest", "e"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: Some(edge_kh),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "length",
            "LengthTest",
            "e",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::length(0.02)),
            "length(edge) must return Some(Value::length(0.02 m)); got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path length must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `length(<literal>)` must fall through to `None` without consulting the
    /// kernel — the `resolve_parent_geometry_handle_arg` guard rejects non-ValueRef.
    #[test]
    fn try_eval_topology_selector_length_literal_arg_falls_through_to_none() {
        use reify_test_support::mocks::MockGeometryKernel;

        let mut kernel = MockGeometryKernel::new();
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_one_literal_arg("length");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "length(<literal>) must return None; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "literal-arg fall-through must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `length(edge_sub_handle)` with no staged EdgeLength result (mock returns
    /// error) must yield `Some(Value::Undef)` + exactly one Warning mentioning
    /// "length".
    #[test]
    fn try_eval_topology_selector_length_kernel_err_returns_undef_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let edge_kh = reify_ir::GeometryHandleId(11);
        let parent_rr = RealizationNodeId::new("LengthTest", 0);
        let parent_hash: [u8; 32] = [0x43; 32];
        // No EdgeLength staged → mock returns QueryFailed.
        let mut kernel = MockGeometryKernel::new();

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("LengthTest", "e"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: Some(edge_kh),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "length",
            "LengthTest",
            "e",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "length with kernel Err must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kernel Err must emit exactly one Warning; got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("length"),
            "diagnostic must mention 'length'; got: {}",
            diag.message
        );
    }

    // ── try_eval_topology_selector perimeter dispatch unit tests ───────────
    // (task 3622, KGQ-ν)
    //
    // Step-3 RED: tests compile but FAIL until step-4 wires the
    // "perimeter" → TopologySelectorHelper::Perimeter arm to the dispatcher.

    /// `perimeter(face_sub_handle)` where the mock kernel returns 4 edges with
    /// exactly-representable lengths 1.0+2.0+3.0+4.0=10.0 must yield
    /// `Some(Value::length(10.0))` and zero diagnostics.
    ///
    /// PRIMARY RED assertion — pre-impl `perimeter` hits the `_ => return None` arm.
    #[test]
    fn try_eval_topology_selector_perimeter_face_subhandle_sums_edge_lengths() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let face_kh = reify_ir::GeometryHandleId(20);
        let e1 = reify_ir::GeometryHandleId(21);
        let e2 = reify_ir::GeometryHandleId(22);
        let e3 = reify_ir::GeometryHandleId(23);
        let e4 = reify_ir::GeometryHandleId(24);
        let parent_rr = RealizationNodeId::new("PerimTest", 0);
        let parent_hash: [u8; 32] = [0x50; 32];
        // Use exactly-representable lengths so summation is bit-exact.
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(face_kh, vec![e1, e2, e3, e4])
            .with_edge_length_result(e1, reify_ir::Value::Real(1.0))
            .with_edge_length_result(e2, reify_ir::Value::Real(2.0))
            .with_edge_length_result(e3, reify_ir::Value::Real(3.0))
            .with_edge_length_result(e4, reify_ir::Value::Real(4.0));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("PerimTest", "f"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_kh),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "perimeter",
            "PerimTest",
            "f",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::length(10.0)),
            "perimeter(face) must return Some(Value::length(10.0 m)); got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            diagnostics.is_empty(),
            "happy-path perimeter must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `perimeter(<literal>)` must fall through to `None` — the
    /// `resolve_parent_geometry_handle_arg` guard rejects non-ValueRef args.
    #[test]
    fn try_eval_topology_selector_perimeter_literal_arg_falls_through_to_none() {
        use reify_test_support::mocks::MockGeometryKernel;

        let mut kernel = MockGeometryKernel::new();
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();

        let expr = topology_selector_call_one_literal_arg("perimeter");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "perimeter(<literal>) must return None; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "literal-arg fall-through must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `perimeter(face_sub_handle)` when `extract_edges` returns an error must
    /// yield `Some(Value::Undef)` + exactly one Warning mentioning "perimeter".
    #[test]
    fn try_eval_topology_selector_perimeter_extract_edges_error_returns_undef_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let face_kh = reify_ir::GeometryHandleId(25);
        let parent_rr = RealizationNodeId::new("PerimTest", 0);
        let parent_hash: [u8; 32] = [0x51; 32];
        let mut kernel = MockGeometryKernel::new()
            .with_extract_edges_error(face_kh, reify_ir::QueryError::InvalidHandle(face_kh));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("PerimTest", "f"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_kh),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "perimeter",
            "PerimTest",
            "f",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "perimeter with extract_edges error must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "extract_edges error must emit exactly one Warning; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("perimeter"),
            "diagnostic must mention 'perimeter'; got: {}",
            diag.message
        );
    }

    /// `perimeter(face_sub_handle)` when edges are staged but one `EdgeLength`
    /// query returns a non-Real value must yield `Some(Value::Undef)` + one Warning.
    #[test]
    fn try_eval_topology_selector_perimeter_non_real_edge_length_returns_undef_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let face_kh = reify_ir::GeometryHandleId(26);
        let e1 = reify_ir::GeometryHandleId(27);
        let e2 = reify_ir::GeometryHandleId(28);
        let parent_rr = RealizationNodeId::new("PerimTest", 0);
        let parent_hash: [u8; 32] = [0x52; 32];
        // e1 returns Real(1.0) ok, e2 returns a non-Real value → should degrade.
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(face_kh, vec![e1, e2])
            .with_edge_length_result(e1, reify_ir::Value::Real(1.0))
            .with_edge_length_result(e2, reify_ir::Value::Bool(true)); // unexpected type

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("PerimTest", "f"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_kh),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "perimeter",
            "PerimTest",
            "f",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "perimeter with non-Real EdgeLength must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Real EdgeLength must emit exactly one Warning; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diagnostics[0].severity
        );
    }

    // ── feature→datum projection over a SELECTOR receiver (review amend) ────
    //
    // The compiler types a selection's feature→datum projection (`s.axis` where
    // `s : FaceSelector` → Axis, design §2.2) but the selector→sub-handle
    // resolution is not yet wired on the eval side. These pin that the eval emits
    // an honest diagnostic instead of a silent `Value::Undef`, and that the β
    // datum→datum path is NOT captured by the new branch.

    /// A feature→datum projection whose receiver STATICALLY types as a topology
    /// selector (`Type::Selector(_)`) but does not resolve to a realized
    /// `Value::GeometryHandle` must emit exactly one select-a-subfeature
    /// `FeatureDatumAmbiguous` error and evaluate to `Value::Undef` — NOT leave the
    /// cell a silent `Value::Undef` with no diagnostic (the
    /// clean-compile-then-silent-runtime failure mode).
    #[test]
    fn feature_datum_projection_over_selector_receiver_emits_diagnostic_not_silent_undef() {
        use reify_core::ty::SelectorKind;
        use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        // `s.axis` where `s : FaceSelector`. The receiver cell is unhydrated (and a
        // Selector value would not resolve to a GeometryHandle anyway), so
        // resolve_selector_target → None and the Selector static type drives the
        // not-yet-supported diagnostic.
        let object = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("S", "s"),
            Type::Selector(SelectorKind::Face),
        );
        let expr =
            reify_ir::CompiledExpr::method_call(object, "axis".to_string(), vec![], Type::Axis);

        let values = reify_ir::ValueMap::new();
        let mut kernel = MockGeometryKernel::new();
        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "a selector-receiver feature→datum projection must yield Some(Value::Undef); \
             got {result:?}"
        );
        let errs: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Error
                    && d.code == Some(DiagnosticCode::FeatureDatumAmbiguous)
            })
            .collect();
        assert_eq!(
            errs.len(),
            1,
            "a selector-receiver projection must emit exactly one FeatureDatumAmbiguous \
             (not-yet-supported / select-a-subfeature) error rather than a silent Undef; \
             got {diagnostics:?}"
        );
    }

    /// A β *datum* receiver (`axis.dir` — receiver statically types as `Axis`, not a
    /// feature/selector) must make the kernel-backed feature-datum path DECLINE
    /// (`None`, no diagnostic) so the pure `eval_datum_projection` owns it. Guards
    /// that the selector not-yet-supported branch does not capture β's datum→datum
    /// projections.
    #[test]
    fn feature_datum_projection_over_datum_receiver_declines_to_pure_path() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let object = reify_ir::CompiledExpr::value_ref(ValueCellId::new("S", "a"), Type::Axis);
        let expr =
            reify_ir::CompiledExpr::method_call(object, "dir".to_string(), vec![], Type::Direction);

        let values = reify_ir::ValueMap::new();
        let mut kernel = MockGeometryKernel::new();
        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "a β datum receiver (`axis.dir`) must decline (None) to the pure projection \
             path; got {result:?}"
        );
        assert!(
            diagnostics.is_empty(),
            "declining to the pure path must emit no diagnostic; got {diagnostics:?}"
        );
    }

    // ── feature→datum projection over a HYDRATED Selector receiver (task 4594) ─
    //
    // These tests verify the new `eval_selector_feature_datum` arm added to
    // `try_eval_feature_datum_projection`: when the receiver cell holds a hydrated
    // `Value::Selector`, the arm resolves it to sub-handles, unions the per-handle
    // `FeatureDatumBundle`s, re-dedups across handles at the confusion-floor
    // tolerance, and calls `feature_datum_projection` — the same select-one-or-
    // diagnose refinement the GeometryHandle arm uses.

    /// Assert that a `Value` is a `Value::Axis` lying on the world Z line
    /// (origin x ≈ y ≈ 0, direction parallel to ±Z, |z| ≈ 1).
    /// Mirrors `assert_value_axis_is_z_line` from feature_datum_tests.rs.
    fn assert_value_axis_is_z_line(v: &reify_ir::Value) {
        match v {
            reify_ir::Value::Axis { origin, direction } => {
                let o = match origin.as_ref() {
                    reify_ir::Value::Point(c) if c.len() == 3 => [
                        c[0].as_f64().expect("axis origin x is numeric"),
                        c[1].as_f64().expect("axis origin y is numeric"),
                        c[2].as_f64().expect("axis origin z is numeric"),
                    ],
                    other => panic!("axis origin must be a 3-component Point; got {other:?}"),
                };
                let d = match direction.as_ref() {
                    reify_ir::Value::Direction { x, y, z } => [*x, *y, *z],
                    other => panic!("axis direction must be a Direction; got {other:?}"),
                };
                assert!(
                    o[0].abs() < 1e-9 && o[1].abs() < 1e-9,
                    "axis origin must lie on the world Z line; got {o:?}"
                );
                assert!(
                    d[0].abs() < 1e-9 && d[1].abs() < 1e-9 && (d[2].abs() - 1.0).abs() < 1e-9,
                    "axis direction must be parallel to ±Z; got {d:?}"
                );
            }
            other => panic!("expected Value::Axis; got {other:?}"),
        }
    }

    /// Build a `Value::Axis` along the world Z line at the given z-origin offset,
    /// direction +Z.  Mirrors `axis_value` from feature_datum_tests.rs.
    fn z_axis_value_at(z_origin: f64) -> reify_ir::Value {
        reify_ir::Value::Axis {
            origin: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(z_origin),
            ])),
            direction: Box::new(reify_ir::Value::Direction {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            }),
        }
    }

    /// `s.axis` where `s : FaceSelector` is backed by a hydrated `Value::Selector`
    /// that `topology_selectors::resolve` expands to a single cylindrical face,
    /// whose `FaceAnalyticDatum` is an Axis on the world-Z line, must evaluate to
    /// `Some(Value::Axis{..})` on Z with zero `FeatureDatumAmbiguous` errors.
    ///
    /// RED today: the existing stub returns `Some(Value::Undef)` + one
    /// `FeatureDatumAmbiguous` error for ANY selector receiver — hydrated or not.
    #[test]
    fn feature_datum_projection_over_selector_receiver_resolves_single_cyl_face_to_axis() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ty::SelectorKind;
        use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent = reify_ir::GeometryHandleId(1);
        let cyl_face = reify_ir::GeometryHandleId(10);

        let sv = reify_ir::value::SelectorValue::leaf(
            SelectorKind::Face,
            reify_ir::value::GeometryHandleRef {
                realization_ref: RealizationNodeId::new("S", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(parent),
            },
            reify_ir::value::LeafQuery::All,
        )
        .expect("SelectorValue::leaf for Face/All must succeed");

        // selector resolve:        extract_faces(parent)   → [cyl_face]
        // feature_datum_bundle:    extract_faces(cyl_face) → [cyl_face]
        // FaceAnalyticDatum(cyl_face)                      → Axis at z=0, dir +Z
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent, vec![cyl_face])
            .with_extracted_faces(cyl_face, vec![cyl_face])
            .with_face_analytic_datum_result(cyl_face, z_axis_value_at(0.0));

        let mut values = reify_ir::ValueMap::new();
        values.insert(ValueCellId::new("S", "s"), reify_ir::Value::Selector(sv));

        let object = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("S", "s"),
            Type::Selector(SelectorKind::Face),
        );
        let expr =
            reify_ir::CompiledExpr::method_call(object, "axis".to_string(), vec![], Type::Axis);

        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        let value = result
            .expect("hydrated-selector s.axis (single cyl face) must yield Some(..), not None");
        assert_value_axis_is_z_line(&value);

        let ambiguous_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Error
                    && d.code == Some(DiagnosticCode::FeatureDatumAmbiguous)
            })
            .collect();
        assert!(
            ambiguous_errors.is_empty(),
            "a hydrated-selector s.axis over a single coaxial face must emit zero \
             FeatureDatumAmbiguous errors; got {diagnostics:?}"
        );
    }

    /// `s.axis` where the selector resolves to TWO coaxial cylindrical faces whose
    /// analytic axes share the world-Z line at different origins must deduplicate
    /// across sub-handles and return `Some(Value::Axis{..})` on Z with zero
    /// `FeatureDatumAmbiguous` errors.
    ///
    /// RED after step-2 (before step-4 dedup): without cross-handle
    /// `dedup_datums` the combined bundle has `axes = [Z@0, Z@5]` (len 2), so
    /// `feature_datum_projection` emits FeatureDatumAmbiguous + Undef.
    #[test]
    fn feature_datum_projection_over_selector_receiver_dedups_coaxial_faces_to_single_axis() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ty::SelectorKind;
        use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent = reify_ir::GeometryHandleId(1);
        let face_a = reify_ir::GeometryHandleId(10);
        let face_b = reify_ir::GeometryHandleId(11);

        let sv = reify_ir::value::SelectorValue::leaf(
            SelectorKind::Face,
            reify_ir::value::GeometryHandleRef {
                realization_ref: RealizationNodeId::new("S", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(parent),
            },
            reify_ir::value::LeafQuery::All,
        )
        .expect("SelectorValue::leaf for Face/All must succeed");

        // selector resolve:     extract_faces(parent) → [face_a, face_b]
        // bundle(face_a):       extract_faces(face_a) → [face_a]
        //                       FaceAnalyticDatum(face_a) → Axis at z=0, dir +Z
        // bundle(face_b):       extract_faces(face_b) → [face_b]
        //                       FaceAnalyticDatum(face_b) → Axis at z=5, dir +Z
        // → two coaxial Z axes, perpendicular distance = 0 → dedup_datums merges to 1
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent, vec![face_a, face_b])
            .with_extracted_faces(face_a, vec![face_a])
            .with_extracted_faces(face_b, vec![face_b])
            .with_face_analytic_datum_result(face_a, z_axis_value_at(0.0))
            .with_face_analytic_datum_result(face_b, z_axis_value_at(5.0));

        let mut values = reify_ir::ValueMap::new();
        values.insert(ValueCellId::new("S", "s"), reify_ir::Value::Selector(sv));

        let object = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("S", "s"),
            Type::Selector(SelectorKind::Face),
        );
        let expr =
            reify_ir::CompiledExpr::method_call(object, "axis".to_string(), vec![], Type::Axis);

        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        let value = result.expect(
            "hydrated-selector s.axis (two coaxial cyl faces) must yield Some(..), not None",
        );
        assert_value_axis_is_z_line(&value);

        let ambiguous_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Error
                    && d.code == Some(DiagnosticCode::FeatureDatumAmbiguous)
            })
            .collect();
        assert!(
            ambiguous_errors.is_empty(),
            "a hydrated-selector s.axis over two coaxial faces must dedup to one axis \
             and emit zero FeatureDatumAmbiguous errors; got {diagnostics:?}"
        );
    }

    /// `s.axis` where the selector resolves to TWO genuinely non-coaxial cylindrical
    /// faces (one on Z, one on X) must NOT merge the axes and must emit exactly one
    /// `FeatureDatumAmbiguous` error and return `Some(Value::Undef)`.
    ///
    /// This guards the most important regression: real ambiguity is still surfaced
    /// even after the cross-handle dedup pass.  The dedup step merges *only* datums
    /// that are geometrically equivalent; distinct axes must survive and produce a
    /// diagnostic rather than silently picking one.
    #[test]
    fn feature_datum_projection_over_selector_receiver_ambiguous_non_coaxial_faces_emit_diagnostic()
    {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ty::SelectorKind;
        use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent = reify_ir::GeometryHandleId(1);
        let face_z = reify_ir::GeometryHandleId(10); // axis on +Z
        let face_x = reify_ir::GeometryHandleId(11); // axis on +X (perpendicular to face_z)

        let sv = reify_ir::value::SelectorValue::leaf(
            SelectorKind::Face,
            reify_ir::value::GeometryHandleRef {
                realization_ref: RealizationNodeId::new("S", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(parent),
            },
            reify_ir::value::LeafQuery::All,
        )
        .expect("SelectorValue::leaf for Face/All must succeed");

        // An axis on +X, perpendicular to Z — not coaxial with the Z axis so
        // dedup_datums will NOT merge the two.
        let x_axis_value = reify_ir::Value::Axis {
            origin: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
            ])),
            direction: Box::new(reify_ir::Value::Direction {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            }),
        };

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(parent, vec![face_z, face_x])
            .with_extracted_faces(face_z, vec![face_z])
            .with_extracted_faces(face_x, vec![face_x])
            .with_face_analytic_datum_result(face_z, z_axis_value_at(0.0))
            .with_face_analytic_datum_result(face_x, x_axis_value);

        let mut values = reify_ir::ValueMap::new();
        values.insert(ValueCellId::new("S", "s"), reify_ir::Value::Selector(sv));

        let object = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("S", "s"),
            Type::Selector(SelectorKind::Face),
        );
        let expr =
            reify_ir::CompiledExpr::method_call(object, "axis".to_string(), vec![], Type::Axis);

        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        let value = result.expect("non-coaxial s.axis must yield Some(Value::Undef), not None");
        assert!(
            matches!(value, reify_ir::Value::Undef),
            "non-coaxial s.axis must return Value::Undef; got {value:?}"
        );

        let ambiguous_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Error
                    && d.code == Some(DiagnosticCode::FeatureDatumAmbiguous)
            })
            .collect();
        assert_eq!(
            ambiguous_errors.len(),
            1,
            "non-coaxial s.axis must emit exactly one FeatureDatumAmbiguous error; \
             got {diagnostics:?}"
        );
    }

    /// `s.axis` where the selector's `topology_selectors::resolve` returns `Err`
    /// (e.g. `extract_faces` on the parent handle fails) must push a
    /// `Severity::Warning` and return `Some(Value::Undef)` — not a hard error, not
    /// `None`.
    ///
    /// Mirrors the `try_eval_resolve_selector` Err handling precedent
    /// (geometry_ops.rs `@try_eval_resolve_selector` Warning arm).
    #[test]
    fn feature_datum_projection_over_selector_receiver_resolve_error_emits_warning_and_undef() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::ty::SelectorKind;
        use reify_core::{Severity, Type, ValueCellId};
        use reify_ir::QueryError;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent = reify_ir::GeometryHandleId(1);

        let sv = reify_ir::value::SelectorValue::leaf(
            SelectorKind::Face,
            reify_ir::value::GeometryHandleRef {
                realization_ref: RealizationNodeId::new("S", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(parent),
            },
            reify_ir::value::LeafQuery::All,
        )
        .expect("SelectorValue::leaf for Face/All must succeed");

        // Inject an error so that extract_faces(parent) fails → resolve returns Err.
        let mut kernel = MockGeometryKernel::new().with_extract_faces_error(
            parent,
            QueryError::QueryFailed("mock extract_faces failure for test".to_string()),
        );

        let mut values = reify_ir::ValueMap::new();
        values.insert(ValueCellId::new("S", "s"), reify_ir::Value::Selector(sv));

        let object = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("S", "s"),
            Type::Selector(SelectorKind::Face),
        );
        let expr =
            reify_ir::CompiledExpr::method_call(object, "axis".to_string(), vec![], Type::Axis);

        let swept_kinds = crate::sweep_classifier::SweptKindTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_feature_datum_projection(
            &expr,
            &values,
            &mut kernel,
            &swept_kinds,
            &mut diagnostics,
        );

        let value = result.expect("resolve-error s.axis must yield Some(Value::Undef), not None");
        assert!(
            matches!(value, reify_ir::Value::Undef),
            "resolve-error s.axis must return Value::Undef; got {value:?}"
        );

        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert!(
            !warnings.is_empty(),
            "resolve-error s.axis must push at least one Severity::Warning diagnostic; \
             got {diagnostics:?}"
        );
    }

    // ── Scalar-branch coverage (suggestion from review, task 3622 amend) ────
    //
    // Both dispatch_edge_length and dispatch_perimeter accept
    // `Ok(Value::Scalar { si_value, .. })` in addition to `Ok(Value::Real(_))`
    // (following the kernel_distance Real-or-Scalar precedent). These tests
    // verify that the Scalar arm accumulates correctly and is not dead code.

    /// `length(edge_sub_handle)` when the kernel returns
    /// `Value::Scalar{si_value: 0.03, dimension: LENGTH}` must accept it and
    /// return `Some(Value::length(0.03))`.
    #[test]
    fn try_eval_topology_selector_length_scalar_reply_accepted_as_length() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let edge_kh = reify_ir::GeometryHandleId(30);
        let parent_rr = RealizationNodeId::new("LengthScalarTest", 0);
        let parent_hash: [u8; 32] = [0x60; 32];
        // Stage a Scalar{LENGTH} reply instead of a plain Real.
        let scalar_reply = reify_ir::Value::Scalar {
            si_value: 0.03,
            dimension: reify_core::DimensionVector::LENGTH,
        };
        let mut kernel = MockGeometryKernel::new().with_edge_length_result(edge_kh, scalar_reply);

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("LengthScalarTest", "e"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: Some(edge_kh),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "length",
            "LengthScalarTest",
            "e",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::length(0.03)),
            "length() with Scalar reply must return Some(Value::length(0.03)); \
             got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            diagnostics.is_empty(),
            "Scalar-reply length must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `perimeter(face_sub_handle)` where one edge returns `Value::Scalar{LENGTH}`
    /// instead of `Value::Real` must accumulate `si_value` correctly and return
    /// `Some(Value::length(total))` with zero diagnostics.
    #[test]
    fn try_eval_topology_selector_perimeter_scalar_edge_length_accepted_in_sum() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let face_kh = reify_ir::GeometryHandleId(31);
        let e1 = reify_ir::GeometryHandleId(32);
        let e2 = reify_ir::GeometryHandleId(33);
        let parent_rr = RealizationNodeId::new("PerimScalarTest", 0);
        let parent_hash: [u8; 32] = [0x61; 32];
        // e1 returns Real(3.0); e2 returns Scalar{si_value: 7.0, LENGTH}.
        // Sum = 10.0 exactly.
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(face_kh, vec![e1, e2])
            .with_edge_length_result(e1, reify_ir::Value::Real(3.0))
            .with_edge_length_result(
                e2,
                reify_ir::Value::Scalar {
                    si_value: 7.0,
                    dimension: reify_core::DimensionVector::LENGTH,
                },
            );

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("PerimScalarTest", "f"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_kh),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "perimeter",
            "PerimScalarTest",
            "f",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::length(10.0)),
            "perimeter() with mixed Real/Scalar edge lengths must return \
             Some(Value::length(10.0)); got {:?}; diags: {:?}",
            result,
            diagnostics
        );
        assert!(
            diagnostics.is_empty(),
            "Scalar-reply perimeter must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `perimeter(face_sub_handle)` when `extract_edges` returns an empty list
    /// must yield `Some(Value::Undef)` + exactly one Warning (degenerate face
    /// guard, task 3622 amend).
    #[test]
    fn try_eval_topology_selector_perimeter_empty_edges_returns_undef_with_warning() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let face_kh = reify_ir::GeometryHandleId(34);
        let parent_rr = RealizationNodeId::new("PerimEmptyTest", 0);
        let parent_hash: [u8; 32] = [0x62; 32];
        // Stage an empty edge list.
        let mut kernel = MockGeometryKernel::new().with_extracted_edges(face_kh, vec![]);

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("PerimEmptyTest", "f"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr,
                upstream_values_hash: parent_hash,
                kernel_handle: Some(face_kh),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "perimeter",
            "PerimEmptyTest",
            "f",
            Type::Geometry,
            Type::length(),
        );
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "perimeter() with empty edge list must yield Some(Value::Undef); \
             got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "empty edge list must emit exactly one Warning; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "diagnostic severity must be Warning; got {:?}",
            diag.severity
        );
        assert!(
            diag.message.contains("perimeter"),
            "diagnostic must mention 'perimeter'; got: {}",
            diag.message
        );
    }

    // -------------------------------------------------------------------------
    // eval_sub_pose tests (T4: sub placement pose evaluation)
    // -------------------------------------------------------------------------

    /// `eval_sub_pose(None, ...)` must return an identity child→parent Transform
    /// (Orientation(1,0,0,0) rotation; Vector[length(0), length(0), length(0)] translation)
    /// and push no diagnostics.
    ///
    /// RED: fails to compile until `eval_sub_pose` is defined (step-2).
    #[test]
    fn eval_sub_pose_none_returns_identity_transform() {
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            None,
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "None pose must not push any diagnostics; got: {:?}",
            diagnostics
        );

        match result {
            reify_ir::Value::Transform {
                rotation,
                translation,
            } => {
                assert_eq!(
                    *rotation,
                    reify_ir::Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0
                    },
                    "identity rotation must be Orientation(1,0,0,0); got {:?}",
                    rotation
                );
                match *translation {
                    reify_ir::Value::Vector(ref components) => {
                        assert_eq!(
                            components.len(),
                            3,
                            "identity translation must have 3 components; got {}",
                            components.len()
                        );
                        for (i, c) in components.iter().enumerate() {
                            assert_eq!(
                                c,
                                &reify_ir::Value::length(0.0),
                                "identity translation component {} must be length(0.0); got {:?}",
                                i,
                                c
                            );
                        }
                    }
                    ref other => panic!("identity translation must be a Vector; got {:?}", other),
                }
            }
            other => panic!("expected Value::Transform for None pose; got {:?}", other),
        }
    }

    /// `eval_sub_pose(Some(&transform_expr), ...)` must return the Transform unchanged
    /// (passthrough) and push no diagnostics.
    ///
    /// Pins the step-3/4 contract: a pose that is already a Transform is not altered.
    #[test]
    fn eval_sub_pose_transform_passthrough() {
        let s = std::f64::consts::FRAC_1_SQRT_2; // 90° about Z: (s, 0, 0, s)
        let input_transform = reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Orientation {
                w: s,
                x: 0.0,
                y: 0.0,
                z: s,
            }),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(10.0),
                reify_ir::Value::length(20.0),
                reify_ir::Value::length(30.0),
            ])),
        };
        let expr = reify_ir::CompiledExpr::literal(
            input_transform.clone(),
            reify_core::Type::transform(3),
        );

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Transform passthrough must not push diagnostics; got: {:?}",
            diagnostics
        );
        assert_eq!(
            result, input_transform,
            "Transform passthrough must return the input unchanged"
        );
    }

    /// `eval_sub_pose` with a `Frame { origin: Point([1m, 2m, 3m]), basis: 90°Z }` must
    /// lower to `Transform { rotation: 90°Z, translation: Vector([1m, 2m, 3m]) }`.
    ///
    /// This is the PRD §11 Q1 convention-pinning numeric test (step-5/6).
    /// Derivation: Transform{Q,t} maps child-local p to parent Q·p + t.
    /// Carrying identity frame onto Frame{o, R} forces t = o and Q = R.
    #[test]
    fn eval_sub_pose_frame_lowers_to_transform_convention() {
        let s = std::f64::consts::FRAC_1_SQRT_2; // 90° about Z
        let input_frame = reify_ir::Value::Frame {
            origin: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(1.0),
                reify_ir::Value::length(2.0),
                reify_ir::Value::length(3.0),
            ])),
            basis: Box::new(reify_ir::Value::Orientation {
                w: s,
                x: 0.0,
                y: 0.0,
                z: s,
            }),
        };
        let expr = reify_ir::CompiledExpr::literal(input_frame, reify_core::Type::frame(3));

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Frame lowering must not push diagnostics; got: {:?}",
            diagnostics
        );

        match result {
            reify_ir::Value::Transform {
                rotation,
                translation,
            } => {
                // Convention: rotation == Frame.basis (exact copy, no normalization)
                assert_eq!(
                    *rotation,
                    reify_ir::Value::Orientation {
                        w: s,
                        x: 0.0,
                        y: 0.0,
                        z: s
                    },
                    "lowered rotation must equal Frame basis; got {:?}",
                    rotation
                );
                // Convention: translation == Frame.origin components as Vector
                match *translation {
                    reify_ir::Value::Vector(ref components) => {
                        assert_eq!(components.len(), 3);
                        assert_eq!(components[0], reify_ir::Value::length(1.0));
                        assert_eq!(components[1], reify_ir::Value::length(2.0));
                        assert_eq!(components[2], reify_ir::Value::length(3.0));
                    }
                    ref other => panic!("lowered translation must be a Vector; got {:?}", other),
                }
            }
            other => panic!(
                "expected Value::Transform after Frame lowering; got {:?}",
                other
            ),
        }
    }

    /// `eval_sub_pose(Some(&non_pose_expr), ...)` must return `Value::Undef` and
    /// push exactly one `Diagnostic::error`.
    ///
    /// T4 owns pose type-validation (T2 deferred it). Pins the step-7/8 contract.
    #[test]
    fn eval_sub_pose_non_pose_value_returns_undef_with_diagnostic() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(5.0),
            reify_core::Type::dimensionless_scalar(),
        );

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );

        assert!(
            result.is_undef(),
            "non-pose value must return Value::Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-pose value must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error severity; got {:?}",
            diagnostics[0].severity
        );
    }

    // -------------------------------------------------------------------------
    // Frame validation branch tests (Suggestion 1: test_coverage)
    // Each of the four Frame-specific guard clauses must individually produce
    // exactly one Diagnostic::error and return Value::Undef.
    // -------------------------------------------------------------------------

    /// Helper: a valid unit Orientation (identity).
    fn identity_orientation() -> reify_ir::Value {
        reify_ir::Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// Helper: a valid 3-component LENGTH Point.
    fn valid_origin() -> reify_ir::Value {
        reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ])
    }

    /// Frame origin is not a `Value::Point` at all (e.g. a bare `Value::Real`).
    /// The first guard in the Frame arm must fire.
    #[test]
    fn eval_sub_pose_frame_non_point_origin_errors() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Frame {
                origin: Box::new(reify_ir::Value::Real(1.0)),
                basis: Box::new(identity_orientation()),
            },
            reify_core::Type::frame(3),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "non-Point origin must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Point origin must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    /// Frame origin is a `Value::Point` but with only 2 components (not 3).
    /// The component-count guard must fire.
    #[test]
    fn eval_sub_pose_frame_origin_two_components_errors() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Frame {
                origin: Box::new(reify_ir::Value::Point(vec![
                    reify_ir::Value::length(1.0),
                    reify_ir::Value::length(2.0),
                    // missing third component
                ])),
                basis: Box::new(identity_orientation()),
            },
            reify_core::Type::frame(3),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "2-component origin must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "2-component origin must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    /// Frame origin has a component with ANGLE dimension instead of LENGTH.
    /// The dimension guard must fire.
    #[test]
    fn eval_sub_pose_frame_origin_non_length_dimension_errors() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Frame {
                origin: Box::new(reify_ir::Value::Point(vec![
                    // First component is ANGLE-dimensioned — invalid for a Point origin
                    reify_ir::Value::Scalar {
                        si_value: 1.0,
                        dimension: reify_core::DimensionVector::ANGLE,
                    },
                    reify_ir::Value::length(2.0),
                    reify_ir::Value::length(3.0),
                ])),
                basis: Box::new(identity_orientation()),
            },
            reify_core::Type::frame(3),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "non-LENGTH origin component must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-LENGTH origin must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    /// Frame origin has a non-finite (NaN) coordinate.
    /// The `si_value.is_finite()` guard must fire.
    #[test]
    fn eval_sub_pose_frame_origin_nan_coordinate_errors() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Frame {
                origin: Box::new(reify_ir::Value::Point(vec![
                    reify_ir::Value::Scalar {
                        si_value: f64::NAN,
                        dimension: reify_core::DimensionVector::LENGTH,
                    },
                    reify_ir::Value::length(2.0),
                    reify_ir::Value::length(3.0),
                ])),
                basis: Box::new(identity_orientation()),
            },
            reify_core::Type::frame(3),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "NaN coordinate must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "NaN coordinate must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    /// Frame basis is not a `Value::Orientation` (e.g. a bare `Value::Real`).
    /// The basis-variant guard must fire.
    #[test]
    fn eval_sub_pose_frame_non_orientation_basis_errors() {
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Frame {
                origin: Box::new(valid_origin()),
                basis: Box::new(reify_ir::Value::Real(1.0)),
            },
            reify_core::Type::frame(3),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "non-Orientation basis must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "non-Orientation basis must push exactly one diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    // -------------------------------------------------------------------------
    // Undef behavior test (Suggestion 3: robustness — pins the chosen behavior)
    // -------------------------------------------------------------------------

    /// A pose expression that evaluates to `Value::Undef` (simulating an upstream
    /// evaluation failure) must produce exactly one `Diagnostic::error`.
    ///
    /// This pins the intentional design choice: we emit a call-site error even when
    /// the expression already produced Undef, giving the consumer a placement-site
    /// anchor in addition to whatever upstream diagnostic the expression emitted.
    /// See the comment in the `_` catch-all arm of `eval_sub_pose` for the rationale.
    #[test]
    fn eval_sub_pose_undef_expr_returns_undef_with_diagnostic() {
        // Build an expression that evaluates to Value::Undef directly.
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Undef,
            reify_core::Type::dimensionless_scalar(), // type doesn't matter; the value is Undef
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::eval_sub_pose(
            Some(&expr),
            &ValueMap::new(),
            &[],
            &HashMap::new(),
            &mut diagnostics,
        );
        assert!(
            result.is_undef(),
            "Undef-evaluating pose must return Undef; got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "Undef pose must push exactly one call-site diagnostic; got {} diags: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Error,
            "call-site diagnostic must be Error; got {:?}",
            diagnostics[0].severity
        );
    }

    // ── T5 step-7: pose decompose / compose helpers ──────────────────────────

    /// Build a `Value::Transform` from a raw quaternion `[w,x,y,z]` and a
    /// LENGTH-dimensioned translation `[tx,ty,tz]` (metres).
    fn transform_of(q: [f64; 4], t: [f64; 3]) -> reify_ir::Value {
        reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Orientation {
                w: q[0],
                x: q[1],
                y: q[2],
                z: q[3],
            }),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(t[0]),
                reify_ir::Value::length(t[1]),
                reify_ir::Value::length(t[2]),
            ])),
        }
    }

    /// The canonical identity `Value::Transform` (mirrors `eval_sub_pose`'s
    /// `None` arm and step-8's `compose_pose_chain` seed).
    fn identity_transform() -> reify_ir::Value {
        transform_of([1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0])
    }

    #[test]
    fn decompose_transform_to_arrays_extracts_quat_and_si_translation() {
        let v = transform_of([0.5, 0.5, 0.5, 0.5], [0.03, -0.01, 0.2]);
        let (q, t) = decompose_transform_to_arrays(&v).expect("valid Transform must decompose");
        assert_eq!(q, [0.5, 0.5, 0.5, 0.5], "quaternion [w,x,y,z]");
        assert_eq!(t, [0.03, -0.01, 0.2], "translation in SI metres");
    }

    #[test]
    fn decompose_transform_to_arrays_rejects_non_transform() {
        assert!(decompose_transform_to_arrays(&reify_ir::Value::Real(1.0)).is_none());
        assert!(decompose_transform_to_arrays(&reify_ir::Value::Undef).is_none());
    }

    #[test]
    fn decompose_transform_to_arrays_rejects_non_orientation_rotation() {
        // rotation is a Vector, not an Orientation → reject.
        let v = reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(1.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
            ])),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
            ])),
        };
        assert!(decompose_transform_to_arrays(&v).is_none());
    }

    #[test]
    fn decompose_transform_to_arrays_rejects_wrong_length_translation() {
        let v = reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
            ])),
        };
        assert!(decompose_transform_to_arrays(&v).is_none());
    }

    #[test]
    fn decompose_transform_to_arrays_rejects_mixed_dimension_translation() {
        // one ANGLE component among LENGTHs → reject (mixed dimensions).
        let v = reify_ir::Value::Transform {
            rotation: Box::new(reify_ir::Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::angle(0.0),
                reify_ir::Value::length(0.0),
            ])),
        };
        assert!(decompose_transform_to_arrays(&v).is_none());
    }

    // ── decode_orientation_to_axis_angle unit tests (task γ, #4166) ─────────

    /// Identity quaternion → canonical no-op: axis [1,0,0], angle 0.0.
    /// The kernel rejects zero-length axes, so [1,0,0]/0 is required — not [0,0,0].
    #[test]
    fn decode_orientation_to_axis_angle_identity() {
        let v = reify_ir::Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        let (axis, angle) = decode_orientation_to_axis_angle(&v)
            .expect("identity orientation must decode");
        assert_eq!(axis, [1.0, 0.0, 0.0], "identity → canonical axis [1,0,0]");
        assert_eq!(angle, 0.0, "identity → angle 0.0");
    }

    /// 90° about Z: Orientation{w=cos(π/4), x=0, y=0, z=sin(π/4)}.
    /// axis ≈ [0,0,1] within 1e-12, angle ≈ π/2 within 1e-12 (exact via 2·atan2).
    #[test]
    fn decode_orientation_to_axis_angle_90_deg_z() {
        let half = std::f64::consts::FRAC_PI_4;
        let v = reify_ir::Value::Orientation {
            w: half.cos(),
            x: 0.0,
            y: 0.0,
            z: half.sin(),
        };
        let (axis, angle) = decode_orientation_to_axis_angle(&v)
            .expect("90° Z orientation must decode");
        assert!((axis[0]).abs() < 1e-12, "axis[0] should be 0, got {}", axis[0]);
        assert!((axis[1]).abs() < 1e-12, "axis[1] should be 0, got {}", axis[1]);
        assert!((axis[2] - 1.0).abs() < 1e-12, "axis[2] should be 1, got {}", axis[2]);
        assert!(
            (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12,
            "angle should be π/2, got {}",
            angle
        );
    }

    /// 180° about X: Orientation{w=0, x=1, y=0, z=0}.
    /// axis ≈ [1,0,0] within 1e-12, angle ≈ π within 1e-12.
    #[test]
    fn decode_orientation_to_axis_angle_180_deg_x() {
        let v = reify_ir::Value::Orientation { w: 0.0, x: 1.0, y: 0.0, z: 0.0 };
        let (axis, angle) = decode_orientation_to_axis_angle(&v)
            .expect("180° X orientation must decode");
        assert!((axis[0] - 1.0).abs() < 1e-12, "axis[0] should be 1, got {}", axis[0]);
        assert!((axis[1]).abs() < 1e-12, "axis[1] should be 0, got {}", axis[1]);
        assert!((axis[2]).abs() < 1e-12, "axis[2] should be 0, got {}", axis[2]);
        assert!(
            (angle - std::f64::consts::PI).abs() < 1e-12,
            "angle should be π, got {}",
            angle
        );
    }

    /// Non-Orientation value → None.
    #[test]
    fn decode_orientation_to_axis_angle_rejects_non_orientation() {
        assert!(decode_orientation_to_axis_angle(&reify_ir::Value::Real(5.0)).is_none());
        assert!(decode_orientation_to_axis_angle(&reify_ir::Value::Undef).is_none());
    }

    /// Non-finite quaternion → None.
    #[test]
    fn decode_orientation_to_axis_angle_rejects_non_finite() {
        let v = reify_ir::Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(decode_orientation_to_axis_angle(&v).is_none());
    }

    /// Parity guard: `decode_orientation_to_axis_angle` must agree bit-for-bit
    /// with `reify_stdlib::eval_builtin("orient_to_axis_angle", …)` for a spread
    /// of orientations.  Both copies use the same formula (v_norm, 2·atan2,
    /// identity→[1,0,0]/0); this test catches divergence if one is updated
    /// (different EPS, sign convention, or formula change) without the other.
    ///
    /// Rationale: `decode_orientation_to_axis_angle` is a local duplicate of the
    /// stdlib math (documented in the function's doc comment) because
    /// `orient_to_axis_angle` is `pub(crate)` and returns `Value::Map`.  The
    /// duplication is justified, but a parity CI guard is the safety net.
    #[test]
    fn decode_orientation_to_axis_angle_parity_with_stdlib() {
        // Helper: extract (axis, angle_rad) from the Value::Map that
        // orient_to_axis_angle returns.  Mirrors the pub(crate) axis_angle_extract
        // in reify-stdlib/src/orientation.rs without importing it.
        fn extract(v: &reify_ir::Value) -> ([f64; 3], f64) {
            let m = match v {
                reify_ir::Value::Map(m) => m,
                other => panic!("expected Map from orient_to_axis_angle, got {:?}", other),
            };
            let axis_v = m
                .get(&reify_ir::Value::String("axis".to_string()))
                .unwrap_or_else(|| panic!("Map missing 'axis' key"));
            let angle_si = m
                .get(&reify_ir::Value::String("angle".to_string()))
                .and_then(|v| v.as_f64())
                .unwrap_or_else(|| panic!("Map missing / non-numeric 'angle' key"));
            let axis = match axis_v {
                reify_ir::Value::Vector(items) if items.len() == 3 => [
                    items[0].as_f64().unwrap(),
                    items[1].as_f64().unwrap(),
                    items[2].as_f64().unwrap(),
                ],
                other => panic!("'axis' is not a 3-vec, got {:?}", other),
            };
            (axis, angle_si)
        }

        let cases: &[reify_ir::Value] = &[
            // identity
            reify_ir::Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 },
            // 90° about Z (half-angle = π/4)
            reify_ir::Value::Orientation {
                w: std::f64::consts::FRAC_PI_4.cos(),
                x: 0.0,
                y: 0.0,
                z: std::f64::consts::FRAC_PI_4.sin(),
            },
            // 180° about X (w=0 boundary)
            reify_ir::Value::Orientation { w: 0.0, x: 1.0, y: 0.0, z: 0.0 },
            // 45° about Y (half-angle = π/8)
            reify_ir::Value::Orientation {
                w: std::f64::consts::FRAC_PI_8.cos(),
                x: 0.0,
                y: std::f64::consts::FRAC_PI_8.sin(),
                z: 0.0,
            },
            // near-identity (|v| just above EPS): should NOT hit identity branch
            reify_ir::Value::Orientation {
                w: (1.0_f64 - 1e-24_f64).sqrt(),
                x: 1e-12 * 1.01,
                y: 0.0,
                z: 0.0,
            },
        ];

        for (i, q) in cases.iter().enumerate() {
            let stdlib_map = reify_stdlib::eval_builtin("orient_to_axis_angle", std::slice::from_ref(q));
            let (ref_axis, ref_angle) = extract(&stdlib_map);
            let (our_axis, our_angle) = decode_orientation_to_axis_angle(q)
                .unwrap_or_else(|| panic!("case {i}: decode returned None for {:?}", q));
            assert_eq!(
                our_axis, ref_axis,
                "case {i}: axis mismatch: decode={our_axis:?} stdlib={ref_axis:?}"
            );
            assert_eq!(
                our_angle, ref_angle,
                "case {i}: angle mismatch: decode={our_angle} stdlib={ref_angle}"
            );
        }
    }

    #[test]
    fn compose_pose_chain_two_equals_eval_builtin() {
        // Identity-quaternion (translation-only) transforms compose exactly
        // (quat_mul / quat_rotate by identity are bit-exact), so the left-fold
        // from identity collapses to a plain transform_compose of the pair.
        let t1 = transform_of([1.0, 0.0, 0.0, 0.0], [0.01, 0.0, 0.0]);
        let t2 = transform_of([1.0, 0.0, 0.0, 0.0], [0.0, 0.02, 0.0]);
        let got = compose_pose_chain(&[t1.clone(), t2.clone()]);
        let expected = reify_stdlib::eval_builtin("transform_compose", &[t1, t2]);
        assert_eq!(
            got, expected,
            "compose_pose_chain([t1,t2]) must equal transform_compose(t1,t2)"
        );
    }

    #[test]
    fn compose_pose_chain_empty_is_identity() {
        assert_eq!(
            compose_pose_chain(&[]),
            identity_transform(),
            "empty chain must be the identity Transform"
        );
    }

    #[test]
    fn compose_pose_chain_single_equals_compose_onto_identity() {
        let t = transform_of([1.0, 0.0, 0.0, 0.0], [0.05, 0.0, 0.0]);
        let got = compose_pose_chain(std::slice::from_ref(&t));
        let expected = reify_stdlib::eval_builtin("transform_compose", &[identity_transform(), t]);
        assert_eq!(
            got, expected,
            "single-element chain == transform_compose(identity, t)"
        );
    }

    // ── decode_plane unit tests (task η, step-1) ─────────────────────────────

    /// True producer→decode round-trip for plane_xy: the real stdlib producer
    /// is used so the test exercises the full Plane value shape that consumers
    /// will encounter at eval time.
    #[test]
    fn decode_plane_producer_round_trip_plane_xy() {
        // plane_xy(3mm) → Plane at z=0.003 m, normal=[0,0,1]
        let z_si = 0.003_f64;
        let val = reify_stdlib::eval_builtin("plane_xy", &[reify_ir::Value::length(z_si)]);
        let (origin, normal) = decode_plane(&val).expect("plane_xy should decode cleanly");
        assert!(
            (origin[0] - 0.0).abs() < 1e-12,
            "ox must be 0.0, got {}",
            origin[0]
        );
        assert!(
            (origin[1] - 0.0).abs() < 1e-12,
            "oy must be 0.0, got {}",
            origin[1]
        );
        assert!(
            (origin[2] - z_si).abs() < 1e-12,
            "oz must be {z_si}, got {}",
            origin[2]
        );
        assert!(
            (normal[0] - 0.0).abs() < 1e-12,
            "nx must be 0.0, got {}",
            normal[0]
        );
        assert!(
            (normal[1] - 0.0).abs() < 1e-12,
            "ny must be 0.0, got {}",
            normal[1]
        );
        assert!(
            (normal[2] - 1.0).abs() < 1e-12,
            "nz must be 1.0, got {}",
            normal[2]
        );
    }

    /// True producer→decode round-trip for plane_xz: offset lands in Y
    /// (index 1) and the normal is [0,1,0].
    #[test]
    fn decode_plane_producer_round_trip_plane_xz() {
        // plane_xz(5mm) → Plane at y=0.005 m, normal=[0,1,0]
        let z_si = 0.005_f64;
        let val = reify_stdlib::eval_builtin("plane_xz", &[reify_ir::Value::length(z_si)]);
        let (origin, normal) = decode_plane(&val).expect("plane_xz should decode cleanly");
        assert!(
            (origin[0] - 0.0).abs() < 1e-12,
            "ox must be 0.0, got {}",
            origin[0]
        );
        assert!(
            (origin[1] - z_si).abs() < 1e-12,
            "oy must be {z_si}, got {}",
            origin[1]
        );
        assert!(
            (origin[2] - 0.0).abs() < 1e-12,
            "oz must be 0.0, got {}",
            origin[2]
        );
        assert!(
            (normal[0] - 0.0).abs() < 1e-12,
            "nx must be 0.0, got {}",
            normal[0]
        );
        assert!(
            (normal[1] - 1.0).abs() < 1e-12,
            "ny must be 1.0, got {}",
            normal[1]
        );
        assert!(
            (normal[2] - 0.0).abs() < 1e-12,
            "nz must be 0.0, got {}",
            normal[2]
        );
    }

    /// True producer→decode round-trip for plane_yz: offset lands in X
    /// (index 0) and the normal is [1,0,0].
    #[test]
    fn decode_plane_producer_round_trip_plane_yz() {
        // plane_yz(7mm) → Plane at x=0.007 m, normal=[1,0,0]
        let z_si = 0.007_f64;
        let val = reify_stdlib::eval_builtin("plane_yz", &[reify_ir::Value::length(z_si)]);
        let (origin, normal) = decode_plane(&val).expect("plane_yz should decode cleanly");
        assert!(
            (origin[0] - z_si).abs() < 1e-12,
            "ox must be {z_si}, got {}",
            origin[0]
        );
        assert!(
            (origin[1] - 0.0).abs() < 1e-12,
            "oy must be 0.0, got {}",
            origin[1]
        );
        assert!(
            (origin[2] - 0.0).abs() < 1e-12,
            "oz must be 0.0, got {}",
            origin[2]
        );
        assert!(
            (normal[0] - 1.0).abs() < 1e-12,
            "nx must be 1.0, got {}",
            normal[0]
        );
        assert!(
            (normal[1] - 0.0).abs() < 1e-12,
            "ny must be 0.0, got {}",
            normal[1]
        );
        assert!(
            (normal[2] - 0.0).abs() < 1e-12,
            "nz must be 0.0, got {}",
            normal[2]
        );
    }

    /// A Plane whose normal vector has magnitude 2 (non-unit) must be
    /// normalized to a unit normal by decode_plane — never returned as-is.
    #[test]
    fn decode_plane_normalizes_non_unit_normal() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let non_unit_normal = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(2.0),
        ]);
        let plane = reify_ir::Value::Plane {
            origin: Box::new(origin),
            normal: Box::new(non_unit_normal),
        };
        let (_, normal) =
            decode_plane(&plane).expect("non-unit normal [0,0,2] should normalize without error");
        assert!(
            (normal[0] - 0.0).abs() < 1e-12,
            "nx must be 0.0, got {}",
            normal[0]
        );
        assert!(
            (normal[1] - 0.0).abs() < 1e-12,
            "ny must be 0.0, got {}",
            normal[1]
        );
        assert!(
            (normal[2] - 1.0).abs() < 1e-12,
            "nz must be 1.0 after normalization, got {}",
            normal[2]
        );
    }

    /// Value::Axis must be rejected by decode_plane — wrong variant.
    #[test]
    fn decode_plane_rejects_axis_value() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let dir = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(1.0),
        ]);
        let axis = reify_ir::Value::Axis {
            origin: Box::new(origin),
            direction: Box::new(dir),
        };
        assert!(
            decode_plane(&axis).is_err(),
            "Value::Axis must be rejected by decode_plane (wrong variant)"
        );
    }

    /// Value::Undef must be rejected by decode_plane — never silently pass through.
    #[test]
    fn decode_plane_rejects_undef() {
        assert!(
            decode_plane(&reify_ir::Value::Undef).is_err(),
            "Value::Undef must be rejected by decode_plane"
        );
    }

    /// A Plane with a zero-magnitude normal must be rejected — the decoder
    /// must never return (0,0,0) as the unit normal.
    #[test]
    fn decode_plane_rejects_zero_magnitude_normal() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let zero_normal = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
        ]);
        let plane = reify_ir::Value::Plane {
            origin: Box::new(origin),
            normal: Box::new(zero_normal),
        };
        assert!(
            decode_plane(&plane).is_err(),
            "zero-magnitude normal must be rejected by decode_plane (never pass through as [0,0,0])"
        );
    }

    // ── decode_axis unit tests (task η, step-3) ─────────────────────────────

    /// Helper: build a Value::Point with three LENGTH-dimensioned components
    /// (metres), as produced by point3() in the stdlib.
    fn make_point3_length_val(x: f64, y: f64, z: f64) -> reify_ir::Value {
        reify_ir::Value::Point(vec![
            reify_ir::Value::length(x),
            reify_ir::Value::length(y),
            reify_ir::Value::length(z),
        ])
    }

    /// True producer→decode round-trip for axis_z with origin at (0,0,0).
    /// decode_axis must return origin=[0,0,0] and direction=[0,0,1].
    #[test]
    fn decode_axis_producer_round_trip_axis_z_origin() {
        let origin = make_point3_length_val(0.0, 0.0, 0.0);
        let val = reify_stdlib::eval_builtin("axis_z", std::slice::from_ref(&origin));
        let (got_origin, got_dir) = decode_axis(&val).expect("axis_z should decode cleanly");
        assert!(
            (got_origin[0] - 0.0).abs() < 1e-12,
            "ox must be 0.0, got {}",
            got_origin[0]
        );
        assert!(
            (got_origin[1] - 0.0).abs() < 1e-12,
            "oy must be 0.0, got {}",
            got_origin[1]
        );
        assert!(
            (got_origin[2] - 0.0).abs() < 1e-12,
            "oz must be 0.0, got {}",
            got_origin[2]
        );
        assert!(
            (got_dir[0] - 0.0).abs() < 1e-12,
            "dx must be 0.0, got {}",
            got_dir[0]
        );
        assert!(
            (got_dir[1] - 0.0).abs() < 1e-12,
            "dy must be 0.0, got {}",
            got_dir[1]
        );
        assert!(
            (got_dir[2] - 1.0).abs() < 1e-12,
            "dz must be 1.0, got {}",
            got_dir[2]
        );
    }

    /// axis_x round-trip: direction=[1,0,0], origin passes through in SI metres.
    #[test]
    fn decode_axis_producer_round_trip_axis_x_with_offset_origin() {
        // 1mm=0.001m, 2mm=0.002m, 3mm=0.003m
        let origin = make_point3_length_val(0.001, 0.002, 0.003);
        let val = reify_stdlib::eval_builtin("axis_x", std::slice::from_ref(&origin));
        let (got_origin, got_dir) =
            decode_axis(&val).expect("axis_x with offset origin should decode");
        assert!(
            (got_origin[0] - 0.001).abs() < 1e-12,
            "ox must be 0.001, got {}",
            got_origin[0]
        );
        assert!(
            (got_origin[1] - 0.002).abs() < 1e-12,
            "oy must be 0.002, got {}",
            got_origin[1]
        );
        assert!(
            (got_origin[2] - 0.003).abs() < 1e-12,
            "oz must be 0.003, got {}",
            got_origin[2]
        );
        assert!(
            (got_dir[0] - 1.0).abs() < 1e-12,
            "dx must be 1.0, got {}",
            got_dir[0]
        );
        assert!(
            (got_dir[1] - 0.0).abs() < 1e-12,
            "dy must be 0.0, got {}",
            got_dir[1]
        );
        assert!(
            (got_dir[2] - 0.0).abs() < 1e-12,
            "dz must be 0.0, got {}",
            got_dir[2]
        );
    }

    /// axis_y round-trip: direction=[0,1,0].
    #[test]
    fn decode_axis_producer_round_trip_axis_y() {
        let origin = make_point3_length_val(0.0, 0.0, 0.0);
        let val = reify_stdlib::eval_builtin("axis_y", std::slice::from_ref(&origin));
        let (got_origin, got_dir) = decode_axis(&val).expect("axis_y should decode cleanly");
        assert!(
            (got_dir[0] - 0.0).abs() < 1e-12,
            "dx must be 0.0, got {}",
            got_dir[0]
        );
        assert!(
            (got_dir[1] - 1.0).abs() < 1e-12,
            "dy must be 1.0, got {}",
            got_dir[1]
        );
        assert!(
            (got_dir[2] - 0.0).abs() < 1e-12,
            "dz must be 0.0, got {}",
            got_dir[2]
        );
        // origin must be [0,0,0]
        assert!((got_origin[0] - 0.0).abs() < 1e-12, "ox");
        assert!((got_origin[1] - 0.0).abs() < 1e-12, "oy");
        assert!((got_origin[2] - 0.0).abs() < 1e-12, "oz");
    }

    /// A non-unit direction (magnitude 2) must be normalized to unit length.
    #[test]
    fn decode_axis_normalizes_non_unit_direction() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let non_unit_dir = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(2.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
        ]);
        let axis = reify_ir::Value::Axis {
            origin: Box::new(origin),
            direction: Box::new(non_unit_dir),
        };
        let (_, got_dir) =
            decode_axis(&axis).expect("non-unit direction [2,0,0] should normalize without error");
        assert!(
            (got_dir[0] - 1.0).abs() < 1e-12,
            "dx must be 1.0 after normalization, got {}",
            got_dir[0]
        );
        assert!((got_dir[1] - 0.0).abs() < 1e-12, "dy");
        assert!((got_dir[2] - 0.0).abs() < 1e-12, "dz");
    }

    /// Value::Plane must be rejected by decode_axis — wrong variant.
    #[test]
    fn decode_axis_rejects_plane_value() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let normal = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(1.0),
        ]);
        let plane = reify_ir::Value::Plane {
            origin: Box::new(origin),
            normal: Box::new(normal),
        };
        assert!(
            decode_axis(&plane).is_err(),
            "Value::Plane must be rejected by decode_axis (wrong variant)"
        );
    }

    /// Value::Undef must be rejected by decode_axis.
    #[test]
    fn decode_axis_rejects_undef() {
        assert!(
            decode_axis(&reify_ir::Value::Undef).is_err(),
            "Value::Undef must be rejected by decode_axis"
        );
    }

    /// An Axis with a zero-magnitude direction must be rejected.
    #[test]
    fn decode_axis_rejects_zero_magnitude_direction() {
        let origin = reify_ir::Value::Point(vec![
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
            reify_ir::Value::length(0.0),
        ]);
        let zero_dir = reify_ir::Value::Vector(vec![
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
            reify_ir::Value::Real(0.0),
        ]);
        let axis = reify_ir::Value::Axis {
            origin: Box::new(origin),
            direction: Box::new(zero_dir),
        };
        assert!(
            decode_axis(&axis).is_err(),
            "zero-magnitude direction must be rejected by decode_axis"
        );
    }

    // ── step-7 (task 4190): split dispatch unit tests ────────────────────────
    //
    // Tests for the `split(solid, plane) -> List<Geometry>` dispatch arm in
    // `try_eval_topology_selector`.  These tests reference
    // `crate::topology_selectors::SubKind::Solid` (added in step-8) and
    // `TopologySelectorHelper::Split` (added in step-8), so the crate fails
    // to compile until step-8 is done → RED.

    /// Thin wrapper around `MockGeometryKernel` that overrides `execute_split`
    /// to return a configurable success result.  All other trait methods
    /// delegate to the inner mock.
    ///
    /// Required because `MockGeometryKernel` does not expose `execute_split`
    /// configuration (it is not in the mock's in-scope file list for this
    /// task), so we define a minimal delegating wrapper inline.
    struct SplitMockKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        /// Returned by every `execute_split` call (cloned on each call).
        split_ids: Vec<GeometryHandleId>,
    }

    impl SplitMockKernel {
        fn new(
            inner: reify_test_support::mocks::MockGeometryKernel,
            split_ids: Vec<GeometryHandleId>,
        ) -> Self {
            Self { inner, split_ids }
        }
    }

    impl reify_ir::GeometryKernel for SplitMockKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            query: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(query)
        }

        fn export(
            &self,
            handle: GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.inner.tessellate(handle, tolerance)
        }

        fn execute_split(
            &mut self,
            _op: &reify_ir::GeometryOp,
        ) -> Result<Vec<GeometryHandleId>, reify_ir::GeometryError> {
            Ok(self.split_ids.clone())
        }
    }

    /// Build a `Value::Plane` with a z=0 normal (z-axis cutting plane) for use
    /// as the plane argument in split dispatch tests.
    fn z_plane_value() -> reify_ir::Value {
        reify_ir::Value::Plane {
            origin: Box::new(reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
            ])),
            normal: Box::new(reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ])),
        }
    }

    /// `split(solid, plane)` dispatch returns `Value::List` of two
    /// `Value::GeometryHandle` elements when the mock kernel returns
    /// [GHId(5), GHId(6)] from `execute_split`.
    ///
    /// Each element must:
    ///   (i)  carry the parent solid's `realization_ref` (unchanged, PRD §4 i);
    ///   (ii) have a `upstream_values_hash` distinct from the other piece
    ///        (PRD §4 iii) — derived from `SubKind::Solid` discriminant (0x03)
    ///        via `compose_sub_handle_hash`.
    ///
    /// RED: `SubKind::Solid` does not exist yet → compile error.
    #[test]
    fn split_dispatch_returns_geometry_handle_list() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};

        let parent_handle = GeometryHandleId(1);
        let parent_rr = RealizationNodeId::new("MySolid", 0);
        let parent_hash: [u8; 32] = [0xAB; 32];

        let piece_ids = vec![GeometryHandleId(5), GeometryHandleId(6)];
        let mut kernel = SplitMockKernel::new(
            reify_test_support::mocks::MockGeometryKernel::new(),
            piece_ids.clone(),
        );

        let mut named_steps = HashMap::new();
        named_steps.insert("solid".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[0]: parent solid as hydrated GeometryHandle in the values map.
        values.insert(
            ValueCellId::new("MySolid", "solid"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        // args[1]: cutting plane as Value::Plane in the values map.
        values.insert(ValueCellId::new("MySolid", "plane"), z_plane_value());

        let expr = topology_selector_call_two_value_refs(
            "split",
            "MySolid",
            "solid",
            Type::Geometry,
            "plane",
            Type::Plane,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let list = match result {
            Some(reify_ir::Value::List(ref elems)) => elems.clone(),
            other => panic!(
                "split dispatch must return Some(Value::List(..)), got {:?}; \
                 diagnostics: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(list.len(), 2, "expected 2 split pieces, got {}", list.len());
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for successful split, got: {:?}",
            diagnostics
        );

        // Verify each piece: correct realization_ref, correct kernel_handle,
        // distinct upstream_values_hash (SubKind::Solid domain-separation).
        let expected_kernel_ids = [GeometryHandleId(5), GeometryHandleId(6)];
        let mut hashes: Vec<[u8; 32]> = Vec::new();
        for (i, (elem, expected_id)) in list.iter().zip(&expected_kernel_ids).enumerate() {
            match elem {
                reify_ir::Value::GeometryHandle {
                    realization_ref,
                    upstream_values_hash,
                    kernel_handle,
                } => {
                    assert_eq!(
                        realization_ref.entity, parent_rr.entity,
                        "piece[{i}] realization_ref.entity must match parent"
                    );
                    assert_eq!(
                        realization_ref.index, parent_rr.index,
                        "piece[{i}] realization_ref.index must match parent"
                    );
                    assert_eq!(
                        kernel_handle, &Some(*expected_id),
                        "piece[{i}] kernel_handle must be {expected_id:?}"
                    );
                    // Verify the hash uses SubKind::Solid (0x03) domain separator.
                    let expected_hash = crate::topology_selectors::compose_sub_handle_hash(
                        &parent_hash,
                        crate::topology_selectors::SubKind::Solid, // RED: not yet defined
                        i as u32,
                    );
                    assert_eq!(
                        *upstream_values_hash, expected_hash,
                        "piece[{i}] upstream_values_hash must use SubKind::Solid"
                    );
                    hashes.push(*upstream_values_hash);
                }
                other => panic!("piece[{i}] is not Value::GeometryHandle: {:?}", other),
            }
        }
        // PRD §4 iii: per-index hashes must be distinct.
        assert_ne!(
            hashes[0], hashes[1],
            "split piece 0 and piece 1 hashes must differ (PRD §4 iii)"
        );
    }

    /// When args[1] is not a `Value::Plane` (e.g. a bare `Value::Real`),
    /// `split` dispatch must fall through to `None` so the cell retains its
    /// compiled default (`Value::Undef`).
    #[test]
    fn split_dispatch_falls_through_when_plane_arg_not_a_plane() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps = HashMap::new();
        named_steps.insert("solid".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        // args[0]: valid parent solid.
        values.insert(
            ValueCellId::new("MySolid", "solid"),
            reify_ir::Value::GeometryHandle {
                realization_ref: reify_core::identity::RealizationNodeId::new("MySolid", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(parent_handle),
            },
        );
        // args[1]: NOT a Plane — should cause decode_plane to fail → fall through.
        values.insert(
            ValueCellId::new("MySolid", "plane"),
            reify_ir::Value::Real(0.0),
        );

        let expr = topology_selector_call_two_value_refs(
            "split",
            "MySolid",
            "solid",
            Type::Geometry,
            "plane",
            Type::dimensionless_scalar(),
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "non-Plane args[1] must fall through to None (cell stays Undef), got {:?}",
            result
        );
    }

    /// When `execute_split` returns an error, `split` dispatch must emit a
    /// `Warning` diagnostic and return `Some(Value::Undef)` — the same
    /// defensive-downgrade contract as other topology-selector dispatch arms.
    #[test]
    fn split_dispatch_emits_warning_and_undef_on_kernel_error() {
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(1);
        // Default MockGeometryKernel inherits the trait default for execute_split:
        // Err(GeometryError::OperationFailed("execute_split not supported by this kernel")).
        let mut kernel = MockGeometryKernel::new();

        let mut named_steps = HashMap::new();
        named_steps.insert("solid".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("MySolid", "solid"),
            reify_ir::Value::GeometryHandle {
                realization_ref: reify_core::identity::RealizationNodeId::new("MySolid", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: Some(parent_handle),
            },
        );
        values.insert(ValueCellId::new("MySolid", "plane"), z_plane_value());

        let expr = topology_selector_call_two_value_refs(
            "split",
            "MySolid",
            "solid",
            Type::Geometry,
            "plane",
            Type::Plane,
            Type::List(Box::new(Type::Geometry)),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "kernel Err must produce Some(Value::Undef), got {:?}; \
             diagnostics: {:?}",
            result,
            diagnostics
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 Warning diagnostic on kernel error, \
             got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert!(
            matches!(diagnostics[0].severity, reify_core::Severity::Warning),
            "diagnostic must be a Warning, got {:?}",
            diagnostics[0].severity
        );
    }

    // ── resolve_subhandle_list (task 3205 step-7/8) ───────────────────────
    //
    // `resolve_subhandle_list(arg, parent)` is the KERNEL-FREE (pure
    // Value→Value) helper that lowers a List<Geometry> of KGQ sub-handles to a
    // canonical `Vec<GeometryHandleId>`: it requires a List of `GeometryHandle`
    // elements, rejects any element whose `realization_ref` differs from the
    // parent's (cross-solid gate), dedups by `kernel_handle`, and returns the
    // ids in ascending canonical order (matching extract_edges' mint order).
    // These cases are built from DIRECTLY-CONSTRUCTED handles via
    // `make_sub_handle` — no live build / scheduling required.

    /// (a) Happy path: a List of N edge sub-handles all sharing the parent's
    /// `realization_ref` resolves to their `kernel_handle` ids in ascending-id
    /// canonical order. The sub-handles are constructed OUT of ascending order
    /// to prove the resolver sorts (canonical = ascending kernel_handle id,
    /// matching extract_edges' TopExp mint order).
    #[test]
    fn resolve_subhandle_list_happy_path_canonical_order() {
        let ra = reify_core::identity::RealizationNodeId::new("PartA", 0);
        let parent_hash = [7u8; 32];
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra.clone(),
            upstream_values_hash: parent_hash,
            kernel_handle: Some(GeometryHandleId(1)),
        };
        // kernel handles deliberately scrambled to prove ascending canonical sort.
        let scrambled = [103u64, 101, 102, 100];
        let edges: Vec<reify_ir::Value> = scrambled
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                crate::topology_selectors::make_sub_handle(
                    &ra,
                    &parent_hash,
                    crate::topology_selectors::SubKind::Edge,
                    i as u32,
                    GeometryHandleId(id),
                )
            })
            .collect();
        let arg = reify_ir::Value::List(edges);
        let result = super::resolve_subhandle_list(&arg, &parent);
        assert_eq!(
            result,
            Ok(vec![
                GeometryHandleId(100),
                GeometryHandleId(101),
                GeometryHandleId(102),
                GeometryHandleId(103),
            ]),
            "happy path must resolve sub-handles to kernel_handle ids in \
             ascending canonical order"
        );
    }

    /// (b) Dedup: the same sub-handle listed twice collapses to one entry.
    #[test]
    fn resolve_subhandle_list_dedups_repeated_handle() {
        let ra = reify_core::identity::RealizationNodeId::new("PartA", 0);
        let parent_hash = [9u8; 32];
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra.clone(),
            upstream_values_hash: parent_hash,
            kernel_handle: Some(GeometryHandleId(1)),
        };
        let edge = crate::topology_selectors::make_sub_handle(
            &ra,
            &parent_hash,
            crate::topology_selectors::SubKind::Edge,
            0,
            GeometryHandleId(200),
        );
        let arg = reify_ir::Value::List(vec![edge.clone(), edge]);
        let result = super::resolve_subhandle_list(&arg, &parent);
        assert_eq!(
            result,
            Ok(vec![GeometryHandleId(200)]),
            "a repeated sub-handle must dedup to a single kernel_handle"
        );
    }

    /// (c) Cross-solid rejection: a sub-handle whose `realization_ref` differs
    /// from the parent's is rejected (a handle minted from a different solid).
    #[test]
    fn resolve_subhandle_list_rejects_cross_solid_handle() {
        let ra = reify_core::identity::RealizationNodeId::new("PartA", 0);
        let rb = reify_core::identity::RealizationNodeId::new("PartB", 0);
        let parent_hash = [1u8; 32];
        let other_hash = [2u8; 32];
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra.clone(),
            upstream_values_hash: parent_hash,
            kernel_handle: Some(GeometryHandleId(1)),
        };
        // One legit edge from PartA, one foreign edge from PartB.
        let good = crate::topology_selectors::make_sub_handle(
            &ra,
            &parent_hash,
            crate::topology_selectors::SubKind::Edge,
            0,
            GeometryHandleId(100),
        );
        let foreign = crate::topology_selectors::make_sub_handle(
            &rb,
            &other_hash,
            crate::topology_selectors::SubKind::Edge,
            0,
            GeometryHandleId(101),
        );
        let arg = reify_ir::Value::List(vec![good, foreign]);
        let result = super::resolve_subhandle_list(&arg, &parent);
        assert!(
            result.is_err(),
            "a sub-handle from a different realization_ref must be rejected \
             (cross-solid), got {:?}",
            result
        );
    }

    /// (d) Non-List arg: a non-List `Value` (e.g. `Real`) is rejected — the
    /// resolver requires a `List<Geometry>`.
    #[test]
    fn resolve_subhandle_list_rejects_non_list_arg() {
        let ra = reify_core::identity::RealizationNodeId::new("PartA", 0);
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra,
            upstream_values_hash: [3u8; 32],
            kernel_handle: Some(GeometryHandleId(1)),
        };
        let arg = reify_ir::Value::Real(2.0);
        let result = super::resolve_subhandle_list(&arg, &parent);
        assert!(
            result.is_err(),
            "a non-List arg must be rejected, got {:?}",
            result
        );
    }

    /// (e) Empty List: an empty selector list resolves to `Ok(vec![])`. The
    /// anti-zero-edges (E_EMPTY_SELECTION) guard lives in the eval arm, NOT in
    /// this kernel-free resolver — the resolver's job is purely structural.
    #[test]
    fn resolve_subhandle_list_empty_list_is_ok_empty() {
        let ra = reify_core::identity::RealizationNodeId::new("PartA", 0);
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra,
            upstream_values_hash: [4u8; 32],
            kernel_handle: Some(GeometryHandleId(1)),
        };
        let arg = reify_ir::Value::List(vec![]);
        let result = super::resolve_subhandle_list(&arg, &parent);
        assert_eq!(
            result,
            Ok(Vec::<GeometryHandleId>::new()),
            "an empty selector List must resolve to Ok(empty) — the \
             anti-zero-edges guard lives in the eval arm, not the resolver"
        );
    }

    // ── MaxDeviation via try_eval_geometry_query (ζ / C4) ───────────────────

    /// ζ / C4 (step-7 RED → step-8 GREEN): `max_deviation(actual, nominal)`
    /// direct call folds to `Value::Scalar<LENGTH>` via
    /// `try_eval_geometry_query`. The seeded kernel returns `Value::Real(5e-4)`
    /// (0.5 mm); the dispatch wraps it as
    /// `Scalar { dimension: LENGTH, si_value: 5e-4 }`.
    ///
    /// RED until step-8 wires the 2-arg `max_deviation` recognizer into
    /// `try_eval_geometry_query` (the current 1-arg gate returns `None` for a
    /// 2-arg `max_deviation` call).
    #[test]
    fn try_eval_geometry_query_max_deviation_direct_happy_path() {
        use reify_test_support::mocks::MockGeometryKernel;
        let actual = reify_ir::GeometryHandleId(20);
        let nominal = reify_ir::GeometryHandleId(21);
        // Tolerance matches the `MAX_DEVIATION_TESSELLATION_TOLERANCE_M` const
        // that step-8 will define in geometry_ops.rs.
        const TOL: f64 = 0.0001;
        let kernel = MockGeometryKernel::new().with_max_deviation_result(
            actual,
            nominal,
            TOL,
            reify_ir::Value::Real(5e-4),
        );

        let mut named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        named_steps.insert("a".to_string(), kh(actual));
        named_steps.insert("b".to_string(), kh(nominal));

        let values = reify_ir::ValueMap::new();
        let functions: Vec<reify_ir::CompiledFunction> = Vec::new();
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();

        // max_deviation(a, b): 2-arg call, both args resolved from named_steps.
        let expr = topology_selector_call_two_value_refs(
            "max_deviation",
            "MaxDevTest",
            "a",
            reify_core::Type::Geometry,
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::length(),
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_geometry_query(
            &expr,
            &named_steps,
            &values,
            &functions,
            &meta_map,
            &kernel,
            &mut diagnostics,
        );

        match result {
            Some(reify_ir::Value::Scalar {
                si_value,
                dimension,
            }) if dimension == reify_core::DimensionVector::LENGTH => {
                let expected = 5e-4_f64;
                let epsilon = 1e-12_f64;
                assert!(
                    (si_value - expected).abs() < epsilon,
                    "max_deviation direct call must produce si_value ≈ 5e-4; \
                     got {si_value:.15} (delta {delta:.3e})",
                    delta = (si_value - expected).abs()
                );
            }
            other => panic!(
                "max_deviation(actual, nominal) must return \
                 Some(Value::Scalar{{LENGTH, ≈5e-4}}); got {:?}",
                other
            ),
        }
        assert!(
            diagnostics.is_empty(),
            "happy-path max_deviation must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// ζ / C4: `max_deviation` with non-ValueRef (literal) args returns `None`
    /// — the cell stays at its compiled default (Value::Undef). Mirrors the
    /// defensive fall-through contract of the other 2-arg selectors.
    #[test]
    fn try_eval_geometry_query_max_deviation_literal_args_returns_none() {
        use reify_test_support::mocks::MockGeometryKernel;
        let kernel = MockGeometryKernel::new();
        let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
        let values = reify_ir::ValueMap::new();
        let functions: Vec<reify_ir::CompiledFunction> = Vec::new();
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();

        // literal args (non-ValueRef) — dispatch must return None
        let expr = topology_selector_call_literal_args("max_deviation");
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = super::try_eval_geometry_query(
            &expr,
            &named_steps,
            &values,
            &functions,
            &meta_map,
            &kernel,
            &mut diagnostics,
        );

        assert!(
            result.is_none(),
            "max_deviation with literal (non-ValueRef) args must return None; \
             got {:?}",
            result
        );
    }

    /// Drift-pin: `MAX_DEVIATION_TESSELLATION_TOLERANCE_M` must equal
    /// `Engine::DEFAULT_TESSELLATION_TOLERANCE` (engine_build.rs:3165 = 0.0001).
    ///
    /// `Engine::DEFAULT_TESSELLATION_TOLERANCE` is a private associated const, so
    /// this test pins its numeric value. **If the engine default ever changes**,
    /// update `MAX_DEVIATION_TESSELLATION_TOLERANCE_M` in this file to match and
    /// also update the `const TOL` literals in the tests above (they mirror the
    /// same value).
    #[test]
    fn max_deviation_tessellation_tolerance_pins_engine_default_value() {
        assert_eq!(
            super::MAX_DEVIATION_TESSELLATION_TOLERANCE_M,
            0.0001_f64,
            "MAX_DEVIATION_TESSELLATION_TOLERANCE_M must equal \
             Engine::DEFAULT_TESSELLATION_TOLERANCE (engine_build.rs:3165); \
             update both if the engine default changes"
        );
    }

    /// Pins the finite/non-negative guard in `dispatch_scalar_query` (amend task
    /// 4479 — reviewer suggestion 3). Kernels that return NaN, ±Inf, or a negative
    /// deviation must produce `Some(Value::Undef)` + exactly one Warning rather
    /// than silently propagating a bogus `Scalar<LENGTH>` into downstream
    /// arithmetic.
    #[test]
    fn dispatch_scalar_query_non_finite_or_negative_emits_warning_and_undef() {
        use reify_test_support::mocks::MockGeometryKernel;
        let actual = reify_ir::GeometryHandleId(40);
        let nominal = reify_ir::GeometryHandleId(41);
        const TOL: f64 = 0.0001;

        for bad_value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e-4_f64] {
            let kernel = MockGeometryKernel::new().with_max_deviation_result(
                actual,
                nominal,
                TOL,
                reify_ir::Value::Real(bad_value),
            );
            let query = reify_ir::GeometryQuery::MaxDeviation {
                actual,
                nominal,
                tolerance: TOL,
            };
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            let result = super::dispatch_scalar_query(
                &kernel,
                query,
                reify_core::DimensionVector::LENGTH,
                "max_deviation",
                &mut diagnostics,
            );
            assert_eq!(
                result,
                Some(reify_ir::Value::Undef),
                "dispatch_scalar_query with bad_value={bad_value:?} must return Some(Undef)"
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "dispatch_scalar_query with bad_value={bad_value:?} must emit exactly \
                 one Warning; got: {:?}",
                diagnostics
            );
        }
    }

    /// β/step-12 — Invariant V at the kernel-reply boundary. A scalar query whose
    /// caller-supplied dimension is DIMENSIONLESS must collapse the finite,
    /// non-negative `Value::Real` reply to `Value::Real` (via the
    /// `from_real_scalar` chokepoint), NOT leak a
    /// `Value::Scalar { dimension.is_dimensionless() }`. A dimensioned (LENGTH)
    /// query still yields a `Value::Scalar`.
    #[test]
    fn dispatch_scalar_query_dimensionless_collapses_to_real() {
        use reify_test_support::mocks::MockGeometryKernel;
        let actual = reify_ir::GeometryHandleId(50);
        let nominal = reify_ir::GeometryHandleId(51);
        const TOL: f64 = 0.0001;

        let kernel = MockGeometryKernel::new().with_max_deviation_result(
            actual,
            nominal,
            TOL,
            reify_ir::Value::Real(2.5),
        );

        // DIMENSIONLESS caller dimension → result must be Value::Real(2.5),
        // never Value::Scalar { dimension.is_dimensionless() }.
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::MaxDeviation {
                actual,
                nominal,
                tolerance: TOL,
            },
            reify_core::DimensionVector::DIMENSIONLESS,
            "leak_guard",
            &mut diagnostics,
        );
        assert_eq!(
            result,
            Some(reify_ir::Value::Real(2.5)),
            "a DIMENSIONLESS dispatch_scalar_query must collapse to Value::Real, \
             not leak Value::Scalar{{DIMENSIONLESS}}"
        );
        assert!(
            diagnostics.is_empty(),
            "a finite, non-negative reply must emit no warning; got: {diagnostics:?}"
        );

        // Guard: a dimensioned (LENGTH) query still yields a Value::Scalar.
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            reify_ir::GeometryQuery::MaxDeviation {
                actual,
                nominal,
                tolerance: TOL,
            },
            reify_core::DimensionVector::LENGTH,
            "leak_guard",
            &mut diagnostics,
        );
        assert_eq!(
            result,
            Some(reify_ir::Value::Scalar {
                si_value: 2.5,
                dimension: reify_core::DimensionVector::LENGTH,
            }),
            "a dimensioned (LENGTH) query must still yield Value::Scalar{{LENGTH}}"
        );
        assert!(diagnostics.is_empty());
    }

    /// Amendment (task 4374/β, reviewer suggestion 2): the defensive `Scalar`
    /// reply arm of `dispatch_scalar_query` must validate finiteness /
    /// non-negativity *identically* to the `Real` arm. A `Scalar` reply carrying
    /// NaN / ±Inf / a negative magnitude is downgraded to `Some(Value::Undef)` +
    /// exactly one Warning (never silently wrapped); a finite, non-negative
    /// `Scalar` reply collapses through `from_real_scalar` like a `Real` reply.
    #[test]
    fn dispatch_scalar_query_scalar_reply_validates_finite_non_negative() {
        use reify_test_support::mocks::MockGeometryKernel;
        let actual = reify_ir::GeometryHandleId(60);
        let nominal = reify_ir::GeometryHandleId(61);
        const TOL: f64 = 0.0001;

        // Bad Scalar replies → Undef + exactly one Warning, just like Real.
        for bad_value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e-4_f64] {
            let kernel = MockGeometryKernel::new().with_max_deviation_result(
                actual,
                nominal,
                TOL,
                reify_ir::Value::Scalar {
                    si_value: bad_value,
                    dimension: reify_core::DimensionVector::LENGTH,
                },
            );
            let query = reify_ir::GeometryQuery::MaxDeviation {
                actual,
                nominal,
                tolerance: TOL,
            };
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            let result = super::dispatch_scalar_query(
                &kernel,
                query,
                reify_core::DimensionVector::LENGTH,
                "max_deviation",
                &mut diagnostics,
            );
            assert_eq!(
                result,
                Some(reify_ir::Value::Undef),
                "Scalar reply with bad_value={bad_value:?} must return Some(Undef)"
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "Scalar reply with bad_value={bad_value:?} must emit exactly one \
                 Warning; got: {diagnostics:?}"
            );
        }

        // A finite, non-negative Scalar reply collapses through the chokepoint:
        // a DIMENSIONLESS caller dimension yields Value::Real (Invariant V).
        let kernel = MockGeometryKernel::new().with_max_deviation_result(
            actual,
            nominal,
            TOL,
            reify_ir::Value::Scalar {
                si_value: 2.5,
                dimension: reify_core::DimensionVector::LENGTH,
            },
        );
        let query = reify_ir::GeometryQuery::MaxDeviation {
            actual,
            nominal,
            tolerance: TOL,
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = super::dispatch_scalar_query(
            &kernel,
            query,
            reify_core::DimensionVector::DIMENSIONLESS,
            "leak_guard",
            &mut diagnostics,
        );
        assert_eq!(
            result,
            Some(reify_ir::Value::Real(2.5)),
            "a finite, non-negative Scalar reply with a DIMENSIONLESS caller \
             dimension must collapse to Value::Real"
        );
        assert!(diagnostics.is_empty());
    }

    // ── L5 registry completeness tests ──────────────────────────────────────

    // Exhaustive-match guards: compile error if a new variant is added without
    // updating the ALL_* array in compile_geometry_op_registry_completeness.
    // No `_` arm — the entire point is to be a compile-time tripwire.

    fn kind_idx_primitive(k: reify_compiler::PrimitiveKind) -> usize {
        use reify_compiler::PrimitiveKind as K;
        match k {
            K::Box => 0,
            K::Cylinder => 1,
            K::Sphere => 2,
            K::Tube => 3,
            K::Cone => 4,
            K::Wedge => 5,
            K::Torus => 6,
            K::HalfSpace => 7,
        }
    }

    fn kind_idx_modify(k: reify_compiler::ModifyKind) -> usize {
        use reify_compiler::ModifyKind as K;
        match k {
            K::Fillet => 0,
            K::Chamfer => 1,
            K::ChamferAsymmetric => 2,
            K::Shell => 3,
            K::Draft => 4,
            K::Thicken => 5,
            K::ZoneSlab => 6,
            K::OffsetSolid => 7,
            K::OffsetCurve => 8,
        }
    }

    fn kind_idx_transform(k: reify_compiler::TransformKind) -> usize {
        use reify_compiler::TransformKind as K;
        match k {
            K::Translate => 0,
            K::Rotate => 1,
            K::Scale => 2,
            K::RotateAround => 3,
            K::ApplyTransform => 4,
            K::AffineApply => 5,
            K::ScaleNonUniform => 6,
        }
    }

    fn kind_idx_pattern(k: reify_compiler::PatternKind) -> usize {
        use reify_compiler::PatternKind as K;
        match k {
            K::Linear => 0,
            K::Circular => 1,
            K::Mirror => 2,
            K::Linear2D => 3,
            K::Arbitrary => 4,
        }
    }

    fn kind_idx_sweep(k: reify_compiler::SweepKind) -> usize {
        use reify_compiler::SweepKind as K;
        match k {
            K::Loft => 0,
            K::Extrude => 1,
            K::Revolve => 2,
            K::Sweep => 3,
            K::ExtrudeSymmetric => 4,
            K::SweepGuided => 5,
            K::LoftGuided => 6,
            K::Pipe => 7,
            K::ExtrudeInfinite => 8,
        }
    }

    fn kind_idx_curve(k: reify_compiler::CurveKind) -> usize {
        use reify_compiler::CurveKind as K;
        match k {
            K::LineSegment => 0,
            K::Arc => 1,
            K::Helix => 2,
            K::InterpCurve => 3,
            K::BezierCurve => 4,
            K::NurbsCurve => 5,
        }
    }

    fn kind_idx_profile(k: reify_compiler::ProfileKind) -> usize {
        use reify_compiler::ProfileKind as K;
        match k {
            K::Rectangle => 0,
            K::Circle => 1,
            K::Polygon => 2,
            K::Ellipse => 3,
        }
    }

    /// L5 step-1: every kind in every family must have a registered compiler
    /// in the fn-table. RED until the 7 lookup fns + static tables are added
    /// in step-2.
    #[test]
    fn compile_geometry_op_registry_completeness() {
        use reify_compiler::{
            CurveKind, ModifyKind, PatternKind, PrimitiveKind, ProfileKind, SweepKind,
            TransformKind,
        };

        // Primitive (7 variants)
        const ALL_PRIMITIVE: [PrimitiveKind; 8] = [
            PrimitiveKind::Box,
            PrimitiveKind::Cylinder,
            PrimitiveKind::Sphere,
            PrimitiveKind::Tube,
            PrimitiveKind::Cone,
            PrimitiveKind::Wedge,
            PrimitiveKind::Torus,
            PrimitiveKind::HalfSpace,
        ];
        for k in ALL_PRIMITIVE {
            let _ = kind_idx_primitive(k);
            assert!(lookup_primitive(k).is_some(), "no Primitive entry: {:?}", k);
        }

        // Modify (9 variants) — VARIANT_COUNT cross-check (ALL is crate-private)
        const ALL_MODIFY: [ModifyKind; 9] = [
            ModifyKind::Fillet,
            ModifyKind::Chamfer,
            ModifyKind::ChamferAsymmetric,
            ModifyKind::Shell,
            ModifyKind::Draft,
            ModifyKind::Thicken,
            ModifyKind::ZoneSlab,
            ModifyKind::OffsetSolid,
            ModifyKind::OffsetCurve,
        ];
        assert_eq!(ALL_MODIFY.len(), ModifyKind::VARIANT_COUNT, "ALL_MODIFY / VARIANT_COUNT mismatch");
        for k in ALL_MODIFY {
            let _ = kind_idx_modify(k);
            assert!(lookup_modify(k).is_some(), "no Modify entry: {:?}", k);
        }

        // Transform (7 variants)
        const ALL_TRANSFORM: [TransformKind; 7] = [
            TransformKind::Translate,
            TransformKind::Rotate,
            TransformKind::Scale,
            TransformKind::RotateAround,
            TransformKind::ApplyTransform,
            TransformKind::AffineApply,
            TransformKind::ScaleNonUniform,
        ];
        for k in ALL_TRANSFORM {
            let _ = kind_idx_transform(k);
            assert!(lookup_transform(k).is_some(), "no Transform entry: {:?}", k);
        }

        // Pattern (5 variants)
        const ALL_PATTERN: [PatternKind; 5] = [
            PatternKind::Linear,
            PatternKind::Circular,
            PatternKind::Mirror,
            PatternKind::Linear2D,
            PatternKind::Arbitrary,
        ];
        for k in ALL_PATTERN {
            let _ = kind_idx_pattern(k);
            assert!(lookup_pattern(k).is_some(), "no Pattern entry: {:?}", k);
        }

        // Sweep (8 variants)
        const ALL_SWEEP: [SweepKind; 8] = [
            SweepKind::Loft,
            SweepKind::Extrude,
            SweepKind::Revolve,
            SweepKind::Sweep,
            SweepKind::ExtrudeSymmetric,
            SweepKind::SweepGuided,
            SweepKind::LoftGuided,
            SweepKind::Pipe,
        ];
        for k in ALL_SWEEP {
            let _ = kind_idx_sweep(k);
            assert!(lookup_sweep(k).is_some(), "no Sweep entry: {:?}", k);
        }

        // Curve (6 variants)
        const ALL_CURVE: [CurveKind; 6] = [
            CurveKind::LineSegment,
            CurveKind::Arc,
            CurveKind::Helix,
            CurveKind::InterpCurve,
            CurveKind::BezierCurve,
            CurveKind::NurbsCurve,
        ];
        for k in ALL_CURVE {
            let _ = kind_idx_curve(k);
            assert!(lookup_curve(k).is_some(), "no Curve entry: {:?}", k);
        }

        // Profile (4 variants)
        const ALL_PROFILE: [ProfileKind; 4] = [
            ProfileKind::Rectangle,
            ProfileKind::Circle,
            ProfileKind::Polygon,
            ProfileKind::Ellipse,
        ];
        for k in ALL_PROFILE {
            let _ = kind_idx_profile(k);
            assert!(lookup_profile(k).is_some(), "no Profile entry: {:?}", k);
        }
    }

    /// L5 step-3: the non-test region of geometry_ops.rs must contain ZERO
    /// nested per-kind behavioral match arms — all dispatch must go through the
    /// fn-tables. RED until step-4 deletes the 7 nested per-kind matches from
    /// compile_geometry_op's body.
    ///
    /// Detection: any line containing both a per-kind enum name (PrimitiveKind,
    /// ModifyKind, …) AND `=>` is a behavioral arm. Table rows `(Kind::X, fn)`
    /// carry no `=>` and are therefore invisible. The top-level
    /// `CompiledGeometryOp::X =>` and `BooleanOp::X =>` arms are NOT in the
    /// 7 kind-enum list and are also invisible.
    #[test]
    fn compile_geometry_op_has_no_nested_per_kind_match() {
        let src = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/geometry_ops.rs"
        ));
        // Non-test region: everything before `\n#[cfg(test)]\nmod tests;`.
        // (Task #5026 evicted this module to a sibling file, so the production
        // file now ends with the `mod tests;` declaration, not `mod tests {`.
        // The scanned region — production before the declaration — is byte-identical.)
        let boundary = "\n#[cfg(test)]\nmod tests;";
        let non_test = src
            .find(boundary)
            .map(|pos| &src[..pos])
            .expect("could not locate '#[cfg(test)]\\nmod tests;' in geometry_ops.rs");

        // A line with both a per-kind enum name AND `=>` is a behavioral arm.
        let bad_arms: Vec<&str> = non_test
            .lines()
            .filter(|line| {
                let has_kind_enum = line.contains("PrimitiveKind::")
                    || line.contains("ModifyKind::")
                    || line.contains("TransformKind::")
                    || line.contains("PatternKind::")
                    || line.contains("SweepKind::")
                    || line.contains("CurveKind::")
                    || line.contains("ProfileKind::");
                let has_fat_arrow = line.contains("=>");
                has_kind_enum && has_fat_arrow
            })
            .collect();

        assert!(
            bad_arms.is_empty(),
            "found {} nested per-kind behavioral match arm(s) in the \
             non-test region — all dispatch must go through the fn-tables \
             (step-4 wires this):\n{}",
            bad_arms.len(),
            bad_arms.join("\n")
        );
    }

    /// Amendment (task #4652, reviewer suggestion 1): `resolve_subhandle_list`
    /// must return `Err` with a "symbolic (unrealized) handle" message when the
    /// edge list contains a `Value::GeometryHandle { kernel_handle: None }`.
    ///
    /// This branch (geometry_ops.rs ~line 360) is newly reachable on the
    /// `reify check`/LSP eval path now that the eval mint produces symbolic
    /// handles.  The test pins that a symbolic edge handle surfaces a clean
    /// diagnostic (Err string) rather than a panic or a silent bogus-id
    /// deref.
    #[test]
    fn resolve_subhandle_list_rejects_symbolic_none_kernel_handle() {
        use reify_core::identity::RealizationNodeId;

        let ra = RealizationNodeId::new("PartA", 0);
        let parent = reify_ir::Value::GeometryHandle {
            realization_ref: ra.clone(),
            upstream_values_hash: [5u8; 32],
            kernel_handle: Some(GeometryHandleId(1)),
        };
        // A symbolic (unrealized) edge sub-handle — same realization_ref as
        // parent, but kernel_handle=None.
        let symbolic_edge = reify_ir::Value::GeometryHandle {
            realization_ref: ra,
            upstream_values_hash: [5u8; 32],
            kernel_handle: None,
        };
        let arg = reify_ir::Value::List(vec![symbolic_edge]);

        let result = super::resolve_subhandle_list(&arg, &parent);
        let msg = result.unwrap_err();
        assert!(
            msg.contains("symbolic (unrealized) handle"),
            "resolve_subhandle_list must Err with 'symbolic (unrealized) handle' \
             for a kernel_handle=None element; got: {msg:?}"
        );
    }

    /// Amendment (task #4652, reviewer suggestion 1): the `resolve_curated_edges_p2`
    /// branch inside `compile_geometry_op` (fillet/chamfer eval arm) must return
    /// `Err` with a "symbolic (unrealized) handle" message when the curated edge
    /// selector list contains a `Value::GeometryHandle { kernel_handle: None }`.
    ///
    /// This branch (geometry_ops.rs ~line 489) is newly reachable on the
    /// `reify check`/LSP eval path.  The test confirms graceful decline (Err,
    /// no panic) and that EmptyEdgeSelection is NOT emitted (a symbolic handle
    /// is not an empty selection).
    #[test]
    fn compile_geometry_op_fillet_symbolic_edge_declines_gracefully() {
        use reify_core::identity::RealizationNodeId;

        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let ra = RealizationNodeId::new("Body", 0);
        let symbolic_edge = reify_ir::Value::GeometryHandle {
            realization_ref: ra,
            upstream_values_hash: [7u8; 32],
            kernel_handle: None,
        };
        let symbolic_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(vec![symbolic_edge]),
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Fillet,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("edges".into(), symbolic_selector),
                ("radius".into(), literal_length(0.002)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "a symbolic (kernel_handle=None) edge handle must decline \
                 gracefully (Err), not produce a GeometryOp; got Ok({:?})",
                other
            ),
        };
        assert!(
            msg.contains("symbolic (unrealized) handle"),
            "decline message must flag the symbolic handle; got: {msg:?}"
        );
        // A symbolic handle is NOT an empty selection — must not trip the
        // anti-zero-edges guard.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a symbolic edge handle must not emit EmptyEdgeSelection; got: {:?}",
            diagnostics
        );
    }

    /// Amendment (task #4652, reviewer suggestion 1): the draft face selector
    /// inside `compile_geometry_op` must return `Err` with a "symbolic
    /// (unrealized) handle" message when the face selector list contains a
    /// `Value::GeometryHandle { kernel_handle: None }`.
    ///
    /// This branch (geometry_ops.rs ~line 1132) is newly reachable on the
    /// `reify check`/LSP eval path.  The test confirms graceful decline (Err,
    /// no panic) and that EmptyEdgeSelection is NOT emitted.
    #[test]
    fn compile_geometry_op_draft_symbolic_face_declines_gracefully() {
        use reify_core::identity::RealizationNodeId;

        // step_handles needs ≥2 entries: [0] = solid target, [1] = plane.
        let step_handles = vec![GeometryHandleId(10), GeometryHandleId(20)];
        let values = ValueMap::new();

        let ra = RealizationNodeId::new("Body", 0);
        let symbolic_face = reify_ir::Value::GeometryHandle {
            realization_ref: ra,
            upstream_values_hash: [9u8; 32],
            kernel_handle: None,
        };
        let symbolic_selector = reify_ir::CompiledExpr::literal(
            reify_ir::Value::List(vec![symbolic_face]),
            reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
        );
        let op = CompiledGeometryOp::Modify {
            kind: reify_compiler::ModifyKind::Draft,
            target: reify_compiler::GeomRef::Step(0),
            args: vec![
                ("target".into(), literal_length(0.0)),
                ("faces".into(), symbolic_selector),
                ("angle".into(), literal_angle(std::f64::consts::PI / 60.0)),
                ("plane".into(), literal_length(0.0)),
            ],
        };

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = compile_geometry_op(
            &op,
            &values,
            &step_handles,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &mut diagnostics,
        );

        let msg = match result {
            Err(msg) => msg,
            Ok(other) => panic!(
                "a symbolic (kernel_handle=None) face handle must decline \
                 gracefully (Err), not produce a GeometryOp; got Ok({:?})",
                other
            ),
        };
        assert!(
            msg.contains("symbolic (unrealized) handle"),
            "decline message must flag the symbolic handle; got: {msg:?}"
        );
        // A symbolic handle is NOT an empty selection — must not trip the
        // anti-zero-faces guard.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != Some(reify_core::DiagnosticCode::EmptyEdgeSelection)),
            "a symbolic face handle must not emit EmptyEdgeSelection; got: {:?}",
            diagnostics
        );
    }

    // ── task 4368: vertices()/vertex() dispatch unit tests ───────────────────
    //
    // These tests pin the contract of `try_eval_topology_selector` for the
    // new `vertices` (arity-1 All-leaf ctor) and `vertex` (arity-2 Named-leaf
    // ctor) helpers.  Both are kernel-FREE at construction (K2/BT7): zero
    // kernel queries occur; the `Selector → List<Geometry>` resolution is
    // deferred to the compiler-inserted `ResolveSelector` coercion node.
    // RED until step-8 adds the `Vertices`/`Vertex` helper variants + arms.

    /// `vertices(solid)` evaluates to `Value::Selector(Vertex)` with a
    /// `SelectorNode::Leaf { query: LeafQuery::All }`.  Zero kernel queries at
    /// construction time (K2/BT7): no `with_extracted_vertices` data is
    /// consumed.  Mirrors `edges_dispatch_returns_geometry_handle_list` /
    /// `mid_surface_ctor_yields_byrole_leaf_selector_of_face_kind`.
    #[test]
    fn vertices_ctor_yields_all_leaf_selector_of_vertex_kind() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("VerticesCtorTest", 0);
        let hash_b: [u8; 32] = [0xBB; 32];

        // Kernel is empty — vertices() must issue ZERO kernel queries.
        let mut kernel = MockGeometryKernel::new();
        let named_steps = HashMap::new(); // no kernel queries at construction

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("VerticesCtorTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );

        let expr = topology_selector_call_one_value_ref(
            "vertices",
            "VerticesCtorTest",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Vertex),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "vertices(b): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Vertex,
            "vertices(b) → Vertex kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, Some(handle_b),
                    "leaf target must be the parent solid handle"
                );
                assert_eq!(
                    *query,
                    reify_ir::value::LeafQuery::All,
                    "vertices(b) → All leaf"
                );
            }
            other => panic!("vertices(b) must be a Leaf selector node, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "kernel-free construction must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// `vertex(solid, "tip")` evaluates to `Value::Selector(Vertex)` with a
    /// `SelectorNode::Leaf { query: LeafQuery::Named("tip") }`.  Zero kernel
    /// queries at construction time (K2/BT7).  Mirrors
    /// `face_named_ctor_yields_named_leaf_selector_of_face_kind`.
    #[test]
    fn vertex_named_ctor_yields_named_leaf_selector_of_vertex_kind() {
        use reify_core::ValueCellId;
        use reify_core::identity::RealizationNodeId;
        use reify_test_support::mocks::MockGeometryKernel;

        let handle_b = GeometryHandleId(1);
        let rr = RealizationNodeId::new("VertexNamedCtorTest", 0);
        let hash_b: [u8; 32] = [0xCC; 32];

        let named_steps = HashMap::new(); // no kernel queries at construction
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("VertexNamedCtorTest", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: hash_b,
                kernel_handle: Some(handle_b),
            },
        );

        let expr = named_selector_call(
            "vertex",
            "VertexNamedCtorTest",
            "b",
            reify_core::ty::SelectorKind::Vertex,
            "tip",
        );
        let mut kernel = MockGeometryKernel::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "vertex(b, \"tip\"): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Vertex,
            "vertex() → Vertex kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf {
                query: reify_ir::value::LeafQuery::Named(n),
                ..
            } => {
                assert_eq!(n, "tip", "vertex(b, \"tip\") → Named(\"tip\") leaf");
            }
            other => panic!("expected Leaf{{ Named }}, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `vertices` with wrong arity (2 args instead of 1) must fall through to
    /// `None` — the arity gate fires before any construction logic runs.
    #[test]
    fn vertices_wrong_arity_falls_through_to_none() {
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();
        let named_steps = HashMap::new();
        let values = reify_ir::ValueMap::new();

        // Passing two value refs for a helper that takes exactly one.
        let expr = topology_selector_call_two_value_refs(
            "vertices",
            "T",
            "a",
            reify_core::Type::Geometry,
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Vertex),
        );
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );
        assert!(
            result.is_none(),
            "`vertices` with 2 args must fall through to None; got {:?}",
            result
        );
    }

    /// `vertex` with wrong arity (1 arg instead of 2) must fall through to
    /// `None` — the arity gate fires before any construction logic runs.
    #[test]
    fn vertex_wrong_arity_falls_through_to_none() {
        use reify_test_support::mocks::MockGeometryKernel;
        let mut kernel = MockGeometryKernel::new();
        let named_steps = HashMap::new();
        let values = reify_ir::ValueMap::new();

        // Passing one value ref for a helper that takes exactly two.
        let expr = topology_selector_call_one_value_ref(
            "vertex",
            "T",
            "b",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Vertex),
        );
        let mut diagnostics = Vec::new();
        let result = super::try_eval_topology_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &mut diagnostics,
        );
        assert!(
            result.is_none(),
            "`vertex` with 1 arg must fall through to None; got {:?}",
            result
        );
    }

    // ── task 4368 amendment: end-to-end pipeline tests for vertices()/vertex() ─
    //
    // These tests exercise the full compile_with_stdlib → Engine::build pipeline
    // for `vertices(body)` and `vertex(body, "tip")`, complementing the unit-
    // level try_eval_topology_selector tests above.  They pin that:
    //  (a) GEOMETRY_TOPOLOGY_SELECTOR_NAMES in units.rs correctly routes both
    //      helpers through the topology-selector post-process in engine_build.rs.
    //  (b) topology_selector_result_type("vertices"|"vertex") → Selector(Vertex)
    //      so the compiler-inserted ResolveSelector coercion wiring is present.
    //  (c) The full compile → post-process path packages a typed
    //      Value::Selector(Vertex) cell (K2/BT7: construction is kernel-free).
    //
    // The coercion→resolve seam (Selector → List<Geometry> → extract_vertices)
    // is now covered by the `vertices_index_coercion` golden in
    // crates/reify-eval/tests/selector_coercion_golden.rs (task #4723);
    // these e2e tests cover the construction half of the integrated pipeline.

    /// `vertices(body)` compiled via `compile_with_stdlib` and built by
    /// `Engine::build` must produce a `Value::Selector(Vertex)` cell whose leaf
    /// is `All` over the box's kernel handle — kernel-free (K2/BT7: the mock
    /// kernel carries no staged vertex data, so any kernel call would panic).
    ///
    /// Mirrors `edges_let_constructs_typed_edge_all_selector` from
    /// `topology_selector_runtime.rs`, exercised here to cover the
    /// GEOMETRY_TOPOLOGY_SELECTOR_NAMES + topology_selector_result_type wiring
    /// introduced in step-10 (units.rs).
    #[test]
    fn vertices_ctor_e2e_pipeline_builds_vertex_all_selector() {
        use reify_ir::value::{LeafQuery, SelectorNode};
        use reify_ir::{ExportFormat, Value};
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};

        let compiled = parse_and_compile_with_stdlib(
            "structure def T {\n    \
             let b = box(10mm, 10mm, 10mm)\n    \
             let vs = vertices(b)\n}",
        );

        let kernel = reify_test_support::mocks::MockGeometryKernel::new(); // UNSTAGED
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );
        let result = engine.build(&compiled, ExportFormat::Step);

        let cell = reify_core::ValueCellId::new("T", "vs");
        let sv = match result.values.get(&cell) {
            Some(Value::Selector(sv)) => sv,
            other => panic!(
                "T.vs: expected Value::Selector(Vertex) from vertices(b); got {:?}",
                other
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Vertex,
            "T.vs: selector kind must be Vertex"
        );
        match &sv.node {
            SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle,
                    Some(GeometryHandleId(1)),
                    "T.vs: leaf target must be the realized box handle (GHId 1)"
                );
                assert_eq!(*query, LeafQuery::All, "vertices(b) → All leaf");
            }
            other => panic!("T.vs must be Leaf node, got {:?}", other),
        }
    }

    /// `vertex(body, "tip")` compiled via `compile_with_stdlib` and built by
    /// `Engine::build` must produce a `Value::Selector(Vertex)` cell whose leaf
    /// is `Named("tip")` over the box's kernel handle — kernel-free (K2/BT7).
    ///
    /// Mirrors `vertex_named_ctor_yields_named_leaf_selector_of_vertex_kind`
    /// above, but via the full compile+build pipeline to pin the compiler-side
    /// wiring ("vertex" ∈ GEOMETRY_TOPOLOGY_SELECTOR_NAMES + result-type map).
    #[test]
    fn vertex_named_ctor_e2e_pipeline_builds_vertex_named_selector() {
        use reify_ir::value::{LeafQuery, SelectorNode};
        use reify_ir::{ExportFormat, Value};
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};

        let compiled = parse_and_compile_with_stdlib(
            "structure def T {\n    \
             let b = box(10mm, 10mm, 10mm)\n    \
             let v = vertex(b, \"tip\")\n}",
        );

        let kernel = reify_test_support::mocks::MockGeometryKernel::new(); // UNSTAGED
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );
        let result = engine.build(&compiled, ExportFormat::Step);

        let cell = reify_core::ValueCellId::new("T", "v");
        let sv = match result.values.get(&cell) {
            Some(Value::Selector(sv)) => sv,
            other => panic!(
                "T.v: expected Value::Selector(Vertex) from vertex(b, \"tip\"); got {:?}",
                other
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Vertex,
            "T.v: selector kind must be Vertex"
        );
        match &sv.node {
            SelectorNode::Leaf {
                target,
                query: LeafQuery::Named(name),
            } => {
                assert_eq!(
                    target.kernel_handle,
                    Some(GeometryHandleId(1)),
                    "T.v: leaf target must be the realized box handle (GHId 1)"
                );
                assert_eq!(name, "tip", "vertex(b, \"tip\") → Named(\"tip\") leaf");
            }
            other => panic!(
                "T.v must be Leaf{{Named(\"tip\")}}, got {:?}",
                other
            ),
        }
    }

    // ── resolve_selector_target / resolve_symbolic_selector_target tests ─────
    //
    // After R2b task #4653 the target-resolution logic is split into two fns:
    // - `resolve_symbolic_selector_target`: accepts both symbolic (None) and
    //   realized (Some) kernel handles — used ONLY by the shared kernel-free
    //   leaf helper `try_build_kernel_free_leaf_selector`.
    // - `resolve_selector_target`: realized-only wrapper — used by
    //   `try_eval_feature_datum_projection`, `eval_named_leaf_selector_ctor`,
    //   and the build-path AdjacentFaces/SharedEdges arms that need a live handle.
    //
    // The split (amendment pass, suggestion 1) prevents the widening from
    // leaking into build-path callers that depend on the realized-only contract.

    /// (a-sym) Symbolic `Value::GeometryHandle { kernel_handle: None }` yields
    /// `Some(GeometryHandleRef { kernel_handle: None, .. })` from
    /// `resolve_symbolic_selector_target`.
    #[test]
    fn resolve_symbolic_selector_target_accepts_symbolic_none_kernel_handle() {
        use reify_core::identity::{RealizationNodeId, ValueCellId};

        let cell_id = ValueCellId::new("Widget", "body");
        let rr = RealizationNodeId::new("Widget", 0);
        let uvh: [u8; 32] = [0xABu8; 32];
        let symbolic = reify_ir::Value::GeometryHandle {
            realization_ref: rr.clone(),
            upstream_values_hash: uvh,
            kernel_handle: None, // symbolic — no kernel handle
        };
        let mut values = reify_ir::ValueMap::new();
        values.insert(cell_id.clone(), symbolic);

        let expr = reify_ir::CompiledExpr::value_ref(cell_id, reify_core::ty::Type::Geometry);

        let result = super::resolve_symbolic_selector_target(&expr, &values);
        let ghr = result.expect(
            "resolve_symbolic_selector_target must return Some(GHR) for a symbolic handle"
        );
        assert_eq!(ghr.realization_ref, rr, "realization_ref must be propagated unchanged");
        assert_eq!(ghr.upstream_values_hash, uvh, "upstream_values_hash must be propagated unchanged");
        assert_eq!(ghr.kernel_handle, None, "kernel_handle must be None (symbolic)");
    }

    /// (a-real-sym) `resolve_selector_target` (realized-only) returns `None`
    /// for a symbolic handle — preserves the pre-R2b realized-only contract for
    /// all existing callers like `try_eval_feature_datum_projection`.
    #[test]
    fn resolve_selector_target_returns_none_for_symbolic_handle() {
        use reify_core::identity::{RealizationNodeId, ValueCellId};

        let cell_id = ValueCellId::new("Widget", "body");
        let rr = RealizationNodeId::new("Widget", 0);
        let uvh: [u8; 32] = [0xCCu8; 32];
        let symbolic = reify_ir::Value::GeometryHandle {
            realization_ref: rr,
            upstream_values_hash: uvh,
            kernel_handle: None, // symbolic
        };
        let mut values = reify_ir::ValueMap::new();
        values.insert(cell_id.clone(), symbolic);

        let expr = reify_ir::CompiledExpr::value_ref(cell_id, reify_core::ty::Type::Geometry);
        let result = super::resolve_selector_target(&expr, &values);
        assert!(
            result.is_none(),
            "resolve_selector_target (realized-only) must return None for a symbolic handle; \
             got {:?}",
            result
        );
    }

    // ── resolve_feature_arg tests (task 4831, P3β step-5 RED) ─────────────────
    //
    // Mirrors `resolve_symbolic_selector_target`: unwraps a `ValueRef` cell,
    // but expects `Value::Feature(fid)` rather than `Value::GeometryHandle`.

    #[test]
    fn resolve_feature_arg_returns_some_for_value_ref_to_feature() {
        use reify_core::identity::ValueCellId;
        use reify_ir::FeatureId;

        let cell_id = ValueCellId::new("Widget", "f");
        let fid = FeatureId::realization("Widget", 0);
        let mut values = reify_ir::ValueMap::new();
        values.insert(cell_id.clone(), reify_ir::Value::Feature(fid.clone()));

        let expr = reify_ir::CompiledExpr::value_ref(cell_id, reify_core::ty::Type::Feature);
        let result = super::resolve_feature_arg(&expr, &values);
        assert_eq!(
            result,
            Some(fid),
            "resolve_feature_arg must unwrap Value::Feature(fid) from the referenced cell"
        );
    }

    #[test]
    fn resolve_feature_arg_returns_none_for_value_ref_to_non_feature() {
        use reify_core::identity::ValueCellId;

        let cell_id = ValueCellId::new("Widget", "not_a_feature");
        let mut values = reify_ir::ValueMap::new();
        values.insert(cell_id.clone(), reify_ir::Value::Real(3.0));

        let expr = reify_ir::CompiledExpr::value_ref(cell_id, reify_core::ty::Type::Feature);
        let result = super::resolve_feature_arg(&expr, &values);
        assert!(
            result.is_none(),
            "resolve_feature_arg must return None when the cell does not hold Value::Feature; \
             got {:?}",
            result
        );
    }

    #[test]
    fn resolve_feature_arg_returns_none_for_non_value_ref_expr() {
        use reify_ir::FeatureId;

        let values = reify_ir::ValueMap::new();
        let expr = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Feature(FeatureId::realization("Widget", 0)),
            reify_core::ty::Type::Feature,
        );
        let result = super::resolve_feature_arg(&expr, &values);
        assert!(
            result.is_none(),
            "resolve_feature_arg must return None for a non-ValueRef expr (mirrors \
             resolve_symbolic_selector_target); got {:?}",
            result
        );
    }

    /// (b) Realized `Value::GeometryHandle { kernel_handle: Some(id) }` must yield
    /// `Some(GeometryHandleRef { kernel_handle: Some(id), .. })`.
    ///
    /// This case already works with the current implementation (the realized
    /// path goes through `resolve_parent_geometry_handle_arg` which unwraps
    /// `Some(kh)` fine). This test pins that the rewrite in step-2 preserves
    /// the realized path.
    #[test]
    fn resolve_selector_target_accepts_realized_some_kernel_handle() {
        use reify_core::identity::{RealizationNodeId, ValueCellId};

        let cell_id = ValueCellId::new("Widget", "body");
        let rr = RealizationNodeId::new("Widget", 0);
        let uvh: [u8; 32] = [0x11u8; 32];
        let kh_id = reify_ir::GeometryHandleId(42);
        let realized = reify_ir::Value::GeometryHandle {
            realization_ref: rr.clone(),
            upstream_values_hash: uvh,
            kernel_handle: Some(kh_id),
        };
        let mut values = reify_ir::ValueMap::new();
        values.insert(cell_id.clone(), realized);

        let expr = reify_ir::CompiledExpr::value_ref(cell_id, reify_core::ty::Type::Geometry);

        let result = super::resolve_selector_target(&expr, &values);
        let ghr = result.expect("resolve_selector_target must return Some for a realized handle");
        assert_eq!(ghr.realization_ref, rr);
        assert_eq!(ghr.upstream_values_hash, uvh);
        assert_eq!(ghr.kernel_handle, Some(kh_id), "kernel_handle must be Some(42)");
    }

    /// (c) A non-GeometryHandle value (e.g. `Value::Undef`) in the cell must yield `None`
    /// (PRD invariant #2: never partially-construct a selector target).
    #[test]
    fn resolve_selector_target_returns_none_for_non_geometry_handle_cell() {
        use reify_core::identity::ValueCellId;

        let cell_id = ValueCellId::new("Widget", "width");
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            cell_id.clone(),
            reify_ir::Value::Scalar {
                si_value: 0.01,
                dimension: reify_core::DimensionVector::LENGTH,
            },
        );
        let expr = reify_ir::CompiledExpr::value_ref(cell_id, reify_core::ty::Type::length());
        let result = super::resolve_selector_target(&expr, &values);
        assert!(
            result.is_none(),
            "must return None for a non-GeometryHandle cell; got {:?}",
            result
        );
    }

    /// (d) A missing cell (no entry in `values`) must yield `None`
    /// (PRD invariant #2).
    #[test]
    fn resolve_selector_target_returns_none_for_missing_cell() {
        use reify_core::identity::ValueCellId;

        let cell_id = ValueCellId::new("Widget", "missing");
        let values = reify_ir::ValueMap::new(); // empty
        let expr = reify_ir::CompiledExpr::value_ref(cell_id, reify_core::ty::Type::Geometry);
        let result = super::resolve_selector_target(&expr, &values);
        assert!(
            result.is_none(),
            "must return None for a missing cell; got {:?}",
            result
        );
    }

    // ── try_eval_symbolic_topology_selector tests (task #4653 step-3 RED) ──────
    //
    // These tests pin the contract of the new kernel-free eval-path dispatch
    // function for leaf selector constructors over symbolic targets.
    //
    // **RED**: `try_eval_symbolic_topology_selector` does not exist yet — the
    // module fails to compile. The compile error is the RED signal (per R2a
    // convention). Step-4 adds the function and makes all cases GREEN.

    /// Helper: build a 3-arg function call over three ValueRef args in the
    /// same template, for `faces_by_normal`/`edges_parallel_to`/`edges_at_height`.
    #[allow(clippy::too_many_arguments)]
    fn symbolic_selector_call_three_value_refs(
        helper_name: &str,
        entity: &str,
        member_a: &str,
        type_a: reify_core::Type,
        member_b: &str,
        type_b: reify_core::Type,
        member_c: &str,
        type_c: reify_core::Type,
    ) -> reify_ir::CompiledExpr {
        let arg_a = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_a),
            type_a,
        );
        let arg_b = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_b),
            type_b,
        );
        let arg_c = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new(entity, member_c),
            type_c,
        );
        let mut content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(helper_name));
        content_hash = content_hash.combine(arg_a.content_hash);
        content_hash = content_hash.combine(arg_b.content_hash);
        content_hash = content_hash.combine(arg_c.content_hash);
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: helper_name.to_string(),
                    qualified_name: helper_name.to_string(),
                },
                args: vec![arg_a, arg_b, arg_c],
            },
            result_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            content_hash,
        }
    }

    /// (a) `faces_by_normal(body, dir, tol)` over a symbolic body handle →
    /// `Some(Value::Selector(Face))` with `ByNormal` leaf and
    /// `target.kernel_handle == None`.
    ///
    /// **RED** until step-4 adds `try_eval_symbolic_topology_selector`.
    #[test]
    fn try_eval_symbolic_topology_selector_faces_by_normal_symbolic_target() {
        use reify_core::identity::{RealizationNodeId, ValueCellId};
        use reify_core::DimensionVector;

        let rr = RealizationNodeId::new("Widget", 0);
        let uvh: [u8; 32] = [0xABu8; 32];

        let mut values = reify_ir::ValueMap::new();
        // Symbolic body handle (kernel_handle = None).
        values.insert(
            ValueCellId::new("Widget", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: None,
            },
        );
        // Let-bound dir = vec3(0,0,1).
        values.insert(
            ValueCellId::new("Widget", "dir"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        // Let-bound tol = 1deg in radians.
        let tol_rad = std::f64::consts::PI / 180.0;
        values.insert(
            ValueCellId::new("Widget", "tol"),
            reify_ir::Value::Scalar {
                si_value: tol_rad,
                dimension: DimensionVector::ANGLE,
            },
        );

        let expr = symbolic_selector_call_three_value_refs(
            "faces_by_normal",
            "Widget",
            "body",
            reify_core::Type::Geometry,
            "dir",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "tol",
            reify_core::Type::angle(),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);

        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "faces_by_normal over symbolic target must yield Some(Value::Selector(..)); \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "faces_by_normal → Face kind"
        );
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, None,
                    "symbolic target must yield kernel_handle == None"
                );
                assert_eq!(target.realization_ref, rr, "realization_ref propagated");
                assert_eq!(
                    *query,
                    reify_ir::value::LeafQuery::ByNormal {
                        dir: [0.0, 0.0, 1.0],
                        tol_rad,
                    },
                    "ByNormal leaf with dir +z and 1° tolerance"
                );
            }
            other => panic!("must be a Leaf node; got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "kernel-free construction must emit zero diagnostics; got: {:?}",
            diagnostics
        );
    }

    /// (b) `edges_parallel_to(body, axis, tol)` over a symbolic body handle →
    /// `Some(Value::Selector(Edge))` with `ByParallel` leaf.
    ///
    /// **RED** until step-4.
    #[test]
    fn try_eval_symbolic_topology_selector_edges_parallel_to_symbolic_target() {
        use reify_core::identity::{RealizationNodeId, ValueCellId};
        use reify_core::DimensionVector;

        let rr = RealizationNodeId::new("Part", 0);
        let uvh: [u8; 32] = [0x22u8; 32];
        let tol_rad = std::f64::consts::PI / 180.0; // 1°

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Part", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: None,
            },
        );
        values.insert(
            ValueCellId::new("Part", "axis"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(1.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
            ]),
        );
        values.insert(
            ValueCellId::new("Part", "tol"),
            reify_ir::Value::Scalar {
                si_value: tol_rad,
                dimension: DimensionVector::ANGLE,
            },
        );

        let expr = symbolic_selector_call_three_value_refs(
            "edges_parallel_to",
            "Part",
            "body",
            reify_core::Type::Geometry,
            "axis",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "tol",
            reify_core::Type::angle(),
        );
        let mut diagnostics = Vec::new();

        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);

        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "edges_parallel_to over symbolic target must yield Some(Value::Selector(..)); \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Edge);
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(target.kernel_handle, None);
                assert_eq!(
                    *query,
                    reify_ir::value::LeafQuery::ByParallel {
                        axis: [1.0, 0.0, 0.0],
                        tol_rad,
                    }
                );
            }
            other => panic!("must be Leaf; got {:?}", other),
        }
    }

    // ── Task 3523: selector_vocabulary_v2 leaf-ctor minting (step-9 RED) ──────
    //
    // Each of the 6 v2 leaf ctors must mint the expected Value::Selector(kind)
    // with the expected LeafQuery node over a SYMBOLIC body target (kernel_handle
    // == None). RED until step-10 wires the names into the helper maps + adds the
    // build arms. Mirrors the faces_by_normal / edges_by_length minting tests.

    /// Build a symbolic FunctionCall CompiledExpr from a name + arg exprs
    /// (content hash folded over name + arg hashes, matching the manual
    /// constructions used by the arity-mismatch / edges_by_length tests).
    fn mk_symbolic_call_3523(
        name: &str,
        args: Vec<reify_ir::CompiledExpr>,
    ) -> reify_ir::CompiledExpr {
        let mut ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(name));
        for a in &args {
            ch = ch.combine(a.content_hash);
        }
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: name.to_string(),
                    qualified_name: name.to_string(),
                },
                args,
            },
            result_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            content_hash: ch,
        }
    }

    #[test]
    fn try_eval_symbolic_topology_selector_v2_leaf_ctors() {
        use reify_core::identity::{RealizationNodeId, ValueCellId};
        use reify_core::DimensionVector;
        use reify_ir::value::{LeafQuery, SelectorNode};
        use reify_ir::Value;

        let entity = "Widget";
        let rr = RealizationNodeId::new(entity, 0);
        let uvh: [u8; 32] = [0x5Au8; 32];
        let tol_rad = std::f64::consts::PI / 180.0; // 1°
        let tol_m = 1e-4; // 0.1 mm extremal tolerance

        let mut values = reify_ir::ValueMap::new();
        let body = |n: &str| ValueCellId::new(entity, n);
        values.insert(
            body("body"),
            Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: None,
            },
        );
        // Directional arg dir = +Z, angular tol = 1°.
        values.insert(
            body("dir"),
            Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]),
        );
        values.insert(
            body("tol"),
            Value::Scalar { si_value: tol_rad, dimension: DimensionVector::ANGLE },
        );
        // String kind/axis/sense args (let-bound, per the dispatcher contract).
        values.insert(body("plane"), Value::String("Plane".to_string()));
        values.insert(body("line"), Value::String("Line".to_string()));
        values.insert(body("ax"), Value::String("Z".to_string()));
        values.insert(body("sense"), Value::String("Max".to_string()));
        values.insert(
            body("etol"),
            Value::Scalar { si_value: tol_m, dimension: DimensionVector::LENGTH },
        );

        let gh = |n: &str| reify_ir::CompiledExpr::value_ref(body(n), reify_core::Type::Geometry);
        let dir = || {
            reify_ir::CompiledExpr::value_ref(
                body("dir"),
                reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            )
        };
        let tol = || reify_ir::CompiledExpr::value_ref(body("tol"), reify_core::Type::angle());
        let strarg =
            |n: &str| reify_ir::CompiledExpr::value_ref(body(n), reify_core::Type::String);
        let etol =
            || reify_ir::CompiledExpr::value_ref(body("etol"), reify_core::Type::length());

        // (name, args, expected kind, expected query)
        let cases: Vec<(&str, Vec<reify_ir::CompiledExpr>, reify_core::ty::SelectorKind, LeafQuery)> = vec![
            (
                "faces_perpendicular_to",
                vec![gh("body"), dir(), tol()],
                reify_core::ty::SelectorKind::Face,
                LeafQuery::ByPerpendicular { axis: [0.0, 0.0, 1.0], tol_rad },
            ),
            (
                "edges_perpendicular_to",
                vec![gh("body"), dir(), tol()],
                reify_core::ty::SelectorKind::Edge,
                LeafQuery::ByPerpendicular { axis: [0.0, 0.0, 1.0], tol_rad },
            ),
            (
                "faces_by_surface_kind",
                vec![gh("body"), strarg("plane")],
                reify_core::ty::SelectorKind::Face,
                LeafQuery::BySurfaceKind(reify_ir::FaceSurfaceKind::Plane),
            ),
            (
                "edges_by_curve_kind",
                vec![gh("body"), strarg("line")],
                reify_core::ty::SelectorKind::Edge,
                LeafQuery::ByCurveKind(reify_ir::EdgeCurveKind::Line),
            ),
            (
                "extremal_by_bbox",
                vec![gh("body"), strarg("ax"), strarg("sense"), etol()],
                reify_core::ty::SelectorKind::Face,
                LeafQuery::ByExtremalBbox { axis_index: 2, max: true, tol_m },
            ),
            (
                "extremal_by_centroid",
                vec![gh("body"), strarg("ax"), strarg("sense"), etol()],
                reify_core::ty::SelectorKind::Face,
                LeafQuery::ByExtremalCentroid { axis_index: 2, max: true, tol_m },
            ),
        ];

        for (name, args, want_kind, want_query) in cases {
            let expr = mk_symbolic_call_3523(name, args);
            let mut diagnostics = Vec::new();
            let result =
                super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
            let sv = match result {
                Some(Value::Selector(sv)) => sv,
                other => panic!(
                    "{name} over symbolic target must mint Some(Value::Selector(..)); \
                     got {:?}; diags: {:?}",
                    other, diagnostics
                ),
            };
            assert_eq!(sv.kind, want_kind, "{name} selector kind");
            match sv.node {
                SelectorNode::Leaf { target, query } => {
                    assert_eq!(
                        target.kernel_handle, None,
                        "{name}: symbolic target must have kernel_handle == None"
                    );
                    assert_eq!(query, want_query, "{name}: minted LeafQuery node");
                }
                other => panic!("{name}: must be a Leaf node; got {:?}", other),
            }
            assert!(
                diagnostics.is_empty(),
                "{name}: kernel-free minting must emit zero diagnostics; got: {:?}",
                diagnostics
            );
        }
    }

    /// Arity guard: `faces_perpendicular_to(body, dir)` (2 args, expects 3) → None.
    #[test]
    fn try_eval_symbolic_topology_selector_v2_leaf_arity_mismatch() {
        use reify_core::identity::ValueCellId;
        let a = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("W", "body"),
            reify_core::Type::Geometry,
        );
        let b = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("W", "dir"),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let expr = mk_symbolic_call_3523("faces_perpendicular_to", vec![a, b]);
        let values = reify_ir::ValueMap::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        assert!(
            result.is_none(),
            "faces_perpendicular_to with wrong arity (2 != 3) must return None; got {:?}",
            result
        );
    }

    // ── CreatedByFeature/SplitByFeature provenance selectors (task 4831, P3β
    // step-5 RED) ──────────────────────────────────────────────────────────

    #[test]
    fn topology_selector_helper_created_by_split_by_feature_arity_is_two() {
        assert_eq!(
            TopologySelectorHelper::CreatedByFeature.expected_arity(),
            2,
            "created_by_feature(solid, f) is arity 2"
        );
        assert_eq!(
            TopologySelectorHelper::SplitByFeature.expected_arity(),
            2,
            "split_by_feature(solid, f) is arity 2"
        );
    }

    /// Mirrors `try_eval_symbolic_topology_selector_v2_leaf_ctors`:
    /// `created_by_feature(solid, f)` / `split_by_feature(solid, f)` over a
    /// SYMBOLIC target must mint `Value::Selector(Face)` carrying
    /// `LeafQuery::CreatedByFeature(fid)` / `SplitByFeature(fid)`, kernel-free.
    #[test]
    fn try_eval_symbolic_topology_selector_created_by_split_by_feature_leaf_ctors() {
        use reify_core::identity::{RealizationNodeId, ValueCellId};
        use reify_ir::value::{LeafQuery, SelectorNode};
        use reify_ir::{FeatureId, Value};

        let entity = "Widget";
        let rr = RealizationNodeId::new(entity, 0);
        let uvh: [u8; 32] = [0x5Au8; 32];
        let fid = FeatureId::realization(entity, 0);

        let mut values = reify_ir::ValueMap::new();
        let body = |n: &str| ValueCellId::new(entity, n);
        values.insert(
            body("body"),
            Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: None,
            },
        );
        values.insert(body("f"), Value::Feature(fid.clone()));

        let gh = || reify_ir::CompiledExpr::value_ref(body("body"), reify_core::Type::Geometry);
        let feat = || reify_ir::CompiledExpr::value_ref(body("f"), reify_core::Type::Feature);

        let cases: Vec<(&str, LeafQuery)> = vec![
            ("created_by_feature", LeafQuery::CreatedByFeature(fid.clone())),
            ("split_by_feature", LeafQuery::SplitByFeature(fid.clone())),
        ];

        for (name, want_query) in cases {
            let expr = mk_symbolic_call_3523(name, vec![gh(), feat()]);
            let mut diagnostics = Vec::new();
            let result =
                super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
            let sv = match result {
                Some(Value::Selector(sv)) => sv,
                other => panic!(
                    "{name} over symbolic target must mint Some(Value::Selector(..)); \
                     got {:?}; diags: {:?}",
                    other, diagnostics
                ),
            };
            assert_eq!(
                sv.kind,
                reify_core::ty::SelectorKind::Face,
                "{name} selector kind"
            );
            match sv.node {
                SelectorNode::Leaf { target, query } => {
                    assert_eq!(
                        target.kernel_handle, None,
                        "{name}: symbolic target must have kernel_handle == None"
                    );
                    assert_eq!(query, want_query, "{name}: minted LeafQuery node");
                }
                other => panic!("{name}: must be a Leaf node; got {:?}", other),
            }
            assert!(
                diagnostics.is_empty(),
                "{name}: kernel-free minting must emit zero diagnostics; got: {:?}",
                diagnostics
            );
        }
    }

    /// Build-path (kernel-bearing dispatcher) mirror of the symbolic-path test
    /// above: `created_by_feature(solid, f)` / `split_by_feature(solid, f)`
    /// over a REALIZED target must ALSO mint `Value::Selector(Face)` via the
    /// shared kernel-free builder, issuing ZERO kernel queries — proving the
    /// "build" name→helper dispatch table (geometry_ops.rs `try_eval_topology_selector`)
    /// is wired identically to the "symbolic" table.
    #[test]
    fn try_eval_topology_selector_created_by_split_by_feature_leaf_ctors() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_ir::{FeatureId, Value};
        use reify_test_support::mocks::MockGeometryKernel;

        let entity = "Widget";
        let handle = GeometryHandleId(1);
        let rr = RealizationNodeId::new(entity, 0);
        let uvh: [u8; 32] = [0xCDu8; 32];
        let fid = FeatureId::realization(entity, 0);

        let named_steps = HashMap::new();
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new(entity, "body"),
            Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: Some(handle),
            },
        );
        values.insert(ValueCellId::new(entity, "f"), Value::Feature(fid.clone()));

        let cases: Vec<(&str, reify_ir::value::LeafQuery)> = vec![
            (
                "created_by_feature",
                reify_ir::value::LeafQuery::CreatedByFeature(fid.clone()),
            ),
            (
                "split_by_feature",
                reify_ir::value::LeafQuery::SplitByFeature(fid.clone()),
            ),
        ];

        for (name, want_query) in cases {
            let expr = topology_selector_call_two_value_refs(
                name,
                entity,
                "body",
                Type::Geometry,
                "f",
                Type::Feature,
                Type::Selector(reify_core::ty::SelectorKind::Face),
            );
            let mut kernel = MockGeometryKernel::new();
            let mut diagnostics = Vec::new();
            let result = super::try_eval_topology_selector(
                &expr,
                &named_steps,
                &values,
                &mut kernel,
                &mut diagnostics,
            );
            let sv = match result {
                Some(Value::Selector(sv)) => sv,
                other => panic!(
                    "{name}(body, f): expected Some(Value::Selector(..)); got {:?}; diags: {:?}",
                    other, diagnostics
                ),
            };
            assert_eq!(sv.kind, reify_core::ty::SelectorKind::Face, "{name} → Face kind");
            match &sv.node {
                reify_ir::value::SelectorNode::Leaf { query, .. } => {
                    assert_eq!(query, &want_query, "{name}: minted LeafQuery node");
                }
                other => panic!("{name}: must be a Leaf node; got {:?}", other),
            }
            assert!(
                diagnostics.is_empty(),
                "{name}: kernel-free minting must emit zero diagnostics; got: {:?}",
                diagnostics
            );
        }
    }

    /// Task 3523 (amendment — reviewer: untested_failure_modes). The four
    /// kind/axis/sense arg resolvers each reject an unrecognised string — and a
    /// non-String value — by pushing exactly ONE Warning and returning None
    /// (leaving the cell at Value::Undef). The minting tests above exercise only
    /// the happy path + arity guard; this pins every rejection branch so a
    /// regression that drops the diagnostic, emits more than one, or stops
    /// returning None is caught.
    #[test]
    fn v2_kind_axis_sense_arg_resolvers_reject_bad_values() {
        use reify_core::Severity;

        fn str_lit(s: &str) -> reify_ir::CompiledExpr {
            reify_ir::CompiledExpr::literal(
                reify_ir::Value::String(s.to_string()),
                reify_core::Type::String,
            )
        }
        // A defined-but-wrong (non-String) value — an Int — routes through
        // resolve_string_literal_arg's ArgRejection branch (got "Int").
        fn int_lit() -> reify_ir::CompiledExpr {
            reify_ir::CompiledExpr::literal(reify_ir::Value::Int(5), reify_core::Type::Int)
        }

        // (case label, arg expr, resolver → true iff it returned None, substring
        //  the single Warning message must contain). The explicit Vec type coerces
        //  the four distinct closures to one Box<dyn Fn> so a single table drives all.
        #[allow(clippy::type_complexity)]
        let cases: Vec<(
            &str,
            reify_ir::CompiledExpr,
            Box<dyn Fn(&reify_ir::CompiledExpr, &mut Vec<Diagnostic>) -> bool>,
            &str,
        )> = vec![
            (
                "unknown surface kind",
                str_lit("Octahedron"),
                Box::new(|e, d| {
                    super::resolve_face_surface_kind_arg(
                        e,
                        &reify_ir::ValueMap::new(),
                        "faces_by_surface_kind",
                        d,
                    )
                    .is_none()
                }),
                "Octahedron",
            ),
            (
                "unknown curve kind",
                str_lit("Helix"),
                Box::new(|e, d| {
                    super::resolve_edge_curve_kind_arg(
                        e,
                        &reify_ir::ValueMap::new(),
                        "edges_by_curve_kind",
                        d,
                    )
                    .is_none()
                }),
                "Helix",
            ),
            (
                "unknown axis",
                str_lit("W"),
                Box::new(|e, d| {
                    super::resolve_axis_index_arg(
                        e,
                        &reify_ir::ValueMap::new(),
                        "extremal_by_bbox",
                        d,
                    )
                    .is_none()
                }),
                "axis",
            ),
            (
                "wrong-case sense \"max\"",
                str_lit("max"),
                Box::new(|e, d| {
                    super::resolve_extremal_sense_arg(
                        e,
                        &reify_ir::ValueMap::new(),
                        "extremal_by_bbox",
                        d,
                    )
                    .is_none()
                }),
                "sense",
            ),
            (
                "non-String axis value",
                int_lit(),
                Box::new(|e, d| {
                    super::resolve_axis_index_arg(
                        e,
                        &reify_ir::ValueMap::new(),
                        "extremal_by_bbox",
                        d,
                    )
                    .is_none()
                }),
                "axis",
            ),
        ];

        for (label, expr, resolve, want_substr) in cases {
            let mut diags = Vec::new();
            let returned_none = resolve(&expr, &mut diags);
            assert!(returned_none, "{label}: resolver must return None for a bad value");
            assert_eq!(
                diags.len(),
                1,
                "{label}: expected exactly one Warning diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].severity,
                Severity::Warning,
                "{label}: rejection must be a Warning, got {:?}",
                diags[0].severity
            );
            assert!(
                diags[0].message.contains(want_substr),
                "{label}: Warning message must mention {:?}; got: {}",
                want_substr,
                diags[0].message
            );
        }
    }

    /// (c) `vertices(body)` over a symbolic body handle →
    /// `Some(Value::Selector(Vertex))` with `All` leaf.
    ///
    /// **RED** until step-4.
    #[test]
    fn try_eval_symbolic_topology_selector_vertices_symbolic_target() {
        use reify_core::identity::{RealizationNodeId, ValueCellId};

        let rr = RealizationNodeId::new("Part", 0);
        let uvh: [u8; 32] = [0x33u8; 32];

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("Part", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: None,
            },
        );

        let arg = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new("Part", "body"),
            reify_core::Type::Geometry,
        );
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("vertices"))
            .combine(arg.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "vertices".to_string(),
                    qualified_name: "vertices".to_string(),
                },
                args: vec![arg],
            },
            result_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            content_hash,
        };
        let mut diagnostics = Vec::new();

        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);

        let sv = match result {
            Some(reify_ir::Value::Selector(ref sv)) => sv.clone(),
            other => panic!(
                "vertices over symbolic target must yield Some(Value::Selector(Vertex)); \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Vertex);
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(target.kernel_handle, None);
                assert_eq!(*query, reify_ir::value::LeafQuery::All);
            }
            other => panic!("must be Leaf{{All}}; got {:?}", other),
        }
    }

    /// (d) A kernel-bearing helper (`closest_point`) → `None` (cells stays Undef → R1a).
    ///
    /// **RED** until step-4 (will be GREEN once the function exists and returns None
    /// for unknown helpers).
    #[test]
    fn try_eval_symbolic_topology_selector_returns_none_for_kernel_bearing_helper() {
        use reify_core::identity::ValueCellId;

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("W", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: reify_core::identity::RealizationNodeId::new("W", 0),
                upstream_values_hash: [0u8; 32],
                kernel_handle: None,
            },
        );
        values.insert(
            ValueCellId::new("W", "pt"),
            reify_ir::Value::Point(vec![
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
                reify_ir::Value::length(0.0),
            ]),
        );

        // closest_point(pt, b) — kernel-bearing, must return None.
        let arg_a = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new("W", "pt"),
            reify_core::Type::point3(reify_core::Type::length()),
        );
        let arg_b = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new("W", "b"),
            reify_core::Type::Geometry,
        );
        let mut ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("closest_point"));
        ch = ch.combine(arg_a.content_hash).combine(arg_b.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "closest_point".to_string(),
                    qualified_name: "closest_point".to_string(),
                },
                args: vec![arg_a, arg_b],
            },
            result_type: reify_core::Type::point3(reify_core::Type::length()),
            content_hash: ch,
        };
        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        assert!(
            result.is_none(),
            "closest_point is kernel-bearing — must return None; got {:?}",
            result
        );
    }

    /// (e) A symbolic body cell that is `Value::Undef` (missing) → `None`
    /// (PRD invariant #2: never partial-construct a selector target).
    ///
    /// **RED** until step-4.
    #[test]
    fn try_eval_symbolic_topology_selector_returns_none_for_undef_target() {
        // values is empty — body cell is absent → resolve_selector_target returns None.
        let values = reify_ir::ValueMap::new();

        let arg_body = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new("W", "body"),
            reify_core::Type::Geometry,
        );
        let ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("faces"))
            .combine(arg_body.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "faces".to_string(),
                    qualified_name: "faces".to_string(),
                },
                args: vec![arg_body],
            },
            result_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            content_hash: ch,
        };
        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        assert!(
            result.is_none(),
            "must return None when target cell is absent/Undef (PRD invariant #2); got {:?}",
            result
        );
    }

    // ── arity-guard tests (amendment pass, suggestion 4) ────────────────────
    //
    // Pins that wrong-arity calls return `None` rather than panicking
    // (arity is checked BEFORE args[N] indexing, so an off-by-one would
    // panic, not return None).  Also exercises the arity-2 predicate
    // constructors (edges_by_length, faces_by_area) that earlier tests skipped.

    /// `faces(a, b)` — wrong arity (2 instead of 1) → `None`.
    #[test]
    fn try_eval_symbolic_topology_selector_returns_none_for_arity_mismatch_faces() {
        use reify_core::identity::ValueCellId;
        let a = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("W", "body"),
            reify_core::Type::Geometry,
        );
        let b = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("W", "extra"),
            reify_core::Type::Geometry,
        );
        let ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("faces"))
            .combine(a.content_hash)
            .combine(b.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "faces".to_string(),
                    qualified_name: "faces".to_string(),
                },
                args: vec![a, b], // wrong arity: expects 1, gets 2
            },
            result_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            content_hash: ch,
        };
        let values = reify_ir::ValueMap::new();
        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        assert!(
            result.is_none(),
            "faces with wrong arity must return None (arity guard); got {:?}",
            result
        );
    }

    /// GREEN: `edges_by_length(body, range)` over a symbolic body → `Some(Value::Selector(Edge))`
    /// with a `ByLength` leaf.  Exercises the arity-2 predicate path that
    /// `faces_by_normal`/`edges_parallel_to` tests do not reach.
    #[test]
    fn try_eval_symbolic_topology_selector_edges_by_length_symbolic_target() {
        use reify_core::identity::{RealizationNodeId, ValueCellId};
        use reify_core::DimensionVector;
        use reify_ir::value::{LeafQuery, SelectorNode};
        use reify_ir::Value;

        let entity = "W";
        let cell_body = ValueCellId::new(entity, "body");
        let cell_range = ValueCellId::new(entity, "len_range");

        // Build a symbolic GeometryHandle (kernel_handle=None).
        let rr = RealizationNodeId::new(entity, 0);
        let uvh: [u8; 32] = [0xAAu8; 32];
        let symbolic_body = Value::GeometryHandle {
            realization_ref: rr,
            upstream_values_hash: uvh,
            kernel_handle: None,
        };

        // Build a Range<Length> value: [1 mm, 10 mm] → (0.001, 0.010) in SI metres.
        let range_val = Value::range(
            Some(Value::Scalar { si_value: 0.001, dimension: DimensionVector::LENGTH }),
            Some(Value::Scalar { si_value: 0.010, dimension: DimensionVector::LENGTH }),
            true,
            true,
        );

        let mut values = reify_ir::ValueMap::new();
        values.insert(cell_body.clone(), symbolic_body);
        values.insert(cell_range.clone(), range_val);

        // Build edges_by_length(body, len_range).
        let arg_body = reify_ir::CompiledExpr::value_ref(cell_body, reify_core::Type::Geometry);
        let arg_range = reify_ir::CompiledExpr::value_ref(
            cell_range,
            reify_core::Type::range(reify_core::Type::length()),
        );
        let ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("edges_by_length"))
            .combine(arg_body.content_hash)
            .combine(arg_range.content_hash);
        let expr = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "edges_by_length".to_string(),
                    qualified_name: "edges_by_length".to_string(),
                },
                args: vec![arg_body, arg_range],
            },
            result_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            content_hash: ch,
        };

        let mut diagnostics = Vec::new();
        let result =
            super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);

        let value = result.expect("edges_by_length over symbolic body must return Some");
        let sv = match value {
            Value::Selector(sv) => sv,
            other => panic!("expected Value::Selector, got {:?}", other),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Edge, "must be Edge selector");
        let leaf = match sv.node {
            SelectorNode::Leaf { query, target } => {
                assert!(
                    target.kernel_handle.is_none(),
                    "symbolic target must have kernel_handle=None"
                );
                query.clone()
            }
            other => panic!("expected Leaf node, got {:?}", other),
        };
        match leaf {
            LeafQuery::ByLength { min_m, max_m } => {
                assert!(
                    (min_m - 0.001).abs() < 1e-12,
                    "min_m must be 0.001 m; got {}",
                    min_m
                );
                assert!(
                    (max_m - 0.010).abs() < 1e-12,
                    "max_m must be 0.010 m; got {}",
                    max_m
                );
            }
            other => panic!("expected ByLength leaf, got {:?}", other),
        }
        assert!(diagnostics.is_empty(), "no diagnostics expected; got {:?}", diagnostics);
    }

    // ── mint_symbolic_topology_selectors_into_values unit tests ─────────────
    // (amendment pass, suggestion 3)
    //
    // Pins the load-bearing behaviours of the pass that are only transitively
    // covered by integration tests:
    // (a) A Undef selector cell with a faces_by_normal expr is minted.
    // (b) A sibling cell already holding a non-Undef value is NOT overwritten.

    #[test]
    fn mint_symbolic_topology_selectors_skips_non_undef_cells_and_mints_undef_ones() {
        use reify_core::identity::{RealizationNodeId, ValueCellId};
        use reify_core::DimensionVector;
        use reify_ir::Value;

        let entity = "W";

        // ── Cell A: faces_by_normal(body, dir, tol) — starts Undef, should be minted.
        let cell_body = ValueCellId::new(entity, "body");
        let cell_dir  = ValueCellId::new(entity, "dir");
        let cell_tol  = ValueCellId::new(entity, "tol");
        let cell_top  = ValueCellId::new(entity, "top");

        let arg_body = reify_ir::CompiledExpr::value_ref(cell_body.clone(), reify_core::Type::Geometry);
        let arg_dir  = reify_ir::CompiledExpr::value_ref(cell_dir.clone(), reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()));
        let arg_tol  = reify_ir::CompiledExpr::value_ref(
            cell_tol.clone(),
            reify_core::Type::Scalar { dimension: reify_core::DimensionVector::ANGLE },
        );
        let ch_a = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("faces_by_normal"))
            .combine(arg_body.content_hash)
            .combine(arg_dir.content_hash)
            .combine(arg_tol.content_hash);
        let expr_a = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "faces_by_normal".to_string(),
                    qualified_name: "faces_by_normal".to_string(),
                },
                args: vec![arg_body, arg_dir, arg_tol],
            },
            result_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            content_hash: ch_a,
        };

        // ── Cell B: faces(body) — pre-populated with a realized selector;
        // its default_expr is also a selector ctor, so the pass would mint it
        // IF the non-Undef guard were absent.  The guard must prevent the write.
        let cell_all_faces = ValueCellId::new(entity, "all_faces");
        let arg_body2 = reify_ir::CompiledExpr::value_ref(cell_body.clone(), reify_core::Type::Geometry);
        let ch_b = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("faces"))
            .combine(arg_body2.content_hash);
        let expr_b = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "faces".to_string(),
                    qualified_name: "faces".to_string(),
                },
                args: vec![arg_body2],
            },
            result_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            content_hash: ch_b,
        };

        // Build the CompiledModule with two ValueCellDecls in one template.
        let cell_decl_a = reify_compiler::ValueCellDecl {
            id: cell_top.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            visibility: reify_compiler::Visibility::Private,
            is_aux: false,
            cell_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            default_expr: Some(expr_a),
            solver_hints: vec![],
            span: reify_core::SourceSpan::new(0, 0),
        };
        let cell_decl_b = reify_compiler::ValueCellDecl {
            id: cell_all_faces.clone(),
            kind: reify_compiler::ValueCellKind::Let,
            visibility: reify_compiler::Visibility::Private,
            is_aux: false,
            cell_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            default_expr: Some(expr_b),
            solver_hints: vec![],
            span: reify_core::SourceSpan::new(0, 0),
        };
        use std::collections::{HashMap, HashSet};
        let template = reify_compiler::TopologyTemplate {
            name: entity.to_string(),
            doc: None,
            entity_kind: reify_compiler::EntityKind::Structure,
            visibility: reify_compiler::Visibility::Public,
            type_params: vec![],
            trait_bounds: vec![],
            value_cells: vec![cell_decl_a, cell_decl_b],
            constraints: vec![],
            realizations: vec![],
            sub_components: vec![],
            relations: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: HashSet::new(),
            objective: None,
            meta: HashMap::new(),
            content_hash: reify_core::ContentHash(0),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
            assoc_fns: vec![],
            assoc_types: vec![],
        };
        let module = reify_compiler::CompiledModule {
            path: reify_core::ModulePath::single("test"),
            imports: vec![],
            enum_defs: vec![],
            functions: vec![],
            trait_defs: vec![],
            fields: vec![],
            compiled_purposes: vec![],
            templates: vec![template],
            units: vec![],
            type_aliases: vec![],
            constraint_defs: vec![],
            pragmas: vec![],
            default_tolerance: None,
            declared_version: None,
            solver_pragma: None,
            kernel_pragma: None,
            deterministic: false,
            auto_type_substitution: reify_compiler::AutoTypeSubstitution::default(),
            diagnostics: vec![],
            content_hash: reify_core::ContentHash::of_str(""),
        };

        // ── Populate values map ──────────────────────────────────────────────
        let rr = RealizationNodeId::new(entity, 0);
        let uvh: [u8; 32] = [0xBBu8; 32];
        let symbolic_body = Value::GeometryHandle {
            realization_ref: rr.clone(),
            upstream_values_hash: uvh,
            kernel_handle: None,
        };
        let dir_val = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(1.0),
        ]);
        let tol_val = Value::Scalar {
            si_value: std::f64::consts::PI / 180.0, // 1 degree in radians
            dimension: DimensionVector::ANGLE,
        };

        // Cell B: a pre-existing realized selector (should NOT be overwritten).
        // We create a minimal realized selector to place in the map.
        let pre_existing_ghr = reify_ir::value::GeometryHandleRef {
            realization_ref: rr.clone(),
            upstream_values_hash: uvh,
            kernel_handle: Some(reify_ir::GeometryHandleId(99)),
        };
        let pre_existing_selector = reify_ir::value::SelectorValue::leaf(
            reify_core::ty::SelectorKind::Face,
            pre_existing_ghr,
            reify_ir::value::LeafQuery::All,
        )
        .expect("valid kind-closure");
        let pre_existing_value = Value::Selector(pre_existing_selector.clone());

        let mut values = reify_ir::ValueMap::new();
        values.insert(cell_body.clone(), symbolic_body);
        values.insert(cell_dir.clone(), dir_val);
        values.insert(cell_tol.clone(), tol_val);
        // Cell B is already non-Undef:
        values.insert(cell_all_faces.clone(), pre_existing_value);
        // Cell A (top) is absent (equivalent to Undef) — should be minted.

        // ── Run the pass ────────────────────────────────────────────────────
        let mut diagnostics = Vec::new();
        super::mint_symbolic_topology_selectors_into_values(&module, &mut values, &mut diagnostics);

        // ── Assert Cell A was minted ────────────────────────────────────────
        let top_value = values
            .get(&cell_top)
            .expect("cell_top must be present after mint pass");
        assert!(
            matches!(top_value, Value::Selector(_)),
            "cell_top must be Value::Selector after mint; got {:?}",
            top_value
        );

        // ── Assert Cell B was NOT overwritten ───────────────────────────────
        let all_faces_value = values
            .get(&cell_all_faces)
            .expect("cell_all_faces must still be present");
        match all_faces_value {
            Value::Selector(sv) => {
                assert_eq!(
                    *sv, pre_existing_selector,
                    "pre-existing selector must not be overwritten by the mint pass"
                );
            }
            other => panic!(
                "cell_all_faces must still hold the pre-existing Selector; got {:?}",
                other
            ),
        }

        assert!(diagnostics.is_empty(), "no diagnostics expected; got {:?}", diagnostics);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Step-1 (task 4651 R1a): classifier unit tests for is_geometry_consumer_call
    // ─────────────────────────────────────────────────────────────────────────

    /// Build a zero-arg `CompiledExpr::FunctionCall` node with `name`.
    ///
    /// Used by step-1 tests only — `is_geometry_consumer_call` keys on the
    /// function name, not arity.
    fn fn_call_named(name: &str) -> reify_ir::CompiledExpr {
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str(name));
        reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: name.to_string(),
                    qualified_name: format!("std::{name}"),
                },
                args: vec![],
            },
            result_type: reify_core::Type::Bool,
            content_hash,
        }
    }

    /// Classifier unit tests for `is_geometry_consumer_call` (task 4651 R1a).
    ///
    /// TRUE: typed-consumption-site FunctionCall names (geometry consumers that
    /// require a kernel and emit `EvalUnresolved` when the kernel is absent).
    ///
    /// FALSE: construction sites, kernel-free leaf selector ctors,
    /// composition/named-leaf ctors, list helpers, and non-FunctionCall exprs.
    ///
    /// RED until step-2 introduces `is_geometry_consumer_call`.
    #[test]
    fn is_geometry_consumer_call_classifier() {
        // ── TRUE: is_geometry_query_call family ───────────────────────────────
        for name in &["volume", "area", "centroid", "bounding_box"] {
            assert!(
                is_geometry_consumer_call(&fn_call_named(name)),
                "expected is_geometry_consumer_call({name}) == true (query family)"
            );
        }
        // ── TRUE: kernel-bearing TopologySelectorHelper consumers ─────────────
        for name in &[
            "adjacent_faces",
            "normal",
            "closest_point",
            "shared_edges",
            "length",
            "perimeter",
            "curvature",
            "center_of_mass",
            "moment_of_inertia",
            "distance",
            "contains",
            "intersects",
            "geo_equiv",
            // task #4759 — relational-walk v2 selectors (RED until step-4)
            "siblings_of_face",
            "ancestor_faces_of_edge",
            // task 4952 α: `angle` is dispatched via the same build()-only
            // TopologySelectorHelper path as `angle_between_surfaces` (see
            // `is_geometry_consumer_call`'s doc comment).
            "angle",
        ] {
            assert!(
                is_geometry_consumer_call(&fn_call_named(name)),
                "expected is_geometry_consumer_call({name}) == true (TopologySelectorHelper consumer)"
            );
        }

        // ── FALSE: GEOMETRY_FUNCTION_NAMES constructors ───────────────────────
        for name in &["box", "cylinder", "sphere", "cone"] {
            assert!(
                !is_geometry_consumer_call(&fn_call_named(name)),
                "expected is_geometry_consumer_call({name}) == false (constructor)"
            );
        }
        // ── FALSE: R2b kernel-free leaf selector ctors ────────────────────────
        for name in &[
            "faces",
            "edges",
            "faces_by_normal",
            "edges_by_length",
            "mid_surface",
            "vertices",
            "faces_by_area",
            "edges_parallel_to",
            "edges_at_height",
        ] {
            assert!(
                !is_geometry_consumer_call(&fn_call_named(name)),
                "expected is_geometry_consumer_call({name}) == false (R2b leaf selector ctor)"
            );
        }
        // ── FALSE: composition / named-leaf ctors ─────────────────────────────
        for name in &["union", "face", "edge", "solid_body"] {
            assert!(
                !is_geometry_consumer_call(&fn_call_named(name)),
                "expected is_geometry_consumer_call({name}) == false (composition/named-leaf ctor)"
            );
        }
        // ── FALSE: list helper ────────────────────────────────────────────────
        assert!(
            !is_geometry_consumer_call(&fn_call_named("single")),
            "expected is_geometry_consumer_call(single) == false (list helper)"
        );

        // ── FALSE: non-FunctionCall exprs ─────────────────────────────────────
        let lit = reify_ir::CompiledExpr::literal(
            reify_ir::Value::Real(1.0),
            reify_core::Type::dimensionless_scalar(),
        );
        assert!(
            !is_geometry_consumer_call(&lit),
            "expected is_geometry_consumer_call(Literal) == false"
        );
        let vref = reify_ir::CompiledExpr::value_ref(
            reify_core::ValueCellId::new("S", "x"),
            reify_core::Type::length(),
        );
        assert!(
            !is_geometry_consumer_call(&vref),
            "expected is_geometry_consumer_call(ValueRef) == false"
        );
    }

    // ── region-resolution capability gate (task #4812, P0β) ─────────────────
    //
    // These tests pin the fail-closed gate in `resolve_selector_to_list`:
    //   - A predicate/bulk selector over a non-supporting repr (Sdf/Voxel/VolumeMesh)
    //     MUST return Some(Value::Undef) + exactly ONE Error with
    //     DiagnosticCode::QueryNotSupportedOnRepr.
    //   - BRep and Mesh reprs must pass through (Value::List, no QNS error).
    //   - ByRole over Mesh must fail closed (BRepOnly capability).
    //   - Named over any repr must NOT fire QNS (un-gated, PRD §7).
    //   - An empty `realized_reprs` map must preserve today's behavior (fail-open).
    //
    // RED: `try_eval_resolve_selector` does not yet accept a `realized_reprs`
    // parameter — these tests fail to compile until step-4 adds it.

    /// Helper: build test state for a single-body `faces(b)` All-leaf resolve,
    /// returning (values, named_steps, kernel, parent_rr, expr).
    fn faces_all_resolve_setup() -> (
        reify_ir::ValueMap,
        HashMap<String, KernelHandle>,
        reify_test_support::mocks::MockGeometryKernel,
        reify_core::identity::RealizationNodeId,
        reify_ir::CompiledExpr,
    ) {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{Type, ValueCellId};
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(10);
        let parent_rr = RealizationNodeId::new("GateBody", 0);
        let parent_hash: [u8; 32] = [0xCC; 32];

        let kernel = MockGeometryKernel::new().with_extracted_faces(
            parent_handle,
            vec![GeometryHandleId(11), GeometryHandleId(12)],
        );

        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));

        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("GateBody", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );

        let inner = topology_selector_call_one_value_ref(
            "faces",
            "GateBody",
            "b",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let expr = reify_ir::CompiledExpr::resolve_selector(inner);

        (values, named_steps, kernel, parent_rr, expr)
    }

    #[test]
    fn gate_closed_faces_all_over_sdf_yields_undef_and_qns_error() {
        use reify_core::{DiagnosticCode, Severity};
        use reify_ir::ReprKind;

        let (values, named_steps, mut kernel, parent_rr, expr) = faces_all_resolve_setup();
        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();

        let mut realized_reprs = std::collections::HashMap::new();
        realized_reprs.insert(parent_rr, ReprKind::Sdf);

        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &realized_reprs,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(reify_ir::Value::Undef),
            "faces(b) over Sdf must return Undef"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "exactly one diagnostic expected; got {diagnostics:?}"
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            Severity::Error,
            "gate diagnostic must be Error severity"
        );
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::QueryNotSupportedOnRepr),
            "gate diagnostic must carry QueryNotSupportedOnRepr code"
        );
    }

    #[test]
    fn gate_closed_faces_all_over_voxel_yields_undef_and_qns_error() {
        use reify_core::{DiagnosticCode, Severity};
        use reify_ir::ReprKind;

        let (values, named_steps, mut kernel, parent_rr, expr) = faces_all_resolve_setup();
        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();

        let mut realized_reprs = std::collections::HashMap::new();
        realized_reprs.insert(parent_rr, ReprKind::Voxel);

        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &realized_reprs,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Undef), "Voxel must be fail-closed");
        assert_eq!(diagnostics.len(), 1, "exactly one QNS error; got {diagnostics:?}");
        assert_eq!(diagnostics[0].code, Some(DiagnosticCode::QueryNotSupportedOnRepr));
        assert_eq!(diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn gate_closed_faces_all_over_volume_mesh_yields_undef_and_qns_error() {
        use reify_core::{DiagnosticCode, Severity};
        use reify_ir::ReprKind;

        let (values, named_steps, mut kernel, parent_rr, expr) = faces_all_resolve_setup();
        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();

        let mut realized_reprs = std::collections::HashMap::new();
        realized_reprs.insert(parent_rr, ReprKind::VolumeMesh);

        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &realized_reprs,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Undef), "VolumeMesh must be fail-closed");
        assert_eq!(diagnostics.len(), 1, "exactly one QNS error; got {diagnostics:?}");
        assert_eq!(diagnostics[0].code, Some(DiagnosticCode::QueryNotSupportedOnRepr));
        assert_eq!(diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn gate_closed_faces_by_normal_over_sdf_yields_undef_and_qns_error() {
        use reify_core::identity::RealizationNodeId;
        use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
        use reify_ir::ReprKind;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(20);
        let parent_rr = RealizationNodeId::new("GateBodyNormal", 0);
        let parent_hash: [u8; 32] = [0xBB; 32];

        let mut kernel = MockGeometryKernel::new().with_extracted_faces(
            parent_handle,
            vec![GeometryHandleId(21)],
        );
        let mut named_steps = HashMap::new();
        named_steps.insert("b".to_string(), kh(parent_handle));
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("GateBodyNormal", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );
        // Direction arg: +Z for faces_by_normal.
        values.insert(
            ValueCellId::new("GateBodyNormal", "dir"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        // Angle-tolerance arg (1°): faces_by_normal is arity-3 (body, dir, tol).
        // The gate fires BEFORE the kernel call, so any valid tolerance works.
        values.insert(
            ValueCellId::new("GateBodyNormal", "tol"),
            reify_ir::Value::Scalar {
                si_value: 0.01_f64, // ~0.57°, a typical tolerance
                dimension: reify_core::DimensionVector::ANGLE,
            },
        );

        // Build faces_by_normal(b, dir, tol) — the gate fires BEFORE resolve, so
        // the actual tolerance value is irrelevant; any valid Angle scalar works.
        let arg_body = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("GateBodyNormal", "b"),
            Type::Geometry,
        );
        let arg_dir = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("GateBodyNormal", "dir"),
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        );
        let arg_tol = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new("GateBodyNormal", "tol"),
            reify_core::Type::Scalar {
                dimension: reify_core::DimensionVector::ANGLE,
            },
        );
        let ch = reify_core::ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
            .combine(reify_core::ContentHash::of_str("faces_by_normal"))
            .combine(arg_body.content_hash)
            .combine(arg_dir.content_hash)
            .combine(arg_tol.content_hash);
        let inner = reify_ir::CompiledExpr {
            kind: reify_ir::CompiledExprKind::FunctionCall {
                function: reify_ir::ResolvedFunction {
                    name: "faces_by_normal".to_string(),
                    qualified_name: "faces_by_normal".to_string(),
                },
                args: vec![arg_body, arg_dir, arg_tol],
            },
            result_type: Type::Selector(reify_core::ty::SelectorKind::Face),
            content_hash: ch,
        };
        let expr = reify_ir::CompiledExpr::resolve_selector(inner);

        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();

        let mut realized_reprs = std::collections::HashMap::new();
        realized_reprs.insert(parent_rr, ReprKind::Sdf);

        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &realized_reprs,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Undef), "faces_by_normal over Sdf must fail-closed");
        assert_eq!(diagnostics.len(), 1, "exactly one QNS error; got {diagnostics:?}");
        assert_eq!(diagnostics[0].code, Some(DiagnosticCode::QueryNotSupportedOnRepr));
        assert_eq!(diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn gate_open_faces_all_over_mesh_yields_list() {
        use reify_core::DiagnosticCode;
        use reify_ir::ReprKind;

        let (values, named_steps, mut kernel, parent_rr, expr) = faces_all_resolve_setup();
        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();

        let mut realized_reprs = std::collections::HashMap::new();
        realized_reprs.insert(parent_rr, ReprKind::Mesh);

        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &realized_reprs,
            &mut diagnostics,
        );

        // BRepAndMesh over Mesh → gate routes Manifold → NOT Unsupported → resolves.
        match result {
            Some(reify_ir::Value::List(_)) => {}
            other => panic!("faces(b) over Mesh must yield Value::List; got {other:?}; diags: {diagnostics:?}"),
        }
        assert!(
            !diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::QueryNotSupportedOnRepr)),
            "Mesh must not emit QNS; got {diagnostics:?}"
        );
    }

    #[test]
    fn gate_open_faces_all_over_brep_yields_list() {
        use reify_core::DiagnosticCode;
        use reify_ir::ReprKind;

        let (values, named_steps, mut kernel, parent_rr, expr) = faces_all_resolve_setup();
        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();

        let mut realized_reprs = std::collections::HashMap::new();
        realized_reprs.insert(parent_rr, ReprKind::BRep);

        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &realized_reprs,
            &mut diagnostics,
        );

        match result {
            Some(reify_ir::Value::List(_)) => {}
            other => panic!("faces(b) over BRep must yield Value::List; got {other:?}; diags: {diagnostics:?}"),
        }
        assert!(
            !diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::QueryNotSupportedOnRepr)),
            "BRep must not emit QNS; got {diagnostics:?}"
        );
    }

    #[test]
    fn gate_closed_mid_surface_over_mesh_yields_undef_and_qns_error() {
        // ByRole (mid_surface) is BRepOnly — fails closed on Mesh.
        use reify_core::identity::RealizationNodeId;
        use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
        use reify_ir::ReprKind;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(30);
        let parent_rr = RealizationNodeId::new("GateBodyRole", 0);
        let parent_hash: [u8; 32] = [0xDD; 32];

        let mut kernel = MockGeometryKernel::new();
        let mut named_steps = HashMap::new();
        named_steps.insert("body".to_string(), kh(parent_handle));
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("GateBodyRole", "body"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );

        let inner = topology_selector_call_one_value_ref(
            "mid_surface",
            "GateBodyRole",
            "body",
            Type::Geometry,
            Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let expr = reify_ir::CompiledExpr::resolve_selector(inner);

        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();

        let mut realized_reprs = std::collections::HashMap::new();
        realized_reprs.insert(parent_rr, ReprKind::Mesh);

        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &realized_reprs,
            &mut diagnostics,
        );

        assert_eq!(result, Some(reify_ir::Value::Undef), "mid_surface over Mesh must fail-closed (BRepOnly)");
        assert_eq!(diagnostics.len(), 1, "exactly one QNS error; got {diagnostics:?}");
        assert_eq!(diagnostics[0].code, Some(DiagnosticCode::QueryNotSupportedOnRepr));
        assert_eq!(diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn gate_open_named_over_sdf_preserves_topology_tag_stale_not_qns() {
        // Named leaf → region_query_capability returns None → gate skipped.
        // Existing TopologyTagStale warning path is preserved; NO QNS error.
        use reify_core::identity::RealizationNodeId;
        use reify_core::{DiagnosticCode, ValueCellId};
        use reify_ir::ReprKind;
        use reify_test_support::mocks::MockGeometryKernel;

        let parent_handle = GeometryHandleId(40);
        let parent_rr = RealizationNodeId::new("GateBodyNamed", 0);
        let parent_hash: [u8; 32] = [0xEE; 32];

        let mut kernel = MockGeometryKernel::new();
        let named_steps = HashMap::new(); // ctor doesn't use named_steps
        let mut values = reify_ir::ValueMap::new();
        values.insert(
            ValueCellId::new("GateBodyNamed", "b"),
            reify_ir::Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: Some(parent_handle),
            },
        );

        // Build: face(b, "top") — Named-leaf ctor.
        let inner = named_selector_call("face", "GateBodyNamed", "b", reify_core::ty::SelectorKind::Face, "top");
        let expr = reify_ir::CompiledExpr::resolve_selector(inner);

        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();

        let mut realized_reprs = std::collections::HashMap::new();
        realized_reprs.insert(parent_rr, ReprKind::Sdf);

        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &realized_reprs,
            &mut diagnostics,
        );

        // Named over Sdf → gate skips → existing TopologyTagStale path fires (empty list).
        // Must NOT be Some(Value::Undef) from the gate; must NOT have QNS code.
        assert!(
            !diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::QueryNotSupportedOnRepr)),
            "Named selector must never emit QNS regardless of repr; got {diagnostics:?}"
        );
        // The existing Named arm yields an empty list (interim D8 contract) + TopologyTagStale.
        match result {
            Some(reify_ir::Value::List(elems)) => {
                assert!(elems.is_empty(), "Named arm must return empty list (interim D8 contract)");
            }
            other => panic!("Named over Sdf must yield Some(Value::List([])); got {other:?}; diags: {diagnostics:?}"),
        }
    }

    #[test]
    fn gate_open_empty_realized_reprs_resolves_as_today() {
        // Unknown repr (absent from map) → gate skipped (fail-open). Resolves exactly
        // as today: MockGeometryKernel returns faces → Value::List (no QNS).
        use reify_core::DiagnosticCode;
        use reify_ir::ReprKind;

        let (values, named_steps, mut kernel, _parent_rr, expr) = faces_all_resolve_setup();
        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics = Vec::new();

        // Empty map: the target is NOT in realized_reprs.
        let realized_reprs: std::collections::HashMap<
            reify_core::identity::RealizationNodeId,
            ReprKind,
        > = std::collections::HashMap::new();

        let result = super::try_eval_resolve_selector(
            &expr,
            &named_steps,
            &values,
            &mut kernel,
            &table,
            &realized_reprs,
            &mut diagnostics,
        );

        match result {
            Some(reify_ir::Value::List(_)) => {}
            other => panic!("empty realized_reprs must fall through to Value::List; got {other:?}; diags: {diagnostics:?}"),
        }
        assert!(
            !diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::QueryNotSupportedOnRepr)),
            "empty realized_reprs must not emit QNS; got {diagnostics:?}"
        );
    }

    // ── project_handle_to_feature (task 4830, P3α) ──────────────────────────

    /// Whole-body resolution: an empty `TopologyAttributeTable` (no sub-shape
    /// entries) must fall through to the handle's realization feature (PRD D3).
    ///
    /// RED — `project_handle_to_feature` does not exist yet.
    #[test]
    fn project_handle_to_feature_whole_body_falls_through_to_realization() {
        use reify_core::identity::RealizationNodeId;

        let rr = RealizationNodeId::new("Box", 0);
        let handle_id = GeometryHandleId(1);
        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = project_handle_to_feature(Some((rr, handle_id)), &table, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_ir::Value::Feature(reify_ir::FeatureId::realization(
                "Box", 0
            ))),
            "a whole-body handle absent from the table must resolve to its \
             realization feature (PRD D3)"
        );
    }

    /// Sub-shape resolution: a handle recorded in the table must resolve to
    /// ITS recorded `feature_id`, not the parent realization's.
    ///
    /// RED — `project_handle_to_feature` does not exist yet.
    #[test]
    fn project_handle_to_feature_sub_shape_resolves_recorded_feature() {
        use reify_core::identity::RealizationNodeId;

        let rr = RealizationNodeId::new("Box", 0);
        let handle_id = GeometryHandleId(1);
        let mut table = reify_ir::TopologyAttributeTable::default();
        table.record(
            KernelHandle { kernel: KernelId::Occt, id: handle_id },
            reify_ir::TopologyAttribute {
                feature_id: reify_ir::FeatureId::realization("Fillet", 1),
                role: reify_ir::Role::Side,
                local_index: 0,
                user_label: None,
                mod_history: vec![],
            },
        );
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = project_handle_to_feature(Some((rr, handle_id)), &table, &mut diagnostics);

        assert_eq!(
            result,
            Some(reify_ir::Value::Feature(reify_ir::FeatureId::realization(
                "Fillet", 1
            ))),
            "a sub-shape handle recorded in the table must resolve to its own \
             feature_id, not the parent realization's"
        );
    }

    /// Fail-closed: the accessor's argument did not resolve to a realized
    /// `Value::GeometryHandle` (`resolved == None`). Must push exactly one
    /// error diagnostic carrying `DiagnosticCode::QueryNotSupportedOnRepr`
    /// and return `None` so the caller leaves the cell at its compiled
    /// default `Value::Undef` (OQ#2).
    ///
    /// RED — step-04's `project_handle_to_feature` returns `None` on the
    /// `None` path WITHOUT emitting a diagnostic.
    #[test]
    fn project_handle_to_feature_none_arg_is_fail_closed() {
        use reify_core::{DiagnosticCode, Severity};

        let table = reify_ir::TopologyAttributeTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        let result = project_handle_to_feature(None, &table, &mut diagnostics);

        assert_eq!(result, None, "unresolved arg must yield None so the cell stays Undef");
        assert_eq!(diagnostics.len(), 1, "exactly one diagnostic expected; got {diagnostics:?}");
        assert_eq!(diagnostics[0].code, Some(DiagnosticCode::QueryNotSupportedOnRepr));
        assert_eq!(diagnostics[0].severity, Severity::Error);
    }

    // ── Task #5120 R2c: composition (union/intersect/difference) wired onto
    // the kernel-free symbolic-eval surface ──────────────────────────────────
    //
    // Sibling to the R2b leaf-ctor tests above (tests.rs:20630+): pins
    // `try_eval_symbolic_topology_selector`'s NEW composition arms over
    // INLINE nested leaf-ctor operands (the bt2/bt3 shape —
    // `union(faces_by_normal(b,up,tol), faces_by_normal(b,down,tol))`),
    // resolving within the single kernel-free mint pass (no cross-cell
    // chaining), plus the union/intersect/difference overload
    // disambiguation against the solid-CSG boolean (non-Selector operands).
    //
    // **RED** until step-2 adds the `union`/`intersect`/`difference` arms to
    // `symbolic_eval_helper_for_name` + `try_eval_symbolic_topology_selector`
    // (via the new kernel-free `eval_variadic_composition_symbolic` /
    // `reconstruct_selector_value_symbolic` siblings).
    //
    // NOTE (amendment review, round 2): the None-overload, arity-gate, and
    // kind-mismatch cases below are triplicated per-operator on purpose, not
    // an oversight — a table-driven `for op in ["union","intersect",
    // "difference"]` helper was considered and declined. Each operator gets
    // its own independently-named `#[test]` so a regression in exactly one
    // operator's guard fails with an unambiguous test name instead of a
    // shared-helper assertion that requires reading output to learn which
    // operator broke; per-test doc-comments above each case already record
    // why that operator's coverage can't be inferred from its siblings
    // (union/intersect share the `< 2` variadic gate, difference has a
    // distinct exact-arity-2 gate; solid-CSG overload examples differ per
    // op — `manifold_boolean` vs `m5_geometry_flange`). Keep new operators
    // added to this trio symmetric with the existing three rather than
    // collapsing them.

    /// Test-setup helper (review amendment, task #5120 R2c,
    /// reviewer_comprehensive suggestion #2 — test-duplication): every
    /// composition/named-leaf test below hand-rolls the same symbolic
    /// (`kernel_handle == None`) `Value::GeometryHandle` insertion — same
    /// `RealizationNodeId::new(entity, index)` + `upstream_values_hash` +
    /// `kernel_handle: None` shape, varying only `entity`/`cell_name`/
    /// `index`/hash. Factored here so the ValueMap/handle boilerplate lives
    /// in one place while every operator/case keeps its own independently-
    /// named `#[test]` and assertions (per the NOTE above — this helper only
    /// dedupes SETUP, it does not collapse the tests). Returns the
    /// `RealizationNodeId` for the tests that assert `target.realization_ref`
    /// against it later.
    fn insert_symbolic_geometry_handle(
        values: &mut reify_ir::ValueMap,
        entity: &str,
        cell_name: &str,
        realization_index: u32,
        uvh: [u8; 32],
    ) -> reify_core::identity::RealizationNodeId {
        use reify_core::identity::{RealizationNodeId, ValueCellId};
        let rr = RealizationNodeId::new(entity, realization_index);
        values.insert(
            ValueCellId::new(entity, cell_name),
            reify_ir::Value::GeometryHandle {
                realization_ref: rr.clone(),
                upstream_values_hash: uvh,
                kernel_handle: None,
            },
        );
        rr
    }

    /// `union(faces_by_normal(b,up,tol), faces_by_normal(b,down,tol))` over a
    /// SYMBOLIC body handle (`kernel_handle == None`) must yield
    /// `Some(Value::Selector(Face))` with a `SelectorNode::Union` of two Leaf
    /// children, each with a symbolic (`kernel_handle == None`) target.
    /// Mirrors BT2 (`bt2_same_kind_union.ri`).
    #[test]
    fn try_eval_symbolic_topology_selector_union_of_inline_nested_leaves() {
        use reify_core::identity::ValueCellId;
        use reify_core::DimensionVector;
        use reify_ir::value::{LeafQuery, SelectorNode};

        let entity = "R2cUnion";
        let tol_rad = std::f64::consts::PI / 180.0;

        let mut values = reify_ir::ValueMap::new();
        let rr = insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x5Cu8; 32]);
        values.insert(
            ValueCellId::new(entity, "up"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        values.insert(
            ValueCellId::new(entity, "down"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(-1.0),
            ]),
        );
        values.insert(
            ValueCellId::new(entity, "tol"),
            reify_ir::Value::Scalar {
                si_value: tol_rad,
                dimension: DimensionVector::ANGLE,
            },
        );

        let vec3_ty = reify_core::Type::vec3(reify_core::Type::dimensionless_scalar());
        let angle_ty = reify_core::Type::angle();

        let up_leaf = symbolic_selector_call_three_value_refs(
            "faces_by_normal",
            entity,
            "body",
            reify_core::Type::Geometry,
            "up",
            vec3_ty.clone(),
            "tol",
            angle_ty.clone(),
        );
        let down_leaf = symbolic_selector_call_three_value_refs(
            "faces_by_normal",
            entity,
            "body",
            reify_core::Type::Geometry,
            "down",
            vec3_ty,
            "tol",
            angle_ty,
        );
        let union_expr = mk_symbolic_call_3523("union", vec![up_leaf, down_leaf]);

        let mut diagnostics = Vec::new();
        let result =
            super::try_eval_symbolic_topology_selector(&union_expr, &values, &mut diagnostics);

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "union(faces_by_normal(b,up,tol), faces_by_normal(b,down,tol)) over a \
                 symbolic target must yield Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(
            sv.kind,
            reify_core::ty::SelectorKind::Face,
            "union of Face selectors → Face kind"
        );
        match &sv.node {
            SelectorNode::Union(children) => {
                assert_eq!(children.len(), 2, "union of 2 operands → 2 children");
                for (i, child) in children.iter().enumerate() {
                    match &child.node {
                        SelectorNode::Leaf { target, query } => {
                            assert_eq!(
                                target.kernel_handle, None,
                                "child[{i}] target must be symbolic"
                            );
                            assert_eq!(
                                target.realization_ref, rr,
                                "child[{i}] realization_ref propagated"
                            );
                            assert!(
                                matches!(query, LeafQuery::ByNormal { .. }),
                                "child[{i}] must be a ByNormal leaf; got {:?}",
                                query
                            );
                        }
                        other => panic!("child[{i}] must be a Leaf, got {:?}", other),
                    }
                }
            }
            other => panic!("expected SelectorNode::Union, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean symbolic composition must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `intersect(faces_by_normal(b,up,tol), faces_by_normal(b,down,tol))`
    /// over a symbolic body handle must yield `Some(Value::Selector(Face))`
    /// with a `SelectorNode::Intersect` of two symbolic Leaf children.
    /// Mirrors BT3's intersect half (`bt3_difference_intersect.ri`).
    #[test]
    fn try_eval_symbolic_topology_selector_intersect_of_inline_nested_leaves() {
        use reify_core::identity::ValueCellId;
        use reify_core::DimensionVector;
        use reify_ir::value::{LeafQuery, SelectorNode};

        let entity = "R2cIntersect";
        let tol_rad = std::f64::consts::PI / 180.0;

        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x5Du8; 32]);
        values.insert(
            ValueCellId::new(entity, "up"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        values.insert(
            ValueCellId::new(entity, "down"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(-1.0),
            ]),
        );
        values.insert(
            ValueCellId::new(entity, "tol"),
            reify_ir::Value::Scalar {
                si_value: tol_rad,
                dimension: DimensionVector::ANGLE,
            },
        );

        let vec3_ty = reify_core::Type::vec3(reify_core::Type::dimensionless_scalar());
        let angle_ty = reify_core::Type::angle();

        let up_leaf = symbolic_selector_call_three_value_refs(
            "faces_by_normal",
            entity,
            "body",
            reify_core::Type::Geometry,
            "up",
            vec3_ty.clone(),
            "tol",
            angle_ty.clone(),
        );
        let down_leaf = symbolic_selector_call_three_value_refs(
            "faces_by_normal",
            entity,
            "body",
            reify_core::Type::Geometry,
            "down",
            vec3_ty,
            "tol",
            angle_ty,
        );
        let intersect_expr = mk_symbolic_call_3523("intersect", vec![up_leaf, down_leaf]);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(
            &intersect_expr,
            &values,
            &mut diagnostics,
        );

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "intersect(faces_by_normal(b,up,tol), faces_by_normal(b,down,tol)) over a \
                 symbolic target must yield Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Face);
        match &sv.node {
            SelectorNode::Intersect(children) => {
                assert_eq!(children.len(), 2, "intersect of 2 operands → 2 children");
                for (i, child) in children.iter().enumerate() {
                    match &child.node {
                        SelectorNode::Leaf { target, query } => {
                            assert_eq!(
                                target.kernel_handle, None,
                                "child[{i}] target must be symbolic"
                            );
                            assert!(
                                matches!(query, LeafQuery::ByNormal { .. }),
                                "child[{i}] must be a ByNormal leaf; got {:?}",
                                query
                            );
                        }
                        other => panic!("child[{i}] must be a Leaf, got {:?}", other),
                    }
                }
            }
            other => panic!("expected SelectorNode::Intersect, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean symbolic composition must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `difference(faces(b), faces_by_normal(b,up,tol))` over a symbolic body
    /// handle must yield `Some(Value::Selector(Face))` with a
    /// `SelectorNode::Difference(a, b)` whose minuend/subtrahend are both
    /// symbolic Leaf nodes. Mirrors BT3's difference half.
    #[test]
    fn try_eval_symbolic_topology_selector_difference_of_inline_nested_leaves() {
        use reify_core::identity::ValueCellId;
        use reify_core::DimensionVector;
        use reify_ir::value::SelectorNode;

        let entity = "R2cDifference";
        let tol_rad = std::f64::consts::PI / 180.0;

        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x5Eu8; 32]);
        values.insert(
            ValueCellId::new(entity, "up"),
            reify_ir::Value::Vector(vec![
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(0.0),
                reify_ir::Value::Real(1.0),
            ]),
        );
        values.insert(
            ValueCellId::new(entity, "tol"),
            reify_ir::Value::Scalar {
                si_value: tol_rad,
                dimension: DimensionVector::ANGLE,
            },
        );

        let faces_expr = topology_selector_call_one_value_ref(
            "faces",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let fbn_expr = symbolic_selector_call_three_value_refs(
            "faces_by_normal",
            entity,
            "body",
            reify_core::Type::Geometry,
            "up",
            reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
            "tol",
            reify_core::Type::angle(),
        );
        let diff_expr = topology_selector_composition_call("difference", faces_expr, fbn_expr);

        let mut diagnostics = Vec::new();
        let result =
            super::try_eval_symbolic_topology_selector(&diff_expr, &values, &mut diagnostics);

        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "difference(faces(b), faces_by_normal(b,up,tol)) over a symbolic target must \
                 yield Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Face);
        match &sv.node {
            SelectorNode::Difference(a, b) => {
                match &a.node {
                    SelectorNode::Leaf { target, .. } => {
                        assert_eq!(target.kernel_handle, None, "minuend target must be symbolic")
                    }
                    other => panic!("minuend must be a Leaf, got {:?}", other),
                }
                match &b.node {
                    SelectorNode::Leaf { target, .. } => {
                        assert_eq!(
                            target.kernel_handle, None,
                            "subtrahend target must be symbolic"
                        )
                    }
                    other => panic!("subtrahend must be a Leaf, got {:?}", other),
                }
            }
            other => panic!("expected SelectorNode::Difference, got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "clean symbolic composition must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// Overload disambiguation: `union(box_a, box_b)` where BOTH operands are
    /// `Value::GeometryHandle` (NOT `Value::Selector`) — the solid-CSG-boolean
    /// overload shape (e.g. `manifold_boolean`'s `union(box_a,box_b):Solid`) —
    /// must yield `None` so the mint falls through and the cell stays
    /// `Value::Undef` (`Type::Geometry`-exempt via clause 7), NOT a
    /// mis-constructed selector.
    #[test]
    fn try_eval_symbolic_topology_selector_union_returns_none_for_non_selector_operands() {
        use reify_core::identity::ValueCellId;

        let entity = "R2cSolidBoolean";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "box_a", 0, [0x01u8; 32]);
        insert_symbolic_geometry_handle(&mut values, entity, "box_b", 1, [0x02u8; 32]);

        let arg_a = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new(entity, "box_a"),
            reify_core::Type::Geometry,
        );
        let arg_b = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new(entity, "box_b"),
            reify_core::Type::Geometry,
        );
        let union_expr = mk_symbolic_call_3523("union", vec![arg_a, arg_b]);

        let mut diagnostics = Vec::new();
        let result =
            super::try_eval_symbolic_topology_selector(&union_expr, &values, &mut diagnostics);
        assert!(
            result.is_none(),
            "union(geometry, geometry) is the solid-CSG-boolean overload, not selector \
             composition — must yield None; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "overload fallthrough must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// Symmetric overload-disambiguation coverage (review amendment, task
    /// #5120 R2c): `intersect` shares the same solid-CSG-boolean-overloaded
    /// name as `union` (e.g. `manifold_boolean`'s `intersect(a,b):Solid`), so
    /// `intersect(box_a, box_b)` over two `Value::GeometryHandle` operands
    /// must ALSO fall through to `None` — pinned separately from `union`
    /// above so a future refactor cannot special-case one operator's
    /// fall-through without the other.
    #[test]
    fn try_eval_symbolic_topology_selector_intersect_returns_none_for_non_selector_operands() {
        use reify_core::identity::ValueCellId;

        let entity = "R2cSolidBooleanIntersect";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "box_a", 0, [0x01u8; 32]);
        insert_symbolic_geometry_handle(&mut values, entity, "box_b", 1, [0x02u8; 32]);

        let arg_a = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new(entity, "box_a"),
            reify_core::Type::Geometry,
        );
        let arg_b = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new(entity, "box_b"),
            reify_core::Type::Geometry,
        );
        let intersect_expr = mk_symbolic_call_3523("intersect", vec![arg_a, arg_b]);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(
            &intersect_expr,
            &values,
            &mut diagnostics,
        );
        assert!(
            result.is_none(),
            "intersect(geometry, geometry) is the solid-CSG-boolean overload, not selector \
             composition — must yield None; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "overload fallthrough must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// Symmetric overload-disambiguation coverage (review amendment, task
    /// #5120 R2c): `difference` shares the same solid-CSG-boolean-overloaded
    /// name (e.g. `m5_geometry_flange`'s `difference(body,holes):Solid`), so
    /// `difference(box_a, box_b)` over two `Value::GeometryHandle` operands
    /// must ALSO fall through to `None`.
    #[test]
    fn try_eval_symbolic_topology_selector_difference_returns_none_for_non_selector_operands() {
        use reify_core::identity::ValueCellId;

        let entity = "R2cSolidBooleanDifference";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "box_a", 0, [0x01u8; 32]);
        insert_symbolic_geometry_handle(&mut values, entity, "box_b", 1, [0x02u8; 32]);

        let arg_a = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new(entity, "box_a"),
            reify_core::Type::Geometry,
        );
        let arg_b = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new(entity, "box_b"),
            reify_core::Type::Geometry,
        );
        let difference_expr = mk_symbolic_call_3523("difference", vec![arg_a, arg_b]);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(
            &difference_expr,
            &values,
            &mut diagnostics,
        );
        assert!(
            result.is_none(),
            "difference(geometry, geometry) is the solid-CSG-boolean overload, not selector \
             composition — must yield None; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "overload fallthrough must emit no diagnostics; got {:?}",
            diagnostics
        );
    }

    /// Arity guard: `union` with a single operand (< 2, below the variadic
    /// minimum) must yield `None`.
    #[test]
    fn try_eval_symbolic_topology_selector_union_arity_below_two_returns_none() {
        let entity = "R2cUnionArity";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x03u8; 32]);
        let faces_expr = topology_selector_call_one_value_ref(
            "faces",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let union_expr = mk_symbolic_call_3523("union", vec![faces_expr]);

        let mut diagnostics = Vec::new();
        let result =
            super::try_eval_symbolic_topology_selector(&union_expr, &values, &mut diagnostics);
        assert!(
            result.is_none(),
            "union with < 2 args must yield None (arity gate); got {:?}",
            result
        );
    }

    /// Arity guard, symmetric coverage (review amendment, task #5120 R2c):
    /// `intersect` with a single operand (< 2) must ALSO yield `None` —
    /// pinned separately from `union` above so a future refactor cannot
    /// special-case one operator's arity gate without the other.
    #[test]
    fn try_eval_symbolic_topology_selector_intersect_arity_below_two_returns_none() {
        let entity = "R2cIntersectArity";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x04u8; 32]);
        let faces_expr = topology_selector_call_one_value_ref(
            "faces",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let intersect_expr = mk_symbolic_call_3523("intersect", vec![faces_expr]);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(
            &intersect_expr,
            &values,
            &mut diagnostics,
        );
        assert!(
            result.is_none(),
            "intersect with < 2 args must yield None (arity gate); got {:?}",
            result
        );
    }

    /// Arity guard (review amendment, task #5120 R2c): `difference` is gated
    /// to EXACTLY 2 operands via the `_ => args.len() != helper.expected_arity()`
    /// branch — a distinct rejection path from union/intersect's `< 2`
    /// variadic gate above. Both a 1-arg and a 3-arg `difference(...)` must
    /// yield `None` before ever reaching `selector_value_difference_pair`
    /// (whose `debug_assert_eq!` assumes exactly 2 children); pinning this
    /// guards against a future refactor accidentally routing `difference`
    /// through the variadic `>= 2` gate instead of the exact-arity one.
    #[test]
    fn try_eval_symbolic_topology_selector_difference_arity_not_two_returns_none() {
        let entity = "R2cDifferenceArity";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x05u8; 32]);
        let faces_expr = topology_selector_call_one_value_ref(
            "faces",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );

        // 1 operand — below expected_arity() == 2.
        let difference_expr_one = mk_symbolic_call_3523("difference", vec![faces_expr.clone()]);
        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(
            &difference_expr_one,
            &values,
            &mut diagnostics,
        );
        assert!(
            result.is_none(),
            "difference with 1 arg must yield None (arity gate); got {:?}",
            result
        );

        // 3 operands — above expected_arity() == 2.
        let difference_expr_three = mk_symbolic_call_3523(
            "difference",
            vec![faces_expr.clone(), faces_expr.clone(), faces_expr],
        );
        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(
            &difference_expr_three,
            &values,
            &mut diagnostics,
        );
        assert!(
            result.is_none(),
            "difference with 3 args must yield None (arity gate); got {:?}",
            result
        );
    }

    // ── Task #5120 R2c amendment: kind-closure-violation (`Err`) coverage for
    // ALL THREE composition operators ─────────────────────────────────────────
    //
    // The clean (`Ok`) path and the `None` arity/overload fall-throughs are
    // pinned above; the defensive `SelectorError::KindMismatch` backstop —
    // `Some(Value::Undef)` + a Warning diagnostic, mirroring the build-path
    // `eval_variadic_composition_kind_mismatch_yields_undef_with_warning`
    // test at :12710 — was previously untested on the symbolic-eval surface.

    /// `union(faces(b), edges(b))` over a SYMBOLIC body handle mixes Face and
    /// Edge kinds (hand-crafted IR, bypassing the compiler's
    /// `E_SELECTOR_KIND_MISMATCH`): `SelectorValue::union` must return
    /// `SelectorError::KindMismatch`, minting `Some(Value::Undef)` + exactly
    /// one Warning diagnostic naming the kind-closure violation.
    #[test]
    fn try_eval_symbolic_topology_selector_union_kind_mismatch_yields_undef_with_warning() {
        let entity = "R2cUnionKindMismatch";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0xC0u8; 32]);
        let faces_expr = topology_selector_call_one_value_ref(
            "faces",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let edges_expr = topology_selector_call_one_value_ref(
            "edges",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge),
        );
        let union_expr = mk_symbolic_call_3523("union", vec![faces_expr, edges_expr]);

        let mut diagnostics = Vec::new();
        let result =
            super::try_eval_symbolic_topology_selector(&union_expr, &values, &mut diagnostics);

        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "union(faces(b), edges(b)) kind-mismatch must yield Some(Value::Undef); got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kind-mismatch must emit exactly 1 Warning diagnostic; got {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "backstop diagnostic must be Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("kind-closure violation"),
            "backstop diagnostic must name the kind-closure violation; got {:?}",
            diagnostics[0].message
        );
    }

    /// `intersect(faces(b), edges(b))` — same kind-mismatch backstop as
    /// `union` above, pinned separately for `intersect`.
    #[test]
    fn try_eval_symbolic_topology_selector_intersect_kind_mismatch_yields_undef_with_warning() {
        let entity = "R2cIntersectKindMismatch";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0xC1u8; 32]);
        let faces_expr = topology_selector_call_one_value_ref(
            "faces",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let edges_expr = topology_selector_call_one_value_ref(
            "edges",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge),
        );
        let intersect_expr = mk_symbolic_call_3523("intersect", vec![faces_expr, edges_expr]);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(
            &intersect_expr,
            &values,
            &mut diagnostics,
        );

        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "intersect(faces(b), edges(b)) kind-mismatch must yield Some(Value::Undef); \
             got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kind-mismatch must emit exactly 1 Warning diagnostic; got {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "backstop diagnostic must be Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("kind-closure violation"),
            "backstop diagnostic must name the kind-closure violation; got {:?}",
            diagnostics[0].message
        );
    }

    /// `difference(faces(b), edges(b))` — same kind-mismatch backstop,
    /// pinned for the binary `difference` operator.
    #[test]
    fn try_eval_symbolic_topology_selector_difference_kind_mismatch_yields_undef_with_warning() {
        let entity = "R2cDifferenceKindMismatch";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0xC2u8; 32]);
        let faces_expr = topology_selector_call_one_value_ref(
            "faces",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let edges_expr = topology_selector_call_one_value_ref(
            "edges",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge),
        );
        let difference_expr =
            topology_selector_composition_call("difference", faces_expr, edges_expr);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(
            &difference_expr,
            &values,
            &mut diagnostics,
        );

        assert!(
            matches!(result, Some(reify_ir::Value::Undef)),
            "difference(faces(b), edges(b)) kind-mismatch must yield Some(Value::Undef); \
             got {:?}",
            result
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "kind-mismatch must emit exactly 1 Warning diagnostic; got {:?}",
            diagnostics
        );
        assert_eq!(
            diagnostics[0].severity,
            reify_core::Severity::Warning,
            "backstop diagnostic must be Warning severity"
        );
        assert!(
            diagnostics[0].message.contains("kind-closure violation"),
            "backstop diagnostic must name the kind-closure violation; got {:?}",
            diagnostics[0].message
        );
    }

    /// Robustness (review amendment, task #5120 R2c): a `None` return from
    /// the composition mint must be a SILENT fall-through, per the
    /// overload-disambiguation contract — a solid-CSG-boolean operand is
    /// `Value::GeometryHandle`, not `Value::Selector`, so the composition
    /// mint returns `None` and the cell stays `Undef`, `Type::Geometry`-
    /// exempt via clause 7. `union(union(faces(b), edges(b)), box_a)`: the
    /// INNER `union` is Face/Edge-kind-mismatched (hand-crafted IR) and
    /// resolves to `Some(Value::Undef)` + a Warning; back in the OUTER
    /// `union`'s operand reconstruction, `Value::Undef` is not a
    /// `Value::Selector`, so that operand fails to reconstruct and the
    /// OUTER `union` must fall through to `None` — with the inner Warning
    /// NOT leaked into the caller's `diagnostics`.
    #[test]
    fn try_eval_symbolic_topology_selector_union_none_fallthrough_no_diagnostic_leak() {
        use reify_core::identity::ValueCellId;

        let entity = "R2cUnionDiagLeak";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0xD0u8; 32]);
        insert_symbolic_geometry_handle(&mut values, entity, "box_a", 1, [0xD1u8; 32]);

        let faces_expr = topology_selector_call_one_value_ref(
            "faces",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let edges_expr = topology_selector_call_one_value_ref(
            "edges",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge),
        );
        // Malformed nested selector (kind-mismatched union), hand-crafted IR.
        let nested_union = mk_symbolic_call_3523("union", vec![faces_expr, edges_expr]);

        let box_a_ref = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new(entity, "box_a"),
            reify_core::Type::Geometry,
        );
        // Outer op shares the `union` name with the malformed nested
        // selector, but its second operand is a plain GeometryHandle — the
        // overload-disambiguation shape.
        let outer_union = mk_symbolic_call_3523("union", vec![nested_union, box_a_ref]);

        let mut diagnostics = Vec::new();
        let result =
            super::try_eval_symbolic_topology_selector(&outer_union, &values, &mut diagnostics);

        assert!(
            result.is_none(),
            "not every operand resolved to a Selector (inner union hit a kind-closure \
             violation) — outer union must fall through to None; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "a None fall-through must be silent — the inner union's kind-closure warning must \
             not leak into the caller's diagnostics; got {:?}",
            diagnostics
        );
    }

    /// Complementary coverage for the drop above (review amendment, task
    /// #5120 R2c, reviewer_comprehensive suggestion #1 — robustness): the
    /// leaked-diagnostic risk isn't limited to the failing operand itself —
    /// it must also hold once `scratch` has ALREADY accumulated a clean,
    /// fully-reconstructed nested selector from an EARLIER operand before a
    /// later operand pushes a warning and fails.
    ///
    /// `union(faces(b), union(faces(b), edges(b)))`: operand[0] (`faces(b)`)
    /// resolves via the `FunctionCall` arm of
    /// `reconstruct_selector_value_symbolic` to `Some(Value::Selector(Face))`
    /// with NO diagnostic — a real success, not just a `ValueRef` fall-
    /// through. Operand[1] is the same Face/Edge-kind-mismatched nested
    /// `union` as the test above, which pushes its kind-closure Warning into
    /// the SAME `scratch` buffer before resolving to `Value::Undef` (i.e.
    /// `None` to the outer `collect`). The outer `union` must still fall
    /// through to `None` with `diagnostics` left EMPTY, proving `scratch` is
    /// discarded wholesale on the `?` short-circuit regardless of how many
    /// operands — successful or not — contributed to it first.
    ///
    /// (A literal "operand[0] warns-but-succeeds, independent sibling fails"
    /// shape is not constructible here: every diagnostic-push in this
    /// composition machinery is paired 1:1 with that same call resolving to
    /// `Some(Value::Undef)` — i.e. `None` from `reconstruct_selector_value_
    /// symbolic`'s point of view — so a warning-emitting operand always IS
    /// the one `collect` short-circuits on. This test instead pins the
    /// realistic form of the same risk: multiple operands feeding `scratch`
    /// before the drop.)
    #[test]
    fn try_eval_symbolic_topology_selector_union_drops_scratch_after_leading_success() {
        let entity = "R2cUnionDiagLeakAfterSuccess";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0xD2u8; 32]);

        let leading_faces_expr = topology_selector_call_one_value_ref(
            "faces",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let faces_expr = topology_selector_call_one_value_ref(
            "faces",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let edges_expr = topology_selector_call_one_value_ref(
            "edges",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge),
        );
        // Malformed nested selector (kind-mismatched union), hand-crafted IR —
        // processed SECOND, after a leading operand that resolves cleanly.
        let nested_union = mk_symbolic_call_3523("union", vec![faces_expr, edges_expr]);
        let outer_union = mk_symbolic_call_3523("union", vec![leading_faces_expr, nested_union]);

        let mut diagnostics = Vec::new();
        let result =
            super::try_eval_symbolic_topology_selector(&outer_union, &values, &mut diagnostics);

        assert!(
            result.is_none(),
            "operand[1]'s kind-closure violation must still fail the outer union even though \
             operand[0] resolved cleanly first; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "a leading successful operand must not cause operand[1]'s dropped warning to leak; \
             got {:?}",
            diagnostics
        );
    }

    // ── Task #5120 R2c: named-leaf (face/edge/solid_body/vertex) wired onto
    // the kernel-free symbolic-eval surface ──────────────────────────────────
    //
    // Sibling to the composition tests above: pins
    // `try_eval_symbolic_topology_selector`'s NEW named-leaf arms over a
    // symbolic body handle, the 4583 chained-first-arg fallback (rooting a
    // Named leaf at an inline nested selector's parent GHR), and the
    // missing-target / arity-mismatch fallthroughs.
    //
    // **RED** until step-4 adds the `face`/`edge`/`solid_body`/`vertex` arms to
    // `symbolic_eval_helper_for_name` + `try_eval_symbolic_topology_selector`
    // (via the new kernel-free `eval_named_leaf_selector_ctor_symbolic` /
    // `resolve_named_leaf_target_symbolic` siblings).

    /// `face`/`edge`/`solid_body`/`vertex` (Named-leaf ctors, task 4119 δ)
    /// over a SYMBOLIC body handle (`kernel_handle == None`) must mint
    /// `Value::Selector(kind)` with `SelectorNode::Leaf { query:
    /// LeafQuery::Named(tag), .. }` and a symbolic target.
    #[test]
    fn try_eval_symbolic_topology_selector_named_leaf_ctors() {
        let entity = "R2cNamedLeaf";
        let mut values = reify_ir::ValueMap::new();
        let rr = insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x6Du8; 32]);

        let cases = [
            ("face", reify_core::ty::SelectorKind::Face),
            ("edge", reify_core::ty::SelectorKind::Edge),
            ("solid_body", reify_core::ty::SelectorKind::Body),
            ("vertex", reify_core::ty::SelectorKind::Vertex),
        ];

        for (name, want_kind) in cases {
            let expr = named_selector_call(name, entity, "body", want_kind, "tag");
            let mut diagnostics = Vec::new();
            let result =
                super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
            let sv = match result {
                Some(reify_ir::Value::Selector(sv)) => sv,
                other => panic!(
                    "{name}(body, \"tag\") over a symbolic target must yield \
                     Some(Value::Selector(..)); got {:?}; diags: {:?}",
                    other, diagnostics
                ),
            };
            assert_eq!(sv.kind, want_kind, "{name} selector kind");
            match &sv.node {
                reify_ir::value::SelectorNode::Leaf { target, query } => {
                    assert_eq!(
                        target.kernel_handle, None,
                        "{name}: symbolic target must have kernel_handle == None"
                    );
                    assert_eq!(
                        target.realization_ref, rr,
                        "{name}: realization_ref propagated"
                    );
                    assert_eq!(
                        query,
                        &reify_ir::value::LeafQuery::Named("tag".to_string()),
                        "{name}: Named(\"tag\") leaf"
                    );
                }
                other => panic!("{name}: must be a Leaf node; got {:?}", other),
            }
            assert!(
                diagnostics.is_empty(),
                "{name}: construction must emit zero diagnostics; got {:?}",
                diagnostics
            );
        }
    }

    // Independently-named siblings of the shared loop above (review
    // amendment, task #5120 R2c, reviewer_comprehensive suggestion #1 —
    // test_coverage): `face` alone got its own inline-nested/missing-target/
    // arity-mismatch tests below, symmetric with how the composition trio
    // (union/intersect/difference) each got independently-named per-operator
    // tests. `edge`/`solid_body`/`vertex` previously only had kind-mapping
    // coverage via the shared `_named_leaf_ctors` loop; a wiring regression
    // that mapped e.g. `solid_body` to the wrong `SelectorKind` would still
    // have been caught there, but as one shared-loop failure rather than an
    // unambiguous, independently-failing test name. These three close that
    // gap without touching the loop test above.

    /// `edge(body, "tag")` over a SYMBOLIC body handle must yield
    /// `Some(Value::Selector(Edge))`.
    #[test]
    fn try_eval_symbolic_topology_selector_edge_over_symbolic_target() {
        let entity = "R2cNamedLeafEdge";
        let mut values = reify_ir::ValueMap::new();
        let rr = insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x70u8; 32]);

        let expr = named_selector_call(
            "edge",
            entity,
            "body",
            reify_core::ty::SelectorKind::Edge,
            "tag",
        );
        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "edge(body, \"tag\") over a symbolic target must yield Some(Value::Selector(..)); \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Edge, "edge() → Edge kind");
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, None,
                    "symbolic target must have kernel_handle == None"
                );
                assert_eq!(target.realization_ref, rr, "realization_ref propagated");
                assert_eq!(query, &reify_ir::value::LeafQuery::Named("tag".to_string()));
            }
            other => panic!("must be a Leaf node; got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit zero diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `solid_body(body, "tag")` over a SYMBOLIC body handle must yield
    /// `Some(Value::Selector(Body))`.
    #[test]
    fn try_eval_symbolic_topology_selector_solid_body_over_symbolic_target() {
        let entity = "R2cNamedLeafSolidBody";
        let mut values = reify_ir::ValueMap::new();
        let rr = insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x71u8; 32]);

        let expr = named_selector_call(
            "solid_body",
            entity,
            "body",
            reify_core::ty::SelectorKind::Body,
            "tag",
        );
        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "solid_body(body, \"tag\") over a symbolic target must yield \
                 Some(Value::Selector(..)); got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Body, "solid_body() → Body kind");
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, None,
                    "symbolic target must have kernel_handle == None"
                );
                assert_eq!(target.realization_ref, rr, "realization_ref propagated");
                assert_eq!(query, &reify_ir::value::LeafQuery::Named("tag".to_string()));
            }
            other => panic!("must be a Leaf node; got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit zero diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `vertex(body, "tag")` over a SYMBOLIC body handle must yield
    /// `Some(Value::Selector(Vertex))`.
    #[test]
    fn try_eval_symbolic_topology_selector_vertex_over_symbolic_target() {
        let entity = "R2cNamedLeafVertex";
        let mut values = reify_ir::ValueMap::new();
        let rr = insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x72u8; 32]);

        let expr = named_selector_call(
            "vertex",
            entity,
            "body",
            reify_core::ty::SelectorKind::Vertex,
            "tag",
        );
        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "vertex(body, \"tag\") over a symbolic target must yield Some(Value::Selector(..)); \
                 got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Vertex, "vertex() → Vertex kind");
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(
                    target.kernel_handle, None,
                    "symbolic target must have kernel_handle == None"
                );
                assert_eq!(target.realization_ref, rr, "realization_ref propagated");
                assert_eq!(query, &reify_ir::value::LeafQuery::Named("tag".to_string()));
            }
            other => panic!("must be a Leaf node; got {:?}", other),
        }
        assert!(
            diagnostics.is_empty(),
            "construction must emit zero diagnostics; got {:?}",
            diagnostics
        );
    }

    /// `face(mid_surface(bodyref), "r")` — arg0 is an INLINE nested selector
    /// FunctionCall (not a ValueRef) — must resolve via the fallback path
    /// (`resolve_named_leaf_target_symbolic`'s
    /// `reconstruct_selector_value_symbolic` + `first_leaf_target`), rooting
    /// the Named leaf at body's GHR. Task #5120 R2c / the 4583
    /// chained-first-arg form.
    #[test]
    fn try_eval_symbolic_topology_selector_face_over_inline_nested_selector() {
        let entity = "R2cChainedFace";
        let mut values = reify_ir::ValueMap::new();
        let rr = insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x6Eu8; 32]);

        let mid_surface_expr = topology_selector_call_one_value_ref(
            "mid_surface",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let name_lit = reify_ir::CompiledExpr::literal(
            reify_ir::Value::String("r".to_string()),
            reify_core::Type::String,
        );
        let expr = topology_selector_composition_call("face", mid_surface_expr, name_lit);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        let sv = match result {
            Some(reify_ir::Value::Selector(sv)) => sv,
            other => panic!(
                "face(mid_surface(body), \"r\") must yield Some(Value::Selector(..)) via the \
                 fallback chained-first-arg path; got {:?}; diags: {:?}",
                other, diagnostics
            ),
        };
        assert_eq!(sv.kind, reify_core::ty::SelectorKind::Face);
        match &sv.node {
            reify_ir::value::SelectorNode::Leaf { target, query } => {
                assert_eq!(target.kernel_handle, None, "chained target must be symbolic");
                assert_eq!(target.realization_ref, rr, "leaf rooted at body's GHR");
                assert_eq!(query, &reify_ir::value::LeafQuery::Named("r".to_string()));
            }
            other => panic!("must be a Leaf node; got {:?}", other),
        }
    }

    /// `face` over a missing target cell (no entry in `values`) must yield
    /// `None` (PRD invariant #2: never partially-construct a selector).
    #[test]
    fn try_eval_symbolic_topology_selector_face_returns_none_for_missing_target() {
        let values = reify_ir::ValueMap::new(); // empty — "body" cell absent
        let expr = named_selector_call(
            "face",
            "R2cMissing",
            "body",
            reify_core::ty::SelectorKind::Face,
            "tag",
        );
        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        assert!(
            result.is_none(),
            "face over a missing target cell must yield None; got {:?}",
            result
        );
    }

    /// Arity guard: `face(body)` (1 arg, expects 2: geometry + name) → None.
    #[test]
    fn try_eval_symbolic_topology_selector_face_arity_mismatch_returns_none() {
        use reify_core::identity::ValueCellId;

        let entity = "R2cFaceArity";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x6Fu8; 32]);
        let body_ref = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new(entity, "body"),
            reify_core::Type::Geometry,
        );
        let expr = mk_symbolic_call_3523("face", vec![body_ref]);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        assert!(
            result.is_none(),
            "face with arity 1 (expects 2) must yield None; got {:?}",
            result
        );
    }

    // ── Review amendment (task #5120 R2c, reviewer_comprehensive suggestion
    // #1 — robustness): `eval_named_leaf_selector_ctor_symbolic` now buffers
    // target- and name-arg resolution into a shared `scratch`, merging into
    // the caller's `diagnostics` only once BOTH resolve (mirroring
    // `eval_variadic_composition_symbolic`'s scratch-then-merge contract).
    // The two tests below pin that a name arg which fails to resolve to a
    // `String` literal (here: an `Int` literal, which makes
    // `resolve_string_literal_arg` push its own `ArgRejection` diagnostic
    // before returning `None`) can't leak that diagnostic into the caller —
    // over BOTH target-resolution paths (primary direct handle, and the
    // fallback chained-first-arg form) so a regression on either path is
    // independently caught.

    /// `face(body, 1)` — target resolves via the PRIMARY path
    /// (`resolve_symbolic_selector_target` over a direct symbolic handle),
    /// but the name arg is an `Int`, not a `String` — must yield `None` with
    /// EMPTY diagnostics (the `ArgRejection` `resolve_string_literal_arg`
    /// pushes internally must not leak while the cell stays `Undef`;
    /// `build()`'s unbuffered, kernel-bearing helper re-emits it once the
    /// cell is actually realized).
    #[test]
    fn eval_named_leaf_selector_ctor_symbolic_drops_diagnostics_when_name_arg_unresolvable() {
        use reify_core::identity::ValueCellId;

        let entity = "R2cNamedLeafBadNamePrimary";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x73u8; 32]);
        let body_ref = reify_ir::CompiledExpr::value_ref(
            ValueCellId::new(entity, "body"),
            reify_core::Type::Geometry,
        );
        let bad_name_lit =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Int(1), reify_core::Type::Int);
        let expr = mk_symbolic_call_3523("face", vec![body_ref, bad_name_lit]);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        assert!(
            result.is_none(),
            "face(body, 1) must yield None — the name arg is not a String; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "a name-arg resolution failure must not leak resolve_string_literal_arg's own \
             diagnostic while the cell stays Undef (target resolved via the primary path); \
             got {:?}",
            diagnostics
        );
    }

    /// `face(mid_surface(body), 1)` — target resolves via the FALLBACK
    /// chained-first-arg path (`reconstruct_selector_value_symbolic` +
    /// `first_leaf_target`, the same shape as
    /// `try_eval_symbolic_topology_selector_face_over_inline_nested_selector`),
    /// but the name arg is an `Int`, not a `String` — must yield `None` with
    /// EMPTY diagnostics, exercising the exact sequence the reviewer flagged
    /// (`resolve_named_leaf_target_symbolic`'s scratch-buffered fallback
    /// succeeding before `resolve_string_literal_arg` fails).
    #[test]
    fn eval_named_leaf_selector_ctor_symbolic_drops_diagnostics_when_name_arg_unresolvable_over_chained_target(
    ) {
        let entity = "R2cNamedLeafBadNameChained";
        let mut values = reify_ir::ValueMap::new();
        insert_symbolic_geometry_handle(&mut values, entity, "body", 0, [0x74u8; 32]);

        let mid_surface_expr = topology_selector_call_one_value_ref(
            "mid_surface",
            entity,
            "body",
            reify_core::Type::Geometry,
            reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
        );
        let bad_name_lit =
            reify_ir::CompiledExpr::literal(reify_ir::Value::Int(1), reify_core::Type::Int);
        let expr = topology_selector_composition_call("face", mid_surface_expr, bad_name_lit);

        let mut diagnostics = Vec::new();
        let result = super::try_eval_symbolic_topology_selector(&expr, &values, &mut diagnostics);
        assert!(
            result.is_none(),
            "face(mid_surface(body), 1) must yield None — the name arg is not a String; got {:?}",
            result
        );
        assert!(
            diagnostics.is_empty(),
            "a name-arg resolution failure must not leak any diagnostic while the cell stays \
             Undef, even though the chained target resolved first via the fallback path; \
             got {:?}",
            diagnostics
        );
    }
