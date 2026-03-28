use reify_compiler::{CompiledModule, ValueCellKind};
use reify_constraints::SimpleConstraintChecker;
use reify_eval::CheckResult;
use reify_syntax::ParsedModule;
use reify_types::{ModulePath, SourceSpan, Type, Value, ValueCellId};
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
        let parsed = reify_syntax::parse(source, ModulePath::single(module_name));
        let compiled = reify_compiler::compile(&parsed);
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
    /// Returns `None` if no value cell with that name exists.
    pub fn find_member_decl(&self, name: &str) -> Option<MemberInfo<'_>> {
        // Get the span, doc, and owning declaration name from the parsed module
        let (span, doc, decl_name) = self.find_parsed_member_span_and_doc(name)?;

        // Find type info from the compiled module, scoped to the same declaration
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
                    });
                }
            }
            // Also search inside guarded groups (where blocks)
            for group in &template.guarded_groups {
                for vc in group.members.iter().chain(group.else_members.iter()) {
                    if vc.id.member == name {
                        return Some(MemberInfo {
                            name: &vc.id.member,
                            kind: vc.kind,
                            cell_type: &vc.cell_type,
                            span,
                            doc,
                        });
                    }
                }
            }
        }

        None
    }

    /// Find the source span, doc comment, and owning declaration name for a
    /// named member in the parsed module.
    fn find_parsed_member_span_and_doc(
        &self,
        name: &str,
    ) -> Option<(SourceSpan, Option<&str>, &str)> {
        for decl in &self.parsed.declarations {
            let (members, decl_name) = match decl {
                reify_syntax::Declaration::Structure(s) => (&s.members, s.name.as_str()),
                reify_syntax::Declaration::Occurrence(o) => (&o.members, o.name.as_str()),
                _ => continue,
            };
            if let Some((span, doc)) = find_named_member_span(members, name) {
                return Some((span, doc, decl_name));
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
                reify_syntax::Declaration::Structure(s) if s.name == name => {
                    return s.doc.as_deref();
                }
                reify_syntax::Declaration::Occurrence(o) if o.name == name => {
                    return o.doc.as_deref();
                }
                reify_syntax::Declaration::Function(f) if f.name == name => {
                    return f.doc.as_deref();
                }
                reify_syntax::Declaration::Trait(t) if t.name == name => return t.doc.as_deref(),
                reify_syntax::Declaration::Enum(e) if e.name == name => return e.doc.as_deref(),
                _ => {}
            }
        }
        None
    }

    /// Return all value cell members: (name, kind, type).
    pub fn member_names(&self) -> Vec<(&str, ValueCellKind, &Type)> {
        let mut result = Vec::new();
        for template in &self.compiled.templates {
            for vc in &template.value_cells {
                result.push((vc.id.member.as_str(), vc.kind, &vc.cell_type));
            }
            // Also include members inside guarded groups (where blocks)
            for group in &template.guarded_groups {
                for vc in group.members.iter().chain(group.else_members.iter()) {
                    result.push((vc.id.member.as_str(), vc.kind, &vc.cell_type));
                }
            }
        }
        result
    }

    /// Return all structure/occurrence names with member counts:
    /// `(name, param_count, let_count, constraint_count, kind)`.
    pub fn structure_names(&self) -> Vec<(&str, usize, usize, usize, &str)> {
        let mut result = Vec::new();
        for decl in &self.parsed.declarations {
            let (members, name, kind) = match decl {
                reify_syntax::Declaration::Structure(s) => {
                    (&s.members, s.name.as_str(), "structure")
                }
                reify_syntax::Declaration::Occurrence(o) => {
                    (&o.members, o.name.as_str(), "occurrence")
                }
                _ => continue,
            };
            let (param_count, let_count, constraint_count) = count_members_recursive(members);
            result.push((name, param_count, let_count, constraint_count, kind));
        }
        result
    }

    /// Look up an evaluated value from the check result.
    pub fn get_value(&self, entity: &str, member: &str) -> Option<&Value> {
        let id = ValueCellId::new(entity, member);
        self.check_result.values.get(&id)
    }
}

/// Recursively search a member list for a named param or let declaration.
///
/// Returns `(span, doc)` for the first match. Recurses into
/// `GuardedGroup.members` and `GuardedGroup.else_members` so that
/// declarations inside `where cond { ... } else { ... }` blocks are found.
pub fn find_named_member_span<'a>(
    members: &'a [reify_syntax::MemberDecl],
    name: &str,
) -> Option<(SourceSpan, Option<&'a str>)> {
    for member in members {
        match member {
            reify_syntax::MemberDecl::Param(p) if p.name == name => {
                return Some((p.span, p.doc.as_deref()));
            }
            reify_syntax::MemberDecl::Let(l) if l.name == name => {
                return Some((l.span, l.doc.as_deref()));
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                if let Some(result) = find_named_member_span(&g.members, name) {
                    return Some(result);
                }
                if let Some(result) = find_named_member_span(&g.else_members, name) {
                    return Some(result);
                }
            }
            _ => {}
        }
    }
    None
}

/// Recursively count Param, Let, and Constraint members, including those
/// nested inside `GuardedGroup.members` and `GuardedGroup.else_members`.
///
/// Returns `(param_count, let_count, constraint_count)`.
pub fn count_members_recursive(members: &[reify_syntax::MemberDecl]) -> (usize, usize, usize) {
    let mut params = 0;
    let mut lets = 0;
    let mut constraints = 0;
    for member in members {
        match member {
            reify_syntax::MemberDecl::Param(_) => params += 1,
            reify_syntax::MemberDecl::Let(_) => lets += 1,
            reify_syntax::MemberDecl::Constraint(_) => constraints += 1,
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                let (p, l, c) = count_members_recursive(&g.members);
                params += p;
                lets += l;
                constraints += c;
                let (p, l, c) = count_members_recursive(&g.else_members);
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
    use reify_types::DimensionVector;
    use tower_lsp::lsp_types::Url;

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

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
        let info = ctx.find_member_decl("width").expect("width should exist");
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
        let info = ctx.find_member_decl("volume").expect("volume should exist");
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
            .find_member_decl("diameter")
            .expect("diameter should exist in occurrence");
        assert_eq!(info.name, "diameter");
        assert_eq!(info.kind, ValueCellKind::Param);
    }

    #[test]
    fn find_member_decl_nonexistent_returns_none() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        assert!(ctx.find_member_decl("nonexistent").is_none());
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
        let names: Vec<&str> = members.iter().map(|(n, _, _)| *n).collect();
        assert!(
            names.contains(&"cond"),
            "should include top-level param 'cond', got: {names:?}"
        );
        assert!(
            names.contains(&"guarded_x"),
            "should include guarded-group param 'guarded_x', got: {names:?}"
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
        let names: Vec<&str> = members.iter().map(|(n, _, _)| *n).collect();
        assert!(
            names.contains(&"cond"),
            "should include top-level param 'cond', got: {names:?}"
        );
        assert!(
            names.contains(&"when_true"),
            "should include where-branch param 'when_true', got: {names:?}"
        );
        assert!(
            names.contains(&"when_false"),
            "should include else-branch param 'when_false', got: {names:?}"
        );
    }

    // --- structure_names tests ---

    #[test]
    fn structure_names_returns_bracket() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let structs = ctx.structure_names();
        assert_eq!(structs.len(), 1);
        let (name, params, lets, constraints, kind) = structs[0];
        assert_eq!(name, "Bracket");
        assert_eq!(params, 5);
        assert_eq!(lets, 2); // volume + body
        assert_eq!(constraints, 3);
        assert_eq!(kind, "structure");
    }

    #[test]
    fn structure_names_includes_occurrence() {
        let source = "structure Bracket {\n    param width: Scalar = 80mm\n}\noccurrence def Joint {\n    param diameter: Scalar = 10mm\n    let radius = diameter / 2\n    constraint diameter > 5mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let names = ctx.structure_names();
        assert_eq!(names.len(), 2, "should have Bracket and Joint");
        let (name0, p0, l0, c0, kind0) = names[0];
        assert_eq!(name0, "Bracket");
        assert_eq!(kind0, "structure");
        assert_eq!(p0, 1);
        assert_eq!(l0, 0);
        assert_eq!(c0, 0);
        let (name1, p1, l1, c1, kind1) = names[1];
        assert_eq!(name1, "Joint");
        assert_eq!(kind1, "occurrence");
        assert_eq!(p1, 1);
        assert_eq!(l1, 1);
        assert_eq!(c1, 1);
    }

    #[test]
    fn structure_names_counts_nested_where_blocks() {
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
        let structs = ctx.structure_names();
        assert_eq!(structs.len(), 1);
        let (_name, param_count, _let_count, _constraint_count, _kind) = structs[0];
        // Should count: a + b + deep = 3 params
        assert_eq!(
            param_count, 3,
            "expected 3 params (a, b, deep), got {param_count}"
        );
    }

    #[test]
    fn structure_names_counts_guarded_group_members() {
        // Bug: structure_names() only counts top-level members, missing those
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
        let structs = ctx.structure_names();
        assert_eq!(structs.len(), 1);
        let (name, param_count, let_count, constraint_count, _kind) = structs[0];
        assert_eq!(name, "S");
        // Should count: a + b + guarded_x = 3 params
        assert_eq!(
            param_count, 3,
            "expected 3 params (a, b, guarded_x), got {param_count}"
        );
        // Should count: guarded_y = 1 let
        assert_eq!(
            let_count, 1,
            "expected 1 let (guarded_y), got {let_count}"
        );
        // Should count: b > 0mm = 1 constraint
        assert_eq!(
            constraint_count, 1,
            "expected 1 constraint, got {constraint_count}"
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
    fn member_info_includes_doc_for_documented_param() {
        let source = "structure Bracket {\n    /// The width.\n    param width: Scalar = 80mm\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx.find_member_decl("width").expect("width should exist");
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
            .find_member_decl("guarded_x")
            .expect("guarded_x inside where block should be found");
        assert_eq!(info.name, "guarded_x");
        assert_eq!(info.kind, ValueCellKind::Param);
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
            .find_member_decl("deep_x")
            .expect("deep_x inside nested where blocks should be found");
        assert_eq!(info.name, "deep_x");
        assert_eq!(info.kind, ValueCellKind::Param);
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
            .find_member_decl("fallback")
            .expect("fallback inside else block should be found");
        assert_eq!(info.name, "fallback");
        assert_eq!(info.kind, ValueCellKind::Let);
    }

    // --- ambiguous member name regression tests ---

    #[test]
    fn find_member_decl_ambiguous_name_returns_first_decl_consistently() {
        // Two structures with identically-named params but different types.
        // find_member_decl must return span AND type from the same declaration.
        let source =
            "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param x: Bool = true\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx.find_member_decl("x").expect("x should exist");
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
        let source =
            "structure A {\n    param x: Scalar = 5mm\n}\nstructure B {\n    param x: Bool = true\n}";
        let ctx = AnalysisContext::new(source, &test_uri());
        let info = ctx.find_member_decl("x").expect("x should exist");
        assert_ne!(
            *info.cell_type,
            Type::Bool,
            "type should not leak from second declaration B"
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
}
