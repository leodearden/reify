//! Compiler → doc-model lowering pass.
//!
//! This module provides [`build_doc_model`], which walks a [`reify_compiler::CompiledModule`]
//! and produces a [`reify_doc::model::DocModel`] that the doc formatters can render.
//!
//! **Design contract** (established by plan.json step-2 decisions):
//! - The function is total/infallible.
//! - Annotation/expression rendering is best-effort string production.
//! - `source: &str` is the raw source text; constraint `expr_repr` is obtained
//!   by slicing it with `CompiledConstraint.span`; `line` is computed via
//!   `reify_types::byte_offset_to_line_col`.

use reify_compiler::{
    CompiledConstraint, CompiledConstraintDef, CompiledField, CompiledFunction, CompiledModule,
    CompiledPurpose, CompiledTrait, CompiledTypeAlias, CompiledUnit, EntityKind, RealizationDecl,
    SubComponentDecl, TopologyTemplate, ValueCellDecl, ValueCellKind, Visibility,
};
use reify_doc::model::{
    AnnotationDoc, ConstraintDoc, DocModel, ItemDoc, ItemHeader, ItemKind, ModuleDoc, ParamDoc,
    PortDoc, PragmaDoc, RealizationDoc, SubComponentDoc,
};
use reify_core::{SourceSpan, Type, byte_offset_to_line_col};
use reify_ir::{AnnotationArg, AnnotationArgValue, ObjectiveSense};
use reify_ast::{Pragma, PragmaArg, PragmaValue};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Lower a compiled module into a documentation model.
///
/// `source` is the raw source text that was parsed and compiled into `compiled`.
/// It is used to slice constraint expressions out as their `expr_repr` and to
/// compute 1-indexed `line` numbers from `SourceSpan` byte offsets.
///
/// The returned `DocModel` contains a single `ModuleDoc` whose items appear in
/// a deterministic, per-surface-grouped order:
/// 1. Structures (in declaration order)
/// 2. Occurrences (in declaration order)
/// 3. Traits
/// 4. Functions
/// 5. Fields
/// 6. Purposes
/// 7. Enums
/// 8. Units
/// 9. Type aliases
/// 10. Constraint definitions
pub fn build_doc_model(compiled: &CompiledModule, source: &str) -> DocModel {
    let mut items: Vec<ItemDoc> = Vec::new();

    // 1. Structures and Occurrences from compiled.templates.
    for t in &compiled.templates {
        items.push(lower_template(t, source));
    }

    // 2. Traits.
    for t in &compiled.trait_defs {
        items.push(lower_trait(t));
    }

    // 3. Functions.
    for f in &compiled.functions {
        items.push(lower_function(f));
    }

    // 4. Fields (module-level let / field declarations).
    for field in &compiled.fields {
        items.push(lower_field(field));
    }

    // 5. Purposes.
    for p in &compiled.compiled_purposes {
        items.push(lower_purpose(p, source));
    }

    // 6. Enums.
    for e in &compiled.enum_defs {
        items.push(lower_enum(e));
    }

    // 7. Units.
    for u in &compiled.units {
        items.push(lower_unit(u));
    }

    // 8. Type aliases.
    for a in &compiled.type_aliases {
        items.push(lower_type_alias(a));
    }

    // 9. Constraint definitions.
    for cd in &compiled.constraint_defs {
        items.push(lower_constraint_def(cd, source));
    }

    let module = ModuleDoc {
        path: compiled.path.to_string(),
        doc: None,
        items,
        annotations: lower_module_annotations(&compiled.pragmas),
        pragmas: lower_module_pragmas(&compiled.pragmas),
        cross_refs: Default::default(),
    };

    DocModel {
        modules: vec![module],
    }
}

// ---------------------------------------------------------------------------
// Template → Structure / Occurrence
// ---------------------------------------------------------------------------

fn lower_template(t: &TopologyTemplate, source: &str) -> ItemDoc {
    let params: Vec<ParamDoc> = t
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .map(|vc| lower_param(vc, source))
        .collect();

    let ports: Vec<PortDoc> = t.ports.iter().map(lower_port).collect();

    let constraints: Vec<ConstraintDoc> = t
        .constraints
        .iter()
        .map(|c| lower_constraint(c, source))
        .collect();

    let sub_components: Vec<SubComponentDoc> = t.sub_components.iter().map(lower_sub).collect();

    let realizations: Vec<RealizationDoc> = t.realizations.iter().map(lower_realization).collect();

    let meta: Vec<(String, String)> = t
        .meta
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let kind = match t.entity_kind {
        EntityKind::Structure => ItemKind::Structure {
            params,
            ports,
            constraints,
            sub_components,
            realizations,
            meta,
        },
        EntityKind::Occurrence => ItemKind::Occurrence {
            params,
            ports,
            constraints,
            sub_components,
            realizations,
            meta,
        },
    };

    ItemDoc {
        header: ItemHeader {
            name: t.name.clone(),
            doc: t.doc.clone(),
            is_pub: matches!(t.visibility, Visibility::Public),
            annotations: lower_annotations(&t.annotations),
            pragmas: lower_pragmas(&t.pragmas),
        },
        kind,
    }
}

// ---------------------------------------------------------------------------
// ValueCellDecl → ParamDoc
// ---------------------------------------------------------------------------

fn lower_param(vc: &ValueCellDecl, source: &str) -> ParamDoc {
    // Derive the local name from the ValueCellId. The cell ID format is
    // typically "<entity>.<param_name>" or just "<param_name>" for module-level
    // cells. We take the part after the last dot.
    let name = cell_local_name(&vc.id.to_string());

    let type_repr = type_to_string(&vc.cell_type);

    // Extract the default expression text from the full declaration span.
    //
    // `ValueCellDecl.span` covers the entire declaration statement, e.g.
    // "param width: Scalar = 10mm".  `CompiledExpr` has no source span of its
    // own, so we derive the default value text by splitting at the first '='
    // separator and trimming the RHS.  This yields the actual default token(s)
    // (e.g. "10mm") rather than the full declaration string.  When no '=' is
    // found (prelude-sentinel span or other malformed span), we return the safe
    // placeholder "<default>" to indicate presence-without-text.
    //
    // `default_repr` is `Some` iff `default_expr.is_some()`: the field is
    // present when-and-only-when the param has a declared default.
    let default_repr = vc
        .default_expr
        .as_ref()
        .map(|_expr| {
            let decl_text = span_text(source, vc.span);
            if let Some(pos) = decl_text.find('=') {
                decl_text[pos + 1..].trim().to_string()
            } else {
                "<default>".to_string()
            }
        });

    // Annotations on value cells are not carried through CompiledModule; they are
    // consumed/validated during compilation. We leave annotations empty.
    ParamDoc {
        name,
        doc: None,
        type_repr,
        default_repr,
        annotations: vec![],
    }
}

// ---------------------------------------------------------------------------
// CompiledPort → PortDoc
// ---------------------------------------------------------------------------

fn lower_port(p: &reify_compiler::CompiledPort) -> PortDoc {
    use reify_core::PortDirection;
    let direction = match p.direction {
        PortDirection::In => "in",
        PortDirection::Out => "out",
        PortDirection::Bidi => "inout",
    };
    let members: Vec<String> = p.members.iter().map(|m| cell_local_name(&m.id.to_string())).collect();
    PortDoc {
        name: p.name.clone(),
        direction: direction.to_string(),
        type_name: p.type_name.clone(),
        members,
    }
}

// ---------------------------------------------------------------------------
// CompiledConstraint → ConstraintDoc
// ---------------------------------------------------------------------------

fn lower_constraint(c: &CompiledConstraint, source: &str) -> ConstraintDoc {
    let expr_repr = span_text(source, c.span).to_string();
    // Guard both the prelude sentinel (u32::MAX) AND any non-sentinel span whose
    // start offset exceeds source.len() (e.g. a span seeded from a different
    // compilation unit).  byte_offset_to_line_col contains debug_assert!(offset
    // <= source.len()), so calling it with an out-of-range offset panics in
    // debug/test builds.
    let line = if c.span.is_prelude() || c.span.start as usize > source.len() {
        None
    } else {
        let (line_num, _col) = byte_offset_to_line_col(source, c.span.start as usize);
        Some(line_num as u32)
    };
    ConstraintDoc {
        label: c.label.clone(),
        expr_repr,
        annotations: vec![],
        line,
    }
}

// ---------------------------------------------------------------------------
// SubComponentDecl → SubComponentDoc
// ---------------------------------------------------------------------------

fn lower_sub(s: &SubComponentDecl) -> SubComponentDoc {
    // Render arg expressions as "<name> = <value>" strings. Since CompiledExpr
    // has no Display impl, we emit "<name> = ..." as a best-effort placeholder.
    let args: Vec<String> = s
        .args
        .iter()
        .map(|(name, _expr)| format!("{name} = ..."))
        .collect();
    SubComponentDoc {
        name: s.name.clone(),
        structure_name: s.structure_name.clone(),
        args,
        annotations: vec![],
    }
}

// ---------------------------------------------------------------------------
// RealizationDecl → RealizationDoc
// ---------------------------------------------------------------------------

fn lower_realization(r: &RealizationDecl) -> RealizationDoc {
    let name = r.name.clone().unwrap_or_else(|| "<realization>".to_string());
    let op_summaries: Vec<String> = r
        .operations
        .iter()
        .map(|op| format!("{op:?}").chars().take(80).collect())
        .collect();
    RealizationDoc { name, op_summaries }
}

// ---------------------------------------------------------------------------
// CompiledTrait → ItemKind::Trait
// ---------------------------------------------------------------------------

fn lower_trait(t: &CompiledTrait) -> ItemDoc {
    let members: Vec<String> = t
        .required_members
        .iter()
        .map(|req| {
            use reify_compiler::RequirementKind;
            match &req.kind {
                RequirementKind::Param(ty) => format!("param {}: {}", req.name, type_to_string(ty)),
                RequirementKind::Let(ty) => format!("let {}: {}", req.name, type_to_string(ty)),
                RequirementKind::Sub(sname) => format!("sub {} = {}", req.name, sname),
                RequirementKind::Fn(sig) => {
                    // task 3939 δ: render the associated-function signature.
                    let params = std::iter::once(sig.has_self.then(|| "self".to_string()))
                        .flatten()
                        .chain(sig.params.iter().map(type_to_string))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("fn {}({}) -> {}", req.name, params, type_to_string(&sig.return_type))
                }
                // task 3972 ιβ: render an associated-type requirement.
                RequirementKind::AssocType(_) => format!("type {}", req.name),
            }
        })
        .collect();

    ItemDoc {
        header: ItemHeader {
            name: t.name.clone(),
            doc: t.doc.clone(),
            is_pub: t.is_pub,
            annotations: lower_annotations(&t.annotations),
            pragmas: lower_pragmas(&t.pragmas),
        },
        kind: ItemKind::Trait { members },
    }
}

// ---------------------------------------------------------------------------
// CompiledFunction → ItemKind::Function
// ---------------------------------------------------------------------------

fn lower_function(f: &CompiledFunction) -> ItemDoc {
    let params_str: Vec<String> = f
        .params
        .iter()
        .map(|(name, ty)| format!("{name}: {}", type_to_string(ty)))
        .collect();
    let signature = format!(
        "fn {}({}) -> {}",
        f.name,
        params_str.join(", "),
        type_to_string(&f.return_type)
    );

    ItemDoc {
        header: ItemHeader {
            name: f.name.clone(),
            doc: f.doc.clone(),
            is_pub: f.is_pub,
            annotations: lower_annotations(&f.annotations),
            pragmas: vec![],
        },
        kind: ItemKind::Function { signature },
    }
}

// ---------------------------------------------------------------------------
// CompiledField → ItemKind::Field
// ---------------------------------------------------------------------------

fn lower_field(field: &CompiledField) -> ItemDoc {
    let type_repr = format!(
        "Field<{}, {}>",
        type_to_string(&field.domain_type),
        type_to_string(&field.codomain_type)
    );

    ItemDoc {
        header: ItemHeader {
            name: field.name.clone(),
            doc: None,
            is_pub: field.is_pub,
            annotations: lower_annotations(&field.annotations),
            pragmas: vec![],
        },
        kind: ItemKind::Field {
            type_repr,
            default_repr: None,
        },
    }
}

// ---------------------------------------------------------------------------
// CompiledPurpose → ItemKind::Purpose
// ---------------------------------------------------------------------------

fn lower_purpose(p: &CompiledPurpose, source: &str) -> ItemDoc {
    let (expr_repr, direction) = match &p.objective {
        Some(obj) => {
            // `CompiledExpr` carries no source span, so we cannot slice the
            // objective expression text from `source` the way constraints do.
            // We emit a clean directive placeholder rather than the unreadable
            // Rust Debug output that `format!("{:?}", obj)` would produce.
            // Direction is determined by the first term's sense.
            if obj.terms.first().map(|t| t.sense) == Some(ObjectiveSense::Maximize) {
                ("<maximize>".to_string(), "maximize".to_string())
            } else {
                ("<minimize>".to_string(), "minimize".to_string())
            }
        }
        None => {
            // Fall back to first constraint expression if available.
            let expr = p
                .constraints
                .first()
                .map(|c| span_text(source, c.span).to_string())
                .unwrap_or_default();
            (expr, "minimize".to_string())
        }
    };

    ItemDoc {
        header: ItemHeader {
            name: p.name.clone(),
            doc: None,
            is_pub: p.is_pub,
            annotations: lower_annotations(&p.annotations),
            pragmas: lower_pragmas(&p.pragmas),
        },
        kind: ItemKind::Purpose {
            expr_repr,
            direction,
        },
    }
}

// ---------------------------------------------------------------------------
// EnumDef → ItemKind::Enum
// ---------------------------------------------------------------------------

fn lower_enum(e: &reify_ir::EnumDef) -> ItemDoc {
    ItemDoc {
        header: ItemHeader {
            name: e.name.clone(),
            doc: e.doc.clone(),
            is_pub: true, // EnumDef has no is_pub field; module-level enums are public by default.
            annotations: vec![],
            pragmas: vec![],
        },
        kind: ItemKind::Enum {
            variants: e.variants.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// CompiledUnit → ItemKind::Unit
// ---------------------------------------------------------------------------

fn lower_unit(u: &CompiledUnit) -> ItemDoc {
    // The base unit is derived from the DimensionVector. For a human-readable
    // representation we format the dimension.
    let base_unit = format!("{}", u.dimension);
    let scale = format!("{}", u.factor);

    ItemDoc {
        header: ItemHeader {
            name: u.name.clone(),
            doc: None,
            is_pub: u.is_pub,
            annotations: vec![],
            pragmas: vec![],
        },
        kind: ItemKind::Unit { base_unit, scale },
    }
}

// ---------------------------------------------------------------------------
// CompiledTypeAlias → ItemKind::TypeAlias
// ---------------------------------------------------------------------------

fn lower_type_alias(a: &CompiledTypeAlias) -> ItemDoc {
    let type_repr = a
        .resolved_type
        .as_ref()
        .map(type_to_string)
        .unwrap_or_else(|| "<parameterized>".to_string());

    ItemDoc {
        header: ItemHeader {
            name: a.name.clone(),
            doc: None,
            is_pub: a.is_pub,
            annotations: vec![],
            pragmas: vec![],
        },
        kind: ItemKind::TypeAlias { type_repr },
    }
}

// ---------------------------------------------------------------------------
// CompiledConstraintDef → ItemKind::ConstraintDef
// ---------------------------------------------------------------------------

fn lower_constraint_def(cd: &CompiledConstraintDef, source: &str) -> ItemDoc {
    // The expr_repr is derived from the constraint def's span-sliced source text.
    let expr_repr = span_text(source, cd.span).to_string();

    ItemDoc {
        header: ItemHeader {
            name: cd.name.clone(),
            doc: None,
            is_pub: cd.is_pub,
            annotations: lower_annotations(&cd.annotations),
            pragmas: lower_pragmas(&cd.pragmas),
        },
        kind: ItemKind::ConstraintDef { expr_repr },
    }
}

// ---------------------------------------------------------------------------
// Annotation / Pragma rendering
// ---------------------------------------------------------------------------

/// Lower a `reify_types::Annotation` to an `AnnotationDoc`.
fn lower_annotation(ann: &reify_ir::annotation::Annotation) -> AnnotationDoc {
    let args: Vec<String> = ann.args.iter().map(render_annotation_arg).collect();
    AnnotationDoc {
        name: ann.name.clone(),
        args,
    }
}

fn lower_annotations(anns: &[reify_ir::annotation::Annotation]) -> Vec<AnnotationDoc> {
    anns.iter().map(lower_annotation).collect()
}

/// Render a single `AnnotationArg` to a printable string.
///
/// Named args render as `name = value`; positional args render the value alone.
fn render_annotation_arg(arg: &AnnotationArg) -> String {
    let value = render_annotation_arg_value(&arg.value);
    match &arg.name {
        Some(name) => format!("{name} = {value}"),
        None => value,
    }
}

/// Render an [`AnnotationArgValue`] to a printable string.
fn render_annotation_arg_value(value: &AnnotationArgValue) -> String {
    match value {
        AnnotationArgValue::String(s) => format!("\"{s}\""),
        AnnotationArgValue::Int(i) => format!("{i}"),
        AnnotationArgValue::Real(r) => format!("{r}"),
        AnnotationArgValue::Bool(b) => format!("{b}"),
        AnnotationArgValue::Ident(s) => s.clone(),
        // Unevaluated expression (task 3555). The parsed `Expr` has no source
        // round-trip Display, so render a placeholder until a doc-side
        // pretty-printer lands. Expr-valued args only arise on
        // materialization-time-eval annotations (structure/occurrence hosts).
        AnnotationArgValue::Expr(_) => "<expr>".to_string(),
    }
}

/// Lower a `reify_syntax::Pragma` to a `PragmaDoc`.
fn lower_pragma(p: &Pragma) -> PragmaDoc {
    let args: Vec<String> = p.args.iter().map(render_pragma_arg).collect();
    PragmaDoc {
        name: p.name.clone(),
        args,
    }
}

fn lower_pragmas(pragmas: &[Pragma]) -> Vec<PragmaDoc> {
    pragmas.iter().map(lower_pragma).collect()
}

/// Render a single `PragmaArg` to a printable string.
fn render_pragma_arg(arg: &PragmaArg) -> String {
    match arg {
        PragmaArg::Bare(v) => render_pragma_value(v),
        PragmaArg::KeyValue { key, value } => format!("{key}={}", render_pragma_value(value)),
    }
}

fn render_pragma_value(v: &PragmaValue) -> String {
    match v {
        PragmaValue::Ident(s) => s.clone(),
        PragmaValue::Number(n) => format!("{n}"),
        PragmaValue::String(s) => format!("\"{s}\""),
        PragmaValue::Bool(b) => format!("{b}"),
        PragmaValue::Quantity { value, unit } => format!("{value}{unit}"),
    }
}

/// Module-level annotations come from the pragmas list (pragmas and annotations
/// are stored differently at module level). There is no `module.annotations` in
/// CompiledModule, so we return empty.
fn lower_module_annotations(_pragmas: &[Pragma]) -> Vec<AnnotationDoc> {
    vec![]
}

/// Module-level pragmas.
fn lower_module_pragmas(pragmas: &[Pragma]) -> Vec<PragmaDoc> {
    lower_pragmas(pragmas)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Render a `Type` to a human-readable string.
fn type_to_string(ty: &Type) -> String {
    format!("{ty}")
}

/// Slice `source` at the byte offsets of `span`. Returns an empty string if
/// the span is a prelude sentinel, out of bounds, or malformed.
///
/// This function is **infallible**: it never panics, even on malformed spans
/// with `end < start` (which `SourceSpan::new` only `debug_assert!`s against),
/// or on spans that land on a non-UTF-8 char boundary (multibyte source).
fn span_text(source: &str, span: SourceSpan) -> &str {
    if span.is_prelude() {
        return "";
    }
    let start = span.start as usize;
    let end = (span.end as usize).min(source.len());
    // Guards: start >= source.len(), malformed end < start, or zero-length span.
    if start >= end {
        return "";
    }
    // Use str::get rather than direct indexing so that a span landing on a
    // non-UTF-8 char boundary (multibyte source) returns "" instead of panicking.
    source.get(start..end).unwrap_or("")
}

/// Extract the local (rightmost) component of a cell id like `"Entity.param_name"`.
fn cell_local_name(id_str: &str) -> String {
    id_str
        .rsplit('.')
        .next()
        .unwrap_or(id_str)
        .to_string()
}
