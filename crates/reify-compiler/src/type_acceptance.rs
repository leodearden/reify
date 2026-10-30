#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::ty::SelectorKind;
    use reify_ir::TypeParam;

    fn enum_defs() -> Vec<EnumDef> {
        vec![
            EnumDef {
                name: "Choice".to_string(),
                variants: vec!["A".into(), "B".into()],
                doc: None,
                type_params: vec![TypeParam {
                    name: "T".to_string(),
                    bounds: vec![],
                    default: None,
                }],
            },
            EnumDef {
                name: "Hue".to_string(),
                variants: vec!["Red".into(), "Green".into()],
                doc: None,
                type_params: vec![],
            },
        ]
    }

    fn length() -> Type {
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }
    }

    fn mass() -> Type {
        Type::Scalar {
            dimension: DimensionVector::MASS,
        }
    }

    fn real() -> Type {
        Type::dimensionless_scalar()
    }

    fn list(inner: Type) -> Type {
        Type::List(Box::new(inner))
    }

    fn field(domain: Type, codomain: Type) -> Type {
        Type::Field {
            domain: Box::new(domain),
            codomain: Box::new(codomain),
        }
    }

    fn choice_of(arg: Type) -> Type {
        Type::Applied {
            name: "Choice".to_string(),
            args: vec![arg],
        }
    }

    type Row<'t> = (&'static str, Type, Type, ArgVerdict<'t>);

    fn assert_rows(rows: &[Row<'_>]) {
        let enum_defs = enum_defs();
        for (label, param, arg, expected) in rows {
            assert_eq!(
                conformance_accepts(param, arg, &enum_defs),
                *expected,
                "{label}: param '{param}' <- arg '{arg}'"
            );
        }
    }

    #[test]
    fn trait_object_param_requires_trait_conformance_for_every_arg() {
        let trait_param = || Type::TraitObject("Marker".to_string());
        let requires = ArgVerdict::RequiresTraitConformance("Marker");
        assert_rows(&[
            (
                "structure arg",
                trait_param(),
                Type::StructureRef("Thing".to_string()),
                requires,
            ),
            (
                "error arg: trait arm precedes anti-cascade",
                trait_param(),
                Type::Error,
                requires,
            ),
            ("scalar-family arg", trait_param(), Type::Int, requires),
        ]);
    }

    #[test]
    fn error_on_either_side_is_unverifiable() {
        assert_rows(&[
            (
                "error arg",
                Type::vec3(length()),
                Type::Error,
                ArgVerdict::Unverifiable,
            ),
            (
                "error param",
                Type::Error,
                Type::Int,
                ArgVerdict::Unverifiable,
            ),
        ]);
    }

    #[test]
    fn wrapper_params_leave_recursion_to_the_caller_and_reject_other_shapes() {
        assert_rows(&[
            (
                "option param",
                Type::Option(Box::new(Type::Int)),
                Type::Int,
                ArgVerdict::Unverifiable,
            ),
            (
                "list pair",
                list(Type::Int),
                list(Type::String),
                ArgVerdict::Unverifiable,
            ),
            (
                "list <- leaf",
                list(Type::Int),
                Type::Int,
                ArgVerdict::Reject,
            ),
            (
                "wrapper shape precedes the unverifiable-arg skip",
                list(Type::Int),
                Type::TypeParam("T".to_string()),
                ArgVerdict::Reject,
            ),
            (
                "map <- set",
                Type::Map(Box::new(Type::String), Box::new(Type::Int)),
                Type::Set(Box::new(Type::Int)),
                ArgVerdict::Reject,
            ),
            (
                "set pair",
                Type::Set(Box::new(Type::Int)),
                Type::Set(Box::new(Type::Int)),
                ArgVerdict::Unverifiable,
            ),
        ]);
    }

    #[test]
    fn unverifiable_arg_kinds_are_not_judged_at_leaf_params() {
        let params = [
            Type::StructureRef("A".to_string()),
            Type::vec3(length()),
            Type::point3(length()),
            Type::matrix(3, 3, length()),
            field(Type::point3(length()), real()),
            Type::Selector(SelectorKind::Face),
            Type::Int,
        ];
        let args = [
            Type::TypeParam("T".to_string()),
            Type::Geometry,
            Type::TraitObject("Marker".to_string()),
        ];
        let rows: Vec<Row<'_>> = params
            .iter()
            .flat_map(|param| {
                args.iter().map(move |arg| {
                    (
                        "unverifiable arg",
                        param.clone(),
                        arg.clone(),
                        ArgVerdict::Unverifiable,
                    )
                })
            })
            .collect();
        assert_rows(&rows);
    }

    #[test]
    fn structure_ref_is_judged_by_type_compatible() {
        let a = || Type::StructureRef("A".to_string());
        assert_rows(&[
            ("same structure", a(), a(), ArgVerdict::Accept),
            ("string arg", a(), Type::String, ArgVerdict::Reject),
        ]);
    }

    #[test]
    fn vector_checks_arity_and_quantity_slot() {
        let (length, mass, real) = (length(), mass(), real());
        assert_rows(&[
            (
                "same arity and quantity",
                Type::vec3(length.clone()),
                Type::vec3(length.clone()),
                ArgVerdict::Accept,
            ),
            (
                "other arity",
                Type::vec3(length.clone()),
                Type::vec2(length.clone()),
                ArgVerdict::Reject,
            ),
            (
                "rank-1 tensor of any n",
                Type::vec3(length.clone()),
                Type::tensor(1, 2, length.clone()),
                ArgVerdict::Accept,
            ),
            (
                "rank-2 tensor",
                Type::vec3(length.clone()),
                Type::tensor(2, 3, length.clone()),
                ArgVerdict::Reject,
            ),
            (
                "arg side tolerates a dimensionless slot",
                Type::vec3(length.clone()),
                Type::vec3(real.clone()),
                ArgVerdict::Accept,
            ),
            (
                "quantity conflict",
                Type::vec3(length.clone()),
                Type::vec3(mass.clone()),
                ArgVerdict::QuantityConflict {
                    param_q: &length,
                    arg_q: &mass,
                },
            ),
            (
                "param side is strict about a dimensionless slot",
                Type::vec3(real.clone()),
                Type::vec3(length.clone()),
                ArgVerdict::QuantityConflict {
                    param_q: &real,
                    arg_q: &length,
                },
            ),
            (
                "scalar-family arg",
                Type::vec3(length.clone()),
                Type::Int,
                ArgVerdict::Reject,
            ),
        ]);
    }

    #[test]
    fn point_checks_arity_and_quantity_slot_and_accepts_numeric_placeholders() {
        let (length, mass) = (length(), mass());
        let p3 = || Type::point3(length.clone());
        assert_rows(&[
            ("same arity and quantity", p3(), p3(), ArgVerdict::Accept),
            (
                "other arity",
                p3(),
                Type::point2(length.clone()),
                ArgVerdict::Reject,
            ),
            ("int placeholder", p3(), Type::Int, ArgVerdict::Accept),
            (
                "dimensioned scalar placeholder",
                p3(),
                mass.clone(),
                ArgVerdict::Accept,
            ),
            (
                "scalar-param placeholder",
                p3(),
                Type::ScalarParam("Q".to_string()),
                ArgVerdict::Accept,
            ),
            ("string arg", p3(), Type::String, ArgVerdict::Reject),
            (
                "quantity conflict",
                p3(),
                Type::point3(mass.clone()),
                ArgVerdict::QuantityConflict {
                    param_q: &length,
                    arg_q: &mass,
                },
            ),
        ]);
    }

    #[test]
    fn matrix_and_tensor_accept_nominal_shapes_numeric_lists_and_scalars() {
        let (length, mass) = (length(), mass());
        let m33 = || Type::matrix(3, 3, length.clone());
        assert_rows(&[
            ("same matrix", m33(), m33(), ArgVerdict::Accept),
            (
                "tensor arg",
                m33(),
                Type::tensor(2, 3, length.clone()),
                ArgVerdict::Accept,
            ),
            (
                "vector arg",
                m33(),
                Type::vec3(length.clone()),
                ArgVerdict::Accept,
            ),
            (
                "quantity conflict",
                m33(),
                Type::matrix(3, 3, mass.clone()),
                ArgVerdict::QuantityConflict {
                    param_q: &length,
                    arg_q: &mass,
                },
            ),
            (
                "numeric nested list",
                m33(),
                list(list(real())),
                ArgVerdict::Unverifiable,
            ),
            ("string list", m33(), list(Type::String), ArgVerdict::Reject),
            (
                "scalar of any dimension",
                m33(),
                mass.clone(),
                ArgVerdict::Accept,
            ),
            ("string arg", m33(), Type::String, ArgVerdict::Reject),
            (
                "rank-0 tensor <- matching scalar",
                Type::tensor(0, 3, length.clone()),
                length.clone(),
                ArgVerdict::Accept,
            ),
        ]);
    }

    #[test]
    fn field_does_not_judge_field_shaped_args_and_rejects_others() {
        let f = || field(Type::point3(length()), real());
        assert_rows(&[
            (
                "field arg",
                f(),
                field(real(), real()),
                ArgVerdict::Unverifiable,
            ),
            (
                "lambda arg",
                f(),
                Type::Function {
                    params: vec![real()],
                    return_type: Box::new(real()),
                },
                ArgVerdict::Unverifiable,
            ),
            ("string arg", f(), Type::String, ArgVerdict::Reject),
        ]);
    }

    #[test]
    fn selector_is_judged_by_type_compatible() {
        let face = || Type::Selector(SelectorKind::Face);
        assert_rows(&[
            ("same kind", face(), face(), ArgVerdict::Accept),
            (
                "other kind",
                face(),
                Type::Selector(SelectorKind::Edge),
                ArgVerdict::Reject,
            ),
            (
                "any selector <- face",
                Type::AnySelector,
                face(),
                ArgVerdict::Accept,
            ),
        ]);
    }

    #[test]
    fn geometry_param_is_not_judged() {
        assert_rows(&[(
            "int arg",
            Type::Geometry,
            Type::Int,
            ArgVerdict::Unverifiable,
        )]);
    }

    #[test]
    fn enum_ignores_generic_arguments_and_rejects_other_enums() {
        let choice = || Type::Enum("Choice".to_string());
        assert_rows(&[
            (
                "applied param <- bare arg",
                choice_of(length()),
                choice(),
                ArgVerdict::Accept,
            ),
            (
                "bare param <- applied arg",
                choice(),
                choice_of(length()),
                ArgVerdict::Accept,
            ),
            (
                "other enum",
                Type::Enum("Hue".to_string()),
                choice(),
                ArgVerdict::Reject,
            ),
            (
                "applied generic structure is unexamined",
                Type::Applied {
                    name: "Coupling".to_string(),
                    args: vec![real()],
                },
                Type::Int,
                ArgVerdict::Unverifiable,
            ),
        ]);
    }

    #[test]
    fn scalar_family_defers_scalar_params_and_uses_type_compatible() {
        let q = || Type::ScalarParam("Q".to_string());
        assert_rows(&[
            (
                "dimensioned slot <- scalar param",
                length(),
                q(),
                ArgVerdict::Unverifiable,
            ),
            (
                "dimensionless slot <- scalar param",
                real(),
                q(),
                ArgVerdict::Unverifiable,
            ),
            (
                "dimensioned slot <- int",
                length(),
                Type::Int,
                ArgVerdict::Reject,
            ),
            (
                "dimensionless slot <- int",
                real(),
                Type::Int,
                ArgVerdict::Accept,
            ),
            ("string <- int", Type::String, Type::Int, ArgVerdict::Reject),
            ("bool <- bool", Type::Bool, Type::Bool, ArgVerdict::Accept),
        ]);
    }

    #[test]
    fn unexamined_families_are_not_judged() {
        assert_rows(&[(
            "frame param",
            Type::Frame(3),
            Type::Int,
            ArgVerdict::Unverifiable,
        )]);
    }
}
