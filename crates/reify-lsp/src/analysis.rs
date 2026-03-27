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
        // Get the span and doc from the parsed module (accurate tree-sitter offsets)
        let (span, doc) = self.find_parsed_member_span_and_doc(name)?;

        // Find type info from the compiled module
        for template in &self.compiled.templates {
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
        }

        None
    }

    /// Find the source span and doc comment for a named member in the parsed module.
    fn find_parsed_member_span_and_doc(&self, name: &str) -> Option<(SourceSpan, Option<&str>)> {
        for decl in &self.parsed.declarations {
            if let reify_syntax::Declaration::Structure(s) = decl {
                for member in &s.members {
                    match member {
                        reify_syntax::MemberDecl::Param(p) if p.name == name => {
                            return Some((p.span, p.doc.as_deref()));
                        }
                        reify_syntax::MemberDecl::Let(l) if l.name == name => {
                            return Some((l.span, l.doc.as_deref()));
                        }
                        _ => {}
                    }
                }
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
        }
        result
    }

    /// Return all structure names with member counts:
    /// `(name, param_count, let_count, constraint_count)`.
    pub fn structure_names(&self) -> Vec<(&str, usize, usize, usize)> {
        let mut result = Vec::new();
        for decl in &self.parsed.declarations {
            if let reify_syntax::Declaration::Structure(s) = decl {
                let param_count = s
                    .members
                    .iter()
                    .filter(|m| matches!(m, reify_syntax::MemberDecl::Param(_)))
                    .count();
                let let_count = s
                    .members
                    .iter()
                    .filter(|m| matches!(m, reify_syntax::MemberDecl::Let(_)))
                    .count();
                let constraint_count = s
                    .members
                    .iter()
                    .filter(|m| matches!(m, reify_syntax::MemberDecl::Constraint(_)))
                    .count();
                result.push((s.name.as_str(), param_count, let_count, constraint_count));
            }
        }
        result
    }

    /// Look up an evaluated value from the check result.
    pub fn get_value(&self, entity: &str, member: &str) -> Option<&Value> {
        let id = ValueCellId::new(entity, member);
        self.check_result.values.get(&id)
    }
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

    // --- structure_names tests ---

    #[test]
    fn structure_names_returns_bracket() {
        let source = reify_test_support::bracket_source();
        let ctx = AnalysisContext::new(source, &test_uri());
        let structs = ctx.structure_names();
        assert_eq!(structs.len(), 1);
        let (name, params, lets, constraints) = structs[0];
        assert_eq!(name, "Bracket");
        assert_eq!(params, 5);
        assert_eq!(lets, 2); // volume + body
        assert_eq!(constraints, 3);
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
