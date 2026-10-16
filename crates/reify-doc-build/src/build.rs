//! Compiler → doc-model lowering pass.
//!
//! This module provides [`build_doc_model`], which walks a [`reify_compiler::CompiledModule`]
//! and produces a [`reify_doc::model::DocModel`] that the doc formatters can render.
//!
//! **Design contract:**
//! - The lowering is total/infallible.
//! - Every expression-valued field takes its text from one of two helpers over
//!   the module's `source`: `expr_text_at` slices a node's own span, and
//!   `declared_value_text` takes the value after the first `=` of a
//!   declaration span.  A field whose IR node is only a `CompiledExpr`, which
//!   records no span, is `ExprText::Unavailable(UnavailableReason::NoSpan)`.
//! - Constraint `line` numbers come from `reify_core::byte_offset_to_line_col`.

use reify_compiler::{
    CompiledConstraint, CompiledConstraintDef, CompiledField, CompiledFunction, CompiledModule,
    CompiledPurpose, CompiledTrait, CompiledTypeAlias, CompiledUnit, EntityKind, RealizationDecl,
    SubComponentDecl, TopologyTemplate, ValueCellDecl, ValueCellKind, Visibility,
};
use reify_doc::expr_text::{ExprText, UnavailableReason};
use reify_doc::model::{
    AnnotationDoc, ConstraintDoc, DocModel, ItemDoc, ItemHeader, ItemKind, ModuleDoc, ParamDoc,
    PortDoc, PragmaDoc, RealizationDoc, SubComponentArgDoc, SubComponentDoc,
};
use reify_core::{ModulePath, SourceSpan, Type, byte_offset_to_line_col};
use reify_ir::{AnnotationArg, AnnotationArgValue, ObjectiveSense};
use reify_ast::{Pragma, PragmaArg, PragmaValue};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Lower a compiled module into a documentation model.
///
/// `source` is the raw source text that was parsed and compiled into `compiled`.
/// Every expression-valued field's text is sliced out of it by span, and
/// constraint `line` numbers are computed from its byte offsets.  A span that
/// does not address `source` yields `UnavailableReason::SpanOutsideSource`.
///
/// The returned `DocModel` contains a single `ModuleDoc` whose items appear in
/// a deterministic, per-surface-grouped order:
/// 1. Structures and Occurrences, interleaved in declaration order (one pass
///    over `compiled.templates`)
/// 2. Traits
/// 3. Functions
/// 4. Fields
/// 5. Purposes
/// 6. Enums
/// 7. Units
/// 8. Type aliases
/// 9. Constraint definitions
pub fn build_doc_model(compiled: &CompiledModule, source: &str) -> DocModel {
    let mut items: Vec<ItemDoc> = Vec::new();

    // 1. Structures and Occurrences from compiled.templates.
    for t in &compiled.templates {
        items.push(lower_template(t, source));
    }

    // 2. Traits.
    for t in &compiled.trait_defs {
        items.push(lower_trait(t, source));
    }

    // 3. Functions.
    for f in &compiled.functions {
        items.push(lower_function(f, source));
    }

    // 4. Fields (module-level let / field declarations).
    for field in &compiled.fields {
        items.push(lower_field(field, source));
    }

    // 5. Purposes — skip those merged from the prelude (declaration_span is the
    // prelude sentinel, start == end == u32::MAX).  Prelude-merged purposes are
    // globally available for activation but were not declared in this module's
    // source, so they must not appear in the module's doc.
    for p in &compiled.compiled_purposes {
        if !p.declaration_span.is_prelude() {
            items.push(lower_purpose(p, source));
        }
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

    let realizations: Vec<RealizationDoc> = t
        .realizations
        .iter()
        .map(|r| lower_realization(r, source))
        .collect();

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
            annotations: lower_annotations(&t.annotations, source),
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

    // `CompiledExpr` has no span, but `ValueCellDecl.span` covers the whole
    // `param width: Length = 10mm` declaration, so its value text is the
    // default.  `Some` iff the param declares a default.
    let default_repr = vc
        .default_expr
        .as_ref()
        .map(|_| declared_value_text(source, vc.span));

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
    let expr_repr = expr_text_at(source, c.span);
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
    // An argument value is a `CompiledExpr`, which records no span.
    let args: Vec<SubComponentArgDoc> = s
        .args
        .iter()
        .map(|(name, _value)| SubComponentArgDoc {
            name: name.clone(),
            value: ExprText::Unavailable(UnavailableReason::NoSpan),
        })
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

/// `RealizationDecl.span` is the span of the `let`/`param` declaration the
/// realization was lowered from, so its value text is the geometry expression.
fn lower_realization(r: &RealizationDecl, source: &str) -> RealizationDoc {
    let name = r.name.clone().unwrap_or_else(|| "<realization>".to_string());
    RealizationDoc {
        name,
        value: declared_value_text(source, r.span),
    }
}

// ---------------------------------------------------------------------------
// CompiledTrait → ItemKind::Trait
// ---------------------------------------------------------------------------

fn lower_trait(t: &CompiledTrait, source: &str) -> ItemDoc {
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
            annotations: lower_annotations(&t.annotations, source),
            pragmas: lower_pragmas(&t.pragmas),
        },
        kind: ItemKind::Trait {
            type_params: render_type_params(&t.type_params),
            members,
        },
    }
}

// ---------------------------------------------------------------------------
// CompiledFunction → ItemKind::Function
// ---------------------------------------------------------------------------

/// Lower a `CompiledFunction`, splicing its type parameters into the rendered
/// signature as `fn {name}<{generics}>({params}) -> {ret}` (task #6342).
///
/// Deliberately NO model change here, unlike `ItemKind::TypeAlias`:
/// `ItemKind::Function { signature }` is already a single rendered DISPLAY
/// string, so the generics fold into it with no new variant field and no
/// literal churn across the doc crates.  `header.name` — the join key for
/// anchors, TOC hrefs and split-mode filenames — stays the bare identifier,
/// which is the same identity invariant the alias heading preserves.
fn lower_function(f: &CompiledFunction, source: &str) -> ItemDoc {
    let params_str: Vec<String> = f
        .params
        .iter()
        .map(|(name, ty)| format!("{name}: {}", type_to_string(ty)))
        .collect();
    // Empty for a non-generic fn, so its signature renders byte-identically to
    // before — no stray `<>`.
    let generics = match render_type_params(&f.type_params) {
        params if params.is_empty() => String::new(),
        params => format!("<{}>", params.join(", ")),
    };
    let signature = format!(
        "fn {}{}({}) -> {}",
        f.name,
        generics,
        params_str.join(", "),
        type_to_string(&f.return_type)
    );

    ItemDoc {
        header: ItemHeader {
            name: f.name.clone(),
            doc: f.doc.clone(),
            is_pub: f.is_pub,
            annotations: lower_annotations(&f.annotations, source),
            pragmas: vec![],
        },
        kind: ItemKind::Function { signature },
    }
}

// ---------------------------------------------------------------------------
// CompiledField → ItemKind::Field
// ---------------------------------------------------------------------------

fn lower_field(field: &CompiledField, source: &str) -> ItemDoc {
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
            annotations: lower_annotations(&field.annotations, source),
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
            // An objective term is a `CompiledExpr`, which records no span.
            // Direction is determined by the first term's sense.
            let direction = if obj.terms.first().map(|t| t.sense) == Some(ObjectiveSense::Maximize)
            {
                "maximize"
            } else {
                "minimize"
            };
            (
                ExprText::Unavailable(UnavailableReason::NoSpan),
                direction.to_string(),
            )
        }
        None => {
            // Fall back to first constraint expression if available.
            let expr = p
                .constraints
                .first()
                .map_or(ExprText::Unavailable(UnavailableReason::NoSpan), |c| {
                    expr_text_at(source, c.span)
                });
            (expr, "minimize".to_string())
        }
    };

    ItemDoc {
        header: ItemHeader {
            name: p.name.clone(),
            doc: None,
            is_pub: p.is_pub,
            annotations: lower_annotations(&p.annotations, source),
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
            variants: e.variants.iter().map(|v| v.name.clone()).collect(),
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
    let type_repr = match (a.resolved_type.as_ref(), a.type_expr.as_ref()) {
        (Some(t), _) => type_to_string(t),
        // A `None` resolved_type means the alias is parametric (its body is
        // instantiated per use site) or its body names an entity/unresolvable
        // type — in both cases the carried `type_expr` is the author's actual
        // spelling, so render that instead of a sentinel. Its `Display` is
        // that spelling already, so it needs no span.
        (None, Some(te)) => te.to_string(),
        // Reachable only via a synthetic fixture (e.g. a hand-built
        // CompiledTypeAlias in a test): a real alias carries neither a
        // resolved type nor a body, which says nothing about whether it is
        // parametric — so the old "<parameterized>" sentinel was actively
        // misleading here. "<unresolved>" is accurate and matches the house
        // `<...>` sentinel style already used by reify_core::Type's Display
        // for Type::Error (`impl Display for Type` in reify-core's `ty.rs`
        // writes "<error>" for the `Type::Error` arm).
        (None, None) => "<unresolved>".to_string(),
    };

    ItemDoc {
        header: ItemHeader {
            name: a.name.clone(),
            doc: None,
            is_pub: a.is_pub,
            annotations: vec![],
            pragmas: vec![],
        },
        kind: ItemKind::TypeAlias {
            type_params: render_type_params(&a.type_params),
            type_repr,
        },
    }
}

// ---------------------------------------------------------------------------
// CompiledConstraintDef → ItemKind::ConstraintDef
// ---------------------------------------------------------------------------

fn lower_constraint_def(cd: &CompiledConstraintDef, source: &str) -> ItemDoc {
    let expr_repr = expr_text_at(source, cd.span);

    ItemDoc {
        header: ItemHeader {
            name: cd.name.clone(),
            doc: None,
            is_pub: cd.is_pub,
            annotations: lower_annotations(&cd.annotations, source),
            pragmas: lower_pragmas(&cd.pragmas),
        },
        kind: ItemKind::ConstraintDef { expr_repr },
    }
}

// ---------------------------------------------------------------------------
// Annotation / Pragma rendering
// ---------------------------------------------------------------------------

/// Lower a `reify_types::Annotation` to an `AnnotationDoc`.
fn lower_annotation(ann: &reify_ir::annotation::Annotation, source: &str) -> AnnotationDoc {
    let args: Vec<String> = ann
        .args
        .iter()
        .map(|arg| render_annotation_arg(arg, source))
        .collect();
    AnnotationDoc {
        name: ann.name.clone(),
        args,
    }
}

fn lower_annotations(
    anns: &[reify_ir::annotation::Annotation],
    source: &str,
) -> Vec<AnnotationDoc> {
    anns.iter()
        .map(|ann| lower_annotation(ann, source))
        .collect()
}

/// Render a single `AnnotationArg` to a printable string.
///
/// Named args render as `name = value`; positional args render the value alone.
fn render_annotation_arg(arg: &AnnotationArg, source: &str) -> String {
    let value = render_annotation_arg_value(&arg.value, source);
    match &arg.name {
        Some(name) => format!("{name} = {value}"),
        None => value,
    }
}

/// Render an [`AnnotationArgValue`] to a printable string.
fn render_annotation_arg_value(value: &AnnotationArgValue, source: &str) -> String {
    match value {
        AnnotationArgValue::String(s) => format!("\"{s}\""),
        AnnotationArgValue::Int(i) => format!("{i}"),
        AnnotationArgValue::Real(r) => format!("{r}"),
        AnnotationArgValue::Bool(b) => format!("{b}"),
        AnnotationArgValue::Ident(s) => s.clone(),
        // An unevaluated expression (task 3555) renders as the source text it
        // was written as. Expr-valued args only arise on
        // materialization-time-eval annotations (structure/occurrence hosts).
        AnnotationArgValue::Expr(expr) => expr_text_at(source, expr.span).to_string(),
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

/// Render a declaration's type parameters to display strings, one per param,
/// in declaration order (task #6342).
///
/// Each entry comes from [`reify_ast::render_type_param`]. The default goes in
/// as the resolved `Type`, so it renders semantically (e.g. `Scalar[m]`).
///
/// `reify-doc` is a pure-data crate with no `reify-ir` dependency, so the
/// rendering has to happen here rather than in a `Display` impl consumed by
/// the formatters.
///
/// `TraitBound.trait_ref.type_args` is intentionally NOT rendered: the only
/// production construction site, `convert_type_params` in
/// `crates/reify-compiler/src/type_resolution.rs`, always builds it as
/// `vec![]`, so a `<…>` arm here would be dead code.
fn render_type_params(params: &[reify_ir::TypeParam]) -> Vec<String> {
    params
        .iter()
        .map(|p| {
            reify_ast::render_type_param(
                &p.name,
                p.bounds.iter().map(|b| b.trait_ref.name.as_str()),
                p.default.as_ref(),
            )
        })
        .collect()
}

/// The text of `source` at the byte offsets of `span`, or `SpanOutsideSource`
/// when the span is a prelude sentinel, empty, starts past the end of
/// `source`, or does not fall on char boundaries.
///
/// This function is **infallible**: it never panics, even on malformed spans
/// with `end < start` (which `SourceSpan::new` only `debug_assert!`s against).
fn expr_text_at(source: &str, span: SourceSpan) -> ExprText {
    let outside = ExprText::Unavailable(UnavailableReason::SpanOutsideSource);
    if span.is_prelude() {
        return outside;
    }
    let start = span.start as usize;
    let end = (span.end as usize).min(source.len());
    if start >= end {
        return outside;
    }
    // `str::get`, not indexing: a span off a char boundary must not panic.
    source
        .get(start..end)
        .map_or(outside, |text| ExprText::Source(text.to_string()))
}

/// The value of the declaration `decl_span` covers: the trimmed text after its
/// first `=` (`param width: Length = 10mm` → `10mm`), or
/// `DeclarationWithoutValue` when the declaration text has no `=`.
fn declared_value_text(source: &str, decl_span: SourceSpan) -> ExprText {
    match expr_text_at(source, decl_span) {
        ExprText::Source(declaration) => match declaration.split_once('=') {
            Some((_, value)) => ExprText::Source(value.trim().to_string()),
            None => ExprText::Unavailable(UnavailableReason::DeclarationWithoutValue),
        },
        unavailable => unavailable,
    }
}

/// Extract the local (rightmost) component of a cell id like `"Entity.param_name"`.
fn cell_local_name(id_str: &str) -> String {
    id_str
        .rsplit('.')
        .next()
        .unwrap_or(id_str)
        .to_string()
}

// ---------------------------------------------------------------------------
// Stdlib doc model entry point (task-3565)
// ---------------------------------------------------------------------------

/// Build a combined [`DocModel`] for the entire standard library.
///
/// Lowers each [`reify_compiler::stdlib_loader::load_stdlib()`] module through
/// [`build_doc_model`] against its own source from
/// [`reify_compiler::stdlib_loader::stdlib_sources()`], so stdlib documentation
/// takes the same path as a user module's.  Modules are paired by path, not by
/// position: `load_stdlib` returns them in topological order, which need not
/// match the source list once a stdlib module imports another.
///
/// The order of modules in the returned `DocModel` matches the order that
/// `load_stdlib()` returns them.
///
/// # Panics
///
/// If a compiled module has no source entry, which would break the invariant
/// that `load_stdlib` compiles exactly `stdlib_sources()`.
pub fn build_stdlib_doc_model() -> DocModel {
    use reify_compiler::stdlib_loader::{load_stdlib, stdlib_sources};
    let sources: Vec<(ModulePath, String)> = stdlib_sources()
        .into_iter()
        .map(|(name, source)| {
            let path = ModulePath::from_dotted(name).unwrap_or_else(|e| {
                panic!("stdlib source name {name:?} is not a module path: {e}")
            });
            (path, source)
        })
        .collect();
    let mut modules = Vec::new();
    for compiled in load_stdlib() {
        let source = sources
            .iter()
            .find(|(path, _)| *path == compiled.path)
            .map(|(_, source)| source.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "stdlib module {} has no entry in stdlib_sources(), but load_stdlib \
                     compiles exactly stdlib_sources()",
                    compiled.path
                )
            });
        // build_doc_model always returns exactly one ModuleDoc.
        modules.extend(build_doc_model(compiled, source).modules);
    }
    DocModel { modules }
}
