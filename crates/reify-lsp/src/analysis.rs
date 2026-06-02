use reify_compiler::{CompiledModule, EntityKind, ValueCellKind};
use reify_constraints::SimpleConstraintChecker;
use reify_eval::CheckResult;
use reify_ast::{Declaration, ParsedModule};
pub use reify_ast::{MemberSpanInfo, find_named_member_span};
use reify_core::{ModulePath, SourceSpan, Type, ValueCellId};
use reify_ir::Value;
use tower_lsp::lsp_types::Url;

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
    pub parsed: ParsedModule,
    pub compiled: CompiledModule,
    pub check_result: CheckResult,
}

impl AnalysisContext {
    /// Build a new analysis context by running the full pipeline.
    pub fn new(source: &str, uri: &Url) -> Self {
        let module_name = module_name_from_uri(uri);
        // Prelude-aware parse so stdlib enum references like `CorrosionClass.C5`
        // disambiguate to `EnumAccess`; pairs with `compile_with_stdlib` below.
        // See task 2525.
        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));
        let compiled = reify_compiler::compile_with_stdlib(&parsed);
        let checker = SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        let check_result = engine.check(&compiled);

        Self {
            parsed,
            compiled,
            check_result,
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
            Declaration::Module(m) => m.span,
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
    // density/volume/centroid_x/y/z params were retired. Rigid still refines
    // Physical and adds moment_of_inertia. Mirrors the canonical spec-shape
    // fixture in structural_physical_tests.rs.
    const STDLIB_PROBE_SRC: &str = r#"structure S : Rigid {
    param geometry: Solid = box(10mm, 20mm, 30mm)
    param material: Material = Material(name: "steel", density: 7850.0, youngs_modulus: 200000000000.0)
    param moment_of_inertia: Real = 1.0
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
        // Span starts at the 'p' in 'param width: Scalar = 80mm'
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
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n}";
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
        param guarded_x : Scalar = 5mm
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
        param when_true : Scalar = 1mm
    } else {
        param when_false : Scalar = 2mm
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
        let source = "structure Bracket {\n    param width: Scalar = 80mm\n}\noccurrence def Joint {\n    param diameter: Scalar = 10mm\n    let radius = diameter / 2\n    constraint diameter > 5mm\n}";
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
            param deep : Scalar = 1mm
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
        param when_true : Scalar = 1mm
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
    param b : Scalar = 1mm
    where a {
        param guarded_x : Scalar = 5mm
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param x: Scalar = 20mm\n}";
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
        let source = "/// A bracket.\nstructure Bracket {\n    param width: Scalar = 80mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        assert_eq!(ctx.find_entity_doc("Bracket"), Some("A bracket."));
    }

    #[test]
    fn find_entity_doc_returns_doc_for_occurrence() {
        let source =
            "/// A joint process.\noccurrence def Joint {\n    param diameter: Scalar = 10mm\n}";
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
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        assert_eq!(ctx.find_entity_doc("Joint"), None);
    }

    #[test]
    fn member_info_includes_doc_for_documented_param() {
        let source = "structure Bracket {\n    /// The width.\n    param width: Scalar = 80mm\n}";
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
        param guarded_x : Scalar = 5mm
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
            param deep_x : Scalar = 1mm
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
        param a : Scalar = 1mm
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
        param only_when_true : Scalar = 1mm
    } else {
        param only_in_else : Scalar = 7mm
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
    param top_p : Scalar = 3mm
    /// top-level let
    let top_l = 9
    param cond : Bool = true
    where cond {
        /// guarded param
        param guarded_p : Scalar = 4mm
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
        let source = "structure Foo {\n    param x: Scalar = 1mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx.find_member_decl("x", None).expect("x should exist");
        assert_eq!(info.decl_name, "Foo");
    }

    // --- enclosing_decl_name_at tests ---

    #[test]
    fn enclosing_decl_name_at_inside_second() {
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        // Offset inside B: 'y' in "param y: Bool = true"
        let b_y_offset = source.find("param y").unwrap() + 6;
        assert_eq!(
            ctx.enclosing_decl_name_at(b_y_offset),
            Some("B"),
            "offset inside B should return Some(\"B\")"
        );
        // Offset inside A: 'x' in "param x: Scalar = 5mm"
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
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n}";
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
        let source = "enum Color { Red, Green }\nstructure S {\n    param x: Scalar = 5mm\n}";
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
        let source = "trait Rigid {\n    param mass: Scalar = 5mm\n}";
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param x: Bool = true\n}";
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param x: Bool = true\n}";
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param x: Bool = true\n}";
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param y: Scalar = 10mm\n}";
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
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
        param guarded_p : Scalar = 5mm
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
    param a : Scalar = 1mm
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        // Offset inside A: 'x' in "param x: Scalar = 5mm"
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
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
        let source = "occurrence def Joint {\n    param diameter: Scalar = 10mm\n}";
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param y: Bool = true\n}";
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
        let source = "structure A {\n    param x: Scalar = 5mm\n}";
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
        let source = "structure Bracket { param width: Scalar = 80mm }";
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
}
