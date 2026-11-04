//! The leaf acceptance relation the conformance walker consults, beside `type_compatible`:
//! one verdict per leaf param/arg type pair. Wrapper recursion and trait conformance belong
//! to the caller. Contract: docs/prds/v0_6/compiler-declaration-passes.md D7 and
//! docs/reify-language-spec.md §4.9.

use reify_core::{DimensionVector, Type};
use reify_ir::EnumDef;

use crate::type_compat::{base_enum_name, type_compatible};

/// What [`conformance_accepts`] decides for one leaf param/arg type pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgVerdict<'t> {
    /// The arg conforms: the caller stays silent.
    Accept,
    /// The arg does not conform: the caller emits the mismatch for the param family named.
    Reject(ParamFamily),
    /// Not judged here: the caller stays silent.
    Unverifiable,
    /// The shapes agree but the quantity slots conflict: the caller emits the quantity mismatch.
    QuantityConflict { param_q: &'t Type, arg_q: &'t Type },
    /// A trait-object param: the caller checks the arg's conformance to the named trait.
    RequiresTraitConformance(&'t str),
}

/// The param family whose arm rejected the arg. A caller picks its diagnostic from this,
/// so it never re-derives the family from the param type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParamFamily {
    /// `List`, `Set` or `Map` given an arg of another shape. An `Option` param lands here
    /// only when a caller breaks [`conformance_accepts`]'s precondition.
    Wrapper,
    StructureRef,
    /// `Selector` or `AnySelector`.
    Selector,
    Vector,
    Point,
    MatrixTensor,
    Field,
    /// A declared enum, bare or applied.
    Enum,
    /// `Bool`, `Int`, `String` and `Scalar`.
    ScalarFamily,
}

/// Judges the leaf pair `arg` against `param`; `enum_defs` tells an applied enum from an
/// applied structure.
///
/// Precondition: the caller has already recursed into every [`is_wrapper_pair`]. A wrapper
/// param therefore arrives only with an arg of another shape, and is rejected.
pub(crate) fn conformance_accepts<'t>(
    param: &'t Type,
    arg: &'t Type,
    enum_defs: &[EnumDef],
) -> ArgVerdict<'t> {
    debug_assert!(
        !is_wrapper_pair(param, arg),
        "conformance_accepts judges leaf pairs: the caller recurses into '{param}' <- '{arg}'"
    );
    if let Type::TraitObject(required_trait) = param {
        return ArgVerdict::RequiresTraitConformance(required_trait);
    }
    if param.is_error() || arg.is_error() {
        return ArgVerdict::Unverifiable;
    }
    if matches!(
        param,
        Type::Option(_) | Type::List(_) | Type::Set(_) | Type::Map(..)
    ) {
        // Spec §4.9 collection rows: the arg left at a wrapper param has the wrong shape.
        return ArgVerdict::Reject(ParamFamily::Wrapper);
    }
    if arg_type_is_unverifiable(arg) {
        return ArgVerdict::Unverifiable;
    }
    match param {
        Type::StructureRef(_) => by_type_compatible(param, arg, ParamFamily::StructureRef),
        Type::Selector(_) | Type::AnySelector => {
            by_type_compatible(param, arg, ParamFamily::Selector)
        }
        Type::Vector { .. } => vector_verdict(param, arg),
        Type::Point { .. } => point_verdict(param, arg),
        Type::Matrix { .. } | Type::Tensor { .. } => matrix_tensor_verdict(param, arg),
        Type::Field { .. } => field_verdict(arg),
        // Spec §4.9 Known limitation (#8101): a Geometry field accepts any argument.
        Type::Geometry => ArgVerdict::Unverifiable,
        _ => match base_enum_name(param, enum_defs) {
            Some(param_enum) => enum_verdict(param, param_enum, arg, enum_defs),
            None => scalar_family_or_unexamined(param, arg),
        },
    }
}

/// The pairs a caller recurses into rather than judging (spec §4.9 `Option<T>` and
/// collection rows). An `Option` param checks a bare arg against `T` (implicit-Some), and
/// a `List`, `Set` or `Map` arg at a param of the same constructor is checked element-wise.
fn is_wrapper_pair(param: &Type, arg: &Type) -> bool {
    matches!(
        (param, arg),
        (Type::Option(_), _)
            | (Type::List(_), Type::List(_))
            | (Type::Set(_), Type::Set(_))
            | (Type::Map(..), Type::Map(..))
    )
}

fn vector_verdict<'t>(param: &'t Type, arg: &'t Type) -> ArgVerdict<'t> {
    match (param, arg) {
        (Type::Vector { n: param_n, .. }, Type::Vector { n: arg_n, .. }) if param_n == arg_n => {
            quantity_verdict(param, arg)
        }
        // Spec §4.9 VectorN row: a rank-1 tensor is legal whatever its element count.
        (_, Type::Tensor { rank: 1, .. }) => quantity_verdict(param, arg),
        _ => ArgVerdict::Reject(ParamFamily::Vector),
    }
}

fn point_verdict<'t>(param: &'t Type, arg: &'t Type) -> ArgVerdict<'t> {
    match (param, arg) {
        (Type::Point { n: param_n, .. }, Type::Point { n: arg_n, .. }) if param_n == arg_n => {
            quantity_verdict(param, arg)
        }
        // Spec §4.9 Known limitation (#8121): a scalar at a Point field is not rejected.
        _ if is_numeric_placeholder_leaf(arg) => ArgVerdict::Accept,
        _ => ArgVerdict::Reject(ParamFamily::Point),
    }
}

fn matrix_tensor_verdict<'t>(param: &'t Type, arg: &'t Type) -> ArgVerdict<'t> {
    match arg {
        Type::Matrix { .. } | Type::Tensor { .. } | Type::Vector { .. } => {
            quantity_verdict(param, arg)
        }
        // Nested-list erasure: Unverifiable until #9570 (PRD esc-8739-2 amendment).
        Type::List(_) if list_bottoms_out_numeric(arg) => ArgVerdict::Unverifiable,
        // Spec §4.9 Matrix/Tensor row lists "a scalar"; #8317 narrows it to a rank-0 match.
        _ if is_numeric_placeholder_leaf(arg) => ArgVerdict::Accept,
        _ => ArgVerdict::Reject(ParamFamily::MatrixTensor),
    }
}

/// Field slots erase to `Field<Real, Real>`, so a field or lambda arg is Unverifiable until
/// #9569 (PRD esc-8739-2 amendment); anything else is rejected (spec §4.9 Field row).
fn field_verdict(arg: &Type) -> ArgVerdict<'static> {
    match arg {
        Type::Field { .. } | Type::Function { .. } => ArgVerdict::Unverifiable,
        _ => ArgVerdict::Reject(ParamFamily::Field),
    }
}

/// Spec §4.9 enum row: generic arguments are not compared, so the same base enum is accepted.
fn enum_verdict(
    param: &Type,
    param_enum: &str,
    arg: &Type,
    enum_defs: &[EnumDef],
) -> ArgVerdict<'static> {
    if base_enum_name(arg, enum_defs) == Some(param_enum) {
        ArgVerdict::Accept
    } else {
        by_type_compatible(param, arg, ParamFamily::Enum)
    }
}

fn scalar_family_or_unexamined(param: &Type, arg: &Type) -> ArgVerdict<'static> {
    if scalar_param_arg_defers_at_scalar_slot(param, arg) {
        ArgVerdict::Unverifiable
    } else if general_leaf_param_family_is_validated(param) {
        by_type_compatible(param, arg, ParamFamily::ScalarFamily)
    } else {
        // Spec §4.9 Known limitation (#7958): this field type is not yet checked.
        ArgVerdict::Unverifiable
    }
}

/// The quantity-slot rule of crates/reify-core/src/ty.rs ("Point / Vector quantity-slot
/// convention"): param side strict, arg side tolerant (owner #9568 for the arg side).
fn quantity_verdict<'t>(param: &'t Type, arg: &'t Type) -> ArgVerdict<'t> {
    let (Some(param_q), Some(arg_q)) = (quantity_slot(param), quantity_slot(arg)) else {
        return ArgVerdict::Accept;
    };
    match (
        param_quantity_slot_dimension(param_q),
        arg_quantity_slot_dimension(arg_q),
    ) {
        (Some(param_dim), Some(arg_dim)) if param_dim != arg_dim => {
            ArgVerdict::QuantityConflict { param_q, arg_q }
        }
        _ => ArgVerdict::Accept,
    }
}

fn by_type_compatible(param: &Type, arg: &Type, family: ParamFamily) -> ArgVerdict<'static> {
    if type_compatible(param, arg) {
        ArgVerdict::Accept
    } else {
        ArgVerdict::Reject(family)
    }
}

/// Arg kinds whose conformance is unknown here and so never judged (spec §4.9 "skipped"
/// row): an earlier error, an unresolved generic, a geometry value, a trait-typed value.
fn arg_type_is_unverifiable(arg_ty: &Type) -> bool {
    matches!(
        arg_ty,
        Type::Error | Type::TypeParam(_) | Type::Geometry | Type::TraitObject(_)
    )
}

/// The scalar-family args the Point and Matrix/Tensor arms tolerate (#8121, #8317): `Int`,
/// any `Scalar` whatever its dimension, and a dimension-generic `ScalarParam`.
fn is_numeric_placeholder_leaf(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Scalar { .. } | Type::ScalarParam(_))
}

/// A dimension-generic `Scalar<Q>` arg at a `Scalar` slot is judged at instantiation
/// (docs/prds/v0_6/dimensioned-construction-strictness.md D4-5).
fn scalar_param_arg_defers_at_scalar_slot(param_type: &Type, arg_ty: &Type) -> bool {
    matches!(param_type, Type::Scalar { .. }) && matches!(arg_ty, Type::ScalarParam(_))
}

/// Arg side of the quantity rule, tolerant: a dimensionless `Scalar` names no dimension
/// (crates/reify-core/src/ty.rs, "Point / Vector quantity-slot convention").
fn arg_quantity_slot_dimension(quantity: &Type) -> Option<DimensionVector> {
    match quantity {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Some(*dimension),
        _ => None,
    }
}

/// Param side of the quantity rule, strict: every `Scalar` names its dimension
/// (crates/reify-core/src/ty.rs, "Point / Vector quantity-slot convention").
fn param_quantity_slot_dimension(quantity: &Type) -> Option<DimensionVector> {
    match quantity {
        Type::Scalar { dimension } => Some(*dimension),
        _ => None,
    }
}

/// The quantity slot of a Vector, Point, Matrix or Tensor; `None` for every other shape.
fn quantity_slot(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Vector { quantity, .. }
        | Type::Point { quantity, .. }
        | Type::Matrix { quantity, .. }
        | Type::Tensor { quantity, .. } => Some(quantity),
        _ => None,
    }
}

/// Whether a `List` arg is plausibly a nested numeric matrix literal; unknown (`Error`,
/// `TypeParam`) elements count as numeric.
fn list_bottoms_out_numeric(ty: &Type) -> bool {
    match ty {
        Type::List(inner) => list_bottoms_out_numeric(inner),
        Type::Vector { .. } | Type::Matrix { .. } | Type::Tensor { .. } => true,
        Type::Error | Type::TypeParam(_) => true,
        other => is_numeric_placeholder_leaf(other),
    }
}

/// The scalar-family params judged by `type_compatible` (spec §4.9 scalar and
/// `String` / `Int` / `Bool` rows).
fn general_leaf_param_family_is_validated(param_type: &Type) -> bool {
    matches!(
        param_type,
        Type::Bool | Type::Int | Type::String | Type::Scalar { .. }
    )
}

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
    fn collection_params_reject_an_arg_of_another_shape() {
        let wrapper = ArgVerdict::Reject(ParamFamily::Wrapper);
        assert_rows(&[
            ("list <- leaf", list(Type::Int), Type::Int, wrapper),
            (
                "wrapper shape precedes the unverifiable-arg skip",
                list(Type::Int),
                Type::TypeParam("T".to_string()),
                wrapper,
            ),
            (
                "map <- set",
                Type::Map(Box::new(Type::String), Box::new(Type::Int)),
                Type::Set(Box::new(Type::Int)),
                wrapper,
            ),
            (
                "set <- list",
                Type::Set(Box::new(Type::Int)),
                list(Type::Int),
                wrapper,
            ),
        ]);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "judges leaf pairs")]
    fn an_option_param_is_the_callers_to_recurse_into() {
        conformance_accepts(&Type::Option(Box::new(Type::Int)), &Type::Int, &enum_defs());
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "judges leaf pairs")]
    fn a_same_constructor_collection_pair_is_the_callers_to_recurse_into() {
        conformance_accepts(&list(Type::Int), &list(Type::String), &enum_defs());
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
            (
                "string arg",
                a(),
                Type::String,
                ArgVerdict::Reject(ParamFamily::StructureRef),
            ),
        ]);
    }

    #[test]
    fn vector_checks_arity_and_quantity_slot() {
        let (length, mass, real) = (length(), mass(), real());
        let reject = ArgVerdict::Reject(ParamFamily::Vector);
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
                reject,
            ),
            (
                "rank-1 tensor of any n",
                Type::vec3(length.clone()),
                Type::tensor(1, 2, length.clone()),
                ArgVerdict::Accept,
            ),
            (
                "rank-1 tensor quantity conflict",
                Type::vec3(length.clone()),
                Type::tensor(1, 2, mass.clone()),
                ArgVerdict::QuantityConflict {
                    param_q: &length,
                    arg_q: &mass,
                },
            ),
            (
                "rank-2 tensor",
                Type::vec3(length.clone()),
                Type::tensor(2, 3, length.clone()),
                reject,
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
                reject,
            ),
        ]);
    }

    #[test]
    fn point_checks_arity_and_quantity_slot_and_accepts_numeric_placeholders() {
        let (length, mass) = (length(), mass());
        let p3 = || Type::point3(length.clone());
        let reject = ArgVerdict::Reject(ParamFamily::Point);
        assert_rows(&[
            ("same arity and quantity", p3(), p3(), ArgVerdict::Accept),
            ("other arity", p3(), Type::point2(length.clone()), reject),
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
            ("string arg", p3(), Type::String, reject),
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
        let reject = ArgVerdict::Reject(ParamFamily::MatrixTensor);
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
            ("string list", m33(), list(Type::String), reject),
            (
                "scalar of any dimension",
                m33(),
                mass.clone(),
                ArgVerdict::Accept,
            ),
            ("string arg", m33(), Type::String, reject),
            (
                "point arg: a quantity slot is not a nominal shape",
                m33(),
                Type::point3(length.clone()),
                reject,
            ),
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
            (
                "string arg",
                f(),
                Type::String,
                ArgVerdict::Reject(ParamFamily::Field),
            ),
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
                ArgVerdict::Reject(ParamFamily::Selector),
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
                ArgVerdict::Reject(ParamFamily::Enum),
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
        let reject = ArgVerdict::Reject(ParamFamily::ScalarFamily);
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
            ("dimensioned slot <- int", length(), Type::Int, reject),
            (
                "dimensionless slot <- int",
                real(),
                Type::Int,
                ArgVerdict::Accept,
            ),
            ("string <- int", Type::String, Type::Int, reject),
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
