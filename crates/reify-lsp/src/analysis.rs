use std::sync::Arc;

use reify_compiler::{CompiledModule, EntityKind, ValueCellKind};
use reify_constraints::SimpleConstraintChecker;
use reify_eval::CheckResult;
use reify_ast::{Declaration, ParsedModule};
pub use reify_ast::{MemberSpanInfo, find_named_member_span};
use reify_core::{ModulePath, SourceSpan, Type, ValueCellId};
use reify_ir::Value;
use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind, Url};

use crate::convert::{is_ident_byte, offset_to_position, span_to_range};

/// Extract a module name from a file URI.
///
/// e.g., `file:///path/to/test.ri` → `"test"`.
/// Returns `"unnamed"` if the URI has no path segments or the file
/// doesn't end with `.ri`.
pub fn module_name_from_uri(uri: &Url) -> &str {
    uri.path_segments()
        .and_then(|mut segs| segs.next_back())
        .and_then(|name| name.strip_suffix(".ri"))
        .unwrap_or("unnamed")
}

/// Information about a member declaration (param, let, or auto).
pub struct MemberInfo<'a> {
    /// The member name.
    pub name: &'a str,
    /// Whether this is a Param, Let, or Auto.
    pub kind: ValueCellKind,
    /// The resolved type from the compiled module.
    pub cell_type: &'a Type,
    /// Source span from the parsed module (accurate tree-sitter byte offsets).
    pub span: SourceSpan,
    /// Doc comment text from the parsed AST, if present.
    pub doc: Option<&'a str>,
    /// The name of the owning structure/occurrence declaration.
    pub decl_name: &'a str,
}

/// Summary of a top-level entity declaration (structure or occurrence) with member counts.
pub struct EntitySummary<'a> {
    /// The entity name.
    pub name: &'a str,
    /// Number of param declarations (including nested where-blocks and ports).
    pub params: usize,
    /// Number of let declarations (including nested where-blocks and ports).
    pub lets: usize,
    /// Number of constraint declarations (including nested where-blocks and ports).
    pub constraints: usize,
    /// Whether this is a structure or occurrence.
    pub kind: EntityKind,
}

/// Shared analysis context that runs the parse → compile → check pipeline once
/// and provides structured accessors for hover, goto-def, and completions.
pub struct AnalysisContext {
    pub parsed: Arc<ParsedModule>,
    pub compiled: CompiledModule,
    pub check_result: CheckResult,
    /// Retained post-`check` engine with `set_capture_undef_causes(true)` enabled.
    ///
    /// Retained so that `undef_cause_line` can call `trace_undef_causes` on demand
    /// without a second `eval` pass — `check` already ran `eval` and populated the
    /// snapshot + `last_undef_causes` side-map (PRD ζ design decision §3).
    ///
    /// Send-safe: the server's hover handler creates `ctx` after its last `.await`
    /// and never holds it across a suspend point (server.rs:268-284); Engine is
    /// composed of `Send+Sync` parts (ConstraintChecker + kernels).
    engine: reify_eval::Engine,
}

impl AnalysisContext {
    /// Build a new analysis context by parsing `source` and running the full
    /// compile + check pipeline.
    pub fn new(source: &str, uri: &Url) -> Self {
        let module_name = module_name_from_uri(uri);
        // Prelude-aware parse so stdlib enum references like `CorrosionClass.C5`
        // disambiguate to `EnumAccess`; pairs with `compile_with_stdlib` in
        // `from_parsed`. See task 2525.
        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));
        Self::from_parsed(Arc::new(parsed))
    }

    /// Build an analysis context from an already-parsed module, running only the
    /// compile + constraint-check stages.
    ///
    /// This is the shared entry point for the LSP providers: the per-document
    /// parse cache (see [`crate::document::DocumentState::parsed_module`]) hands
    /// the same `Arc<ParsedModule>` to every request for a given document
    /// version, so hover and completion compile + check the cached parse instead
    /// of re-parsing. [`AnalysisContext::new`] delegates here after parsing.
    pub fn from_parsed(parsed: Arc<ParsedModule>) -> Self {
        // `&parsed` (`&Arc<ParsedModule>`) deref-coerces to the `&ParsedModule`
        // the compiler expects.
        let compiled = reify_compiler::compile_with_stdlib(&parsed);
        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        // Enable undef-cause capture BEFORE `check` so the post-eval snapshot
        // and last_undef_causes side-map are populated by the eval pass that
        // `check` calls internally.  Capture is additive (PRD A1 transparency):
        // check_result.values, determinacy, and constraint outcomes are
        // byte-identical whether or not capture is on — all existing tests
        // continue to pass.
        engine.set_capture_undef_causes(true);
        let check_result = engine.check(&compiled);

        Self {
            parsed,
            compiled,
            check_result,
            engine,
        }
    }

    /// Find a member declaration by name, returning combined info from
    /// parsed (span) and compiled (kind, type) modules.
    ///
    /// When `enclosing_decl` is `Some`, only the named declaration is searched.
    /// When `None`, returns the first match across all declarations.
    ///
    /// Returns `None` if no value cell with that name exists.
    pub fn find_member_decl(
        &self,
        name: &str,
        enclosing_decl: Option<&str>,
    ) -> Option<MemberInfo<'_>> {
        // Get the span, doc, and owning declaration name from the parsed module
        let (span, doc, decl_name) = self.find_parsed_member_span_and_doc(name, enclosing_decl)?;

        // Find type info from the compiled module, scoped to the same declaration.
        //
        // The compiler separates members into two collections:
        //   - template.value_cells: top-level params, lets, and autos
        //   - template.guarded_groups: members declared inside
        //     `where cond { ... } else { ... }` blocks
        //
        // Both must be searched to get complete coverage regardless of
        // where a member appears syntactically.
        for template in &self.compiled.templates {
            if template.name != decl_name {
                continue;
            }
            for vc in &template.value_cells {
                if vc.id.member == name {
                    return Some(MemberInfo {
                        name: &vc.id.member,
                        kind: vc.kind,
                        cell_type: &vc.cell_type,
                        span,
                        doc,
                        decl_name,
                    });
                }
            }
            // The compiler flattens nested `where` blocks into a flat
            // Vec<CompiledGuardedGroup> using parent_guard pointers for nesting
            // relationships, so flat iteration here correctly visits all guarded
            // members regardless of source-level nesting depth.
            for group in &template.guarded_groups {
                for vc in group.members.iter().chain(group.else_members.iter()) {
                    if vc.id.member == name {
                        return Some(MemberInfo {
                            name: &vc.id.member,
                            kind: vc.kind,
                            cell_type: &vc.cell_type,
                            span,
                            doc,
                            decl_name,
                        });
                    }
                }
            }
        }

        None
    }

    /// Find the source span, doc comment, and owning declaration name for a
    /// named member in the parsed module.
    ///
    /// When `enclosing_decl` is `Some`, only the declaration with that name is
    /// searched; otherwise all declarations are searched in order.
    fn find_parsed_member_span_and_doc(
        &self,
        name: &str,
        enclosing_decl: Option<&str>,
    ) -> Option<(SourceSpan, Option<&str>, &str)> {
        for decl in &self.parsed.declarations {
            let (members, decl_name) = match decl {
                reify_ast::Declaration::Structure(s) => (&s.members, s.name.as_str()),
                reify_ast::Declaration::Occurrence(o) => (&o.members, o.name.as_str()),
                reify_ast::Declaration::Trait(t) => (&t.members, t.name.as_str()),
                reify_ast::Declaration::Purpose(p) => (&p.members, p.name.as_str()),
                _ => continue,
            };
            if let Some(target) = enclosing_decl
                && decl_name != target
            {
                continue;
            }
            if let Some(info) = find_named_member_span(members, name) {
                return Some((info.span, info.doc, decl_name));
            }
        }
        None
    }

    /// Look up the doc comment for a top-level entity (structure, fn, trait, or enum) by name.
    ///
    /// Returns `None` if the entity has no doc comment or doesn't exist.
    pub fn find_entity_doc(&self, name: &str) -> Option<&str> {
        for decl in &self.parsed.declarations {
            match decl {
                reify_ast::Declaration::Structure(s) if s.name == name => {
                    return s.doc.as_deref();
                }
                reify_ast::Declaration::Occurrence(o) if o.name == name => {
                    return o.doc.as_deref();
                }
                reify_ast::Declaration::Function(f) if f.name == name => {
                    return f.doc.as_deref();
                }
                reify_ast::Declaration::Trait(t) if t.name == name => return t.doc.as_deref(),
                reify_ast::Declaration::Enum(e) if e.name == name => return e.doc.as_deref(),
                _ => {}
            }
        }
        None
    }

    /// Return value cell members for a specific structure/occurrence: (name, kind, type).
    pub fn member_names_for_structure(&self, name: &str) -> Vec<(&str, ValueCellKind, &Type)> {
        let mut result = Vec::new();
        for template in &self.compiled.templates {
            if template.name != name {
                continue;
            }
            for vc in &template.value_cells {
                result.push((vc.id.member.as_str(), vc.kind, &vc.cell_type));
            }
            // Flat iteration: the compiler flattens nested `where` blocks into a
            // flat Vec using parent_guard pointers for nesting relationships.
            for group in &template.guarded_groups {
                for vc in group.members.iter().chain(group.else_members.iter()) {
                    result.push((vc.id.member.as_str(), vc.kind, &vc.cell_type));
                }
            }
        }
        result
    }

    /// Return all value cell members: (name, kind, type).
    pub fn member_names(&self) -> Vec<(&str, ValueCellKind, &Type)> {
        let mut result = Vec::new();
        for template in &self.compiled.templates {
            for vc in &template.value_cells {
                result.push((vc.id.member.as_str(), vc.kind, &vc.cell_type));
            }
            // Flat iteration: the compiler flattens nested `where` blocks into a
            // flat Vec using parent_guard pointers for nesting relationships.
            for group in &template.guarded_groups {
                for vc in group.members.iter().chain(group.else_members.iter()) {
                    result.push((vc.id.member.as_str(), vc.kind, &vc.cell_type));
                }
            }
        }
        result
    }

    /// Return all structure/occurrence declarations with member counts.
    pub fn entity_names(&self) -> Vec<EntitySummary<'_>> {
        let mut result = Vec::new();
        for decl in &self.parsed.declarations {
            let (members, name, kind) = match decl {
                reify_ast::Declaration::Structure(s) => {
                    (&s.members, s.name.as_str(), EntityKind::Structure)
                }
                reify_ast::Declaration::Occurrence(o) => {
                    (&o.members, o.name.as_str(), EntityKind::Occurrence)
                }
                _ => continue,
            };
            let (param_count, let_count, constraint_count) = count_members_recursive(members);
            result.push(EntitySummary {
                name,
                params: param_count,
                lets: let_count,
                constraints: constraint_count,
                kind,
            });
        }
        result
    }

    /// Look up an evaluated value from the check result.
    pub fn get_value(&self, entity: &str, member: &str) -> Option<&Value> {
        let id = ValueCellId::new(entity, member);
        self.check_result.values.get(&id)
    }

    /// Return the name of the structure/occurrence/trait/purpose whose span contains `offset`,
    /// or `None` if the offset is outside all declarations.
    pub fn enclosing_decl_name_at(&self, offset: usize) -> Option<&str> {
        enclosing_decl_at(&self.parsed.declarations, offset).and_then(|decl| match decl {
            Declaration::Structure(s) => Some(s.name.as_str()),
            Declaration::Occurrence(o) => Some(o.name.as_str()),
            Declaration::Trait(t) => Some(t.name.as_str()),
            Declaration::Purpose(p) => Some(p.name.as_str()),
            _ => None,
        })
    }

    /// Return a cause-set line for an undef member, or `None` when determined.
    ///
    /// Traces the complete set of root [`reify_ir::UndefCause`]s for the cell
    /// `(entity, member)` using the retained post-`check` engine (which ran with
    /// `set_capture_undef_causes(true)`).  Formats them via the shared
    /// [`reify_eval::format_undef_causes`] and wraps the body as
    /// `"undef because: <body>"` — the LSP-specific framing (PRD §11 Q5 "surfaces wrap").
    ///
    /// Returns `None` when the cause set is empty (cell is determined, or the
    /// cell id is not part of the module).
    pub fn undef_cause_line(&self, entity: &str, member: &str) -> Option<String> {
        let id = ValueCellId::new(entity, member);
        let causes = self.engine.trace_undef_causes(&id);
        reify_eval::format_undef_causes(&causes).map(|body| format!("undef because: {body}"))
    }

    /// Synthesise the union type for a match-arm cluster member named `name`.
    ///
    /// When `enclosing_decl` is `Some`, only the named template is searched;
    /// when `None`, returns the first match across all templates (mirrors the
    /// scoping contract of [`AnalysisContext::find_member_decl`]).
    ///
    /// A cluster member's union is a freshly-built `Type::Union(arms[].arm_type.clone())`
    /// with no home in the compiled module, so it is returned by value rather
    /// than by reference — this avoids lifetime gymnastics that a by-ref variant
    /// would require (design decision D1 in plan.json).
    ///
    /// Returns `None` when:
    /// - `enclosing_decl` is `Some` but the named template doesn't exist, or
    /// - no group with the given `name` is present in `match_arm_groups`.
    ///
    /// This is the fallback used by `compute_hover_in_context` after
    /// `find_member_decl` misses — cluster subs live in `sub_components`, not
    /// `value_cells`/`guarded_groups`, so `find_member_decl` always misses them
    /// (task #3567).
    pub fn find_match_arm_group_union(
        &self,
        name: &str,
        enclosing_decl: Option<&str>,
    ) -> Option<Type> {
        for template in &self.compiled.templates {
            if let Some(target) = enclosing_decl
                && template.name != target
            {
                continue;
            }
            if let Some(group) = template.match_arm_groups.iter().find(|g| g.name == name) {
                let arm_types: Vec<Type> =
                    group.arms.iter().map(|a| a.arm_type.clone()).collect();
                return Some(Type::Union(arm_types));
            }
        }
        None
    }

    /// Surface the ΔDOF contract for a geometric-relation builtin call named
    /// `name`, scoped to the enclosing declaration (the structure/occurrence the
    /// hover cursor sits in). Returns `name(ArgTys) -> Relation removes N`, or
    /// `None` if no such relation call is present in that scope
    /// (geometric-relations γ, task 4383).
    ///
    /// A thin pass-through to the compiler-side traversal
    /// [`reify_compiler::relation_signatures::relation_contract_for_call`], which
    /// keeps `CompiledExprKind` matching inside reify-compiler rather than
    /// exposing the IR's expression internals across the crate boundary.
    pub fn relation_contract(&self, name: &str, enclosing_decl: Option<&str>) -> Option<String> {
        reify_compiler::relation_signatures::relation_contract_for_call(
            &self.compiled,
            name,
            enclosing_decl,
        )
    }
}

/// Return the declaration whose span contains `offset`,
/// or `None` if the offset is outside all declarations.
///
/// This is a free function that operates on `&[Declaration]` directly, so it can
/// be used by callers that only have a `ParsedModule` (e.g., goto-def) without
/// needing a full `AnalysisContext`.
pub fn enclosing_decl_at(declarations: &[Declaration], offset: usize) -> Option<&Declaration> {
    let offset_u32 = offset as u32;
    for decl in declarations {
        let decl_span = match decl {
            Declaration::Structure(s) => s.span,
            Declaration::Occurrence(o) => o.span,
            Declaration::Import(i) => i.span,
            Declaration::Enum(e) => e.span,
            Declaration::Function(f) => f.span,
            Declaration::Trait(t) => t.span,
            Declaration::Field(f) => f.span,
            Declaration::Purpose(p) => p.span,
            Declaration::Constraint(c) => c.span,
            Declaration::Unit(u) => u.span,
            Declaration::TypeAlias(t) => t.span,
            Declaration::Default(d) => d.span,
            Declaration::Module(m) => m.span,
            // Grammar producer only (task α 4395). Joint bodies are not
            // navigable by the LSP yet; included here so the exhaustive match
            // stays complete. Semantics deferred to task β.
            Declaration::Joint(j) => j.span,
        };
        if offset_u32 >= decl_span.start && offset_u32 < decl_span.end {
            return Some(decl);
        }
    }
    None
}

/// Recursively count Param, Let, and Constraint members, including those
/// nested inside `GuardedGroup.members` and `GuardedGroup.else_members`.
///
/// Returns `(param_count, let_count, constraint_count)`.
pub fn count_members_recursive(members: &[reify_ast::MemberDecl]) -> (usize, usize, usize) {
    let mut params = 0;
    let mut lets = 0;
    let mut constraints = 0;
    for member in members {
        match member {
            reify_ast::MemberDecl::Param(_) => params += 1,
            reify_ast::MemberDecl::Let(_) => lets += 1,
            reify_ast::MemberDecl::Constraint(_) => constraints += 1,
            reify_ast::MemberDecl::GuardedGroup(g) => {
                let (p, l, c) = count_members_recursive(&g.members);
                params += p;
                lets += l;
                constraints += c;
                let (p, l, c) = count_members_recursive(&g.else_members);
                params += p;
                lets += l;
                constraints += c;
            }
            reify_ast::MemberDecl::Port(port) => {
                let (p, l, c) = count_members_recursive(&port.members);
                params += p;
                lets += l;
                constraints += c;
            }
            _ => {}
        }
    }
    (params, lets, constraints)
}

/// Format a `Value` for user-friendly display in hover tooltips.
///
/// Delegates to [`Value::format_hover()`] — the canonical implementation lives
/// on Value itself so that adding a new variant only requires editing value.rs.
pub fn format_value(value: &Value) -> String {
    value.format_hover()
}

/// Compute a hierarchical [`DocumentSymbol`] tree for `source` (task 4207 η).
///
/// This is a purely SYNTACTIC outline: it parses the module via
/// [`reify_compiler::parse_with_stdlib`] (the same prelude-aware parse used by
/// goto-def) and walks `parsed.declarations` WITHOUT compiling or
/// constraint-checking. It is deliberately kept distinct from the outline's
/// semantic realization tree (`get_entity_tree`) per PRD design decision 5 —
/// the symbol list reflects declaration structure, not evaluation.
///
/// Top-level declarations map to symbols as: structure→STRUCT,
/// occurrence→CLASS, trait→INTERFACE, enum→ENUM, fn→FUNCTION. All other
/// top-level declarations (import/unit/type-alias/constraint-def/field/
/// purpose/module) are not navigable symbols and are skipped.
pub fn compute_document_symbols(source: &str, uri: &Url) -> Vec<DocumentSymbol> {
    let module_name = module_name_from_uri(uri);
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));
    compute_document_symbols_from_parsed(&parsed, source)
}

/// Compute the [`DocumentSymbol`] tree from a pre-built [`ParsedModule`].
///
/// Injectable core shared by the per-request wrapper [`compute_document_symbols`]
/// (which parses internally) and the server's cache-fed path (which supplies the
/// per-document cached parse — one parse per edit). Walks `parsed.declarations`
/// WITHOUT compiling or constraint-checking; see [`compute_document_symbols`] for
/// the declaration→symbol mapping.
pub fn compute_document_symbols_from_parsed(
    parsed: &ParsedModule,
    source: &str,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    for decl in &parsed.declarations {
        match decl {
            Declaration::Structure(s) => {
                symbols.push(make_symbol(
                    &s.name,
                    SymbolKind::STRUCT,
                    span_to_range(source, s.span),
                    name_selection_range(source, s.span, &s.name),
                    children_or_none(members_to_symbols(source, &s.members)),
                ));
            }
            Declaration::Occurrence(o) => {
                symbols.push(make_symbol(
                    &o.name,
                    SymbolKind::CLASS,
                    span_to_range(source, o.span),
                    name_selection_range(source, o.span, &o.name),
                    children_or_none(members_to_symbols(source, &o.members)),
                ));
            }
            Declaration::Trait(t) => {
                symbols.push(make_symbol(
                    &t.name,
                    SymbolKind::INTERFACE,
                    span_to_range(source, t.span),
                    name_selection_range(source, t.span, &t.name),
                    children_or_none(members_to_symbols(source, &t.members)),
                ));
            }
            Declaration::Enum(e) => {
                // Each variant becomes an ENUM_MEMBER child. Named-payload
                // variant fields (`Circle { radius: Length }`) are not expanded
                // into grandchildren for this task.
                let variants = e
                    .variants
                    .iter()
                    .map(|v| {
                        make_symbol(
                            &v.name,
                            SymbolKind::ENUM_MEMBER,
                            span_to_range(source, v.span),
                            name_selection_range(source, v.span, &v.name),
                            None,
                        )
                    })
                    .collect();
                symbols.push(make_symbol(
                    &e.name,
                    SymbolKind::ENUM,
                    span_to_range(source, e.span),
                    name_selection_range(source, e.span, &e.name),
                    children_or_none(variants),
                ));
            }
            Declaration::Function(f) => {
                symbols.push(make_symbol(
                    &f.name,
                    SymbolKind::FUNCTION,
                    span_to_range(source, f.span),
                    name_selection_range(source, f.span, &f.name),
                    None,
                ));
            }
            // All other top-level declarations are not navigable symbols:
            // Import, Unit, TypeAlias, Constraint (ConstraintDef), Field,
            // Purpose, and Module have no stable jump target and are skipped.
            _ => {}
        }
    }
    symbols
}

/// Convert a possibly-empty child list into the `children` field of a
/// [`DocumentSymbol`]: `None` when there are no navigable members, `Some(_)`
/// otherwise. Keeps leaf declarations as `children: None` rather than
/// `Some(vec![])`.
fn children_or_none(children: Vec<DocumentSymbol>) -> Option<Vec<DocumentSymbol>> {
    if children.is_empty() {
        None
    } else {
        Some(children)
    }
}

/// Recursively convert a member list into child [`DocumentSymbol`]s (task 4207 η).
///
/// Mirrors the recursion shape of [`count_members_recursive`] /
/// [`find_named_member_span`]: `where`/`else` ([`reify_ast::MemberDecl::GuardedGroup`])
/// and match-arm ([`reify_ast::MemberDecl::MatchArmDeclGroup`]) members are
/// FLATTENED up to the owning declaration's children — no synthetic guard nodes —
/// so guarded and match-arm params/lets stay discoverable for symbol-jump. Named
/// members map as: param→FIELD, let→VARIABLE, sub→OBJECT, port→INTERFACE; subs
/// and ports form true nested nodes (a sub's specialization body and a port's
/// internal members become grandchildren). Unlabeled constraints, connects,
/// chains, minimize/maximize, and meta blocks have no stable identifier to jump
/// to and are skipped.
///
/// Recursion is bounded by [`reify_ast::MAX_MEMBER_NESTING_DEPTH`] to prevent
/// stack overflow on pathological input, matching the AST member-walk helpers.
fn members_to_symbols(source: &str, members: &[reify_ast::MemberDecl]) -> Vec<DocumentSymbol> {
    members_to_symbols_depth(source, members, 0)
}

fn members_to_symbols_depth(
    source: &str,
    members: &[reify_ast::MemberDecl],
    depth: usize,
) -> Vec<DocumentSymbol> {
    use reify_ast::MemberDecl;
    if depth > reify_ast::MAX_MEMBER_NESTING_DEPTH {
        return Vec::new();
    }
    let mut symbols = Vec::new();
    for member in members {
        match member {
            MemberDecl::Param(p) => symbols.push(make_symbol(
                &p.name,
                SymbolKind::FIELD,
                span_to_range(source, p.span),
                name_selection_range(source, p.span, &p.name),
                None,
            )),
            MemberDecl::Let(l) => symbols.push(make_symbol(
                &l.name,
                SymbolKind::VARIABLE,
                span_to_range(source, l.span),
                name_selection_range(source, l.span, &l.name),
                None,
            )),
            // where/else members flatten up to the owning declaration.
            MemberDecl::GuardedGroup(g) => {
                symbols.extend(members_to_symbols_depth(source, &g.members, depth + 1));
                symbols.extend(members_to_symbols_depth(source, &g.else_members, depth + 1));
            }
            // match-arm members flatten up to the owning declaration.
            MemberDecl::MatchArmDeclGroup(g) => {
                for arm in &g.arms {
                    symbols.extend(members_to_symbols_depth(
                        source,
                        std::slice::from_ref(&*arm.member),
                        depth + 1,
                    ));
                }
            }
            // sub-components nest their specialization-body members (when the
            // `sub` opens a `{ … }` body) as grandchildren; a bare `sub` is a leaf.
            MemberDecl::Sub(s) => {
                let body = s
                    .body
                    .as_ref()
                    .map(|b| members_to_symbols_depth(source, b, depth + 1))
                    .unwrap_or_default();
                symbols.push(make_symbol(
                    &s.name,
                    SymbolKind::OBJECT,
                    span_to_range(source, s.span),
                    name_selection_range(source, s.span, &s.name),
                    children_or_none(body),
                ));
            }
            // ports nest their internal members as grandchildren.
            MemberDecl::Port(port) => symbols.push(make_symbol(
                &port.name,
                SymbolKind::INTERFACE,
                span_to_range(source, port.span),
                name_selection_range(source, port.span, &port.name),
                children_or_none(members_to_symbols_depth(source, &port.members, depth + 1)),
            )),
            // Constraints/connects/chains/minimize/maximize/meta are not emitted
            // here — they have no stable identifier to jump to.
            _ => {}
        }
    }
    symbols
}

/// Build a [`DocumentSymbol`] with every field set explicitly.
///
/// `lsp-types` 0.94 marks `DocumentSymbol.deprecated` as `#[deprecated]` and
/// provides no `Default`, so the field cannot be omitted; centralizing the
/// `#[allow(deprecated)]` here keeps the verify/clippy pipeline clean while
/// every construction site stays a one-liner (PRD design decision 6).
#[allow(deprecated)]
fn make_symbol(
    name: &str,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol {
        name: name.to_string(),
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

/// Compute the `selection_range` (the name token) for a declaration whose full
/// span is `span` and whose declared name is `name`.
///
/// Declaration AST nodes carry only `name: String` plus the full declaration
/// span (no separate name-token span), so the name's byte offset must be
/// recovered from the source. LSP requires `selection_range ⊆ range` and ideally
/// on the name token; this performs a *word-boundary* search bounded to the
/// declaration's own span, falling back to `span.start` when the name cannot be
/// located (defensive; should not happen for well-formed declarations).
fn name_selection_range(source: &str, span: SourceSpan, name: &str) -> Range {
    let name_offset = find_name_offset_in_span(source, span, name);
    // Clamp the selection end to the declaration span's end so the LSP
    // `selection_range ⊆ range` invariant holds even for degenerate or
    // error-recovery spans shorter than the name. When the name is located
    // the match already fits inside the span (`name_offset + name.len() ≤
    // span.end`), so this clamp is a no-op there; it only constrains the
    // `span.start` fallback, where `span.start + name.len()` could otherwise
    // push the selection end past `span.end` (and thus past `range.end`).
    let name_end = (name_offset + name.len() as u32).min(span.end);
    Range {
        start: offset_to_position(source, name_offset),
        end: offset_to_position(source, name_end),
    }
}

/// Find the byte offset of `name` as a *whole identifier* within
/// `[span.start, span.end)`, returning `span.start` if no word-boundary match
/// is found.
///
/// A naive substring search is unsafe — e.g. the member name `a` would match
/// inside the `param` keyword — so a match must have non-identifier neighbours
/// (or a source boundary) on both sides. Search bounds are clamped to the
/// source length and snapped forward to UTF-8 character boundaries, mirroring
/// the safety pattern in `convert::offset_to_position` and
/// `goto_def::find_name_offset_in_decl`.
fn find_name_offset_in_span(source: &str, span: SourceSpan, name: &str) -> u32 {
    if name.is_empty() {
        return span.start;
    }
    let len = source.len();
    let mut start = (span.start as usize).min(len);
    let mut end = (span.end as usize).min(len);
    // Snap both ends forward to valid UTF-8 boundaries so the slice is valid
    // even when tree-sitter error-recovery spans land mid-character.
    while start < len && !source.is_char_boundary(start) {
        start += 1;
    }
    while end < len && !source.is_char_boundary(end) {
        end += 1;
    }
    if start >= end {
        return span.start;
    }

    let hay = &source[start..end];
    let bytes = source.as_bytes();
    let name_len = name.len();
    let mut search_from = 0usize;
    while search_from < hay.len() {
        let Some(rel) = hay[search_from..].find(name) else {
            break;
        };
        let abs = start + search_from + rel; // absolute byte offset of the match
        // Left and right neighbours must be non-identifier bytes (or source
        // boundaries) for this to be a whole-word match.
        let left_ok = abs == 0 || !is_ident_byte(bytes[abs - 1]);
        let right = abs + name_len;
        let right_ok = right >= len || !is_ident_byte(bytes[right]);
        if left_ok && right_ok {
            return abs as u32;
        }
        search_from += rel + 1;
    }
    span.start
}

/// Narrow a member-statement span down to the span of just its NAME identifier
/// token.
///
/// Searches forward from `member_span.start`, **bounded by `member_span.end`**,
/// for the first **whole-word** occurrence of `name` (a match whose neighbouring
/// bytes are not identifier characters), returning
/// `SourceSpan::new(name_start, name_start + name.len())`. Falls back to an empty
/// span at `member_span.start` when `name` is not found *within the member span*
/// — so a member whose own name token is unexpectedly absent never borrows a
/// same-named token from a sibling member.
///
/// The declaration name always follows its leading keyword
/// (`param`/`let`/`sub`/`port`), so the first whole-word match is the declaration
/// token. Whole-word matching (rather than the bare substring search in
/// `goto_def::find_name_offset_in_decl`) guards against a longer identifier that
/// merely contains `name` as a substring. The UTF-8 char-boundary snap mirrors
/// `convert::offset_to_position`.
///
/// Reused by `references.rs` for the declaration name-token span, the
/// `include_declaration` token, and the prepare/compute-rename declaration path.
pub fn name_token_span(source: &str, member_span: SourceSpan, name: &str) -> SourceSpan {
    let mut start = (member_span.start as usize).min(source.len());
    // Snap forward to a valid UTF-8 boundary if we landed mid-character.
    while start < source.len() && !source.is_char_boundary(start) {
        start += 1;
    }
    // Bound the search to the member's OWN span. Without this, a member whose
    // name token is unexpectedly absent (e.g. a malformed/recovered AST node
    // whose span does not actually contain the declared name) would match the
    // first same-named whole word anywhere later in the source — silently
    // borrowing a token from a sibling member. Clamping to `member_span.end`
    // makes such a case fall through to the empty-span fallback instead.
    let end = (member_span.end as usize).min(source.len()).max(start);

    let bytes = source.as_bytes();
    let name_len = name.len();
    for (rel, _) in source[start..].match_indices(name) {
        let abs = start + rel;
        // Past the member span: `match_indices` yields ascending offsets, so once
        // a match would extend beyond `end` every later match does too — stop.
        if abs + name_len > end {
            break;
        }
        // Whole-word check: neither the byte before nor the byte after the match
        // may be an identifier character.
        let prev_ok = abs == 0 || !is_ident_byte(bytes[abs - 1]);
        let next = abs + name_len;
        let next_ok = next >= bytes.len() || !is_ident_byte(bytes[next]);
        if prev_ok && next_ok {
            return SourceSpan::new(abs as u32, (abs + name_len) as u32);
        }
    }

    SourceSpan::empty(member_span.start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;
    use tower_lsp::lsp_types::Url;

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    /// Minimal source that references two stdlib symbols (Rigid trait, Material struct).
    /// Shared across all task-2176 stdlib-resolution tests to avoid tripling the literal.
    // Post-GHR-α (task 3603): Physical is spec-shape (geometry : Solid +
    // material : Material struct slot); the legacy flat-scalar
    // density/volume/centroid_x/y/z params were retired. Rigid refines Physical;
    // moment_of_inertia is now auto-derived (task 4229 Option A — no longer a
    // required param). Dimensioned density (7850kg/m^3) required so body_density
    // let resolves to a clean Density (avoids resolve_density_arg Warning).
    const STDLIB_PROBE_SRC: &str = r#"structure S : Rigid {
    param geometry: Solid = box(10mm, 20mm, 30mm)
    param material: Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
}"#;

    // --- module_name_from_uri tests ---

    #[test]
    fn module_name_from_uri_extracts_test() {
        let uri = Url::parse("file:///test.ri").unwrap();
        assert_eq!(module_name_from_uri(&uri), "test");
    }

    #[test]
    fn module_name_from_uri_nested_path() {
        let uri = Url::parse("file:///path/to/bracket.ri").unwrap();
        assert_eq!(module_name_from_uri(&uri), "bracket");
    }

    #[test]
    fn module_name_from_uri_no_ri_suffix() {
        let uri = Url::parse("file:///test.txt").unwrap();
        assert_eq!(module_name_from_uri(&uri), "unnamed");
    }

    // --- AnalysisContext construction tests ---

    #[test]
    fn analysis_context_builds_on_bracket_source() {
        let source = reify_test_support::bracket_source();
        let uri = test_uri();
        let ctx = AnalysisContext::new(source, &uri);
        assert!(!ctx.parsed.declarations.is_empty());
        assert!(!ctx.compiled.templates.is_empty());
    }

    #[test]
    fn analysis_context_builds_on_empty_source() {
        let ctx = AnalysisContext::new("", &test_uri());
        assert!(ctx.parsed.declarations.is_empty());
        assert!(ctx.compiled.templates.is_empty());
    }

    /// `from_parsed` must produce an `AnalysisContext` observably equivalent to
    /// `new` when fed the same parse — it just skips the parse step and reuses a
    /// shared `Arc<ParsedModule>`. Equivalence is asserted on the public
    /// accessors hover/completion/goto-def actually rely on (compiled templates,
    /// `find_member_decl`, `entity_names`), not on private fields.
    #[test]
    fn analysis_context_from_parsed_matches_new() {
        let source = reify_test_support::bracket_source();
        let uri = test_uri();
        let parsed = std::sync::Arc::new(reify_compiler::parse_with_stdlib(
            source,
            ModulePath::single("test"),
        ));

        let ctx_fp = AnalysisContext::from_parsed(parsed.clone());
        let ctx_new = AnalysisContext::new(source, &uri);

        // compile ran: templates are present.
        assert!(
            !ctx_fp.compiled.templates.is_empty(),
            "from_parsed should compile templates"
        );

        // find_member_decl("width", None): kind + type + span agree with `new`.
        let m_fp = ctx_fp
            .find_member_decl("width", None)
            .expect("width via from_parsed");
        let m_new = ctx_new
            .find_member_decl("width", None)
            .expect("width via new");
        assert_eq!(m_fp.name, m_new.name);
        assert_eq!(m_fp.kind, m_new.kind);
        assert_eq!(*m_fp.cell_type, *m_new.cell_type);
        assert_eq!(m_fp.span, m_new.span);

        // entity_names(): names + member counts agree with `new`.
        let summarize = |ctx: &AnalysisContext| -> Vec<(String, usize, usize, usize)> {
            ctx.entity_names()
                .into_iter()
                .map(|e| (e.name.to_string(), e.params, e.lets, e.constraints))
                .collect()
        };
        assert_eq!(summarize(&ctx_fp), summarize(&ctx_new));
    }

    // --- find_member_decl tests ---

    #[test]
    fn find_member_decl_width_is_param() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx
            .find_member_decl("width", None)
            .expect("width should exist");
        assert_eq!(info.name, "width");
        assert_eq!(info.kind, ValueCellKind::Param);
        assert_eq!(*info.cell_type, Type::length());
        // Span starts at the 'p' in 'param width: Length = 80mm'
        assert_eq!(info.span.start, 24);
        // Verify span covers the full declaration text
        let decl_text = &source[info.span.start as usize..info.span.end as usize];
        assert!(
            decl_text.contains("width") && decl_text.contains("80mm"),
            "span should cover full param declaration, got: {decl_text:?}"
        );
    }

    #[test]
    fn find_member_decl_volume_is_let() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx
            .find_member_decl("volume", None)
            .expect("volume should exist");
        assert_eq!(info.name, "volume");
        assert_eq!(info.kind, ValueCellKind::Let);
        // Volume is width*height*thickness → Scalar with VOLUME dimension
        assert!(matches!(info.cell_type, Type::Scalar { .. }));
    }

    #[test]
    fn find_member_decl_occurrence_param() {
        let source = "occurrence def Joint {\n    param diameter: Length = 10mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx
            .find_member_decl("diameter", None)
            .expect("diameter should exist in occurrence");
        assert_eq!(info.name, "diameter");
        assert_eq!(info.kind, ValueCellKind::Param);
        assert!(matches!(info.cell_type, Type::Scalar { .. }));
    }

    #[test]
    fn find_member_decl_nonexistent_returns_none() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        assert!(ctx.find_member_decl("nonexistent", None).is_none());
    }

    #[test]
    fn find_member_decl_nonexistent_decl_returns_none() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        assert!(ctx.find_member_decl("width", Some("NonExistent")).is_none());
    }

    // --- member_names tests ---

    #[test]
    fn member_names_returns_all_value_cells() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let members = ctx.member_names();
        // 5 params + volume + body = at least 6 value cells
        assert!(
            members.len() >= 6,
            "expected at least 6 members, got {}",
            members.len()
        );
        let names: Vec<&str> = members.iter().map(|(n, _, _)| *n).collect();
        assert!(names.contains(&"width"));
        assert!(names.contains(&"height"));
        assert!(names.contains(&"thickness"));
        assert!(names.contains(&"fillet_radius"));
        assert!(names.contains(&"hole_diameter"));
        assert!(names.contains(&"volume"));
    }

    // --- member_names guarded-group regression tests ---

    #[test]
    fn member_names_includes_guarded_group_members() {
        let source = r#"structure S {
    param cond : Bool = true
    where cond {
        param guarded_x : Length = 5mm
    }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let members = ctx.member_names();

        let cond = members
            .iter()
            .find(|(n, _, _)| *n == "cond")
            .expect("should include top-level param 'cond'");
        assert_eq!(cond.1, ValueCellKind::Param, "cond should be a Param");
        assert_eq!(*cond.2, Type::Bool, "cond should have Bool type");

        let guarded = members
            .iter()
            .find(|(n, _, _)| *n == "guarded_x")
            .expect("should include guarded-group param 'guarded_x'");
        assert_eq!(
            guarded.1,
            ValueCellKind::Param,
            "guarded_x should be a Param"
        );
        assert_eq!(
            *guarded.2,
            Type::length(),
            "guarded_x should have length type"
        );
    }

    #[test]
    fn member_names_includes_else_block_members() {
        let source = r#"structure S {
    param cond : Bool = true
    where cond {
        param when_true : Length = 1mm
    } else {
        param when_false : Length = 2mm
    }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let members = ctx.member_names();

        let cond = members
            .iter()
            .find(|(n, _, _)| *n == "cond")
            .expect("should include top-level param 'cond'");
        assert_eq!(cond.1, ValueCellKind::Param, "cond should be a Param");
        assert_eq!(*cond.2, Type::Bool, "cond should have Bool type");

        let when_true = members
            .iter()
            .find(|(n, _, _)| *n == "when_true")
            .expect("should include where-branch param 'when_true'");
        assert_eq!(
            when_true.1,
            ValueCellKind::Param,
            "when_true should be a Param"
        );
        assert_eq!(
            *when_true.2,
            Type::length(),
            "when_true should have length type"
        );

        let when_false = members
            .iter()
            .find(|(n, _, _)| *n == "when_false")
            .expect("should include else-branch param 'when_false'");
        assert_eq!(
            when_false.1,
            ValueCellKind::Param,
            "when_false should be a Param"
        );
        assert_eq!(
            *when_false.2,
            Type::length(),
            "when_false should have length type"
        );
    }

    // --- EntityKind / EntitySummary tests ---

    #[test]
    fn entity_summary_fields() {
        let summary = EntitySummary {
            name: "Bracket",
            params: 5,
            lets: 2,
            constraints: 3,
            kind: EntityKind::Structure,
        };
        assert_eq!(summary.name, "Bracket");
        assert_eq!(summary.params, 5);
        assert_eq!(summary.lets, 2);
        assert_eq!(summary.constraints, 3);
        assert_eq!(summary.kind, EntityKind::Structure);
    }

    #[test]
    fn entity_names_includes_occurrence() {
        let source = "structure Bracket {\n    param width: Length = 80mm\n}\noccurrence def Joint {\n    param diameter: Length = 10mm\n    let radius = diameter / 2\n    constraint diameter > 5mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let entities = ctx.entity_names();
        assert_eq!(entities.len(), 2, "should have Bracket and Joint");
        assert_eq!(entities[0].name, "Bracket");
        assert_eq!(entities[0].kind, EntityKind::Structure);
        assert_eq!(entities[0].params, 1);
        assert_eq!(entities[0].lets, 0);
        assert_eq!(entities[0].constraints, 0);
        assert_eq!(entities[1].name, "Joint");
        assert_eq!(entities[1].kind, EntityKind::Occurrence);
        assert_eq!(entities[1].params, 1);
        assert_eq!(entities[1].lets, 1);
        assert_eq!(entities[1].constraints, 1);
    }

    #[test]
    fn entity_kind_display() {
        assert_eq!(EntityKind::Structure.to_string(), "structure");
        assert_eq!(EntityKind::Occurrence.to_string(), "occurrence");
        // Verify Debug and PartialEq derives work
        assert_eq!(EntityKind::Structure, EntityKind::Structure);
        assert_ne!(EntityKind::Structure, EntityKind::Occurrence);
        assert_eq!(format!("{:?}", EntityKind::Structure), "Structure");
    }

    #[test]
    fn entity_names_returns_bracket() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let entities = ctx.entity_names();
        assert_eq!(entities.len(), 1);
        let e = &entities[0];
        assert_eq!(e.name, "Bracket");
        assert_eq!(e.params, 5);
        assert_eq!(e.lets, 2); // volume + body
        assert_eq!(e.constraints, 3);
        assert_eq!(e.kind, EntityKind::Structure);
    }

    #[test]
    fn entity_names_counts_nested_where_blocks() {
        let source = r#"structure S {
    param a : Bool = true
    param b : Bool = true
    where a {
        where b {
            param deep : Length = 1mm
        }
    }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let entities = ctx.entity_names();
        assert_eq!(entities.len(), 1);
        // Should count: a + b + deep = 3 params
        assert_eq!(
            entities[0].params, 3,
            "expected 3 params (a, b, deep), got {}",
            entities[0].params
        );
    }

    #[test]
    fn entity_names_counts_else_branch_members() {
        let source = r#"structure S {
    param cond : Bool = true
    where cond {
        param when_true : Length = 1mm
    } else {
        let fallback = 2
    }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let entities = ctx.entity_names();
        assert_eq!(entities.len(), 1);
        let e = &entities[0];
        // Should count: cond + when_true = 2 params
        assert_eq!(
            e.params, 2,
            "expected 2 params (cond, when_true), got {}",
            e.params
        );
        // Should count: fallback = 1 let (from else branch)
        assert_eq!(
            e.lets, 1,
            "expected 1 let (fallback in else branch), got {}",
            e.lets
        );
    }

    #[test]
    fn entity_names_counts_guarded_group_members() {
        // entity_names() counts members recursively, including those
        // inside where-blocks. This test expects the CORRECT (recursive) counts.
        let source = r#"structure S {
    param a : Bool = true
    param b : Length = 1mm
    where a {
        param guarded_x : Length = 5mm
        let guarded_y = 2
    }
    constraint b > 0mm
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let entities = ctx.entity_names();
        assert_eq!(entities.len(), 1);
        let e = &entities[0];
        assert_eq!(e.name, "S");
        assert_eq!(e.kind, EntityKind::Structure);
        // Should count: a + b + guarded_x = 3 params
        assert_eq!(
            e.params, 3,
            "expected 3 params (a, b, guarded_x), got {}",
            e.params
        );
        // Should count: guarded_y = 1 let
        assert_eq!(e.lets, 1, "expected 1 let (guarded_y), got {}", e.lets);
        // Should count: b > 0mm = 1 constraint
        assert_eq!(
            e.constraints, 1,
            "expected 1 constraint, got {}",
            e.constraints
        );
    }

    // --- get_value tests ---

    #[test]
    fn get_value_returns_width_scalar() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let value = ctx
            .get_value("Bracket", "width")
            .expect("width should have a value");
        match value {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (*si_value - 0.08).abs() < 1e-10,
                    "expected 0.08m, got {si_value}"
                );
                assert_eq!(*dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar, got {other:?}"),
        }
    }

    #[test]
    fn get_value_multi_declaration_scoped_correctly() {
        // Two structures with same-named param 'x' but different default values.
        // get_value must return the correct value scoped to each declaration.
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param x: Length = 20mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());

        // A's x should be 0.005 m (5mm in SI)
        let val_a = ctx.get_value("A", "x").expect("A.x should have a value");
        match val_a {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (*si_value - 0.005).abs() < 1e-10,
                    "expected A.x = 0.005m (5mm), got {si_value}"
                );
                assert_eq!(*dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar for A.x, got {other:?}"),
        }

        // B's x should be 0.02 m (20mm in SI)
        let val_b = ctx.get_value("B", "x").expect("B.x should have a value");
        match val_b {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (*si_value - 0.02).abs() < 1e-10,
                    "expected B.x = 0.02m (20mm), got {si_value}"
                );
                assert_eq!(*dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar for B.x, got {other:?}"),
        }
    }

    // --- doc retrieval tests ---

    #[test]
    fn find_entity_doc_returns_doc_for_documented_structure() {
        let source = "/// A bracket.\nstructure Bracket {\n    param width: Length = 80mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        assert_eq!(ctx.find_entity_doc("Bracket"), Some("A bracket."));
    }

    #[test]
    fn find_entity_doc_returns_doc_for_occurrence() {
        let source =
            "/// A joint process.\noccurrence def Joint {\n    param diameter: Length = 10mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        assert_eq!(ctx.find_entity_doc("Joint"), Some("A joint process."));
    }

    #[test]
    fn find_entity_doc_returns_none_for_undocumented() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        assert_eq!(ctx.find_entity_doc("Bracket"), None);
    }

    #[test]
    fn find_entity_doc_returns_none_for_undocumented_occurrence() {
        let source = "occurrence def Joint {\n    param diameter: Length = 10mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        assert_eq!(ctx.find_entity_doc("Joint"), None);
    }

    #[test]
    fn member_info_includes_doc_for_documented_param() {
        let source = "structure Bracket {\n    /// The width.\n    param width: Length = 80mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx
            .find_member_decl("width", None)
            .expect("width should exist");
        assert_eq!(info.doc, Some("The width."));
    }

    // --- find_member_decl inside guarded groups ---

    #[test]
    fn find_member_decl_param_inside_where_block() {
        let source = r#"structure S {
    param cond : Bool = true
    where cond {
        param guarded_x : Length = 5mm
    }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx
            .find_member_decl("guarded_x", None)
            .expect("guarded_x inside where block should be found");
        assert_eq!(info.name, "guarded_x");
        assert_eq!(info.kind, ValueCellKind::Param);
        assert_eq!(
            *info.cell_type,
            Type::length(),
            "guarded_x should have length type"
        );
        // Exact byte-position assertions: span must start at the 'p' in
        // 'param guarded_x' and end immediately after '5mm'.
        let expected_start = source.find("param guarded_x").unwrap() as u32;
        let expected_end = (source.find("5mm").unwrap() + "5mm".len()) as u32;
        assert_eq!(
            info.span.start, expected_start,
            "span.start should point at 'param guarded_x'"
        );
        assert_eq!(
            info.span.end, expected_end,
            "span.end should end after '5mm'"
        );
        assert_eq!(info.decl_name, "S");
    }

    #[test]
    fn find_member_decl_nested_where_blocks() {
        let source = r#"structure S {
    param a : Bool = true
    param b : Bool = true
    where a {
        where b {
            param deep_x : Length = 1mm
        }
    }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx
            .find_member_decl("deep_x", None)
            .expect("deep_x inside nested where blocks should be found");
        assert_eq!(info.name, "deep_x");
        assert_eq!(info.kind, ValueCellKind::Param);
        assert_eq!(
            *info.cell_type,
            Type::length(),
            "deep_x should have length type"
        );
        // Exact byte-position assertions: span must start at the 'p' in
        // 'param deep_x' and end immediately after '1mm'. This closes
        // the depth-2 nesting coverage gap.
        let expected_start = source.find("param deep_x").unwrap() as u32;
        let expected_end = (source.find("1mm").unwrap() + "1mm".len()) as u32;
        assert_eq!(
            info.span.start, expected_start,
            "span.start should point at 'param deep_x'"
        );
        assert_eq!(
            info.span.end, expected_end,
            "span.end should end after '1mm'"
        );
        assert_eq!(info.decl_name, "S");
    }

    #[test]
    fn find_member_decl_let_inside_else_block() {
        let source = r#"structure S {
    param cond : Bool = true
    where cond {
        param a : Length = 1mm
    } else {
        let fallback = 2
    }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx
            .find_member_decl("fallback", None)
            .expect("fallback inside else block should be found");
        assert_eq!(info.name, "fallback");
        assert_eq!(info.kind, ValueCellKind::Let);
        assert_eq!(
            *info.cell_type,
            Type::Int,
            "fallback (literal 2) should have Int type"
        );
        // Exact byte-position assertions: span must start at the 'l' in
        // 'let fallback' and end immediately after the RHS literal '2'.
        // Using "= 2".len() (instead of + 1) makes the literal width
        // explicit and keeps the assertion resilient if the literal is
        // ever widened (e.g. "= 22") during a future edit.
        let expected_start = source.find("let fallback").unwrap() as u32;
        let expected_end = (source.find("= 2").unwrap() + "= 2".len()) as u32;
        assert_eq!(
            info.span.start, expected_start,
            "span.start should point at 'let fallback'"
        );
        assert_eq!(
            info.span.end, expected_end,
            "span.end should end after the '2' literal"
        );
        assert_eq!(info.decl_name, "S");
    }

    #[test]
    fn find_member_decl_param_only_in_else_branch() {
        // Covers the else-only-branch lookup path explicitly: a where-block
        // whose `else` contains the only occurrence of a named `param`.
        let source = r#"structure S {
    param cond : Bool = true
    where cond {
        param only_when_true : Length = 1mm
    } else {
        param only_in_else : Length = 7mm
    }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx
            .find_member_decl("only_in_else", None)
            .expect("only_in_else declared only in else branch should be found");
        assert_eq!(info.name, "only_in_else");
        assert_eq!(info.kind, ValueCellKind::Param);
        assert_eq!(
            *info.cell_type,
            Type::length(),
            "only_in_else should have length type"
        );
        let expected_start = source.find("param only_in_else").unwrap() as u32;
        let expected_end = (source.find("7mm").unwrap() + "7mm".len()) as u32;
        assert_eq!(
            info.span.start, expected_start,
            "span.start should point at 'param only_in_else'"
        );
        assert_eq!(
            info.span.end, expected_end,
            "span.end should end after '7mm'"
        );
        assert_eq!(info.decl_name, "S");
    }

    // --- refactor regression: top-level + guarded-group lookup coexistence ---

    #[test]
    fn find_parsed_member_span_and_doc_top_level_after_refactor_regression() {
        // Pins the contract that the refactored private
        // `find_parsed_member_span_and_doc` preserves top-level resolution
        // and did not accidentally divert all lookups through the
        // guarded-group path. Source contains BOTH a top-level param/let
        // AND a guarded-group param; every MemberInfo field is asserted.
        let source = r#"structure S {
    /// top-level param
    param top_p : Length = 3mm
    /// top-level let
    let top_l = 9
    param cond : Bool = true
    where cond {
        /// guarded param
        param guarded_p : Length = 4mm
    }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());

        // --- top-level param ---
        let top_p = ctx
            .find_member_decl("top_p", None)
            .expect("top-level param 'top_p' must still resolve after refactor");
        assert_eq!(top_p.name, "top_p");
        assert_eq!(top_p.kind, ValueCellKind::Param);
        assert_eq!(*top_p.cell_type, Type::length());
        assert_eq!(top_p.doc, Some("top-level param"));
        assert_eq!(top_p.decl_name, "S");
        let top_p_start = source.find("param top_p").unwrap() as u32;
        let top_p_end = (source.find("3mm").unwrap() + "3mm".len()) as u32;
        assert_eq!(top_p.span.start, top_p_start);
        assert_eq!(top_p.span.end, top_p_end);

        // --- top-level let ---
        let top_l = ctx
            .find_member_decl("top_l", None)
            .expect("top-level let 'top_l' must still resolve after refactor");
        assert_eq!(top_l.name, "top_l");
        assert_eq!(top_l.kind, ValueCellKind::Let);
        assert_eq!(*top_l.cell_type, Type::Int);
        assert_eq!(top_l.doc, Some("top-level let"));
        assert_eq!(top_l.decl_name, "S");
        let top_l_start = source.find("let top_l").unwrap() as u32;
        let nine_pos = source.find("= 9").unwrap() + "= ".len();
        let top_l_end = (nine_pos + 1) as u32;
        assert_eq!(top_l.span.start, top_l_start);
        assert_eq!(top_l.span.end, top_l_end);

        // --- guarded-group param (different code path) ---
        let guarded = ctx
            .find_member_decl("guarded_p", None)
            .expect("guarded-group param 'guarded_p' must resolve");
        assert_eq!(guarded.name, "guarded_p");
        assert_eq!(guarded.kind, ValueCellKind::Param);
        assert_eq!(*guarded.cell_type, Type::length());
        assert_eq!(guarded.doc, Some("guarded param"));
        assert_eq!(guarded.decl_name, "S");
        let guarded_start = source.find("param guarded_p").unwrap() as u32;
        let guarded_end = (source.find("4mm").unwrap() + "4mm".len()) as u32;
        assert_eq!(guarded.span.start, guarded_start);
        assert_eq!(guarded.span.end, guarded_end);
    }

    // --- decl_name field tests ---

    #[test]
    fn find_member_decl_decl_name_populated() {
        let source = "structure Foo {\n    param x: Length = 1mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx.find_member_decl("x", None).expect("x should exist");
        assert_eq!(info.decl_name, "Foo");
    }

    // --- enclosing_decl_name_at tests ---

    #[test]
    fn enclosing_decl_name_at_inside_second() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        // Offset inside B: 'y' in "param y: Bool = true"
        let b_y_offset = source.find("param y").unwrap() + 6;
        assert_eq!(
            ctx.enclosing_decl_name_at(b_y_offset),
            Some("B"),
            "offset inside B should return Some(\"B\")"
        );
        // Offset inside A: 'x' in "param x: Length = 5mm"
        let a_x_offset = source.find("param x").unwrap() + 6;
        assert_eq!(
            ctx.enclosing_decl_name_at(a_x_offset),
            Some("A"),
            "offset inside A should return Some(\"A\")"
        );
        // Offset outside any structure (between A and B)
        let between_offset = source.find("\nstructure B").unwrap();
        assert_eq!(
            ctx.enclosing_decl_name_at(between_offset),
            None,
            "offset between structures should return None"
        );
    }

    #[test]
    fn enclosing_decl_name_at_inside_occurrence() {
        let source = "occurrence def Joint {\n    param diameter: Length = 10mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let offset = source.find("diameter").unwrap();
        assert_eq!(
            ctx.enclosing_decl_name_at(offset),
            Some("Joint"),
            "offset inside occurrence should return Some(\"Joint\")"
        );
    }

    #[test]
    fn enclosing_decl_name_at_inside_enum_returns_none() {
        let source = "enum Color { Red, Green }\nstructure S {\n    param x: Length = 5mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        // Offset inside enum body: 'Red' variant
        let red_offset = source.find("Red").unwrap();
        assert_eq!(
            ctx.enclosing_decl_name_at(red_offset),
            None,
            "offset inside enum should return None (graceful degradation, not panic)"
        );
    }

    #[test]
    fn enclosing_decl_name_at_inside_trait() {
        let source = "trait Rigid {\n    param mass: Length = 5mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let offset = source.find("mass").unwrap();
        assert_eq!(
            ctx.enclosing_decl_name_at(offset),
            Some("Rigid"),
            "offset inside trait should return Some(\"Rigid\"), not None"
        );
    }

    #[test]
    fn enclosing_decl_name_at_inside_purpose() {
        let source = "purpose Assemble(part: Structure) {\n    let total = 42\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let offset = source.find("total").unwrap();
        assert_eq!(
            ctx.enclosing_decl_name_at(offset),
            Some("Assemble"),
            "offset inside purpose should return Some(\"Assemble\"), not None"
        );
    }

    // --- enclosing_decl_name_at (renamed method) tests ---

    #[test]
    fn enclosing_decl_name_at_delegates_to_free_function() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        // Offset inside B
        let b_y_offset = source.find("param y").unwrap() + 6;
        assert_eq!(
            ctx.enclosing_decl_name_at(b_y_offset),
            Some("B"),
            "renamed method should return same result as old method"
        );
        // Offset inside A
        let a_x_offset = source.find("param x").unwrap() + 6;
        assert_eq!(ctx.enclosing_decl_name_at(a_x_offset), Some("A"),);
        // Offset outside
        let between_offset = source.find("\nstructure B").unwrap();
        assert_eq!(ctx.enclosing_decl_name_at(between_offset), None,);
    }

    #[test]
    fn find_member_decl_scoped_to_second_structure() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param x: Bool = true\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx
            .find_member_decl("x", Some("B"))
            .expect("x should exist in B");
        assert_eq!(
            *info.cell_type,
            Type::Bool,
            "expected Bool type from structure B, got {:?}",
            info.cell_type
        );
        assert_eq!(info.decl_name, "B");
        // Span should be within B's byte range
        let b_start = source.find("structure B").unwrap() as u32;
        assert!(
            info.span.start >= b_start,
            "span should be within B's range"
        );
    }

    // --- ambiguous member name regression tests ---

    #[test]
    fn find_member_decl_ambiguous_name_returns_first_decl_consistently() {
        // Two structures with identically-named params but different types.
        // find_member_decl must return span AND type from the same declaration.
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param x: Bool = true\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx.find_member_decl("x", None).expect("x should exist");
        // Should return type from A (first match) — Scalar with LENGTH dimension
        assert!(
            matches!(info.cell_type, Type::Scalar { .. }),
            "expected Scalar type from first declaration A, got {:?}",
            info.cell_type
        );
        // Span should be within A's byte range (before B starts)
        let b_start = source.find("structure B").unwrap() as u32;
        assert!(
            info.span.end <= b_start,
            "span should be within structure A's byte range, span.end={} but B starts at {}",
            info.span.end,
            b_start
        );
    }

    #[test]
    fn find_member_decl_ambiguous_name_second_decl_type_not_leaked() {
        // Verify the returned cell_type is NOT Bool (proving type didn't leak from B).
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param x: Bool = true\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx.find_member_decl("x", None).expect("x should exist");
        assert_ne!(
            *info.cell_type,
            Type::Bool,
            "type should not leak from second declaration B"
        );
    }

    #[test]
    fn find_member_decl_non_first_decl_propagates_decl_name() {
        // "y" exists only in B (the second declaration).
        // find_member_decl("y", None) must iterate past A and find y in B,
        // returning decl_name="B" (not "A") and a span within B's byte range.
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Length = 10mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx
            .find_member_decl("y", None)
            .expect("y should exist in B");
        assert_eq!(info.name, "y");
        assert_eq!(info.kind, ValueCellKind::Param);
        assert_eq!(info.decl_name, "B", "decl_name should be B, not A");
        // Span should be within B's byte range
        let b_start = source.find("structure B").unwrap() as u32;
        assert!(
            info.span.start >= b_start,
            "span should be within B's range, span.start={} but B starts at {}",
            info.span.start,
            b_start
        );
    }

    // --- member_names_for_structure tests ---

    #[test]
    fn member_names_for_structure_returns_scoped_members() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let a_members = ctx.member_names_for_structure("A");
        let a_names: Vec<&str> = a_members.iter().map(|(n, _, _)| *n).collect();
        assert_eq!(a_names, vec!["x"], "A should only have 'x'");

        let b_members = ctx.member_names_for_structure("B");
        let b_names: Vec<&str> = b_members.iter().map(|(n, _, _)| *n).collect();
        assert_eq!(b_names, vec!["y"], "B should only have 'y'");

        // Non-existent structure returns empty
        let empty = ctx.member_names_for_structure("C");
        assert!(
            empty.is_empty(),
            "non-existent structure should return empty"
        );
    }

    // --- format_value tests ---

    #[test]
    fn format_value_scalar_length() {
        let v = Value::Scalar {
            si_value: 0.08,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(format_value(&v), "0.08 m");
    }

    #[test]
    fn format_value_scalar_money_renders_usd() {
        let v = Value::Scalar {
            si_value: 25.0,
            dimension: DimensionVector::MONEY,
        };
        assert_eq!(format_value(&v), "25 USD");
    }

    #[test]
    fn format_value_bool() {
        assert_eq!(format_value(&Value::Bool(true)), "true");
        assert_eq!(format_value(&Value::Bool(false)), "false");
    }

    #[test]
    fn format_value_int() {
        assert_eq!(format_value(&Value::Int(42)), "42");
    }

    #[test]
    fn format_value_real() {
        assert_eq!(format_value(&Value::Real(3.125)), "3.125");
    }

    #[test]
    fn format_value_string() {
        assert_eq!(format_value(&Value::String("hello".into())), "\"hello\"");
    }

    #[test]
    fn format_value_undef() {
        assert_eq!(format_value(&Value::Undef), "(undefined)");
    }

    // --- format_value for M5 types (step-13) ---

    #[test]
    fn format_value_enum() {
        let v = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        };
        assert_eq!(format_value(&v), "Color::Red");
    }

    #[test]
    fn format_value_list() {
        let v = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(format_value(&v), "[1, 2, 3]");
    }

    #[test]
    fn format_value_set() {
        use std::collections::BTreeSet;
        let mut s = BTreeSet::new();
        s.insert(Value::Int(1));
        s.insert(Value::Int(2));
        let v = Value::Set(s);
        assert_eq!(format_value(&v), "{1, 2}");
    }

    #[test]
    fn format_value_map() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("a".into()), Value::Int(1));
        let v = Value::Map(m);
        assert_eq!(format_value(&v), "{\"a\": 1}");
    }

    #[test]
    fn format_value_option_none() {
        assert_eq!(format_value(&Value::Option(None)), "none");
    }

    #[test]
    fn format_value_option_some() {
        let v = Value::Option(Some(Box::new(Value::Int(42))));
        assert_eq!(format_value(&v), "some(42)");
    }

    // --- format_value for Complex (step-11) ---

    #[test]
    fn format_value_complex_positive_im() {
        let v = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format_value(&v), "3 + 4i");
    }

    #[test]
    fn format_value_complex_negative_im() {
        // Negative imaginary must display as '3 - 4i', NOT '3 + -4i'.
        let v = Value::Complex {
            re: 3.0,
            im: -4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(format_value(&v), "3 - 4i");
    }

    #[test]
    fn format_value_complex_dimensioned() {
        let v = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(format_value(&v), "3 + 4i m");
    }

    // --- port internal member tests ---

    #[test]
    fn find_named_member_span_finds_port_internal_param() {
        let source = r#"structure S {
    port x : MechPort { param d : Length = 10mm }
}"#;
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let structure = match &parsed.declarations[0] {
            reify_ast::Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };
        let result = find_named_member_span(&structure.members, "d");
        assert!(
            result.is_some(),
            "d inside port body should be found via find_named_member_span"
        );
        let info = result.unwrap();
        let decl_text = &source[info.span.start as usize..info.span.end as usize];
        assert!(
            decl_text.contains("d") && decl_text.contains("10mm"),
            "span should cover full param declaration, got: {decl_text:?}"
        );
    }

    #[test]
    fn entity_names_counts_port_with_guarded_group() {
        // Verifies that count_members_recursive correctly handles both Port
        // and GuardedGroup recursion in the same structure. The tree-sitter
        // grammar does not support where-blocks inside port bodies, so we
        // test them at the same level instead.
        let source = r#"structure S {
    param cond : Bool = true
    port x : MechPort { param d : Length = 10mm  constraint d > 0mm }
    where cond {
        param guarded_p : Length = 5mm
    }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let entities = ctx.entity_names();
        assert_eq!(entities.len(), 1);
        let e = &entities[0];
        assert_eq!(e.name, "S");
        assert_eq!(e.kind, EntityKind::Structure);
        // Should count: cond + d (inside port) + guarded_p (inside where) = 3 params
        assert_eq!(
            e.params, 3,
            "expected 3 params (cond, d inside port, guarded_p inside where), got {}",
            e.params
        );
        assert_eq!(e.lets, 0, "expected 0 lets, got {}", e.lets);
        // Should count: d > 0mm (inside port) = 1 constraint
        assert_eq!(
            e.constraints, 1,
            "expected 1 constraint (d > 0mm inside port), got {}",
            e.constraints
        );
    }

    #[test]
    fn find_named_member_span_finds_port_internal_let() {
        let source = r#"structure S {
    port x : MechPort { let ratio = 2 }
}"#;
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let structure = match &parsed.declarations[0] {
            reify_ast::Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };
        let result = find_named_member_span(&structure.members, "ratio");
        assert!(
            result.is_some(),
            "ratio inside port body should be found via find_named_member_span"
        );
        let info = result.unwrap();
        let decl_text = &source[info.span.start as usize..info.span.end as usize];
        assert!(
            decl_text.contains("ratio"),
            "span should cover the let declaration, got: {decl_text:?}"
        );
    }

    #[test]
    fn entity_names_counts_port_internal_members() {
        let source = r#"structure S {
    param a : Length = 1mm
    port x : MechPort { param d : Length = 10mm  let ratio = 2  constraint d > 0mm }
}"#;
        let ctx = AnalysisContext::new(source, &test_uri());
        let entities = ctx.entity_names();
        assert_eq!(entities.len(), 1);
        let e = &entities[0];
        assert_eq!(e.name, "S");
        assert_eq!(e.kind, EntityKind::Structure);
        // Should count: a + d = 2 params (d is inside port body)
        assert_eq!(
            e.params, 2,
            "expected 2 params (a, d inside port), got {}",
            e.params
        );
        // Should count: ratio = 1 let (inside port body)
        assert_eq!(
            e.lets, 1,
            "expected 1 let (ratio inside port), got {}",
            e.lets
        );
        // Should count: d > 0mm = 1 constraint (inside port body)
        assert_eq!(
            e.constraints, 1,
            "expected 1 constraint (d > 0mm inside port), got {}",
            e.constraints
        );
    }

    // --- enclosing_decl_at free function tests ---

    #[test]
    fn enclosing_decl_at_returns_structure_for_offset_inside() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        // Offset inside A: 'x' in "param x: Length = 5mm"
        let a_x_offset = source.find("param x").unwrap() + 6;
        let decl = enclosing_decl_at(&parsed.declarations, a_x_offset);
        assert!(decl.is_some(), "offset inside A should return Some");
        match decl.unwrap() {
            reify_ast::Declaration::Structure(s) => assert_eq!(s.name, "A"),
            other => panic!("expected Structure A, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_returns_correct_structure_for_second_decl() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        // Offset inside B: 'y' in "param y: Bool = true"
        let b_y_offset = source.find("param y").unwrap() + 6;
        let decl = enclosing_decl_at(&parsed.declarations, b_y_offset);
        assert!(decl.is_some(), "offset inside B should return Some");
        match decl.unwrap() {
            reify_ast::Declaration::Structure(s) => assert_eq!(s.name, "B"),
            other => panic!("expected Structure B, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_returns_occurrence() {
        let source = "occurrence def Joint {\n    param diameter: Length = 10mm\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let offset = source.find("diameter").unwrap();
        let decl = enclosing_decl_at(&parsed.declarations, offset);
        assert!(
            decl.is_some(),
            "offset inside occurrence should return Some"
        );
        match decl.unwrap() {
            reify_ast::Declaration::Occurrence(o) => assert_eq!(o.name, "Joint"),
            other => panic!("expected Occurrence Joint, got {:?}", other),
        }
    }

    #[test]
    fn enclosing_decl_at_returns_none_between_declarations() {
        let source = "structure A {\n    param x: Length = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        // Offset between A and B (the newline between them)
        let between_offset = source.find("\nstructure B").unwrap();
        let decl = enclosing_decl_at(&parsed.declarations, between_offset);
        assert!(
            decl.is_none(),
            "offset between declarations should return None"
        );
    }

    #[test]
    fn enclosing_decl_at_returns_none_for_offset_past_end() {
        let source = "structure A {\n    param x: Length = 5mm\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        let decl = enclosing_decl_at(&parsed.declarations, source.len() + 100);
        assert!(decl.is_none(), "offset past end should return None");
    }

    #[test]
    fn enclosing_decl_at_empty_declarations() {
        let decl = enclosing_decl_at(&[], 0);
        assert!(decl.is_none(), "empty declarations should return None");
    }

    // --- depth-limit tests for find_named_member_span ---

    /// Build a member tree with `depth` levels of GuardedGroup nesting,
    /// placing a single Param named `target` at the innermost level.
    fn build_nested_guarded_members(depth: usize, target: &str) -> Vec<reify_ast::MemberDecl> {
        use reify_ast::{Expr, ExprKind, GuardedGroupDecl, MemberDecl, ParamDecl};
        use reify_core::{ContentHash, SourceSpan};

        let dummy_span = SourceSpan::new(0, 1);
        let dummy_hash = ContentHash(0);
        let dummy_expr = Expr {
            kind: ExprKind::BoolLiteral(true),
            span: dummy_span,
        };

        // Innermost level: a single param with the target name
        let leaf = vec![MemberDecl::Param(ParamDecl {
            name: target.to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: Vec::new(),
            span: dummy_span,
            content_hash: dummy_hash,
        })];

        // Wrap in `depth` levels of GuardedGroup
        let mut current = leaf;
        for _ in 0..depth {
            current = vec![MemberDecl::GuardedGroup(GuardedGroupDecl {
                condition: dummy_expr.clone(),
                members: current,
                else_members: vec![],
                span: dummy_span,
                content_hash: dummy_hash,
            })];
        }
        current
    }

    #[test]
    fn find_named_member_span_succeeds_within_depth_limit() {
        // 5 levels of nesting — well within the 32-level limit
        let members = build_nested_guarded_members(5, "deep_param");
        let result = find_named_member_span(&members, "deep_param");
        assert!(
            result.is_some(),
            "param at 5 levels of nesting should be found"
        );
    }

    #[test]
    fn find_named_member_span_returns_none_beyond_depth_limit() {
        // 33 levels of nesting — beyond the MAX_MEMBER_NESTING_DEPTH (32)
        let members = build_nested_guarded_members(33, "unreachable_param");
        let result = find_named_member_span(&members, "unreachable_param");
        assert!(
            result.is_none(),
            "param at 33 levels of nesting should NOT be found (depth limit exceeded)"
        );
    }

    // --- task-2176 step-7: AnalysisContext resolves stdlib types ---

    #[test]
    fn analysis_context_resolves_stdlib_types() {
        // Source references Rigid trait and Material struct from stdlib.
        // With the empty-prelude compile() these produce ERROR diagnostics;
        // after swapping to compile_with_stdlib the module must be error-free.
        let uri = Url::parse("file:///test.ri").unwrap();
        let ctx = AnalysisContext::new(STDLIB_PROBE_SRC, &uri);
        let errors: Vec<_> = ctx
            .compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == reify_core::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "AnalysisContext: stdlib source should compile without errors; \
             got: {errors:?}"
        );
    }

    // --- compute_document_symbols tests (task 4207 η) ---

    #[test]
    fn compute_document_symbols_single_structure_top_level() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = "structure Bracket { param width: Length = 80mm }";
        let symbols = compute_document_symbols(source, &test_uri());
        assert_eq!(
            symbols.len(),
            1,
            "should return exactly one top-level symbol for one structure"
        );
        let bracket = &symbols[0];
        assert_eq!(bracket.name, "Bracket");
        assert_eq!(bracket.kind, SymbolKind::STRUCT);
        // range covers the full declaration, which starts on line 0.
        assert_eq!(bracket.range.start.line, 0);
        // selection_range is the "Bracket" name token: line 0, chars 10..17
        // ("structure " is 10 chars; "Bracket" is 7 chars).
        assert_eq!(bracket.selection_range.start.line, 0);
        assert_eq!(bracket.selection_range.start.character, 10);
        assert_eq!(bracket.selection_range.end.line, 0);
        assert_eq!(bracket.selection_range.end.character, 17);
    }

    /// Regression (task 4207 η amendment): `name_selection_range` must preserve
    /// the LSP `selection_range ⊆ range` invariant even for a degenerate span
    /// shorter than the name. Here the span is 2 bytes wide but the name is 7
    /// bytes, so the word-boundary search fails and the `span.start` fallback
    /// runs; without the end-clamp the selection end would be `0 + 7 = 7`,
    /// pushing `selection_range.end` past `range.end`.
    #[test]
    fn name_selection_range_clamps_end_to_span_for_degenerate_span() {
        let source = "Bracket";
        let span = SourceSpan::new(0, 2);
        let sel = name_selection_range(source, span, "Bracket");
        let range = span_to_range(source, span);
        assert!(
            (sel.start.line, sel.start.character) >= (range.start.line, range.start.character),
            "selection_range.start {:?} must be >= range.start {:?}",
            sel.start,
            range.start,
        );
        assert!(
            (sel.end.line, sel.end.character) <= (range.end.line, range.end.character),
            "selection_range.end {:?} must be clamped within range.end {:?}",
            sel.end,
            range.end,
        );
    }

    /// Assert `sym.selection_range` lies within `sym.range` and covers exactly
    /// `sym.name` in `source`. Shared across the document-symbol tests.
    fn assert_selection_on_name(source: &str, sym: &DocumentSymbol) {
        let r = &sym.range;
        let sr = &sym.selection_range;
        assert!(
            (sr.start.line, sr.start.character) >= (r.start.line, r.start.character),
            "selection_range.start {:?} should be >= range.start {:?} for {}",
            sr.start,
            r.start,
            sym.name
        );
        assert!(
            (sr.end.line, sr.end.character) <= (r.end.line, r.end.character),
            "selection_range.end {:?} should be <= range.end {:?} for {}",
            sr.end,
            r.end,
            sym.name
        );
        let start = crate::convert::position_to_offset(source, sr.start);
        let end = crate::convert::position_to_offset(source, sr.end);
        assert_eq!(
            &source[start..end],
            sym.name,
            "selection_range should cover the name token for {}",
            sym.name
        );
    }

    #[test]
    fn compute_document_symbols_members_as_children() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = reify_test_support::bracket_source();
        let symbols = compute_document_symbols(source, &test_uri());
        assert_eq!(symbols.len(), 1, "bracket_source has one structure");
        let bracket = &symbols[0];
        assert_eq!(bracket.name, "Bracket");

        let children = bracket
            .children
            .as_ref()
            .expect("Bracket should have children");
        // 5 params + 2 lets = 7 named members; the 3 unlabeled constraints
        // are NOT navigable symbols and must be excluded.
        assert_eq!(
            children.len(),
            7,
            "expected 7 named members (constraints excluded), got: {:?}",
            children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>()
        );

        // Source order: the 5 params, then volume, then body.
        let expected: [(&str, SymbolKind); 7] = [
            ("width", SymbolKind::FIELD),
            ("height", SymbolKind::FIELD),
            ("thickness", SymbolKind::FIELD),
            ("fillet_radius", SymbolKind::FIELD),
            ("hole_diameter", SymbolKind::FIELD),
            ("volume", SymbolKind::VARIABLE),
            ("body", SymbolKind::VARIABLE),
        ];
        for (child, (name, kind)) in children.iter().zip(expected.iter()) {
            assert_eq!(&child.name, name, "child name/order mismatch");
            assert_eq!(child.kind, *kind, "child {name} kind mismatch");
            assert!(
                child.children.is_none() || child.children.as_ref().unwrap().is_empty(),
                "leaf member {name} should have no children"
            );
            assert_selection_on_name(source, child);
        }
    }

    #[test]
    fn compute_document_symbols_occurrence_and_trait() {
        use tower_lsp::lsp_types::SymbolKind;

        // --- occurrence → CLASS with param FIELD + let VARIABLE children ---
        let occ_source =
            "occurrence def Joint {\n    param diameter: Length = 10mm\n    let radius = diameter / 2\n}";
        let occ_symbols = compute_document_symbols(occ_source, &test_uri());
        assert_eq!(occ_symbols.len(), 1, "one occurrence → one top-level symbol");
        let joint = &occ_symbols[0];
        assert_eq!(joint.name, "Joint");
        assert_eq!(joint.kind, SymbolKind::CLASS);
        assert_selection_on_name(occ_source, joint);
        let occ_children = joint
            .children
            .as_ref()
            .expect("Joint should have children");
        assert_eq!(occ_children.len(), 2, "Joint has diameter + radius");
        assert_eq!(occ_children[0].name, "diameter");
        assert_eq!(occ_children[0].kind, SymbolKind::FIELD);
        assert_eq!(occ_children[1].name, "radius");
        assert_eq!(occ_children[1].kind, SymbolKind::VARIABLE);
        for c in occ_children {
            assert_selection_on_name(occ_source, c);
        }

        // --- trait → INTERFACE with param FIELD child ---
        let trait_source = "trait Rigid {\n    param mass: Length = 5mm\n}";
        let trait_symbols = compute_document_symbols(trait_source, &test_uri());
        assert_eq!(trait_symbols.len(), 1, "one trait → one top-level symbol");
        let rigid = &trait_symbols[0];
        assert_eq!(rigid.name, "Rigid");
        assert_eq!(rigid.kind, SymbolKind::INTERFACE);
        assert_selection_on_name(trait_source, rigid);
        let trait_children = rigid
            .children
            .as_ref()
            .expect("Rigid should have children");
        assert_eq!(trait_children.len(), 1, "Rigid has mass");
        assert_eq!(trait_children[0].name, "mass");
        assert_eq!(trait_children[0].kind, SymbolKind::FIELD);
        assert_selection_on_name(trait_source, &trait_children[0]);
    }

    #[test]
    fn compute_document_symbols_enum_variants() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = "enum Shape { Point, Circle { radius: Length } }";
        let symbols = compute_document_symbols(source, &test_uri());
        assert_eq!(symbols.len(), 1, "one enum → one top-level symbol");
        let shape = &symbols[0];
        assert_eq!(shape.name, "Shape");
        assert_eq!(shape.kind, SymbolKind::ENUM);
        assert_selection_on_name(source, shape);

        let variants = shape
            .children
            .as_ref()
            .expect("Shape should have variant children");
        assert_eq!(variants.len(), 2, "Shape has Point + Circle in source order");
        assert_eq!(variants[0].name, "Point");
        assert_eq!(variants[0].kind, SymbolKind::ENUM_MEMBER);
        assert_eq!(variants[1].name, "Circle");
        assert_eq!(variants[1].kind, SymbolKind::ENUM_MEMBER);
        for v in variants {
            // range covers the variant span; selection_range sits on the name.
            assert_selection_on_name(source, v);
            assert!(
                v.children.is_none() || v.children.as_ref().unwrap().is_empty(),
                "variant {} should have no grandchildren in this task",
                v.name
            );
        }
    }

    #[test]
    fn compute_document_symbols_fn_and_excludes_non_symbol_decls() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = "import std.math\nunit meter : Length\nfn area(w: Length) -> Length { w }";
        let symbols = compute_document_symbols(source, &test_uri());
        // import + unit are NOT navigable symbols; only the fn is.
        assert_eq!(
            symbols.len(),
            1,
            "only the fn should be a symbol (import + unit excluded), got: {:?}",
            symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
        );
        let area = &symbols[0];
        assert_eq!(area.name, "area");
        assert_eq!(area.kind, SymbolKind::FUNCTION);
        assert!(
            area.children.is_none() || area.children.as_ref().unwrap().is_empty(),
            "fn is a leaf symbol (params are not surfaced as children)"
        );
        assert_selection_on_name(source, area);
    }

    #[test]
    fn compute_document_symbols_sub_port_and_guarded() {
        use tower_lsp::lsp_types::SymbolKind;
        let source = r#"structure Assembly {
    param cond : Bool = true
    sub motor = Motor()
    port intake : MechPort { param d : Length = 5mm }
    where cond {
        param guarded_x : Length = 1mm
    }
}"#;
        let symbols = compute_document_symbols(source, &test_uri());
        assert_eq!(symbols.len(), 1, "one structure → one top-level symbol");
        let assembly = &symbols[0];
        assert_eq!(assembly.name, "Assembly");
        let children = assembly
            .children
            .as_ref()
            .expect("Assembly should have children");
        let find = |name: &str| children.iter().find(|c| c.name == name);

        // sub motor → OBJECT
        let motor = find("motor").expect("motor sub should be a child symbol");
        assert_eq!(motor.kind, SymbolKind::OBJECT);
        assert_selection_on_name(source, motor);

        // port intake → INTERFACE, with port-internal param d → FIELD grandchild
        let intake = find("intake").expect("intake port should be a child symbol");
        assert_eq!(intake.kind, SymbolKind::INTERFACE);
        assert_selection_on_name(source, intake);
        let intake_children = intake
            .children
            .as_ref()
            .expect("intake port should have internal members");
        let d = intake_children
            .iter()
            .find(|c| c.name == "d")
            .expect("port-internal param d should be a grandchild");
        assert_eq!(d.kind, SymbolKind::FIELD);
        assert_selection_on_name(source, d);

        // where-block param guarded_x flattened as a direct child of the structure
        let guarded = find("guarded_x").expect("guarded_x should be flattened as a direct child");
        assert_eq!(guarded.kind, SymbolKind::FIELD);
        assert_selection_on_name(source, guarded);
    }

    // --- step-11: injectable document-symbol core over a shared ParsedModule ---

    /// `compute_document_symbols_from_parsed`, fed a `ParsedModule` built once
    /// by the caller, must yield the same symbol tree as the
    /// `compute_document_symbols` wrapper (which parses internally) — proving
    /// the cache-fed core is output-equivalent to the per-request path.
    #[test]
    fn compute_document_symbols_from_parsed_matches_wrapper() {
        // Multi-declaration source spanning every navigable symbol kind plus
        // members, so the equivalence covers names, kinds, ranges, and the
        // nested children tree — not just the top-level list.
        let source = r#"structure Bracket {
    param width : Length = 80mm
    sub motor = Motor()
}
occurrence def Joint {
    param diameter : Length = 10mm
}
trait Rigid {
    param mass : Length = 5mm
}
enum Shape { Circle, Square }
fn area(w: Length) -> Length { w }"#;
        let uri = test_uri();

        let parsed = reify_compiler::parse_with_stdlib(
            source,
            ModulePath::single(module_name_from_uri(&uri)),
        );

        let via_parsed = compute_document_symbols_from_parsed(&parsed, source);
        let via_wrapper = compute_document_symbols(source, &uri);

        assert!(
            !via_parsed.is_empty(),
            "multi-declaration source should yield symbols"
        );
        assert_eq!(
            via_parsed, via_wrapper,
            "from-parsed document symbols must match the wrapper (names/kinds/ranges/children)"
        );
    }

    // --- name_token_span tests (step-1) ---
    //
    // `name_token_span(source, member_span, name)` narrows a member-statement
    // span down to the span of just the NAME identifier token. Expected offsets
    // are derived from `source.find` so the assertions stay fixture-robust.

    #[test]
    fn name_token_span_param_width_covers_just_the_name() {
        let source = "param width: Length = 80mm";
        let member_span = SourceSpan::new(0, source.len() as u32);
        let span = name_token_span(source, member_span, "width");
        let start = source.find("width").unwrap() as u32;
        let end = start + "width".len() as u32;
        assert_eq!(span, SourceSpan::new(start, end));
        assert_eq!(&source[span.start as usize..span.end as usize], "width");
    }

    #[test]
    fn name_token_span_let_volume_covers_just_the_name() {
        let source = "let volume = width * height * thickness";
        let member_span = SourceSpan::new(0, source.len() as u32);
        let span = name_token_span(source, member_span, "volume");
        let start = source.find("volume").unwrap() as u32;
        let end = start + "volume".len() as u32;
        assert_eq!(span, SourceSpan::new(start, end));
        assert_eq!(&source[span.start as usize..span.end as usize], "volume");
    }

    #[test]
    fn name_token_span_indented_guarded_param_covers_just_the_name() {
        // A guarded member's statement span begins at the indented `param`
        // keyword. name_token_span must still land on the identifier token,
        // searching forward from the (non-zero) member-span start.
        let source = "        param guarded_x : Length = 5mm";
        let decl_start = source.find("param").unwrap() as u32;
        let member_span = SourceSpan::new(decl_start, source.len() as u32);
        let span = name_token_span(source, member_span, "guarded_x");
        let start = source.find("guarded_x").unwrap() as u32;
        let end = start + "guarded_x".len() as u32;
        assert_eq!(span, SourceSpan::new(start, end));
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "guarded_x"
        );
    }

    #[test]
    fn name_token_span_absent_name_falls_back_within_member_not_sibling() {
        // The member span covers ONLY the first statement (`param alpha …`), which
        // does NOT contain the name `beta`. `beta` appears only in the LATER
        // sibling statement. name_token_span must fall back to an empty span at the
        // member start rather than borrowing the sibling's `beta` token — a
        // malformed/recovered node whose span lacks its own name must not match a
        // same-named token elsewhere in the source.
        let source = "param alpha: Length = 1mm\nlet beta = alpha";
        let member_end = source.find('\n').unwrap() as u32; // end of `param alpha …`
        let member_span = SourceSpan::new(0, member_end);

        let span = name_token_span(source, member_span, "beta");
        assert_eq!(
            span,
            SourceSpan::empty(0),
            "name absent from the member span must fall back to empty, not borrow \
             the sibling `beta` token at {:?}",
            source.find("beta"),
        );
        assert!(span.is_empty(), "fallback span must be empty");
    }

    // ── undef_cause_line tests ─────────────────────────────────────────────────

    /// (a) An unbound required param → undef_cause_line returns Some(line) that
    /// contains "because", the member name, and "unbound".
    #[test]
    fn undef_cause_line_unbound_param_returns_cause() {
        let source = "structure S {\n    param outer_d: Length\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let line = ctx
            .undef_cause_line("S", "outer_d")
            .expect("unbound param should have a cause line");
        assert!(
            line.to_lowercase().contains("because"),
            "cause line should contain 'because', got: {line:?}"
        );
        assert!(
            line.contains("outer_d"),
            "cause line should contain the member name 'outer_d', got: {line:?}"
        );
        assert!(
            line.contains("unbound"),
            "cause line should contain 'unbound', got: {line:?}"
        );
    }

    /// (b) A determined param (bracket_source width = 80mm) → undef_cause_line returns None.
    #[test]
    fn undef_cause_line_determined_param_returns_none() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let line = ctx.undef_cause_line("Bracket", "width");
        assert!(
            line.is_none(),
            "determined param should return None, got: {line:?}"
        );
    }

}
