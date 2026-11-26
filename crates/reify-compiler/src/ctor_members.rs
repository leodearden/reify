//! What a structure ctor carries from its template: the one harvest shared by
//! the source-level ctor lowering (`expr.rs`) and the auto-type-param
//! synthetic ctor (`auto_type_param.rs`).

use reify_core::Type;
use reify_ir::{CompiledExpr, CtorGeometryLet};

use crate::types::{TopologyTemplate, ValueCellDecl, ValueCellKind};

pub(crate) struct CtorTemplateMembers {
    pub(crate) lets: Vec<(String, CompiledExpr)>,
    pub(crate) geometry_lets: Vec<CtorGeometryLet>,
}

/// The template's lets and geometry lets a ctor carries, in value-cell order.
///
/// `lets` holds every source-declared `Let` cell with a default, Public or
/// Private (a private helper may feed a public let), except:
/// - auto-synthesized `__count_<coll>` collection-count lets, which are
///   compiler-internal and may read sub-component values absent at ctor time;
/// - geometry-typed lets (γ, task #4954): materializing one as a field would
///   shift ctor cache identity beyond γ's ratified value-cell shape, mirroring
///   β's `cell_type != Type::Geometry` compute-input exclusion.
///
/// `geometry_lets` holds exactly those geometry lets that are backed by a
/// named, non-list realization; any other geometry let stays out of both.
pub(crate) fn ctor_template_members(template: &TopologyTemplate) -> CtorTemplateMembers {
    let source_lets = template
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Let && !vc.id.member.starts_with("__count_"));
    let (geometry_cells, non_geometry): (Vec<&ValueCellDecl>, Vec<&ValueCellDecl>) =
        source_lets.partition(|vc| vc.cell_type == Type::Geometry);
    CtorTemplateMembers {
        lets: non_geometry
            .into_iter()
            .filter_map(|vc| {
                vc.default_expr
                    .as_ref()
                    .map(|e| (vc.id.member.clone(), e.clone()))
            })
            .collect(),
        geometry_lets: geometry_cells
            .into_iter()
            .filter_map(|vc| geometry_let(template, &vc.id.member))
            .collect(),
    }
}

fn geometry_let(template: &TopologyTemplate, member: &str) -> Option<CtorGeometryLet> {
    let realization = template
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some(member) && r.list_binding.is_none())?;
    Some(CtorGeometryLet {
        member: member.to_string(),
        realization: realization.id.clone(),
        upstream_args: realization
            .operations
            .iter()
            .flat_map(|op| op.args())
            .cloned()
            .collect(),
    })
}
